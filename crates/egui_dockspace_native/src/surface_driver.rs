//! Shared root/child surface driver over one native coordinator.

use dockspace::model::{DockPlacement, DockspaceView, NativeWindowPlacement, RootId, SurfaceId};
use dockspace::runtime::{HostInputOutcome, HostWindowToken, SurfaceUnavailableReason};
use eframe::egui::emath::GuiRounding;
use eframe::egui::{self, Id, ViewportId};
use eframe::{NativeHostWake, NativeOutputToken, queue_native_viewport_pointer_passthrough};
use egui_dockspace::{DockStyle, DockspaceActionStatus, PaneView};

use crate::application_action::{NativeActionRequestError, NativeApplicationActions};
use crate::close_control::NativeWindowClosePolicy;
use crate::coordinator::{NativeCoordinator, NativeShutdownAdvance, NativeShutdownRegistration};
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum NativeSurfaceUpdate {
    Semantic,
    RetainPrevious,
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

fn request_follow_up_root_cycle(
    context: &egui::Context,
    retirement_committed: bool,
    cycle_progressed: bool,
) {
    if retirement_committed || cycle_progressed {
        context.request_repaint_of(ViewportId::ROOT);
    }
}

pub(crate) struct NativeRuntimeState<P> {
    coordinator: NativeCoordinator,
    root_surface: SurfaceId,
    instance_id: Id,
    style: DockStyle,
    panes: P,
    application_actions: NativeApplicationActions,
    pass_actions: NativePassActions,
    close_policy: NativeWindowClosePolicy,
    root_context: Option<egui::Context>,
    shutdown: Option<NativeShutdownState>,
}

#[derive(Debug)]
struct NativeShutdownState {
    primary: NativeRuntimeError,
    cleanup_errors: Vec<NativeRuntimeError>,
}

impl<P: PaneView> NativeRuntimeState<P> {
    pub(crate) fn new(
        instance_id: Id,
        session: dockspace::runtime::DockspaceSession,
        root_surface: SurfaceId,
        panes: P,
        style: DockStyle,
        close_policy: NativeWindowClosePolicy,
    ) -> Result<Self, NativeRuntimeError> {
        if session.view().surface(root_surface).is_none() {
            return Err(NativeHostProtocolError::RootSurfaceUnavailable(root_surface).into());
        }
        let mut coordinator =
            NativeCoordinator::new(session, dockspace::runtime::NativePointerRoster::Unknown)?;
        coordinator.register_native_root(root_surface, ROOT_WINDOW_TOKEN)?;
        Ok(Self {
            coordinator,
            root_surface,
            instance_id,
            style,
            panes,
            application_actions: NativeApplicationActions::default(),
            pass_actions: NativePassActions::default(),
            close_policy,
            root_context: None,
            shutdown: None,
        })
    }

    pub(crate) fn native_host_handler(&self) -> std::sync::Arc<dyn eframe::NativeHostHandler> {
        self.coordinator.native_host_handler()
    }

    pub(crate) const fn root_surface(&self) -> SurfaceId {
        self.root_surface
    }

    pub(crate) fn with_view<R>(&self, inspect: impl FnOnce(DockspaceView<'_>) -> R) -> Option<R> {
        Some(inspect(self.coordinator.session().view()))
    }

    pub(crate) const fn panes(&self) -> &P {
        &self.panes
    }

    pub(crate) const fn panes_mut(&mut self) -> &mut P {
        &mut self.panes
    }

    pub(crate) const fn error(&self) -> Option<&NativeRuntimeError> {
        match &self.shutdown {
            Some(shutdown) => Some(&shutdown.primary),
            None => None,
        }
    }

    pub(crate) fn is_surface_presented(&self, surface: SurfaceId) -> bool {
        self.coordinator
            .session()
            .presented_surface(surface)
            .is_some()
    }

    pub(crate) fn is_quiescent(&self) -> bool {
        self.shutdown.is_none()
            && self
                .coordinator
                .session()
                .presented_surface(self.root_surface)
                .is_some()
            && !self.application_actions.is_occupied()
            && !self.pass_actions.has_pending_work()
            && self.coordinator.is_quiescent()
    }

    pub(crate) fn request_dock_root(
        &mut self,
        root: RootId,
        placement: DockPlacement,
    ) -> Result<(), NativeActionRequestError> {
        if self.shutdown.is_some() {
            return Err(NativeActionRequestError::stopped());
        }
        let action = self
            .coordinator
            .session()
            .prepare_dock_root(root, placement);
        self.application_actions.enqueue(action)?;
        self.request_root_repaint();
        Ok(())
    }

    pub(crate) fn request_dock_root_back(
        &mut self,
        root: RootId,
    ) -> Result<(), NativeActionRequestError> {
        if self.shutdown.is_some() {
            return Err(NativeActionRequestError::stopped());
        }
        let action = self.coordinator.session().prepare_dock_root_back(root);
        self.application_actions.enqueue(action)?;
        self.request_root_repaint();
        Ok(())
    }

    pub(crate) fn request_tear_off_root(
        &mut self,
        root: RootId,
        placement: NativeWindowPlacement,
    ) -> Result<(), NativeActionRequestError> {
        if self.shutdown.is_some() {
            return Err(NativeActionRequestError::stopped());
        }
        let action = self
            .coordinator
            .session()
            .prepare_tear_off_root(root, placement);
        self.application_actions.enqueue(action)?;
        self.request_root_repaint();
        Ok(())
    }

    pub(crate) fn take_action_status(&mut self) -> Option<DockspaceActionStatus> {
        self.application_actions.take_result()
    }

    fn request_root_repaint(&self) {
        if let Some(context) = &self.root_context {
            context.request_repaint_of(ViewportId::ROOT);
        }
    }

    pub(crate) fn deferred_viewport_specs(&self) -> Vec<DeferredViewportSpec> {
        self.coordinator.deferred_viewport_specs()
    }

    pub(crate) fn deferred_viewport_paint(
        &self,
        token: NativeOutputToken,
    ) -> crate::mailbox::DeferredViewportPaint {
        self.coordinator.deferred_viewport_paint(token)
    }

    pub(crate) fn update_surface(
        &mut self,
        ui: &mut egui::Ui,
        surface: SurfaceId,
    ) -> Result<NativeSurfaceUpdate, NativeRuntimeError> {
        let context = ui.ctx().clone();
        if surface == self.root_surface {
            self.root_context = Some(context.clone());
        }
        let token = eframe::current_native_output_token()
            .ok_or(NativeHostProtocolError::OutputTokenUnavailable)?;
        let prelude = {
            let coordinator = &mut self.coordinator;
            (|| {
                let pending_effect_reported = coordinator.try_report_pending_effect_results()?;
                let quiescence_recorded = surface == self.root_surface
                    && coordinator.try_report_retirement_quiescence()?;
                let reduced_callback = coordinator.reduce_callback_head()?;
                let callback_error = coordinator.take_callback_error();
                Ok::<_, NativeRuntimeError>((
                    pending_effect_reported,
                    quiescence_recorded,
                    reduced_callback,
                    callback_error,
                ))
            })()
        };
        let internal_action_settled = self.settle_internal_application_transitions()?;
        let (pending_effect_reported, quiescence_recorded, reduced_callback, callback_error) =
            prelude?;
        if let Some(error) = callback_error {
            return Err(error);
        }
        let coordinator = &mut self.coordinator;
        let prepared_retirements = if surface == self.root_surface {
            coordinator.prepare_committed_retirements()?
        } else {
            None
        };

        let instance_id = self.instance_id.with(surface.get());
        let style = &self.style;
        let panes = &mut self.panes;
        let application_actions = &mut self.application_actions;
        let pass_actions = &mut self.pass_actions;
        let root_surface = self.root_surface;
        let mut host_frame = coordinator.begin_resolved_host_frame(&context)?;
        let retain_previous_output = !host_frame.frame.surfaces().contains(&surface);
        let mut final_paint = None;
        let mut painted_output_expected = false;
        let mut post_action_repaint = false;
        let mut discarded = false;
        let mut submitted_application_action = None;

        let render_result = if retain_previous_output {
            pass_actions.abandon(token);
            host_frame.complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        } else {
            (|| -> Result<(), NativeRuntimeError> {
                let dock_rect = ui.available_rect_before_wrap();
                let popup_rect = context.input(egui::InputState::content_rect).round_ui();
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
                let has_application_action =
                    surface == root_surface && application_actions.has_queued();
                let has_actions = actions.has_actions() || has_application_action;
                let disposition = surface_frame_disposition(
                    paint.had_ready_plan(),
                    paint.transient_visuals_complete(),
                    paint.deferred_measurement(),
                    has_actions,
                );
                if disposition == SurfaceFrameDisposition::RejectIncompletePaint {
                    return Err(NativeHostProtocolError::IncompleteTransientPaint(surface).into());
                }
                if has_application_action {
                    let action = application_actions
                        .take()
                        .expect("a checked application action remains pending");
                    submitted_application_action = Some(host_frame.submit_prepared_action(action)?);
                }
                for action in actions.into_ordered() {
                    host_frame.submit_surface_action(action)?;
                }

                match disposition {
                    SurfaceFrameDisposition::ConfirmPainted => {
                        host_frame.confirm_surface_painted(surface)?;
                        host_frame
                            .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)?;
                        painted_output_expected = true;
                    }
                    SurfaceFrameDisposition::Defer => {
                        host_frame
                            .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)?;
                        post_action_repaint = has_actions;
                    }
                    SurfaceFrameDisposition::Measure => {
                        host_frame
                            .measure_surface(surface, ui, dock_rect, popup_rect, panes, style)?;
                        host_frame
                            .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)?;
                    }
                    SurfaceFrameDisposition::RejectIncompletePaint => unreachable!(
                        "incomplete transient paint is rejected before submitting surface actions"
                    ),
                }
                final_paint = Some(paint);
                Ok(())
            })()
        };
        render_result?;
        if discarded {
            return Ok(NativeSurfaceUpdate::Semantic);
        }

        let mut report = host_frame.commit()?;
        let submitted_application_outcome = submitted_application_action
            .and_then(|submitted| report.submitted_action_outcome(submitted));
        let mut application_action_settled = application_actions
            .settle(
                submitted_application_outcome,
                report.presentation_transitions(),
            )
            .map_err(|()| NativeHostProtocolError::ApplicationActionOutcomeMissing)?;
        let coordinator = &mut self.coordinator;
        let native_snapshot_applied = coordinator.settle_host_frame_inputs(report.inputs());
        let native_close_settled = coordinator.settle_close_control_inputs(report.inputs())?;
        let native_admission_settled =
            coordinator.settle_native_admissions(report.native_admissions())?;
        let retirement_committed = prepared_retirements.is_some();
        if let Some(prepared_retirements) = prepared_retirements {
            for token in coordinator.commit_retirements(prepared_retirements) {
                self.pass_actions.abandon(token);
            }
        }
        if surface == self.root_surface {
            Self::bind_root_registration(coordinator, self.root_surface, token, report.inputs())?;
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
            if let Err(error) = self.coordinator.bind_painted_surface(token, output, paint) {
                let kind = error.kind();
                drop(error.into_output());
                let _ = self.coordinator.abandon_output_token(token);
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
                self.coordinator.abandon_output_token(token),
                NativeHostWake::RepaintRoot
            ) {
                context.request_repaint_of(egui::ViewportId::ROOT);
            }
        }

        let coordinator = &mut self.coordinator;
        let native_effects_emitted = !native_effects.is_empty();
        coordinator.accept_native_effects(native_effects)?;
        let native_close_progress =
            coordinator.drive_close_policy(self.close_policy, self.root_surface);
        let internal_transitions = coordinator.take_internal_presentation_transitions();
        if !internal_transitions.is_empty() {
            application_action_settled |= application_actions
                .settle_presentation_transitions(&internal_transitions)
                .map_err(|()| NativeHostProtocolError::ApplicationActionOutcomeMissing)?;
        }
        let native_close_progress = native_close_progress?;
        for (viewport, command) in coordinator.take_viewport_commands() {
            context.send_viewport_cmd_to(viewport, command);
        }
        if let Some(command) = coordinator.pointer_passthrough_command() {
            let token = queue_native_viewport_pointer_passthrough(
                &context,
                command.viewport(),
                command.enabled(),
            );
            if !coordinator.mark_pointer_passthrough_dispatched(command, token) {
                let removed = coordinator.fail_pointer_passthrough_dispatch(command);
                debug_assert!(removed, "the failed dispatch still owns the queued command");
                return Err(NativeHostProtocolError::NativeInputDispatchChanged.into());
            }
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
        request_follow_up_root_cycle(
            &context,
            retirement_committed,
            pending_effect_reported
                || quiescence_recorded
                || reduced_callback
                || native_snapshot_applied
                || native_admission_settled
                || post_action_repaint
                || internal_action_settled
                || application_action_settled
                || native_effects_emitted
                || native_close_settled
                || native_close_progress,
        );
        Ok(if retain_previous_output {
            NativeSurfaceUpdate::RetainPrevious
        } else {
            NativeSurfaceUpdate::Semantic
        })
    }

    fn bind_root_registration(
        coordinator: &mut NativeCoordinator,
        root_surface: SurfaceId,
        token: NativeOutputToken,
        inputs: &[HostInputOutcome],
    ) -> Result<(), NativeRuntimeError> {
        for input in inputs {
            match input {
                HostInputOutcome::NativeSurfaceRegistered { binding }
                    if binding.surface() == root_surface =>
                {
                    coordinator
                        .bind_viewport(egui::ViewportId::ROOT, token.window_id(), *binding)
                        .map_err(|_| NativeHostProtocolError::RootViewportBindingFailed)?;
                }
                HostInputOutcome::NativeSurfaceRegistrationRejected { surface }
                    if *surface == root_surface =>
                {
                    return Err(NativeHostProtocolError::RootRegistrationRejected(*surface).into());
                }
                _ => {}
            }
        }
        Ok(())
    }

    pub(crate) fn stop_current_output(&mut self, error: NativeRuntimeError) {
        if self.shutdown.is_some() {
            return;
        }
        let _ = self.settle_internal_application_transitions();
        for token in self.coordinator.quarantine_after_fatal() {
            self.pass_actions.abandon(token);
        }
        self.application_actions.abandon_unsettled();
        self.shutdown = Some(NativeShutdownState {
            primary: error,
            cleanup_errors: Vec::new(),
        });
    }

    fn settle_internal_application_transitions(&mut self) -> Result<bool, NativeRuntimeError> {
        let transitions = self.coordinator.take_internal_presentation_transitions();
        if transitions.is_empty() {
            return Ok(false);
        }
        self.application_actions
            .settle_presentation_transitions(&transitions)
            .map_err(|()| NativeHostProtocolError::ApplicationActionOutcomeMissing.into())
    }

    pub(crate) fn advance_shutdown(
        &mut self,
        context: &egui::Context,
    ) -> (bool, Vec<NativeRuntimeError>) {
        debug_assert!(
            self.shutdown.is_some(),
            "shutdown advance requires a primary failure"
        );
        let advance = self.coordinator.advance_shutdown_boundary();
        self.apply_shutdown_advance(context, advance)
    }

    fn apply_shutdown_advance(
        &mut self,
        context: &egui::Context,
        advance: NativeShutdownAdvance,
    ) -> (bool, Vec<NativeRuntimeError>) {
        let root_window = eframe::current_native_output_token()
            .filter(|token| token.viewport_id() == egui::ViewportId::ROOT)
            .map(NativeOutputToken::window_id);
        self.apply_shutdown_advance_for_root_window(context, advance, root_window)
    }

    fn apply_shutdown_advance_for_root_window(
        &mut self,
        context: &egui::Context,
        advance: NativeShutdownAdvance,
        root_window: Option<winit::window::WindowId>,
    ) -> (bool, Vec<NativeRuntimeError>) {
        let NativeShutdownAdvance {
            progress,
            registrations,
            commands,
            pointer_passthrough,
            abandoned_outputs,
            mut errors,
        } = advance;
        for token in abandoned_outputs {
            self.pass_actions.abandon(token);
        }
        for (viewport, command) in commands {
            context.send_viewport_cmd_to(viewport, command);
        }
        if let Some(command) = pointer_passthrough {
            let token = queue_native_viewport_pointer_passthrough(
                context,
                command.viewport(),
                command.enabled(),
            );
            if !self
                .coordinator
                .mark_pointer_passthrough_dispatched(command, token)
            {
                let removed = self.coordinator.fail_pointer_passthrough_dispatch(command);
                debug_assert!(removed, "the failed dispatch still owns the queued command");
                errors.push(NativeHostProtocolError::NativeInputDispatchChanged.into());
            }
        }
        if let Some(root_window) = root_window {
            for registration in registrations {
                match registration {
                    NativeShutdownRegistration::Registered(binding)
                        if binding.surface() == self.root_surface =>
                    {
                        if self
                            .coordinator
                            .bind_viewport(egui::ViewportId::ROOT, root_window, binding)
                            .is_err()
                        {
                            errors.push(NativeHostProtocolError::RootViewportBindingFailed.into());
                        }
                    }
                    NativeShutdownRegistration::Rejected(surface)
                        if surface == self.root_surface =>
                    {
                        errors.push(
                            NativeHostProtocolError::RootRegistrationRejected(surface).into(),
                        );
                    }
                    NativeShutdownRegistration::Registered(_)
                    | NativeShutdownRegistration::Rejected(_) => {}
                }
            }
        }
        (progress, errors)
    }

    pub(crate) fn record_cleanup_error(&mut self, error: NativeRuntimeError) {
        let Some(shutdown) = &mut self.shutdown else {
            self.stop_current_output(error);
            return;
        };
        shutdown.cleanup_errors.push(error);
    }

    pub(crate) fn render_error(&self, ui: &mut egui::Ui) {
        ui.painter()
            .rect_filled(ui.max_rect(), 0.0, ui.visuals().panel_fill);
        ui.heading("Native dockspace stopped");
        if let Some(shutdown) = &self.shutdown {
            ui.label(shutdown.primary.to_string());
            if !shutdown.cleanup_errors.is_empty() {
                ui.label(format!(
                    "{} cleanup error(s) retained",
                    shutdown.cleanup_errors.len()
                ));
            }
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
    use std::error::Error as _;

    use dockspace::model::{
        DockAnchor, DockPlacement, DockspaceLayout, DockspaceNode, DockspaceRootLayout,
        DockspaceSurfaceLayout, ItemId, RootId,
    };
    use eframe::egui::{Ui, WidgetText};

    use super::*;

    const SURFACE: SurfaceId = SurfaceId::new(1);

    struct TestPanes;

    impl PaneView for TestPanes {
        fn title(&self, _item: ItemId) -> Option<WidgetText> {
            Some("Pane".into())
        }

        fn ui(&mut self, _item: ItemId, _ui: &mut Ui) {}
    }

    fn test_state() -> NativeRuntimeState<TestPanes> {
        let layout = DockspaceLayout::new([DockspaceSurfaceLayout::new(
            SURFACE,
            DockspaceRootLayout::new(
                RootId::new(1),
                DockspaceNode::central_tabs([ItemId::new(1)]),
            ),
        )])
        .expect("the native runtime test layout validates");
        let session = dockspace::runtime::DockspaceSession::from_layout(
            layout,
            dockspace::policy::DockPolicy::default(),
        )
        .expect("the native runtime test session initializes");
        NativeRuntimeState::new(
            Id::new("native-runtime-stop-test"),
            session,
            SURFACE,
            TestPanes,
            DockStyle::default(),
            NativeWindowClosePolicy::Cancel,
        )
        .expect("the native runtime test state initializes")
    }

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

    #[test]
    fn committed_route_retirement_requests_a_follow_up_root_cycle() {
        let context = egui::Context::default();
        for _ in 0..8 {
            if !context.has_requested_repaint_for(&ViewportId::ROOT) {
                break;
            }
            context.begin_pass(egui::RawInput::default());
            let mut output = context.end_pass();
            output.textures_delta.clear();
        }
        assert!(!context.has_requested_repaint_for(&ViewportId::ROOT));

        request_follow_up_root_cycle(&context, false, false);
        assert!(!context.has_requested_repaint_for(&ViewportId::ROOT));

        request_follow_up_root_cycle(&context, true, false);

        assert!(context.has_requested_repaint_for(&ViewportId::ROOT));
    }

    #[test]
    fn programmatic_action_queue_is_bounded_and_stops_with_the_runtime() {
        let mut state = test_state();
        let placement = DockPlacement::Center(DockAnchor::Item(ItemId::new(1)));

        state
            .request_dock_root(RootId::new(1), placement)
            .expect("the empty application-action slot accepts one request");
        assert_eq!(
            state
                .request_dock_root(RootId::new(1), placement)
                .expect_err("a second action cannot overtake the pending request"),
            crate::NativeActionRequestError::Busy
        );

        let action = state
            .application_actions
            .take()
            .expect("the final root pass takes the pending action");
        assert_eq!(
            action.expected_version(),
            state.coordinator.session().version()
        );
        state
            .application_actions
            .settle(
                Some(&HostInputOutcome::ProductActionRejected(
                    dockspace::model::DockspaceActionRejection::PolicyDenied,
                )),
                &[],
            )
            .expect("the exact product result settles the action");
        assert!(!state.application_actions.has_queued());
        assert!(state.application_actions.is_occupied());
        assert!(matches!(
            state.take_action_status(),
            Some(DockspaceActionStatus::Rejected(
                dockspace::model::DockspaceActionRejection::PolicyDenied
            ))
        ));

        state
            .request_dock_root(RootId::new(1), placement)
            .expect("consuming the result releases the bounded slot");

        state.stop_current_output(NativeHostProtocolError::OutputTokenUnavailable.into());
        assert!(!state.application_actions.is_occupied());
        assert_eq!(
            state
                .request_dock_root(RootId::new(1), placement)
                .expect_err("a stopped runtime rejects new application actions"),
            crate::NativeActionRequestError::Stopped
        );
    }

    #[test]
    fn presentation_gated_action_rejects_an_uncorrelated_legacy_outcome() {
        let mut state = test_state();
        let placement = DockPlacement::Center(DockAnchor::Item(ItemId::new(1)));
        state
            .request_dock_root(RootId::new(1), placement)
            .expect("the presentation action is queued");
        let _action = state
            .application_actions
            .take()
            .expect("the final root pass takes the pending action");

        assert!(
            state
                .application_actions
                .settle(
                    Some(&HostInputOutcome::ProductActionApplied(
                        dockspace::model::DockspaceActionOutcome::RootDockRequested {
                            root: RootId::new(1),
                            source_surface: SURFACE,
                            target_root: RootId::new(1),
                            items: vec![ItemId::new(1)],
                        },
                    )),
                    &[],
                )
                .is_err()
        );
        assert!(state.application_actions.is_occupied());
        assert!(state.take_action_status().is_none());
    }

    #[test]
    fn fatal_stop_retains_the_coordinator_and_first_error() {
        let mut state = test_state();

        state.stop_current_output(NativeHostProtocolError::OutputTokenUnavailable.into());
        let first_error = state
            .error()
            .expect("fatal stop records its primary error")
            .to_string();
        state
            .stop_current_output(NativeHostProtocolError::RootRegistrationRejected(SURFACE).into());

        assert_eq!(
            state
                .error()
                .expect("later failures do not replace the primary error")
                .to_string(),
            first_error
        );
        assert!(state.with_view(|_| ()).is_some());
    }

    #[test]
    fn fatal_stop_preserves_a_committed_application_action_result() {
        let mut state = test_state();
        let placement = DockPlacement::Center(DockAnchor::Item(ItemId::new(1)));
        state
            .request_dock_root(RootId::new(1), placement)
            .expect("the application action is queued");
        let _action = state
            .application_actions
            .take()
            .expect("the final root pass takes the pending action");
        state
            .application_actions
            .settle(
                Some(&HostInputOutcome::ProductActionRejected(
                    dockspace::model::DockspaceActionRejection::PolicyDenied,
                )),
                &[],
            )
            .expect("the exact product result settles the action");

        state.stop_current_output(NativeHostProtocolError::OutputTokenUnavailable.into());

        assert!(matches!(
            state.take_action_status(),
            Some(DockspaceActionStatus::Rejected(
                dockspace::model::DockspaceActionRejection::PolicyDenied
            ))
        ));
        assert!(!state.application_actions.is_occupied());
    }

    #[test]
    fn shutdown_retains_cleanup_errors_in_observation_order() {
        let mut state = test_state();
        state.stop_current_output(NativeHostProtocolError::OutputTokenUnavailable.into());
        state.record_cleanup_error(
            NativeHostProtocolError::RootRegistrationRejected(SURFACE).into(),
        );
        state.record_cleanup_error(
            NativeHostProtocolError::IncompleteTransientPaint(SURFACE).into(),
        );

        let shutdown = state
            .shutdown
            .as_ref()
            .expect("fatal stop owns one shutdown record");
        assert_eq!(shutdown.cleanup_errors.len(), 2);
        assert!(
            shutdown.cleanup_errors[0]
                .source()
                .expect("cleanup error retains its source")
                .to_string()
                .contains("registration")
        );
        assert!(
            shutdown.cleanup_errors[1]
                .source()
                .expect("cleanup error retains its source")
                .to_string()
                .contains("transient")
        );
        assert!(
            shutdown
                .primary
                .source()
                .expect("primary error retains its source")
                .to_string()
                .contains("output callback")
        );
    }

    #[test]
    fn shutdown_applies_committed_commands_before_registration_errors() {
        let mut state = test_state();
        let context = egui::Context::default();
        context.begin_pass(egui::RawInput::default());

        let (progress, errors) = state.apply_shutdown_advance_for_root_window(
            &context,
            NativeShutdownAdvance {
                progress: true,
                registrations: vec![NativeShutdownRegistration::Rejected(SURFACE)],
                commands: vec![(egui::ViewportId::ROOT, egui::ViewportCommand::Close)],
                pointer_passthrough: None,
                abandoned_outputs: Vec::new(),
                errors: vec![NativeHostProtocolError::IncompleteTransientPaint(SURFACE).into()],
            },
            Some(winit::window::WindowId::from(7)),
        );
        assert!(progress);
        let [committed_error, registration_error] = errors.as_slice() else {
            panic!("committed and adapter cleanup errors remain ordered")
        };
        assert!(
            committed_error
                .source()
                .expect("the committed cleanup error retains its source")
                .to_string()
                .contains("transient")
        );
        assert!(
            registration_error
                .source()
                .expect("the adapter cleanup error retains its source")
                .to_string()
                .contains("registration")
        );

        let mut output = context.end_pass();
        let close_was_queued = output
            .viewport_output
            .get(&egui::ViewportId::ROOT)
            .expect("the committed root command remains in egui output")
            .commands
            .contains(&egui::ViewportCommand::Close);
        output.textures_delta.clear();
        assert!(close_was_queued);
    }
}
