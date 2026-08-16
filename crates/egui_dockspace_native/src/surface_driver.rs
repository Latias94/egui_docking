//! Shared root/child surface driver over one native coordinator.

use dockspace::model::{DockspaceView, SurfaceId};
use dockspace::runtime::{
    HostInputOutcome, HostWindowToken, SurfaceUnavailableReason,
};
use eframe::egui::{self, Id};
use eframe::{NativeHostWake, NativeOutputToken};
use egui_dockspace::{DockStyle, PaneView};

use crate::coordinator::NativeCoordinator;
use crate::deferred_viewport::DeferredViewportSpec;
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

pub(crate) struct NativeRuntimeState<P> {
    coordinator: Option<NativeCoordinator>,
    root_surface: SurfaceId,
    instance_id: Id,
    style: DockStyle,
    panes: P,
    pass_actions: NativePassActions,
    error: Option<NativeRuntimeError>,
}

impl<P: PaneView> NativeRuntimeState<P> {
    pub(crate) fn new(
        instance_id: Id,
        session: dockspace::runtime::DockspaceSession,
        root_surface: SurfaceId,
        panes: P,
        style: DockStyle,
    ) -> Result<Self, NativeRuntimeError> {
        if session.view().surface(root_surface).is_none() {
            return Err(NativeHostProtocolError::RootSurfaceUnavailable(root_surface).into());
        }
        let mut coordinator = NativeCoordinator::new(
            session,
            dockspace::runtime::NativePointerRoster::Unknown,
        )?;
        coordinator.register_native_root(root_surface, ROOT_WINDOW_TOKEN)?;
        Ok(Self {
            coordinator: Some(coordinator),
            root_surface,
            instance_id,
            style,
            panes,
            pass_actions: NativePassActions::default(),
            error: None,
        })
    }

    pub(crate) fn native_host_handler(
        &self,
    ) -> std::sync::Arc<dyn eframe::NativeHostHandler> {
        self.coordinator
            .as_ref()
            .expect("an initialized native runtime retains its coordinator")
            .native_host_handler()
    }

    pub(crate) const fn root_surface(&self) -> SurfaceId {
        self.root_surface
    }

    pub(crate) fn with_view<R>(&self, inspect: impl FnOnce(DockspaceView<'_>) -> R) -> Option<R> {
        self.coordinator
            .as_ref()
            .map(|coordinator| inspect(coordinator.session().view()))
    }

    pub(crate) const fn panes(&self) -> &P {
        &self.panes
    }

    pub(crate) const fn panes_mut(&mut self) -> &mut P {
        &mut self.panes
    }

    pub(crate) const fn error(&self) -> Option<&NativeRuntimeError> {
        self.error.as_ref()
    }

    pub(crate) fn deferred_viewport_specs(&self) -> Vec<DeferredViewportSpec> {
        self.coordinator
            .as_ref()
            .map_or_else(Vec::new, NativeCoordinator::deferred_viewport_specs)
    }

    pub(crate) fn deferred_viewport_paint(
        &self,
        token: NativeOutputToken,
    ) -> crate::mailbox::DeferredViewportPaint {
        self.coordinator
            .as_ref()
            .map_or(crate::mailbox::DeferredViewportPaint::Waiting, |coordinator| {
                coordinator.deferred_viewport_paint(token)
            })
    }

    pub(crate) fn abandon_output(&mut self, token: NativeOutputToken) {
        self.pass_actions.abandon(token);
        if let Some(coordinator) = self.coordinator.as_mut() {
            let _ = coordinator.abandon_output_token(token);
        }
    }

    pub(crate) fn update_surface(
        &mut self,
        ui: &mut egui::Ui,
        surface: SurfaceId,
    ) -> Result<(), NativeRuntimeError> {
        let context = ui.ctx().clone();
        let token = eframe::current_native_output_token()
            .ok_or(NativeHostProtocolError::OutputTokenUnavailable)?;
        let coordinator = self
            .coordinator
            .as_mut()
            .expect("an error-free native runtime retains its coordinator");
        let quiescence_recorded = surface == self.root_surface
            && coordinator.try_report_retirement_quiescence()?;
        let reduced_callback = coordinator.reduce_callback_head()?;
        let prepared_retirements = if surface == self.root_surface {
            coordinator.prepare_committed_retirements()?
        } else {
            None
        };

        let instance_id = self.instance_id.with(surface.get());
        let style = &self.style;
        let panes = &mut self.panes;
        let pass_actions = &mut self.pass_actions;
        let mut host_frame = coordinator.begin_resolved_host_frame(&context)?;
        let mut final_paint = None;
        let mut painted_output_expected = false;
        let mut post_action_repaint = false;
        let mut discarded = false;

        let render_result = egui::CentralPanel::default().show(ui, |ui| {
            let mut paint = host_frame.paint_surface(instance_id, surface, ui, panes, style)?;
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
                return Err(NativeHostProtocolError::IncompleteTransientPaint(surface).into());
            }
            for action in actions.into_ordered() {
                host_frame.submit_surface_action(action)?;
            }

            match disposition {
                SurfaceFrameDisposition::ConfirmPainted => {
                    host_frame.confirm_surface_painted(surface)?;
                    host_frame.complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)?;
                    painted_output_expected = true;
                }
                SurfaceFrameDisposition::Defer => {
                    host_frame.complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)?;
                    post_action_repaint = has_actions;
                }
                SurfaceFrameDisposition::Measure => {
                    host_frame.measure_surface(surface, ui, panes, style)?;
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
        let coordinator = self
            .coordinator
            .as_mut()
            .expect("an error-free native runtime retains its coordinator");
        let native_snapshot_applied = coordinator.settle_host_frame_inputs(report.inputs());
        let native_admission_settled =
            coordinator.settle_native_admissions(report.native_admissions())?;
        if let Some(prepared_retirements) = prepared_retirements {
            for token in coordinator.commit_retirements(prepared_retirements) {
                self.pass_actions.abandon(token);
            }
        }
        if surface == self.root_surface {
            self.bind_root_registration(token, report.inputs())?;
        }
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
                .expect("an error-free native runtime retains its coordinator")
                .bind_painted_surface(token, output, paint)
            {
                let kind = error.kind();
                drop(error.into_output());
                let _ = self
                    .coordinator
                    .as_mut()
                    .expect("an error-free native runtime retains its coordinator")
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
                    .expect("an error-free native runtime retains its coordinator")
                    .abandon_output_token(token),
                NativeHostWake::RepaintRoot
            ) {
                context.request_repaint_of(egui::ViewportId::ROOT);
            }
        }

        let coordinator = self
            .coordinator
            .as_mut()
            .expect("an error-free native runtime retains its coordinator");
        let native_effects_emitted = !native_effects.is_empty();
        coordinator.accept_native_effects(native_effects)?;
        for (viewport, command) in coordinator.take_viewport_commands() {
            context.send_viewport_cmd_to(viewport, command);
        }
        for repaint_surface in report.repaint_surfaces() {
            match coordinator.repaint_viewport(*repaint_surface) {
                Some(egui::ViewportId::ROOT) | None if *repaint_surface == self.root_surface => {
                    context.request_repaint_of(egui::ViewportId::ROOT);
                }
                Some(viewport) => context.request_repaint_of(viewport),
                None => {}
            }
        }
        if quiescence_recorded
            || reduced_callback
            || native_snapshot_applied
            || native_admission_settled
            || post_action_repaint
            || native_effects_emitted
        {
            context.request_repaint_of(egui::ViewportId::ROOT);
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
                        .expect("an error-free native runtime retains its coordinator")
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

    pub(crate) fn stop_current_output(&mut self, error: NativeRuntimeError) {
        if let Some(token) = eframe::current_native_output_token() {
            self.abandon_output(token);
        }
        self.error = Some(error);
        self.coordinator = None;
    }

    pub(crate) fn render_error(&self, ui: &mut egui::Ui) {
        ui.painter()
            .rect_filled(ui.max_rect(), 0.0, self.style.workspace_fill);
        ui.heading("Native dockspace stopped");
        if let Some(error) = &self.error {
            ui.label(error.to_string());
        } else {
            ui.label("unknown native runtime error");
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
