//! Shared root/child surface driver over one native coordinator.

use dockspace::model::{DockPlacement, DockspaceView, NativeWindowPlacement, RootId, SurfaceId};
use dockspace::runtime::{
    DockspaceSubmittedAction, HostFrameReport, HostInputOutcome, HostWindowToken,
    NativeInputBudget, PreparedPaneFocusObservation, PreparedSurfaceAction,
    SurfaceUnavailableReason,
};
use eframe::egui::emath::GuiRounding;
use eframe::egui::{self, Id, ViewportId};
use eframe::{NativeHostWake, NativeOutputToken};
#[cfg(test)]
use egui_dockspace::DockspaceActionStatus;
use egui_dockspace::{DockStyle, PaneView};

use crate::application_action::{
    NativeActionRequestError, NativeActionTerminal, NativeApplicationActions,
};
use crate::close_control::NativeWindowClosePolicy;
#[cfg(test)]
use crate::coordinator::split_no_frame_invalidations_for_frame;
use crate::coordinator::{NativeCoordinator, NativeShutdownAdvance, NativeShutdownRegistration};
use crate::deferred_viewport::DeferredViewportSpec;
#[cfg(test)]
use crate::error::NativeRuntimeErrorKind;
use crate::error::{NativeHostProtocolError, NativeRuntimeError};
use crate::lifecycle_progress::NativeCloseProgress;
use crate::pass_actions::{NativeFinalPass, NativePassActionError, NativePassActions};

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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum NativeErrorPresentation {
    NoFrame,
    RetainedOverlay,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RetainedOutputKind {
    Skip,
    Overlay,
}

impl RetainedOutputKind {
    const fn omits_accessibility_when_retained(self) -> bool {
        matches!(self, Self::Skip)
    }

    fn request(self) -> bool {
        match self {
            Self::Skip => eframe::retain_current_native_output(),
            Self::Overlay => eframe::retain_current_native_output_with_overlay(),
        }
    }
}

fn finalize_retained_accessibility(
    output: &mut egui::FullOutput,
    kind: RetainedOutputKind,
    retained: bool,
) {
    if retained && kind.omits_accessibility_when_retained() {
        // A skipped renderer pass keeps the exact previous framebuffer and
        // its matching accessibility tree. An overlay is different: its
        // inert status label is visible now and must remain accessible.
        output.platform_output.accesskit_update = None;
    }
}

fn native_error_paint_rect(
    available: egui::Rect,
    presentation: NativeErrorPresentation,
    status_height: f32,
) -> egui::Rect {
    match presentation {
        NativeErrorPresentation::NoFrame => available,
        NativeErrorPresentation::RetainedOverlay => egui::Rect::from_min_max(
            egui::pos2(
                available.left(),
                (available.bottom() - status_height).max(available.top()),
            ),
            available.right_bottom(),
        ),
    }
}

fn paint_native_error_status(
    ui: &mut egui::Ui,
    presentation: NativeErrorPresentation,
    message: &str,
) {
    let available = ui.max_rect();
    let status_height = ui.spacing().interact_size.y.max(30.0);
    let paint_rect = native_error_paint_rect(available, presentation, status_height);
    let (fill, text_color, layout) = match presentation {
        NativeErrorPresentation::NoFrame => (
            ui.visuals().panel_fill,
            ui.visuals().error_fg_color,
            egui::Layout::centered_and_justified(egui::Direction::LeftToRight),
        ),
        NativeErrorPresentation::RetainedOverlay => (
            ui.visuals().extreme_bg_color,
            ui.visuals().warn_fg_color,
            egui::Layout::left_to_right(egui::Align::Center),
        ),
    };
    ui.painter().rect_filled(paint_rect, 0.0, fill);
    ui.disable();

    let _ = ui.scope_builder(
        egui::UiBuilder::new().max_rect(paint_rect).layout(layout),
        |ui| {
            if presentation == NativeErrorPresentation::RetainedOverlay {
                ui.add_space(8.0);
            }
            ui.add(
                egui::Label::new(
                    egui::RichText::new(message)
                        .font(egui::FontId::proportional(13.0))
                        .color(text_color),
                )
                .selectable(false)
                .sense(egui::Sense::hover())
                .show_tooltip_when_elided(false)
                .truncate(),
            );
        },
    );
}

const fn surface_frame_disposition(
    had_ready_plan: bool,
    transient_visuals_complete: bool,
    deferred_measurement: bool,
    has_actions: bool,
) -> SurfaceFrameDisposition {
    if had_ready_plan && !transient_visuals_complete {
        SurfaceFrameDisposition::RejectIncompletePaint
    } else if had_ready_plan && !has_actions {
        // A preview defers feeding geometry back into measurement, not the
        // completed renderer receipt required to settle its pending release.
        SurfaceFrameDisposition::ConfirmPainted
    } else if has_actions || deferred_measurement {
        SurfaceFrameDisposition::Defer
    } else {
        SurfaceFrameDisposition::Measure
    }
}

const fn should_retain_previous_output(
    output_prefix_pending: bool,
    callback_reduced: bool,
) -> bool {
    output_prefix_pending || callback_reduced
}

const fn retained_pass_requires_semantic_retry(
    retain_previous_output: bool,
    surface_is_presented: bool,
    application_action_queued: bool,
) -> bool {
    retain_previous_output && (!surface_is_presented || application_action_queued)
}

#[cfg(test)]
fn select_no_frame_invalidations_for_frame<T>(
    context: &egui::Context,
    frame_surfaces: &[SurfaceId],
    ready: impl IntoIterator<Item = T>,
    surface: impl Fn(&T) -> SurfaceId,
    viewport: impl Fn(&T) -> ViewportId,
) -> Vec<T> {
    let (accepted, repaint_viewports) =
        split_no_frame_invalidations_for_frame(frame_surfaces, ready, surface, viewport);
    for viewport in repaint_viewports {
        context.request_repaint_once_of(viewport);
    }
    accepted
}

fn request_follow_up_root_cycle(
    context: &egui::Context,
    retirement_committed: bool,
    cycle_progressed: bool,
) {
    if retirement_committed || cycle_progressed {
        context.request_repaint_once_of(ViewportId::ROOT);
    }
}

fn require_durable_root_cycle(
    coordinator: &NativeCoordinator,
    token: NativeOutputToken,
    durable_progress: bool,
) -> Result<(), NativeRuntimeError> {
    if durable_progress && !coordinator.require_output_wake_after_current(token) {
        return Err(NativeHostProtocolError::OutputRouteAttachmentFailed.into());
    }
    Ok(())
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
    #[cfg(test)]
    pub(crate) fn new(
        instance_id: Id,
        session: dockspace::runtime::DockspaceSession,
        root_surface: SurfaceId,
        panes: P,
        style: DockStyle,
        close_policy: NativeWindowClosePolicy,
    ) -> Result<Self, NativeRuntimeError> {
        Self::new_with_storage_limit(
            instance_id,
            session,
            root_surface,
            panes,
            style,
            close_policy,
            None,
        )
    }

    pub(crate) fn new_with_storage_limit(
        instance_id: Id,
        session: dockspace::runtime::DockspaceSession,
        root_surface: SurfaceId,
        panes: P,
        style: DockStyle,
        close_policy: NativeWindowClosePolicy,
        storage: Option<crate::NativeStorageLimit>,
    ) -> Result<Self, NativeRuntimeError> {
        if session.view().surface(root_surface).is_none() {
            return Err(NativeHostProtocolError::RootSurfaceUnavailable(root_surface).into());
        }
        let mut coordinator = match storage {
            Some(limit) => NativeCoordinator::new_with_combined_storage_limit(
                session,
                dockspace::runtime::NativePointerRoster::Unknown,
                limit.combined,
            )?,
            None => {
                NativeCoordinator::new(session, dockspace::runtime::NativePointerRoster::Unknown)?
            }
        };
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

    pub(crate) fn input_storage_reserved_bytes(&self) -> Option<usize> {
        self.coordinator.combined_storage_reserved_bytes()
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

    pub(crate) fn close_progress(&self) -> NativeCloseProgress {
        self.coordinator.close_progress()
    }

    pub(crate) fn owned_input_capacity_bytes(&self) -> Result<usize, NativeRuntimeError> {
        self.coordinator.owned_input_capacity_bytes()
    }

    pub(crate) fn is_quiescent(&self) -> bool {
        committed_boundary_is_quiescent(
            self.shutdown.is_some(),
            self.coordinator
                .session()
                .presented_surface(self.root_surface)
                .is_some(),
            self.application_actions.is_occupied(),
            self.pass_actions.has_retained_action_work(),
            self.coordinator.is_quiescent(),
        )
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

    pub(crate) fn request_move_root_to_new_window(
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
            .prepare_move_root_to_new_window(root, placement);
        self.application_actions.enqueue(action)?;
        self.request_root_repaint();
        Ok(())
    }

    pub(crate) fn take_action_status(&mut self) -> Option<NativeActionTerminal> {
        self.application_actions.take_result()
    }

    fn request_root_repaint(&self) {
        if let Some(token) = eframe::current_native_output_token() {
            let armed = self.coordinator.require_output_wake_after_current(token);
            if armed {
                return;
            }
        }
        if let Some(context) = &self.root_context {
            context.request_repaint_of(ViewportId::ROOT);
        }
    }

    pub(crate) fn prepare_deferred_viewport_specs(
        &self,
    ) -> Result<Vec<DeferredViewportSpec>, NativeRuntimeError> {
        self.coordinator.prepare_deferred_viewport_specs()
    }

    pub(crate) fn deferred_viewport_paint(
        &self,
        token: NativeOutputToken,
    ) -> crate::mailbox::DeferredViewportPaint {
        self.coordinator.deferred_viewport_paint(token)
    }

    pub(crate) fn stage_retain_previous(
        &mut self,
        token: NativeOutputToken,
        surface: SurfaceId,
    ) -> Result<(), NativeRuntimeError> {
        self.pass_actions
            .stage_retain_previous(token, surface)
            .map_err(map_pass_action_error)
    }

    pub(crate) fn stage_retained_overlay(
        &mut self,
        token: NativeOutputToken,
    ) -> Result<(), NativeRuntimeError> {
        self.pass_actions
            .stage_retained_overlay(token)
            .map_err(map_pass_action_error)
    }

    pub(crate) fn update_surface(
        &mut self,
        ui: &mut egui::Ui,
        surface: SurfaceId,
    ) -> Result<NativeSurfaceUpdate, NativeRuntimeError> {
        let token = eframe::current_native_output_token()
            .ok_or(NativeHostProtocolError::OutputTokenUnavailable)?;
        let bridge = self.coordinator.dispatch_bridge();
        let mut budget = bridge
            .dispatch_budget(token)
            .ok_or(NativeHostProtocolError::OutputBudgetUnavailable)?;
        self.update_surface_budgeted(ui, surface, &mut budget)
    }

    fn update_surface_budgeted(
        &mut self,
        ui: &mut egui::Ui,
        surface: SurfaceId,
        budget: &mut NativeInputBudget,
    ) -> Result<NativeSurfaceUpdate, NativeRuntimeError> {
        let context = ui.ctx().clone();
        if surface == self.root_surface {
            self.root_context = Some(context.clone());
        }
        let token = eframe::current_native_output_token()
            .ok_or(NativeHostProtocolError::OutputTokenUnavailable)?;
        self.abandon_unfinalized_output_passes(&context)?;
        let prelude = {
            let coordinator = &mut self.coordinator;
            (|| {
                let output_prefix_pending = coordinator.has_pending_output_prefix();
                let pending_effect_reported = coordinator.try_report_pending_effect_results()?;
                let quiescence_recorded = surface == self.root_surface
                    && coordinator.try_report_retirement_quiescence()?;
                let reduced_callback = coordinator
                    .reduce_callback_prefix(&context, budget)?
                    .is_some();
                let callback_error = coordinator.take_callback_error();
                Ok::<_, NativeRuntimeError>((
                    output_prefix_pending,
                    pending_effect_reported,
                    quiescence_recorded,
                    reduced_callback,
                    callback_error,
                ))
            })()
        };
        let internal_action_settled = self.settle_internal_application_transitions()?;
        let (
            output_prefix_pending,
            pending_effect_reported,
            quiescence_recorded,
            reduced_callback,
            callback_error,
        ) = prelude?;
        if let Some(error) = callback_error {
            return Err(error);
        }
        let coordinator = &mut self.coordinator;
        let prepared_retirements = if surface == self.root_surface {
            coordinator.prepare_committed_retirements()?
        } else {
            None
        };
        let mut no_frame_invalidations = coordinator.prepare_no_frame_invalidations()?;
        let no_frame_invalidation_pending = coordinator.has_pending_no_frame_invalidation(surface);

        let instance_id = self.instance_id.with(surface.get());
        let style = &self.style;
        let panes = &mut self.panes;
        let pass_actions = &mut self.pass_actions;
        let root_surface = self.root_surface;
        let bridge = coordinator.dispatch_bridge();
        let control_output = bridge.is_control_output(token);
        let surface_is_presented = coordinator.session().presented_surface(surface).is_some();
        let Some(mut host_frame) = coordinator.poll_resolved_host_frame(&context, budget)? else {
            if surface != root_surface {
                bridge.block_semantic_viewport(token.viewport_id());
            }
            pass_actions
                .stage_retain_previous(token, surface)
                .map_err(map_pass_action_error)?;
            require_durable_root_cycle(coordinator, token, true)?;
            context.request_repaint_once_of(ViewportId::ROOT);
            context.request_repaint_once_of(token.viewport_id());
            return Ok(NativeSurfaceUpdate::RetainPrevious);
        };
        let native_input_pending = host_frame.native_input_pending();
        let prepared_retirements = if native_input_pending {
            None
        } else {
            prepared_retirements
        };
        let frame_surfaces = host_frame.frame.surfaces();
        no_frame_invalidations.select_for_surfaces(&frame_surfaces, true);
        for viewport in &no_frame_invalidations.repaint_viewports {
            context.request_repaint_once_of(*viewport);
        }
        let no_frame_invalidations = no_frame_invalidations.ready;
        // A renderer terminal callback must commit before any later callback,
        // but confirming another painted output in that settlement frame
        // creates an unbounded Presented -> repaint -> Presented loop. Retain
        // the existing framebuffer for this exact pass. If the settlement
        // changes product authority, the committed report requests the next
        // semantic frame explicitly.
        let retain_previous_output = control_output
            || native_input_pending
            || should_retain_previous_output(output_prefix_pending, reduced_callback)
            || no_frame_invalidation_pending
            || !no_frame_invalidations.is_empty();
        if surface != root_surface
            && (native_input_pending
                || retained_pass_requires_semantic_retry(
                    retain_previous_output,
                    surface_is_presented,
                    false,
                ))
        {
            // Claiming a semantic child pass consumes its wake. A retained
            // prefix must carry that paint obligation through the final commit.
            bridge.block_semantic_viewport(token.viewport_id());
        }
        let mut pass_draft = None;
        let mut painted_output_expected = false;

        let render_result = if retain_previous_output {
            for invalidation in no_frame_invalidations
                .iter()
                .filter(|_| !native_input_pending)
            {
                host_frame.defer_surface(
                    invalidation.surface(),
                    SurfaceUnavailableReason::SurfaceUnavailable,
                )?;
            }
            host_frame.complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        } else {
            (|| -> Result<(), NativeRuntimeError> {
                let dock_rect = ui.available_rect_before_wrap();
                let popup_rect = context.input(egui::InputState::content_rect).round_ui();
                let paint = host_frame.paint_surface(instance_id, surface, ui, panes, style)?;
                let application_action_ready =
                    surface == root_surface && self.application_actions.has_queued();
                let mut draft = pass_actions
                    .prepare_pass(token, paint, application_action_ready)
                    .map_err(map_pass_action_error)?;
                let has_actions = pass_actions.pass_has_actions(&draft);
                // A frame that can mutate workspace or interaction authority
                // cannot also publish paint captured against the pre-action
                // workspace. Its terminal action frame schedules a fresh paint.
                let disposition = surface_frame_disposition(
                    draft.had_ready_plan(),
                    draft.transient_visuals_complete(),
                    draft.deferred_measurement(),
                    has_actions,
                );
                if disposition == SurfaceFrameDisposition::RejectIncompletePaint {
                    return Err(NativeHostProtocolError::IncompleteTransientPaint(surface).into());
                }

                match disposition {
                    SurfaceFrameDisposition::ConfirmPainted => {
                        if !draft.paint_mut().confirm_into(&mut host_frame.frame)? {
                            return Err(
                                NativeHostProtocolError::IncompleteTransientPaint(surface).into()
                            );
                        }
                        host_frame
                            .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)?;
                        painted_output_expected = true;
                    }
                    SurfaceFrameDisposition::Defer => {
                        host_frame
                            .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)?;
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
                pass_draft = Some(draft);
                Ok(())
            })()
        };
        render_result?;

        let mut report = host_frame.commit()?;
        if !native_input_pending {
            coordinator.finish_no_frame_invalidations(&no_frame_invalidations);
        }
        let mut application_action_settled = self
            .application_actions
            .settle(None, report.presentation_transitions())
            .map_err(|()| NativeHostProtocolError::ApplicationActionOutcomeMissing)?;
        let coordinator = &mut self.coordinator;
        let native_snapshot_applied = coordinator.settle_host_frame_report(&report)?;
        let native_close_settled = coordinator.settle_close_control_report(&report)?;
        let native_admission_settled =
            coordinator.settle_native_admissions(report.native_admissions())?;
        let retirement_committed = prepared_retirements.is_some();
        if let Some(prepared_retirements) = prepared_retirements {
            let mut first_error = None;
            for token in coordinator.commit_retirements(prepared_retirements) {
                if let Err(error) = self.pass_actions.abandon(token) {
                    first_error.get_or_insert_with(|| map_pass_action_error(error));
                }
            }
            if let Some(error) = first_error {
                return Err(error);
            }
        }
        if surface == self.root_surface {
            Self::bind_root_registration(coordinator, self.root_surface, token, report.inputs())?;
        }
        let native_effects = report.take_native_effects();
        let mut outputs = report.take_painted_outputs();
        let mut painted_output = if painted_output_expected {
            if outputs.len() != 1 {
                let actual = outputs.len();
                drop(outputs);
                return Err(NativeHostProtocolError::PaintedOutputCountMismatch {
                    expected: 1,
                    actual,
                }
                .into());
            }
            outputs.pop()
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
            None
        };
        let coordinator = &mut self.coordinator;
        let native_effects_emitted = !native_effects.is_empty();
        coordinator.accept_native_effects(&context, native_effects)?;
        let native_close_progress =
            coordinator.drive_close_policy(&context, self.close_policy, self.root_surface, budget);
        let internal_transitions = coordinator.take_internal_presentation_transitions();
        if !internal_transitions.is_empty() {
            application_action_settled |= self
                .application_actions
                .settle_presentation_transitions(&internal_transitions)
                .map_err(|()| NativeHostProtocolError::ApplicationActionOutcomeMissing)?;
        }
        let native_close_progress = native_close_progress?;
        if native_close_progress {
            // Close policy commits a later core frame than the one that
            // produced this paint. Dropping the old affine output forces the
            // follow-up cycle to publish semantics from the new workspace.
            painted_output = None;
        }
        let post_output_quiescence_recorded = surface == self.root_surface
            && output_prefix_pending
            && coordinator.try_report_retirement_quiescence()?;
        if retain_previous_output {
            // Eframe retains the previous visual immediately, but the newly
            // announced output token is not abandoned until this egui pass is
            // known to be terminal.
            self.pass_actions
                .stage_retain_previous(token, surface)
                .map_err(map_pass_action_error)?;
        } else {
            self.pass_actions
                .stage_pass(
                    token,
                    pass_draft.expect("a semantic surface owns pass paint metadata"),
                    painted_output,
                )
                .map_err(map_pass_action_error)?;
        }
        coordinator.dispatch_viewport_commands(&context)?;
        if let Some(command) = coordinator.pointer_passthrough_command() {
            if !coordinator.dispatch_pointer_passthrough(&context, command) {
                let removed = coordinator.fail_pointer_passthrough_dispatch(command);
                if !removed {
                    return Err(NativeHostProtocolError::NativeInputDispatchChanged.into());
                }
                context.request_repaint_once_of(egui::ViewportId::ROOT);
            }
        }
        for repaint_surface in report.repaint_surfaces() {
            match coordinator.repaint_viewport(*repaint_surface) {
                Some(egui::ViewportId::ROOT) | None if *repaint_surface == self.root_surface => {
                    context.request_repaint_once_of(egui::ViewportId::ROOT);
                }
                Some(viewport) => context.request_repaint_once_of(viewport),
                None => {}
            }
        }
        let pending_callback_records = coordinator.has_pending_callback_records();
        request_follow_up_root_cycle(
            &context,
            retirement_committed,
            pending_effect_reported
                || quiescence_recorded
                || reduced_callback
                || native_snapshot_applied
                || native_admission_settled
                || internal_action_settled
                || application_action_settled
                || native_effects_emitted
                || native_close_settled
                || native_close_progress
                || post_output_quiescence_recorded
                || native_input_pending
                || pending_callback_records,
        );
        let semantic_retry_pending = retained_pass_requires_semantic_retry(
            retain_previous_output,
            coordinator.session().presented_surface(surface).is_some(),
            self.application_actions.has_queued(),
        );
        let durable_armed = internal_action_settled
            || native_input_pending
            || application_action_settled
            || native_admission_settled
            || semantic_retry_pending;
        require_durable_root_cycle(coordinator, token, durable_armed)?;
        Ok(if retain_previous_output {
            NativeSurfaceUpdate::RetainPrevious
        } else {
            NativeSurfaceUpdate::Semantic
        })
    }

    fn abandon_unfinalized_output_passes(
        &mut self,
        context: &egui::Context,
    ) -> Result<(), NativeRuntimeError> {
        let mut prefix = self.coordinator.unfinalized_output_prefix();
        while let Some(token) = self
            .coordinator
            .next_unfinalized_not_presented_output(&mut prefix)?
        {
            self.pass_actions
                .abandon(token)
                .map_err(map_pass_action_error)?;
            let wake = self.coordinator.abandon_output_token(token);
            debug_assert_eq!(
                wake,
                NativeHostWake::RepaintRoot,
                "an unattached NotPresented terminal retains one exact reservation",
            );
            // The terminal root wake lets the coordinator consume its journal.
            // A discarded child pass may also own durable local actions, so
            // explicitly reschedule the viewport which produced that token.
            if wake == NativeHostWake::RepaintRoot {
                context.request_repaint_once_of(egui::ViewportId::ROOT);
            }
            context.request_repaint_once_of(token.viewport_id());
        }
        Ok(())
    }

    pub(crate) fn settle_egui_pass(
        &mut self,
        context: &egui::Context,
        output: &mut egui::FullOutput,
        token: NativeOutputToken,
        discarded: bool,
    ) -> Result<(), NativeRuntimeError> {
        let bridge = self.coordinator.dispatch_bridge();
        let mut budget = bridge
            .dispatch_budget(token)
            .ok_or(NativeHostProtocolError::OutputBudgetUnavailable)?;
        self.settle_egui_pass_budgeted(context, output, token, discarded, &mut budget)
    }

    fn settle_egui_pass_budgeted(
        &mut self,
        context: &egui::Context,
        output: &mut egui::FullOutput,
        token: NativeOutputToken,
        discarded: bool,
        budget: &mut NativeInputBudget,
    ) -> Result<(), NativeRuntimeError> {
        if !self.pass_actions.has_staged(token) {
            return Ok(());
        }
        if discarded {
            self.pass_actions
                .discard_pass(token)
                .map_err(map_pass_action_error)?;
            return Ok(());
        }

        let final_pass = self
            .pass_actions
            .finish_pass(token)
            .map_err(map_pass_action_error)?
            .expect("a checked staged native pass remains available");
        let final_pass = match final_pass {
            NativeFinalPass::RetainPrevious {
                surface,
                local_actions_ready,
            } => {
                let control_output = self.coordinator.dispatch_bridge().is_control_output(token);
                let application_action_ready = !control_output
                    && surface == self.root_surface
                    && self.application_actions.has_queued();
                if !control_output && (local_actions_ready || application_action_ready) {
                    self.commit_terminal_actions(
                        context,
                        token,
                        surface,
                        Vec::new(),
                        None,
                        local_actions_ready,
                        application_action_ready,
                        budget,
                    )?;
                }
                self.settle_retained_egui_output(context, output, token, RetainedOutputKind::Skip)?;
                return Ok(());
            }
            NativeFinalPass::RetainedOverlay => {
                self.settle_retained_egui_output(
                    context,
                    output,
                    token,
                    RetainedOutputKind::Overlay,
                )?;
                return Ok(());
            }
            NativeFinalPass::Semantic(final_pass) => final_pass,
        };
        let surface = final_pass.paint_surface();
        let has_application_action = final_pass.application_action_ready();
        let has_actions = final_pass.has_actions() || has_application_action;
        let (
            presentation_actions,
            pane_focus_observation,
            local_actions,
            application_action_ready,
            paint,
            painted_output,
        ) = final_pass.into_parts();
        debug_assert_eq!(has_application_action, application_action_ready);
        let local_actions_ready = self
            .pass_actions
            .retain_local_actions(surface, local_actions)
            .map_err(map_pass_action_error)?;
        if has_actions && painted_output.is_some() {
            return Err(NativeHostProtocolError::PaintedOutputCountMismatch {
                expected: 0,
                actual: 1,
            }
            .into());
        }

        if has_actions {
            if !self.commit_terminal_actions(
                context,
                token,
                surface,
                presentation_actions,
                pane_focus_observation,
                local_actions_ready,
                has_application_action,
                budget,
            )? {
                self.settle_retained_egui_output(context, output, token, RetainedOutputKind::Skip)?;
                return Ok(());
            }
        }

        if let Some(output) = painted_output {
            if let Err(error) = self
                .coordinator
                .bind_painted_surface(context, token, output, &paint)
            {
                let kind = error.kind();
                drop(error.into_output());
                let _ = self.coordinator.abandon_output_token(token);
                return Err(NativeHostProtocolError::OutputBindingFailed(kind).into());
            }
        } else if matches!(
            self.coordinator.abandon_output_token(token),
            NativeHostWake::RepaintRoot
        ) {
            context.request_repaint_once_of(egui::ViewportId::ROOT);
        }
        Ok(())
    }

    fn settle_retained_egui_output(
        &mut self,
        context: &egui::Context,
        output: &mut egui::FullOutput,
        token: NativeOutputToken,
        kind: RetainedOutputKind,
    ) -> Result<(), NativeRuntimeError> {
        let retained = kind.request();
        finalize_retained_accessibility(output, kind, retained);
        let wake = self
            .coordinator
            .settle_retained_output_token(token, retained)
            .map_err(NativeHostProtocolError::OutputBindingFailed)?;
        if matches!(wake, NativeHostWake::RepaintRoot) {
            context.request_repaint_once_of(egui::ViewportId::ROOT);
        }
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    fn commit_terminal_actions(
        &mut self,
        context: &egui::Context,
        token: NativeOutputToken,
        surface: SurfaceId,
        presentation_actions: Vec<PreparedSurfaceAction>,
        pane_focus_observation: Option<PreparedPaneFocusObservation>,
        local_actions_ready: bool,
        application_action_ready: bool,
        budget: &mut NativeInputBudget,
    ) -> Result<bool, NativeRuntimeError> {
        if self.coordinator.has_pending_callback_records()
            && !self
                .coordinator
                .dispatch_bridge()
                .callback_boundary_pending()
        {
            // The output finalizer cannot translate a later callback prefix.
            // Keep queued actions owned until the next root prelude drains it.
            require_durable_root_cycle(&self.coordinator, token, true)?;
            context.request_repaint_once_of(ViewportId::ROOT);
            context.request_repaint_once_of(token.viewport_id());
            return Ok(false);
        }
        let Some(mut host_frame) = self.coordinator.poll_resolved_host_frame(context, budget)?
        else {
            require_durable_root_cycle(&self.coordinator, token, true)?;
            context.request_repaint_once_of(ViewportId::ROOT);
            context.request_repaint_once_of(token.viewport_id());
            return Ok(false);
        };
        if host_frame.native_input_pending() {
            host_frame.complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)?;
            let report = host_frame.commit()?;
            self.settle_terminal_action_report(context, token, surface, None, report, budget)?;
            require_durable_root_cycle(&self.coordinator, token, true)?;
            context.request_repaint_once_of(token.viewport_id());
            return Ok(false);
        }
        if !host_frame.frame.surfaces().contains(&surface) {
            return Err(NativeHostProtocolError::MultipassOutputChanged.into());
        }
        let submitted_application_action = if application_action_ready {
            let action = self
                .application_actions
                .take()
                .ok_or(NativeHostProtocolError::MultipassOutputChanged)?;
            Some(host_frame.submit_prepared_action(action)?)
        } else {
            None
        };
        for action in presentation_actions {
            host_frame.submit_surface_action(action)?;
        }
        if let Some(observation) = pane_focus_observation {
            host_frame.submit_pane_focus_observation(observation)?;
        }
        if local_actions_ready {
            let (local_actions, local_action_attempt) = self
                .pass_actions
                .begin_local_action_submission(surface)
                .map_err(map_pass_action_error)?;
            debug_assert_eq!(local_actions.surface(), surface);
            let (_, actions) = local_actions.into_parts();
            let core_result = (|| -> Result<HostFrameReport, NativeRuntimeError> {
                for action in actions {
                    host_frame.submit_surface_action(action)?;
                }
                host_frame.complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)?;
                host_frame.commit()
            })();
            let report = match local_action_attempt
                .settle(core_result)
                .map_err(map_pass_action_error)?
            {
                Ok(report) => report,
                Err(error) => return Err(error),
            };
            return self
                .settle_terminal_action_report(
                    context,
                    token,
                    surface,
                    submitted_application_action,
                    report,
                    budget,
                )
                .map(|()| true);
        }
        host_frame.complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)?;
        let report = host_frame.commit()?;
        self.settle_terminal_action_report(
            context,
            token,
            surface,
            submitted_application_action,
            report,
            budget,
        )
        .map(|()| true)
    }

    fn settle_terminal_action_report(
        &mut self,
        context: &egui::Context,
        token: NativeOutputToken,
        surface: SurfaceId,
        submitted_application_action: Option<DockspaceSubmittedAction>,
        mut report: HostFrameReport,
        budget: &mut NativeInputBudget,
    ) -> Result<(), NativeRuntimeError> {
        let submitted_application_outcome = submitted_application_action
            .and_then(|submitted| report.submitted_action_outcome(submitted));
        let application_action_settled = self
            .application_actions
            .settle(
                submitted_application_outcome,
                report.presentation_transitions(),
            )
            .map_err(|()| NativeHostProtocolError::ApplicationActionOutcomeMissing)?;
        let _ = self.coordinator.settle_host_frame_report(&report)?;
        let _ = self.coordinator.settle_close_control_report(&report)?;
        let native_admission_settled = self
            .coordinator
            .settle_native_admissions(report.native_admissions())?;
        if surface == self.root_surface {
            Self::bind_root_registration(
                &mut self.coordinator,
                self.root_surface,
                token,
                report.inputs(),
            )?;
        }
        let native_effects = report.take_native_effects();
        let outputs = report.take_painted_outputs();
        if !outputs.is_empty() {
            let actual = outputs.len();
            drop(outputs);
            return Err(NativeHostProtocolError::PaintedOutputCountMismatch {
                expected: 0,
                actual,
            }
            .into());
        }
        self.coordinator
            .accept_native_effects(context, native_effects)?;
        let native_close_progress = self.coordinator.drive_close_policy(
            context,
            self.close_policy,
            self.root_surface,
            budget,
        );
        let internal_transitions = self.coordinator.take_internal_presentation_transitions();
        if !internal_transitions.is_empty() {
            let internal_action_settled = self
                .application_actions
                .settle_presentation_transitions(&internal_transitions)
                .map_err(|()| NativeHostProtocolError::ApplicationActionOutcomeMissing)?;
            require_durable_root_cycle(&self.coordinator, token, internal_action_settled)?;
        }
        let _ = native_close_progress?;
        self.dispatch_post_commit(context)?;
        for repaint_surface in report.repaint_surfaces() {
            match self.coordinator.repaint_viewport(*repaint_surface) {
                Some(egui::ViewportId::ROOT) | None if *repaint_surface == self.root_surface => {
                    context.request_repaint_once_of(egui::ViewportId::ROOT);
                }
                Some(viewport) => context.request_repaint_once_of(viewport),
                None => {}
            }
        }
        request_follow_up_root_cycle(context, false, true);
        require_durable_root_cycle(
            &self.coordinator,
            token,
            application_action_settled || native_admission_settled,
        )?;
        Ok(())
    }

    fn dispatch_post_commit(&mut self, context: &egui::Context) -> Result<(), NativeRuntimeError> {
        self.coordinator.dispatch_viewport_commands(context)?;
        let Some(command) = self.coordinator.pointer_passthrough_command() else {
            return Ok(());
        };
        if self
            .coordinator
            .dispatch_pointer_passthrough(context, command)
        {
            return Ok(());
        }
        let removed = self.coordinator.fail_pointer_passthrough_dispatch(command);
        if !removed {
            return Err(NativeHostProtocolError::NativeInputDispatchChanged.into());
        }
        context.request_repaint_once_of(egui::ViewportId::ROOT);
        Ok(())
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
        let kind = error.kind();
        let internal_transition_error = self.settle_internal_application_transitions().err();
        let mut cleanup_errors = internal_transition_error.into_iter().collect::<Vec<_>>();
        self.coordinator.quarantine_after_fatal_into(|token| {
            if let Err(error) = self.pass_actions.abandon(token) {
                cleanup_errors.push(map_pass_action_error(error));
            }
        });
        self.pass_actions
            .abandon_all_outputs_into(|error| cleanup_errors.push(map_pass_action_error(error)));
        // Durable local actions stay visible as unresolved work. A fatal
        // output teardown is not a core action terminal.
        let _ = self.application_actions.begin_shutdown(kind);
        self.shutdown = Some(NativeShutdownState {
            primary: error,
            cleanup_errors,
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
        let Some(token) = eframe::current_native_output_token() else {
            return (
                false,
                vec![NativeHostProtocolError::OutputTokenUnavailable.into()],
            );
        };
        let bridge = self.coordinator.dispatch_bridge();
        let Some(mut budget) = bridge.dispatch_budget(token) else {
            return (
                false,
                vec![NativeHostProtocolError::OutputBudgetUnavailable.into()],
            );
        };
        debug_assert!(
            self.shutdown.is_some(),
            "shutdown advance requires a primary failure"
        );
        let advance = self
            .coordinator
            .advance_shutdown_boundary_with_close_policy(
                context,
                self.close_policy,
                self.root_surface,
                &mut budget,
            );
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
            mut progress,
            transition_source_exhausted,
            presentation_transitions,
            registrations,
            commands_dispatched: _,
            repaint_viewports,
            pointer_passthrough,
            abandoned_outputs,
            mut errors,
        } = advance;
        if !presentation_transitions.is_empty() {
            match self
                .application_actions
                .settle_presentation_transitions(&presentation_transitions)
            {
                Ok(settled) => progress |= settled,
                Err(()) => {
                    errors.push(NativeHostProtocolError::ApplicationActionOutcomeMissing.into())
                }
            }
        }
        for token in abandoned_outputs {
            if let Err(error) = self.pass_actions.abandon(token) {
                errors.push(map_pass_action_error(error));
            }
        }
        for viewport in repaint_viewports {
            context.request_repaint_once_of(viewport);
        }
        if let Some(command) = pointer_passthrough {
            if !self
                .coordinator
                .dispatch_pointer_passthrough(context, command)
            {
                let removed = self.coordinator.fail_pointer_passthrough_dispatch(command);
                if !removed {
                    errors.push(NativeHostProtocolError::NativeInputDispatchChanged.into());
                }
                context.request_repaint_once_of(egui::ViewportId::ROOT);
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
        if transition_source_exhausted {
            progress |= self.application_actions.finish_shutdown_exhausted();
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

    pub(crate) fn render_error(&self, ui: &mut egui::Ui, presentation: NativeErrorPresentation) {
        let primary = self.shutdown.as_ref().map_or_else(
            || "unknown native runtime error".to_owned(),
            |shutdown| shutdown.primary.to_string(),
        );
        let cleanup_count = self
            .shutdown
            .as_ref()
            .map_or(0, |shutdown| shutdown.cleanup_errors.len());
        let cleanup = (cleanup_count > 0).then(|| format!(" · {cleanup_count} cleanup error(s)"));
        let message = format!(
            "Native dockspace stopped — {primary}{}",
            cleanup.as_deref().unwrap_or_default()
        );
        paint_native_error_status(ui, presentation, &message);
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

fn committed_boundary_is_quiescent(
    stopped: bool,
    root_presented: bool,
    application_action_pending: bool,
    retained_local_actions_pending: bool,
    coordinator_quiescent: bool,
) -> bool {
    !stopped
        && root_presented
        && !application_action_pending
        && !retained_local_actions_pending
        && coordinator_quiescent
}

#[cfg(test)]
mod tests {
    use std::error::Error as _;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use dockspace::model::{
        DockAnchor, DockPlacement, DockspaceLayout, DockspaceNode, DockspaceRootLayout,
        DockspaceSurfaceLayout, ItemId, RootId,
    };
    use dockspace::runtime::{SurfacePresentationResult, SurfaceUnavailableReason};
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

    fn test_session() -> dockspace::runtime::DockspaceSession {
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
        session
    }

    fn test_state() -> NativeRuntimeState<TestPanes> {
        NativeRuntimeState::new(
            Id::unique("native-runtime-stop-test"),
            test_session(),
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
        assert_eq!(
            surface_frame_disposition(true, true, true, false),
            SurfaceFrameDisposition::ConfirmPainted,
            "a complete transient/handoff paint defers measurement, not its exact presentation proof"
        );
    }

    #[test]
    fn control_frames_retain_without_minting_another_output() {
        assert!(should_retain_previous_output(true, false));
        assert!(should_retain_previous_output(false, true));
        assert!(!should_retain_previous_output(false, false));
    }

    #[test]
    fn retained_control_frame_rearms_pending_semantic_work() {
        assert!(retained_pass_requires_semantic_retry(true, false, false));
        assert!(retained_pass_requires_semantic_retry(true, true, true));
        assert!(!retained_pass_requires_semantic_retry(true, true, false));
        assert!(!retained_pass_requires_semantic_retry(false, false, true));
    }

    #[test]
    fn no_frame_invalidation_requests_its_child_until_that_frame_accepts_it() {
        let context = egui::Context::default();
        let child_surface = SurfaceId::new(2);
        let child_viewport = ViewportId::from_hash_of("no-frame-invalidation-child");
        let repaint_viewports = Arc::new(std::sync::Mutex::new(Vec::new()));
        context.set_request_repaint_callback({
            let repaint_viewports = Arc::clone(&repaint_viewports);
            move |info| {
                repaint_viewports
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner())
                    .push(info.viewport_id);
            }
        });

        let accepted = select_no_frame_invalidations_for_frame(
            &context,
            &[SURFACE],
            [
                (child_surface, child_viewport),
                (child_surface, child_viewport),
            ],
            |(surface, _)| *surface,
            |(_, viewport)| *viewport,
        );

        assert!(accepted.is_empty());
        assert_eq!(
            *repaint_viewports
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner()),
            [child_viewport],
            "an out-of-roster child obligation requests exactly its owning callback"
        );

        let accepted = select_no_frame_invalidations_for_frame(
            &context,
            &[child_surface],
            [(child_surface, child_viewport)],
            |(surface, _)| *surface,
            |(_, viewport)| *viewport,
        );
        assert_eq!(accepted, [(child_surface, child_viewport)]);
    }

    #[test]
    fn fatal_overlay_uses_only_a_bottom_status_strip() {
        let available = egui::Rect::from_min_size(egui::pos2(10.0, 20.0), egui::vec2(800.0, 600.0));

        assert_eq!(
            native_error_paint_rect(available, NativeErrorPresentation::NoFrame, 30.0),
            available
        );
        assert_eq!(
            native_error_paint_rect(available, NativeErrorPresentation::RetainedOverlay, 30.0,),
            egui::Rect::from_min_max(egui::pos2(10.0, 590.0), egui::pos2(810.0, 620.0))
        );
    }

    #[test]
    fn retained_overlay_keeps_an_inert_accessible_status_label() {
        let context = egui::Context::default();
        context.enable_accesskit();
        let message = "Native dockspace stopped — test failure";
        let mut probe_enabled = true;
        let mut output = context.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(800.0, 600.0),
                )),
                ..Default::default()
            },
            |ui| {
                paint_native_error_status(ui, NativeErrorPresentation::RetainedOverlay, message);
                probe_enabled = ui.button("mutation probe").enabled();
            },
        );

        let (status_actions, observed_nodes) = {
            let tree = output
                .platform_output
                .accesskit_update
                .as_ref()
                .expect("the status pass publishes accessibility");
            let status_actions = tree.nodes.iter().find_map(|(_, node)| {
                (node.role() == egui::accesskit::Role::Label && node.value() == Some(message)).then(
                    || {
                        (
                            node.supports_action(egui::accesskit::Action::Click),
                            node.supports_action(egui::accesskit::Action::Focus),
                            node.is_disabled(),
                        )
                    },
                )
            });
            let observed_nodes = tree
                .nodes
                .iter()
                .map(|(_, node)| (node.role(), node.label(), node.value()))
                .collect::<Vec<_>>();
            (status_actions, observed_nodes)
        };
        output.textures_delta.clear();
        let (supports_click, supports_focus, disabled) = status_actions.unwrap_or_else(|| {
            panic!("the visible native status strip owns one label; nodes={observed_nodes:?}")
        });
        assert!(!supports_click);
        assert!(!supports_focus);
        assert!(
            !probe_enabled,
            "fatal/quarantined UI must reject later semantic input"
        );
        assert!(
            disabled,
            "fatal/quarantined status must be exposed as non-interactive"
        );
    }

    #[test]
    fn no_frame_placeholder_disables_semantic_input_and_accessibility() {
        let context = egui::Context::default();
        context.enable_accesskit();
        let mut probe_enabled = true;
        let mut output = context.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(800.0, 600.0),
                )),
                ..Default::default()
            },
            |ui| {
                crate::deferred_viewport::paint_placeholder(
                    ui,
                    egui::ViewportClass::Deferred,
                    Some(crate::mailbox::DeferredViewportPaint::Created),
                );
                probe_enabled = ui.button("mutation probe").enabled();
            },
        );
        output.textures_delta.clear();

        let tree = output
            .platform_output
            .accesskit_update
            .as_ref()
            .expect("the no-frame status publishes accessibility");
        let status = tree
            .nodes
            .iter()
            .find_map(|(_, node)| {
                (node.role() == egui::accesskit::Role::Label
                    && node.value() == Some("Creating dockspace window…"))
                .then_some(node)
            })
            .expect("the no-frame status owns one semantic node");
        assert!(!probe_enabled, "NoFrame must reject semantic input");
        assert!(
            status.is_disabled(),
            "NoFrame status must be disabled in AccessKit"
        );
        assert!(
            context.has_requested_repaint_for(&ViewportId::ROOT),
            "creating a no-frame viewport must request the next lifecycle boundary"
        );
    }

    #[test]
    fn only_a_skipped_retain_omits_the_accessibility_update() {
        assert!(RetainedOutputKind::Skip.omits_accessibility_when_retained());
        assert!(!RetainedOutputKind::Overlay.omits_accessibility_when_retained());
    }

    #[test]
    fn presented_pending_keeps_the_previous_accessibility_tree() {
        let context = egui::Context::default();
        context.enable_accesskit();
        let mut pending_probe_enabled = false;
        let mut output = context.run_ui(egui::RawInput::default(), |ui| {
            pending_probe_enabled = ui.button("pending mutation").enabled();
        });
        assert!(pending_probe_enabled);
        assert!(
            !output.shapes.is_empty(),
            "the pending pass may produce paint, but the retained renderer path must skip it"
        );
        assert!(output.platform_output.accesskit_update.is_some());

        finalize_retained_accessibility(&mut output, RetainedOutputKind::Overlay, true);
        assert!(
            output.platform_output.accesskit_update.is_some(),
            "a visible fatal overlay must publish its inert status semantics"
        );
        finalize_retained_accessibility(&mut output, RetainedOutputKind::Skip, false);
        assert!(
            output.platform_output.accesskit_update.is_some(),
            "a failed retain falls back to the current NoFrame semantics"
        );
        finalize_retained_accessibility(&mut output, RetainedOutputKind::Skip, true);

        assert!(
            output.platform_output.accesskit_update.is_none(),
            "PresentedPending must keep the prior semantic tree instead of publishing pending UI"
        );
        output.textures_delta.clear();
    }

    #[test]
    fn no_frame_recovery_reaches_enabled_first_live_at_the_presented_boundary() {
        let mut session = test_session();
        let context = egui::Context::default();
        context.enable_accesskit();
        let raw_input = || egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(800.0, 600.0),
            )),
            ..Default::default()
        };

        let mut no_frame_probe_enabled = true;
        let mut no_frame_output = context.run_ui(raw_input(), |ui| {
            crate::deferred_viewport::paint_placeholder(
                ui,
                egui::ViewportClass::Deferred,
                Some(crate::mailbox::DeferredViewportPaint::Created),
            );
            no_frame_probe_enabled = ui.button("no-frame mutation probe").enabled();
        });
        let no_frame_accesskit = no_frame_output
            .platform_output
            .accesskit_update
            .as_ref()
            .expect("NoFrame publishes its preparing status");
        assert!(!no_frame_probe_enabled);
        assert!(no_frame_accesskit.nodes.iter().any(|(_, node)| {
            node.role() == egui::accesskit::Role::Label
                && node.value() == Some("Creating dockspace window…")
                && node.is_disabled()
        }));
        no_frame_output.textures_delta.clear();

        let mut measurement_output = context.run_ui(raw_input(), |ui| {
            let mut frame = session
                .begin_host_frame()
                .expect("the first-live measurement frame begins");
            let dock_rect = ui.available_rect_before_wrap();
            egui_dockspace::native_support::measure_surface(
                &mut frame,
                SURFACE,
                ui,
                dock_rect,
                dock_rect,
                &TestPanes,
                &DockStyle::default(),
            )
            .expect("the first-live surface measures through the production renderer");
            frame
                .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
                .expect("the measurement frame settles every surface");
            drop(
                frame
                    .commit()
                    .expect("the measurement contribution commits"),
            );
        });
        measurement_output.textures_delta.clear();

        let mut panes = TestPanes;
        let mut paint_report = None;
        let mut paint_had_ready_plan = false;
        let mut mutation_probe_enabled = false;
        let mut semantic_output = context.run_ui(raw_input(), |ui| {
            let mut frame = session
                .begin_host_frame()
                .expect("the first-live semantic frame begins");
            let mut register_scroll = |ui: &egui::Ui, rect: egui::Rect, id: egui::Id| {
                let identity = ui.register_scroll_hit_candidate(rect, id);
                (identity.id(), identity.layer_id())
            };
            let mut paint = egui_dockspace::native_support::paint_surface(
                &mut frame,
                Id::unique("first-live-semantic-paint"),
                SURFACE,
                ui,
                &mut panes,
                &DockStyle::default(),
                &mut register_scroll,
            )
            .expect("the first-live surface paints through the production renderer");
            paint_had_ready_plan = paint.had_ready_plan();
            mutation_probe_enabled = ui.button("first-live mutation probe").enabled();
            assert!(
                paint
                    .confirm_into(&mut frame)
                    .expect("the production paint confirms against its issuing frame"),
                "the production renderer completes the first-live program"
            );
            frame
                .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
                .expect("the semantic frame settles every surface");
            paint_report = Some(frame.commit().expect("the first-live paint commits"));
        });

        assert!(paint_had_ready_plan);
        assert!(
            mutation_probe_enabled,
            "FirstLive must leave product input enabled"
        );
        let accesskit = semantic_output
            .platform_output
            .accesskit_update
            .as_ref()
            .expect("FirstLive publishes the current semantic tree");
        let mutation_probe = accesskit
            .nodes
            .iter()
            .find_map(|(_, node)| {
                (node.role() == egui::accesskit::Role::Button
                    && node.label() == Some("first-live mutation probe"))
                .then_some(node)
            })
            .expect("the enabled FirstLive mutation probe is accessible");
        assert!(!mutation_probe.is_disabled());
        assert!(mutation_probe.supports_action(egui::accesskit::Action::Click));
        assert!(
            accesskit.nodes.iter().all(|(_, node)| {
                node.value().is_none_or(|value| {
                    value != "Creating dockspace window…"
                        && value != "Waiting for dockspace authority…"
                        && !value.starts_with("Native dockspace stopped")
                })
            }),
            "FirstLive removes transitional and fatal status UI"
        );

        let mut report = paint_report.take().expect("the semantic report exists");
        let outputs = report.take_painted_outputs();
        let [painted] = outputs.try_into().unwrap_or_else(|outputs: Vec<_>| {
            panic!("expected one first-live output, got {}", outputs.len())
        });
        session
            .report_surface_presentation(painted, SurfacePresentationResult::Presented)
            .expect("the renderer presents the exact first-live output");
        let mut admission = session
            .begin_host_frame()
            .expect("the first-live admission boundary begins");
        admission
            .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
            .expect("the admission boundary settles every surface");
        drop(
            admission
                .commit()
                .expect("the first-live admission commits"),
        );
        assert!(session.presented_surface(SURFACE).is_some());
        semantic_output.textures_delta.clear();
    }

    #[test]
    fn protocol_follow_up_requests_exactly_one_root_cycle() {
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
        let repaint_count = Arc::new(AtomicUsize::new(0));
        context.set_request_repaint_callback({
            let repaint_count = Arc::clone(&repaint_count);
            move |_| {
                repaint_count.fetch_add(1, Ordering::Relaxed);
            }
        });

        request_follow_up_root_cycle(&context, false, false);
        assert!(!context.has_requested_repaint_for(&ViewportId::ROOT));

        request_follow_up_root_cycle(&context, true, false);
        assert!(context.has_requested_repaint_for(&ViewportId::ROOT));
        assert_eq!(repaint_count.load(Ordering::Relaxed), 1);

        let mut output = context.run_ui(egui::RawInput::default(), |_| {});
        output.textures_delta.clear();
        assert_eq!(repaint_count.load(Ordering::Relaxed), 1);
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
            Some(Ok(DockspaceActionStatus::Rejected(
                dockspace::model::DockspaceActionRejection::PolicyDenied
            )))
        ));

        state
            .request_dock_root(RootId::new(1), placement)
            .expect("consuming the result releases the bounded slot");

        state.stop_current_output(NativeHostProtocolError::OutputTokenUnavailable.into());
        assert!(state.application_actions.is_occupied());
        assert!(matches!(
            state.take_action_status(),
            Some(Err(error))
                if error.interruption() == crate::NativeActionInterruption::NotCommitted
                    && error.kind() == NativeRuntimeErrorKind::InvalidConfiguration
        ));
        assert!(!state.application_actions.is_occupied());
        assert_eq!(
            state
                .request_dock_root(RootId::new(1), placement)
                .expect_err("a stopped runtime rejects new application actions"),
            crate::NativeActionRequestError::Stopped
        );
    }

    #[test]
    fn quiescence_tracks_the_committed_boundary_and_real_pending_actions() {
        let mut state = test_state();
        assert!(committed_boundary_is_quiescent(
            false, true, false, false, true,
        ));

        state
            .request_dock_root(
                RootId::new(1),
                DockPlacement::Center(DockAnchor::Item(ItemId::new(1))),
            )
            .expect("the committed boundary accepts one application action");

        assert!(
            !committed_boundary_is_quiescent(
                false,
                true,
                state.application_actions.is_occupied(),
                state.pass_actions.has_retained_action_work(),
                true,
            ),
            "a real durable application action still blocks quiescence"
        );
        assert!(
            !committed_boundary_is_quiescent(false, true, false, true, true),
            "a local action retained across egui passes remains durable work"
        );
    }

    #[test]
    fn committed_action_reports_an_uncorrelated_outcome_as_a_terminal_failure() {
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
        assert!(matches!(
            state.take_action_status(),
            Some(Err(error))
                if error.interruption()
                    == crate::NativeActionInterruption::CommittedOutcomeUnavailable
                    && error.kind() == NativeRuntimeErrorKind::Internal
        ));
    }

    #[test]
    fn fatal_stop_terminalizes_an_uncommitted_application_frame_once() {
        let mut state = test_state();
        let placement = DockPlacement::Center(DockAnchor::Item(ItemId::new(1)));
        state
            .request_dock_root(RootId::new(1), placement)
            .expect("the application action is queued");
        let _action = state
            .application_actions
            .take()
            .expect("the final root pass takes the pending action");

        state.stop_current_output(NativeHostProtocolError::OutputTokenUnavailable.into());
        state
            .stop_current_output(NativeHostProtocolError::RootRegistrationRejected(SURFACE).into());

        assert!(matches!(
            state.take_action_status(),
            Some(Err(error))
                if error.interruption() == crate::NativeActionInterruption::NotCommitted
                    && error.kind() == NativeRuntimeErrorKind::InvalidConfiguration
        ));
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
            Some(Ok(DockspaceActionStatus::Rejected(
                dockspace::model::DockspaceActionRejection::PolicyDenied
            )))
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
    fn fatal_quarantine_recovery_commits_close_before_exit_errors() {
        let mut state = test_state();
        state.stop_current_output(NativeHostProtocolError::OutputTokenUnavailable.into());
        assert!(state.error().is_some());
        assert!(!state.is_quiescent());
        assert_eq!(
            state
                .request_dock_root_back(RootId::new(1))
                .expect_err("fatal quarantine rejects semantic mutation"),
            crate::NativeActionRequestError::Stopped
        );

        let context = egui::Context::default();
        context.begin_pass(egui::RawInput::default());

        // Shutdown dispatches the command before returning its progress report.
        context.send_viewport_cmd_to(egui::ViewportId::ROOT, egui::ViewportCommand::Close);
        let (progress, errors) = state.apply_shutdown_advance_for_root_window(
            &context,
            NativeShutdownAdvance {
                progress: true,
                transition_source_exhausted: false,
                presentation_transitions: Vec::new(),
                registrations: vec![NativeShutdownRegistration::Rejected(SURFACE)],
                commands_dispatched: true,
                repaint_viewports: Vec::new(),
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
        assert!(
            close_was_queued,
            "fatal recovery exits only after the committed close command survives the output"
        );
    }
}
