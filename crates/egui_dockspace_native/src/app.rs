//! Thin eframe application over one shared native runtime state.

use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use dockspace::model::{DockspaceView, SurfaceId};
use dockspace::runtime::DockspaceSession;
use eframe::NativeHostHandler;
use eframe::egui::{self, Id, ViewportClass, ViewportId};
use egui_dockspace::{DockStyle, PaneView};

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
        let state = NativeRuntimeState::new(instance_id, session, root_surface, panes, style)?;
        let native_host = state.native_host_handler();
        Ok(Self {
            state: Arc::new(Mutex::new(state)),
            native_host,
        })
    }

    /// Returns the host callback to install in [`eframe::NativeOptions::native_host`].
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

    fn update_root(&self, ui: &mut egui::Ui) {
        let context = ui.ctx().clone();
        let (specs, stopped) = {
            let mut state = lock_state(&self.state);
            if state.error().is_some() {
                let progress = match state.advance_shutdown(&context) {
                    Ok(progress) => progress,
                    Err(error) => {
                        state.record_cleanup_error(error);
                        false
                    }
                };
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
