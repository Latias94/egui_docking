//! Thin eframe application over one shared native runtime state.

use std::sync::{Arc, Mutex, MutexGuard, PoisonError, Weak};

use dockspace::model::{DockPlacement, DockspaceView, NativeWindowPlacement, RootId, SurfaceId};
use dockspace::runtime::DockspaceSession;
use eframe::egui::{self, Id, ViewportClass, ViewportId};
use eframe::{
    NativeHostHandler, NativeHostWake, NativeOutputPassFinalization, NativeOutputPassFinalizer,
};
use egui_dockspace::{DockStyle, DockspaceActionStatus, PaneView};

use crate::close_control::NativeWindowClosePolicy;
use crate::deferred_viewport::{
    DeferredViewportSpec, declare_deferred_viewports, paint_placeholder,
};
use crate::error::{NativeHostProtocolError, NativeRuntimeError, NativeRuntimeErrorKind};
use crate::lifecycle_progress::NativeCloseProgress;
use crate::mailbox::DeferredViewportPaint;
use crate::surface_driver::{NativeErrorPresentation, NativeRuntimeState, NativeSurfaceUpdate};
use crate::{NativeActionFailure, NativeActionRequestError};

/// Fork-backed eframe application whose [`DockspaceSession`] is the sole graph authority.
///
/// Root and child callbacks borrow the same locked runtime state. The lock is
/// released before eframe declares deferred viewports, so embedded fallback
/// callbacks cannot re-enter a held state lock. Native product panes must be
/// [`Send`] because eframe may invoke deferred viewport callbacks independently
/// of the root callback.
pub struct NativeDockspaceApp<P> {
    state: Arc<Mutex<NativeRuntimeState<P>>>,
    native_host: Arc<dyn NativeHostHandler>,
}

impl<P: PaneView + Send + 'static> NativeDockspaceApp<P> {
    /// Creates one native application around an existing product session.
    ///
    /// The managed pointer provider starts with Unknown global button/capture
    /// authority. Exact event-time edges can still advance their own streams,
    /// while unavailable facts remain fail-closed rather than being inferred
    /// from application startup.
    ///
    /// The default close policy restores managed child presentations through
    /// their core-owned recovery anchors. Closing the external application
    /// root is accepted and retains the logical layout so the application can
    /// complete its normal shutdown.
    ///
    /// # Errors
    ///
    /// Returns an error when the requested root surface is absent, native
    /// enrollment fails, or the root registration cannot be queued.
    pub fn new(
        instance_id: Id,
        session: DockspaceSession,
        root_surface: SurfaceId,
        panes: P,
        style: DockStyle,
    ) -> Result<Self, NativeRuntimeError> {
        Self::new_with_close_policy(
            instance_id,
            session,
            root_surface,
            panes,
            style,
            NativeWindowClosePolicy::default(),
        )
    }

    /// Creates one native application with an explicit window-close policy.
    ///
    /// The original OS request remains held while ordered input or its close
    /// decision yields to the next native dispatch budget.
    ///
    /// # Errors
    ///
    /// Returns the same initialization failures as [`Self::new`].
    pub fn new_with_close_policy(
        instance_id: Id,
        session: DockspaceSession,
        root_surface: SurfaceId,
        panes: P,
        style: DockStyle,
        close_policy: NativeWindowClosePolicy,
    ) -> Result<Self, NativeRuntimeError> {
        Self::new_configured(
            instance_id,
            session,
            root_surface,
            panes,
            style,
            close_policy,
            None,
        )
    }

    /// Creates a native application with complete finite input/lifecycle storage.
    ///
    /// Ordinary ingress freezes before it can exceed the selected byte, record
    /// or age allowance. Previously accepted input and terminal obligations
    /// retain their original ordered recovery path.
    ///
    /// # Errors
    ///
    /// Returns configuration or enrollment failure before returning a host
    /// handler, including invalid core subprofile storage allowances.
    pub fn new_with_storage_limit(
        instance_id: Id,
        session: DockspaceSession,
        root_surface: SurfaceId,
        panes: P,
        style: DockStyle,
        close_policy: NativeWindowClosePolicy,
        storage: crate::NativeStorageLimit,
    ) -> Result<Self, NativeRuntimeError> {
        Self::new_configured(
            instance_id,
            session,
            root_surface,
            panes,
            style,
            close_policy,
            Some(storage),
        )
    }

    fn new_configured(
        instance_id: Id,
        session: DockspaceSession,
        root_surface: SurfaceId,
        panes: P,
        style: DockStyle,
        close_policy: NativeWindowClosePolicy,
        storage: Option<crate::NativeStorageLimit>,
    ) -> Result<Self, NativeRuntimeError> {
        let state = NativeRuntimeState::new_with_storage_limit(
            instance_id,
            session,
            root_surface,
            panes,
            style,
            close_policy,
            storage,
        )?;
        let native_host = state.native_host_handler();
        let state = Arc::new(Mutex::new(state));
        let finalizer: Arc<dyn NativeOutputPassFinalizer> = Arc::new(NativePassFinalizer {
            state: Arc::downgrade(&state),
        });
        assert!(
            native_host.install_output_pass_finalizer(finalizer),
            "the Dockspace native host accepts exactly one output-pass finalizer"
        );
        Ok(Self { state, native_host })
    }

    /// Returns the one-shot host callback to install in [`eframe::NativeOptions::native_host`].
    ///
    /// The returned `Arc` may be cloned for ownership plumbing, but the native
    /// runtime accepts exactly one eframe context attachment. Construct a new
    /// application and session instead of reusing this handler for another
    /// event loop.
    #[must_use]
    pub fn native_host_handler(&self) -> Arc<dyn NativeHostHandler> {
        Arc::clone(&self.native_host)
    }

    /// Inspects the current item/surface-centric product view while holding the runtime lock.
    ///
    /// A fatal native-cycle error freezes further host ingress but preserves
    /// the last committed read-only view while owned host state is quarantined.
    pub fn with_view<R>(&self, inspect: impl FnOnce(DockspaceView<'_>) -> R) -> Option<R> {
        lock_state(&self.state).with_view(inspect)
    }

    /// Inspects the application pane registry while holding the runtime lock.
    pub fn with_panes<R>(&self, inspect: impl FnOnce(&P) -> R) -> R {
        inspect(lock_state(&self.state).panes())
    }

    /// Mutates the application pane registry while holding the runtime lock.
    pub fn with_panes_mut<R>(&self, mutate: impl FnOnce(&mut P) -> R) -> R {
        mutate(lock_state(&self.state).panes_mut())
    }

    /// Returns the stable category of the first fatal native-cycle error.
    #[must_use]
    pub fn error_kind(&self) -> Option<NativeRuntimeErrorKind> {
        lock_state(&self.state)
            .error()
            .map(NativeRuntimeError::kind)
    }

    /// Returns whether one logical surface has an exact currently presented output.
    ///
    /// This is a product readiness query, not a renderer token or historical
    /// acknowledgement. It becomes false again when the current presentation
    /// authority is retired or invalidated.
    #[must_use]
    pub fn is_surface_presented(&self, surface: SurfaceId) -> bool {
        lock_state(&self.state).is_surface_presented(surface)
    }

    /// Returns whether native lifecycle work is settled at the current committed boundary.
    ///
    /// Stable live viewports and their ordinary repaint outputs do not make the
    /// runtime busy. Pending application actions, create/show barriers, close or
    /// focus effects, callback records, and binding retirement do.
    #[must_use]
    pub fn is_quiescent(&self) -> bool {
        lock_state(&self.state).is_quiescent()
    }

    /// Returns bounded retained-request and committed-cancellation close counters.
    #[must_use]
    pub fn close_progress(&self) -> NativeCloseProgress {
        lock_state(&self.state).close_progress()
    }

    /// Returns the current owned data capacity of native input and lifecycle transport.
    ///
    /// Includes preallocated raw/roster backing, recorder/replay transport,
    /// pending report queues, exact window/output/close/retirement owners,
    /// callback controls, pointer translation state and snapshot/work-area
    /// sidecars. Shared roster and abandoned-result data are counted once
    /// through their existing owners. Spare vector capacity remains charged.
    ///
    /// Excludes allocator/BTree node metadata, Arc control blocks, opaque host
    /// callbacks/clock closures, core workspace/authority, renderer/receiver
    /// data, application action/transition queues, diagnostic errors and
    /// caller-owned buffers. This
    /// measures current data at a locked host boundary; it does not reserve
    /// future terminal data or certify the complete 4 MiB admission limit.
    ///
    /// # Errors
    ///
    /// Returns an invariant error if an internal producer bypasses the finite
    /// native payload contract of the recorder.
    pub fn input_storage_capacity_bytes(&self) -> Result<usize, NativeRuntimeError> {
        lock_state(&self.state).owned_input_capacity_bytes()
    }

    /// Returns current retained data plus prepaid native construction promises.
    ///
    /// Returns `None` for the unlimited constructor. This query uses the same
    /// combined allowance as ingress admission; it is distinct from resident
    /// capacity and does not certify physical-input timing or renderer memory.
    #[must_use]
    pub fn input_storage_reserved_bytes(&self) -> Option<usize> {
        lock_state(&self.state).input_storage_reserved_bytes()
    }

    /// Queues one complete-root dock action for the next final root pass.
    ///
    /// The action is prepared against the exact published revision while the
    /// native runtime lock is held. At most one application action may wait at
    /// a time, which keeps retention bounded and preserves deterministic input
    /// ordering ahead of local actions produced by that pass.
    ///
    /// # Errors
    ///
    /// Returns an error when another application action is pending or the
    /// native runtime has stopped.
    pub fn request_dock_root(
        &self,
        root: RootId,
        placement: DockPlacement,
    ) -> Result<(), NativeActionRequestError> {
        lock_state(&self.state).request_dock_root(root, placement)
    }

    /// Queues recovery of one contained or native complete-root presentation.
    ///
    /// The core derives and revalidates the current recovery target. Native
    /// ownership remains with the source window until the target output is
    /// actually presented.
    ///
    /// # Errors
    ///
    /// Returns an error when another application action is pending or the
    /// native runtime has stopped.
    pub fn request_dock_root_back(&self, root: RootId) -> Result<(), NativeActionRequestError> {
        lock_state(&self.state).request_dock_root_back(root)
    }

    /// Queues one complete-root move to a new native window for the next final root pass.
    ///
    /// Source ownership remains in place until the managed native lifecycle
    /// reaches its exact post-show transfer and first-live barriers.
    ///
    /// # Errors
    ///
    /// Returns an error when another application action is pending or the
    /// native runtime has stopped.
    pub fn request_move_root_to_new_window(
        &self,
        root: RootId,
        placement: NativeWindowPlacement,
    ) -> Result<(), NativeActionRequestError> {
        lock_state(&self.state).request_move_root_to_new_window(root, placement)
    }

    /// Takes the exact terminal status of the last queued application action.
    ///
    /// A new action remains busy until this result is consumed, so the native
    /// facade never overwrites or grows an unbounded action-result history.
    pub fn take_action_status(&self) -> Option<Result<DockspaceActionStatus, NativeActionFailure>> {
        lock_state(&self.state).take_action_status()
    }

    fn update_root(&self, ui: &mut egui::Ui) {
        let context = ui.ctx().clone();
        let token = eframe::current_native_output_token();
        let (specs, request_repaint) = {
            let mut state = lock_state(&self.state);
            let mut request_repaint = if state.error().is_some() {
                let (progress, cleanup_errors) = state.advance_shutdown(&context);
                for error in cleanup_errors {
                    state.record_cleanup_error(error);
                }
                render_native_error(&mut state, token, ui);
                progress
            } else {
                let root_surface = state.root_surface();
                match state.update_surface(ui, root_surface) {
                    Ok(NativeSurfaceUpdate::Semantic) => false,
                    Ok(NativeSurfaceUpdate::RetainPrevious) => {
                        paint_root_waiting(ui);
                        false
                    }
                    Err(error) => {
                        state.stop_current_output(error);
                        render_native_error(&mut state, token, ui);
                        true
                    }
                }
            };
            let specs = match state.prepare_deferred_viewport_specs() {
                Ok(specs) => specs,
                Err(error) => {
                    state.stop_current_output(error);
                    render_native_error(&mut state, token, ui);
                    request_repaint = true;
                    Vec::new()
                }
            };
            (specs, request_repaint)
        };
        if request_repaint {
            context.request_repaint_once_of(ViewportId::ROOT);
        }

        let state = Arc::clone(&self.state);
        declare_deferred_viewports(&context, specs, move |spec, ui, class| {
            render_deferred_viewport(&state, spec, ui, class);
        });
    }
}

impl<P: PaneView + Send + 'static> eframe::App for NativeDockspaceApp<P> {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.update_root(ui);
    }
}

fn render_deferred_viewport<P: PaneView + Send + 'static>(
    shared: &Arc<Mutex<NativeRuntimeState<P>>>,
    spec: DeferredViewportSpec,
    ui: &mut egui::Ui,
    class: ViewportClass,
) {
    let token = eframe::current_native_output_token();
    let mut state = lock_state(shared);
    if state.error().is_some() {
        let (progress, cleanup_errors) = state.advance_shutdown(ui.ctx());
        for error in cleanup_errors {
            state.record_cleanup_error(error);
        }
        if progress {
            ui.ctx().request_repaint_once_of(ViewportId::ROOT);
        }
        render_native_error(&mut state, token, ui);
        return;
    }
    if class != ViewportClass::Deferred {
        paint_placeholder(ui, class, None);
        return;
    }
    let Some(token) = token else {
        paint_placeholder(ui, class, None);
        return;
    };

    let disposition = state.deferred_viewport_paint(token);
    match disposition {
        DeferredViewportPaint::Semantic(binding) if binding == spec.binding() => {
            match state.update_surface(ui, binding.surface()) {
                Ok(NativeSurfaceUpdate::Semantic) => {}
                Ok(NativeSurfaceUpdate::RetainPrevious) => {
                    paint_placeholder(ui, class, Some(DeferredViewportPaint::Waiting));
                }
                Err(error) => {
                    state.stop_current_output(error);
                    ui.ctx().request_repaint_once_of(ViewportId::ROOT);
                    render_native_error(&mut state, Some(token), ui);
                }
            }
        }
        DeferredViewportPaint::Semantic(_binding) => {
            state.stop_current_output(NativeHostProtocolError::OutputRouteAttachmentFailed.into());
            ui.ctx().request_repaint_once_of(ViewportId::ROOT);
            render_native_error(&mut state, Some(token), ui);
        }
        DeferredViewportPaint::Created | DeferredViewportPaint::Staging(_) => {
            paint_placeholder(ui, class, Some(disposition));
        }
        DeferredViewportPaint::Retain(binding) => {
            if let Err(error) = state.stage_retain_previous(token, binding.surface()) {
                state.stop_current_output(error);
                ui.ctx().request_repaint_once_of(ViewportId::ROOT);
                state.render_error(ui, NativeErrorPresentation::NoFrame);
            } else {
                paint_placeholder(ui, class, Some(DeferredViewportPaint::Waiting));
            }
        }
        DeferredViewportPaint::Waiting => {
            paint_placeholder(ui, class, Some(disposition));
        }
    }
}

fn render_native_error<P: PaneView>(
    state: &mut NativeRuntimeState<P>,
    token: Option<eframe::NativeOutputToken>,
    ui: &mut egui::Ui,
) {
    let retained_overlay = token.is_some_and(|token| {
        if let Err(error) = state.stage_retained_overlay(token) {
            state.record_cleanup_error(error);
            return false;
        }
        request_retained_error_overlay()
    });
    let presentation = if retained_overlay {
        NativeErrorPresentation::RetainedOverlay
    } else {
        NativeErrorPresentation::NoFrame
    };
    state.render_error(ui, presentation);
}

fn request_retained_error_overlay() -> bool {
    eframe::retain_current_native_output_with_overlay()
}

fn lock_state<P>(state: &Arc<Mutex<P>>) -> MutexGuard<'_, P> {
    state.lock().unwrap_or_else(PoisonError::into_inner)
}

struct NativePassFinalizer<P> {
    state: Weak<Mutex<NativeRuntimeState<P>>>,
}

impl<P: PaneView + Send + 'static> NativeOutputPassFinalizer for NativePassFinalizer<P> {
    fn finalize(&self, finalization: NativeOutputPassFinalization<'_>) -> NativeHostWake {
        let Some(state) = self.state.upgrade() else {
            return NativeHostWake::Wait;
        };
        let (context, token, ended_viewport, disposition, output) = finalization.into_parts();
        let mut state = lock_state(&state);
        let result = if token.viewport_id() == ended_viewport {
            state.settle_egui_pass(
                context,
                output,
                token,
                disposition == egui::OutputPassDisposition::Repeat,
            )
        } else {
            Err(NativeHostProtocolError::MultipassOutputChanged.into())
        };
        if let Err(error) = result {
            if state.error().is_some() {
                state.record_cleanup_error(error);
            } else {
                state.stop_current_output(error);
                return NativeHostWake::RepaintRoot;
            }
        }
        NativeHostWake::Wait
    }
}

fn paint_root_waiting(ui: &mut egui::Ui) {
    ui.painter()
        .rect_filled(ui.max_rect(), 0.0, egui::Color32::from_gray(24));
    ui.centered_and_justified(|ui| {
        ui.label("Waiting for dockspace authority…");
    });
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex, PoisonError};

    use eframe::egui;

    #[derive(Default)]
    struct LateDiscardProbe {
        remaining: usize,
    }

    impl egui::plugin::Plugin for LateDiscardProbe {
        fn debug_name(&self) -> &'static str {
            "egui_dockspace_native::late_discard_request"
        }

        fn output_hook(&mut self, _context: &egui::Context, output: &mut egui::FullOutput) {
            if self.remaining == 0 {
                return;
            }
            self.remaining -= 1;
            output
                .platform_output
                .request_discard_reasons
                .push(egui::RepaintCause::new_reason(
                    "outer application requested a later pass",
                ));
        }
    }

    #[test]
    fn post_output_settlement_observes_a_late_discard() {
        let context = egui::Context::default();
        let observations = Arc::new(Mutex::new(Vec::new()));
        context
            .plugin_or_default::<LateDiscardProbe>()
            .lock()
            .remaining = 1;

        let finalizer_observations = Arc::clone(&observations);
        let mut output = context.run_ui_with_pass_finalizer(
            egui::RawInput::default(),
            |ui| {
                ui.label("native dockspace ui has returned");
            },
            move |_context, _ended_viewport, disposition, _output| {
                finalizer_observations
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .push(disposition);
            },
        );
        output.textures_delta.clear();

        assert_eq!(
            *observations.lock().unwrap_or_else(PoisonError::into_inner),
            [
                egui::OutputPassDisposition::Repeat,
                egui::OutputPassDisposition::Terminal,
            ],
            "only the pass followed by another egui pass is discarded"
        );
    }

    #[test]
    fn denied_discard_is_the_terminal_pass() {
        let context = egui::Context::default();
        context.options_mut(|options| {
            options.max_passes = std::num::NonZeroUsize::MIN;
        });
        let observations = Arc::new(Mutex::new(Vec::new()));
        context
            .plugin_or_default::<LateDiscardProbe>()
            .lock()
            .remaining = 1;

        let finalizer_observations = Arc::clone(&observations);
        let mut output = context.run_ui_with_pass_finalizer(
            egui::RawInput::default(),
            |ui| {
                ui.label("the configured pass limit denies the late request");
            },
            move |_context, _ended_viewport, disposition, _output| {
                finalizer_observations
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .push(disposition);
            },
        );
        output.textures_delta.clear();

        assert_eq!(
            *observations.lock().unwrap_or_else(PoisonError::into_inner),
            [egui::OutputPassDisposition::Terminal],
            "a denied discard must still settle the current output"
        );
    }
}
