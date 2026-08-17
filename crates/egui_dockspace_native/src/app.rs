//! Thin eframe application over one shared native runtime state.

use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use dockspace::model::{DockPlacement, DockspaceView, NativeWindowPlacement, RootId, SurfaceId};
use dockspace::runtime::DockspaceSession;
use eframe::NativeHostHandler;
use eframe::egui::{self, Id, ViewportClass, ViewportId};
use egui_dockspace::{DockStyle, DockspaceActionStatus, PaneView};

use crate::NativeActionRequestError;
use crate::close_control::NativeWindowClosePolicy;
use crate::deferred_viewport::{
    DeferredViewportSpec, declare_deferred_viewports, paint_placeholder,
};
use crate::error::{NativeHostProtocolError, NativeRuntimeError, NativeRuntimeErrorKind};
use crate::mailbox::DeferredViewportPaint;
use crate::surface_driver::NativeRuntimeState;

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
            NativeWindowClosePolicy::Cancel,
        )
    }

    /// Creates one native application with an explicit synchronous window-close policy.
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
        let state = NativeRuntimeState::new(
            instance_id,
            session,
            root_surface,
            panes,
            style,
            close_policy,
        )?;
        let native_host = state.native_host_handler();
        Ok(Self {
            state: Arc::new(Mutex::new(state)),
            native_host,
        })
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

    /// Queues one complete-root native tear-off for the next final root pass.
    ///
    /// Source ownership remains in place until the managed native lifecycle
    /// reaches its exact post-show transfer and first-live barriers.
    ///
    /// # Errors
    ///
    /// Returns an error when another application action is pending or the
    /// native runtime has stopped.
    pub fn request_tear_off_root(
        &self,
        root: RootId,
        placement: NativeWindowPlacement,
    ) -> Result<(), NativeActionRequestError> {
        lock_state(&self.state).request_tear_off_root(root, placement)
    }

    /// Takes the exact terminal status of the last queued application action.
    ///
    /// A new action remains busy until this result is consumed, so the native
    /// facade never overwrites or grows an unbounded action-result history.
    pub fn take_action_status(&self) -> Option<DockspaceActionStatus> {
        lock_state(&self.state).take_action_status()
    }

    fn update_root(&self, ui: &mut egui::Ui) {
        let context = ui.ctx().clone();
        let (specs, stopped) = {
            let mut state = lock_state(&self.state);
            if state.error().is_some() {
                let (progress, cleanup_errors) = state.advance_shutdown(&context);
                for error in cleanup_errors {
                    state.record_cleanup_error(error);
                }
                state.render_error(ui);
                if progress {
                    context.request_repaint_of(ViewportId::ROOT);
                }
                return;
            }
            let root_surface = state.root_surface();
            if let Err(error) = state.update_surface(ui, root_surface) {
                state.stop_current_output(error);
                state.render_error(ui);
                (Vec::new(), true)
            } else {
                (state.deferred_viewport_specs(), false)
            }
        };
        if stopped {
            context.request_repaint();
            return;
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
        state.render_error(ui);
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
            if let Err(error) = state.update_surface(ui, binding.surface()) {
                state.stop_current_output(error);
                ui.ctx().request_repaint_of(ViewportId::ROOT);
                state.render_error(ui);
            }
        }
        DeferredViewportPaint::Semantic(_) => {
            state.stop_current_output(NativeHostProtocolError::OutputRouteAttachmentFailed.into());
            ui.ctx().request_repaint_of(ViewportId::ROOT);
            state.render_error(ui);
        }
        DeferredViewportPaint::Created | DeferredViewportPaint::Staging(_) => {
            paint_placeholder(ui, class, Some(disposition));
        }
        DeferredViewportPaint::Waiting => {
            paint_placeholder(ui, class, Some(disposition));
        }
    }
}

fn lock_state<P>(state: &Arc<Mutex<P>>) -> MutexGuard<'_, P> {
    state.lock().unwrap_or_else(PoisonError::into_inner)
}
