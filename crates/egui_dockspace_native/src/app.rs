//! Thin eframe application over one renderer-neutral native coordinator.

use std::sync::Arc;

use dockspace::model::{DockspaceView, SurfaceId};
use dockspace::runtime::{
    DockspaceSession, HostInputOutcome, HostWindowToken, NativePointerRoster,
    SurfaceUnavailableReason,
};
use eframe::egui::{self, Id};
use eframe::{NativeHostHandler, NativeHostWake, NativeOutputToken};
use egui_dockspace::{DockStyle, PaneView};

use crate::coordinator::NativeCoordinator;
use crate::error::{NativeHostProtocolError, NativeRuntimeError};
use crate::pass_actions::{NativePassActionError, NativePassActions};

const ROOT_WINDOW_TOKEN: HostWindowToken = HostWindowToken::new(1);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SurfaceFrameDisposition {
    ConfirmPainted,
    Defer,
    Measure,
    RejectIncompletePaint,
}

const fn surface_frame_disposition(
    had_ready_plan: bool,
    transient_visuals_complete: bool,
    deferred_measurement: bool,
    has_actions: bool,
) -> SurfaceFrameDisposition {
    if had_ready_plan && !transient_visuals_complete {
        SurfaceFrameDisposition::RejectIncompletePaint
    } else if had_ready_plan && !has_actions && !deferred_measurement {
        SurfaceFrameDisposition::ConfirmPainted
    } else if has_actions || deferred_measurement {
        SurfaceFrameDisposition::Defer
    } else {
        SurfaceFrameDisposition::Measure
    }
}

/// Fork-backed eframe application whose [`DockspaceSession`] is the sole graph authority.
///
/// The app owns the complete egui update callback so it can discard a core candidate when egui
/// requests another pass, retain only exact local response actions, and commit receiver authority
/// only for the final pass. The current vertical slice drives hidden child creation, native
/// staging, and showing. Replacement, close, focus, and pointer pass-through remain explicitly
/// unsupported until their exact platform lanes are connected.
pub struct NativeDockspaceApp<P> {
    coordinator: Option<NativeCoordinator>,
    native_host: Arc<dyn NativeHostHandler>,
    root_surface: SurfaceId,
    instance_id: Id,
    style: DockStyle,
    panes: P,
    pass_actions: NativePassActions,
    error: Option<NativeRuntimeError>,
}

impl<P: PaneView> NativeDockspaceApp<P> {
    /// Creates one native root application around an existing product session.
    ///
    /// The managed pointer provider starts with Unknown global button/capture authority. Exact
    /// event-time edges can still advance their own streams, while unavailable facts remain
    /// fail-closed rather than being inferred from application startup.
    ///
    /// # Errors
    ///
    /// Returns an error when the requested root surface is absent, native enrollment fails, or
    /// the root registration cannot be queued.
    pub fn new(
        instance_id: Id,
        session: DockspaceSession,
        root_surface: SurfaceId,
        panes: P,
        style: DockStyle,
    ) -> Result<Self, NativeRuntimeError> {
        if session.view().surface(root_surface).is_none() {
            return Err(NativeHostProtocolError::RootSurfaceUnavailable(root_surface).into());
        }
        let mut coordinator = NativeCoordinator::new(session, NativePointerRoster::Unknown)?;
        coordinator.register_native_root(root_surface, ROOT_WINDOW_TOKEN)?;
        let native_host = coordinator.native_host_handler();
        Ok(Self {
            coordinator: Some(coordinator),
            native_host,
            root_surface,
            instance_id,
            style,
            panes,
            pass_actions: NativePassActions::default(),
            error: None,
        })
    }

    /// Returns the host callback to install in [`eframe::NativeOptions::native_host`].
    #[must_use]
    pub fn native_host_handler(&self) -> Arc<dyn NativeHostHandler> {
        Arc::clone(&self.native_host)
    }

    /// Returns the published item/surface-centric workspace view while the runtime is active.
    #[must_use]
    pub fn view(&self) -> Option<DockspaceView<'_>> {
        self.coordinator
            .as_ref()
            .map(|coordinator| coordinator.session().view())
    }

    /// Returns the pane registry owned by this application.
    #[must_use]
    pub const fn panes(&self) -> &P {
        &self.panes
    }

    /// Returns mutable access to the pane registry owned by this application.
    pub const fn panes_mut(&mut self) -> &mut P {
        &mut self.panes
    }

    /// Returns the first fatal native-cycle error, if one occurred.
    #[must_use]
    pub const fn error(&self) -> Option<&NativeRuntimeError> {
        self.error.as_ref()
    }

    fn update_native(&mut self, ui: &mut egui::Ui) -> Result<(), NativeRuntimeError> {
        let context = ui.ctx().clone();
        let token = eframe::current_native_output_token()
            .ok_or(NativeHostProtocolError::OutputTokenUnavailable)?;
        let coordinator = self
            .coordinator
            .as_mut()
            .expect("an error-free native app retains its coordinator");
        let reduced_callback = coordinator.reduce_callback_head()?;

        let root_surface = self.root_surface;
        let instance_id = self.instance_id;
        let style = &self.style;
        let panes = &mut self.panes;
        let pass_actions = &mut self.pass_actions;
        let mut host_frame = coordinator.begin_resolved_host_frame(&context)?;
        let mut final_paint = None;
        let mut painted_output_expected = false;
        let mut post_action_repaint = false;
        let mut discarded = false;

        let render_result = egui::CentralPanel::default().show(ui, |ui| {
            let mut paint =
                host_frame.paint_surface(instance_id, root_surface, ui, panes, style)?;
            if context.will_discard() {
                pass_actions
                    .discard_pass(token, &mut paint)
                    .map_err(map_pass_action_error)?;
                discarded = true;
                return Ok::<(), NativeRuntimeError>(());
            }

            let actions = pass_actions
                .finish_pass(token, &mut paint)
                .map_err(map_pass_action_error)?;
            let has_actions = actions.has_actions();
            let disposition = surface_frame_disposition(
                paint.had_ready_plan(),
                paint.transient_visuals_complete(),
                paint.deferred_measurement(),
                has_actions,
            );
            if disposition == SurfaceFrameDisposition::RejectIncompletePaint {
                return Err(NativeHostProtocolError::IncompleteTransientPaint(root_surface).into());
            }
            for action in actions.into_ordered() {
                host_frame.submit_surface_action(action)?;
            }

            match disposition {
                SurfaceFrameDisposition::ConfirmPainted => {
                    host_frame.confirm_surface_painted(root_surface)?;
                    host_frame.complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)?;
                    painted_output_expected = true;
                }
                SurfaceFrameDisposition::Defer => {
                    host_frame.complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)?;
                    post_action_repaint = has_actions;
                }
                SurfaceFrameDisposition::Measure => {
                    host_frame.measure_surface(root_surface, ui, panes, style)?;
                    host_frame.complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)?;
                }
                SurfaceFrameDisposition::RejectIncompletePaint => unreachable!(
                    "incomplete transient paint is rejected before submitting surface actions"
                ),
            }
            final_paint = Some(paint);
            Ok(())
        });
        render_result.inner?;
        if discarded {
            return Ok(());
        }

        let mut report = host_frame.commit()?;
        self.bind_root_registration(token, report.inputs())?;
        let native_effects = report.take_native_effects();

        let mut outputs = report.take_painted_outputs();
        if painted_output_expected {
            if outputs.len() != 1 {
                let actual = outputs.len();
                drop(outputs);
                return Err(NativeHostProtocolError::PaintedOutputCountMismatch {
                    expected: 1,
                    actual,
                }
                .into());
            }
            let output = outputs.pop().expect("one painted output was checked");
            let paint = final_paint
                .as_ref()
                .expect("a committed painted output has final-pass paint metadata");
            if let Err(error) = self
                .coordinator
                .as_mut()
                .expect("an error-free native app retains its coordinator")
                .bind_painted_surface(token, output, paint)
            {
                let kind = error.kind();
                drop(error.into_output());
                let _ = self
                    .coordinator
                    .as_mut()
                    .expect("an error-free native app retains its coordinator")
                    .abandon_output_token(token);
                return Err(NativeHostProtocolError::OutputBindingFailed(kind).into());
            }
        } else {
            if !outputs.is_empty() {
                let actual = outputs.len();
                drop(outputs);
                return Err(NativeHostProtocolError::PaintedOutputCountMismatch {
                    expected: 0,
                    actual,
                }
                .into());
            }
            if matches!(
                self.coordinator
                    .as_mut()
                    .expect("an error-free native app retains its coordinator")
                    .abandon_output_token(token),
                NativeHostWake::RepaintRoot
            ) {
                context.request_repaint();
            }
        }

        let coordinator = self
            .coordinator
            .as_mut()
            .expect("an error-free native app retains its coordinator");
        coordinator.accept_native_effects(native_effects)?;
        coordinator.declare_deferred_viewports(&context);

        for surface in report.repaint_surfaces() {
            match coordinator.repaint_viewport(*surface) {
                Some(egui::ViewportId::ROOT) | None if *surface == root_surface => {
                    context.request_repaint();
                }
                Some(viewport) => context.request_repaint_of(viewport),
                None => {}
            }
        }
        if reduced_callback {
            context.request_repaint();
        }
        if post_action_repaint {
            context.request_repaint();
        }
        Ok(())
    }

    fn bind_root_registration(
        &mut self,
        token: NativeOutputToken,
        inputs: &[HostInputOutcome],
    ) -> Result<(), NativeRuntimeError> {
        for input in inputs {
            match input {
                HostInputOutcome::NativeSurfaceRegistered { binding }
                    if binding.surface() == self.root_surface =>
                {
                    self.coordinator
                        .as_mut()
                        .expect("an error-free native app retains its coordinator")
                        .bind_viewport(egui::ViewportId::ROOT, token.window_id(), *binding)
                        .map_err(|_| NativeHostProtocolError::RootViewportBindingFailed)?;
                }
                HostInputOutcome::NativeSurfaceRegistrationRejected { surface }
                    if *surface == self.root_surface =>
                {
                    return Err(NativeHostProtocolError::RootRegistrationRejected(*surface).into());
                }
                _ => {}
            }
        }
        Ok(())
    }

    fn render_error(&mut self, ui: &mut egui::Ui) {
        ui.painter()
            .rect_filled(ui.max_rect(), 0.0, self.style.workspace_fill);
        ui.heading("Native dockspace stopped");
        if let Some(error) = &self.error {
            ui.label(error.to_string());
        } else {
            ui.label("unknown native runtime error");
        }
    }

    fn stop(&mut self, error: NativeRuntimeError) {
        if let Some(token) = eframe::current_native_output_token() {
            self.pass_actions.abandon(token);
            if let Some(coordinator) = self.coordinator.as_mut() {
                let _ = coordinator.abandon_output_token(token);
            }
        }
        self.error = Some(error);
        self.coordinator = None;
    }
}

impl<P: PaneView> eframe::App for NativeDockspaceApp<P> {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        if self.error.is_some() {
            self.render_error(ui);
            return;
        }
        if let Err(error) = self.update_native(ui) {
            self.stop(error);
            ui.ctx().request_repaint();
            self.render_error(ui);
        }
    }
}

fn map_pass_action_error(error: NativePassActionError) -> NativeRuntimeError {
    match error {
        NativePassActionError::OutputChanged => {
            NativeHostProtocolError::MultipassOutputChanged.into()
        }
        NativePassActionError::LocalActionConflict => {
            NativeHostProtocolError::MultipassLocalActionConflict.into()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn actions_defer_and_incomplete_transient_paint_is_rejected() {
        assert_eq!(
            surface_frame_disposition(true, false, false, true),
            SurfaceFrameDisposition::RejectIncompletePaint
        );
        assert_eq!(
            surface_frame_disposition(true, true, false, true),
            SurfaceFrameDisposition::Defer
        );
        assert_eq!(
            surface_frame_disposition(true, true, true, true),
            SurfaceFrameDisposition::Defer
        );
        assert_eq!(
            surface_frame_disposition(true, true, false, false),
            SurfaceFrameDisposition::ConfirmPainted
        );
    }
}
