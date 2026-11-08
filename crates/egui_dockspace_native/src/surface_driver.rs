//! Shared root/child surface driver over one native coordinator.

#[cfg(test)]
use crate::NativeCommand;
use dockspace::model::{DockspaceView, SurfaceId};
use dockspace::runtime::{
    HostFrameReport, HostInputOutcome, HostWindowToken, NativeInputBudget, SurfaceUnavailableReason,
};
use eframe::egui::emath::GuiRounding;
use eframe::egui::{self, Id, ViewportId};
use eframe::{NativeHostWake, NativeOutputToken};
#[cfg(test)]
use egui_dockspace::DockspaceActionStatus;
use egui_dockspace::{DockStyle, PaneView};

use crate::application_action::{NativeApplicationActions, NativeSubmittedApplicationAction};
use crate::close_control::NativeWindowClosePolicy;
#[cfg(test)]
use crate::coordinator::split_no_frame_invalidations_for_frame;
use crate::coordinator::{NativeCoordinator, NativeShutdownAdvance, NativeShutdownRegistration};
use crate::deferred_viewport::DeferredViewportSpec;
#[cfg(test)]
use crate::error::NativeRuntimeErrorKind;
use crate::error::{NativeHostProtocolError, NativeRuntimeError};
use crate::lifecycle_progress::{NativeCloseProgress, NativeOutputBacklog};
use crate::mailbox::NativeDispatch;
use crate::pass_actions::{NativeFinalPass, NativePassActionError, NativePassActions};

mod application;
#[path = "native_owned_update.rs"]
mod native_owned_update;

use native_owned_update::{NativeOwnedUpdate, NativeUpdatePublication, NativeUpdateRun};

const ROOT_WINDOW_TOKEN: HostWindowToken = HostWindowToken::new(1);

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
    Rejected,
}

impl RetainedOutputKind {
    const fn omits_accessibility_when_retained(self) -> bool {
        matches!(self, Self::Skip | Self::Rejected)
    }

    fn request(self) -> bool {
        match self {
            Self::Skip => eframe::retain_current_native_output(),
            Self::Overlay => eframe::retain_current_native_output_with_overlay(),
            Self::Rejected => {
                if eframe::retain_current_native_output() {
                    true
                } else {
                    // The checked active token has no exact framebuffer. The
                    // fork selects NoFrame on a failed overlay request; the
                    // rejected shapes have already been removed.
                    eframe::retain_current_native_output_with_overlay()
                }
            }
        }
    }
}

fn discard_unproven_output(output: &mut egui::FullOutput) {
    output.shapes.clear();
    output.platform_output.accesskit_update = None;
    output.platform_output.events.clear();
    output.platform_output.ime = None;
    output.platform_output.mutable_text_under_cursor = false;
    // Textures and explicit clipboard/URL/viewport commands still belong to
    // this actual pass and must reach the integration once.
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

const fn should_retain_previous_output(
    output_prefix_pending: bool,
    callback_reduced: bool,
) -> bool {
    output_prefix_pending || callback_reduced
}

const fn retained_pass_requires_semantic_retry(
    retain_previous_output: bool,
    surface_is_presented: bool,
    presentation_work_pending: bool,
) -> bool {
    retain_previous_output && (!surface_is_presented || presentation_work_pending)
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
    owned_update: Option<NativeOwnedUpdate>,
    pending_service: bool,
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
    #[cfg(all(test, target_os = "linux"))]
    pub(crate) fn boundary_coordinator(&mut self) -> &mut NativeCoordinator {
        &mut self.coordinator
    }

    #[cfg(all(test, target_os = "linux"))]
    pub(crate) fn boundary_close_policy(&mut self) -> &mut NativeWindowClosePolicy {
        &mut self.close_policy
    }

    pub(crate) fn prepare_output_input(
        &mut self,
        context: &egui::Context,
        token: NativeOutputToken,
    ) -> Result<eframe::NativeOutputInputAdmission, NativeRuntimeError> {
        self.abandon_unfinalized_output_passes(context)?;
        if let Some(update) = &self.owned_update {
            self.pending_service = true;
            if update.run.token == token {
                return Ok(eframe::NativeOutputInputAdmission::Proceed);
            }
            return self.defer_output_input(token);
        }
        let bridge = self.coordinator.dispatch_bridge();
        if self.shutdown.is_some()
            || bridge.is_control_output(token)
            || bridge.create_binding(token.viewport_id()).is_some()
            || bridge.output_has_staging_request(token)
        {
            return Ok(eframe::NativeOutputInputAdmission::Proceed);
        }
        let mut budget = bridge
            .dispatch_budget(token)
            .ok_or(NativeHostProtocolError::OutputBudgetUnavailable)?;
        let surface = bridge
            .output_reservation(token)
            .and_then(|reservation| reservation.binding())
            .map_or(self.root_surface, |binding| binding.surface());
        if !self.service_pending_prefixes(context, Some(token), surface, &mut budget)? {
            return self.defer_output_input(token);
        }
        self.coordinator
            .stage_stationary_observation(context, token, &mut budget);
        if budget.is_exhausted() {
            return self.defer_output_input(token);
        }
        if token.viewport_id() == ViewportId::ROOT
            && context.viewport_for(ViewportId::ROOT, |viewport| {
                viewport.commands.contains(&egui::ViewportCommand::Close)
            })
        {
            // An application close queued outside UI needs one final control
            // pass, without publishing a new semantic output during exit.
            bridge.abandon_output(token);
        }
        Ok(eframe::NativeOutputInputAdmission::Proceed)
    }

    pub(crate) fn service_native_events(
        &mut self,
        context: &egui::Context,
    ) -> Result<NativeHostWake, NativeRuntimeError> {
        self.abandon_unfinalized_output_passes(context)?;
        if self.owned_update.is_some() {
            self.pending_service = true;
            return Ok(NativeHostWake::ServiceAfterCurrent);
        }
        if self.shutdown.is_some() {
            return Ok(NativeHostWake::RepaintRootAfterCurrent);
        }
        let mut budget = self.coordinator.dispatch_bridge().service_budget();
        Ok(
            if self.service_pending_prefixes(context, None, self.root_surface, &mut budget)? {
                NativeHostWake::Wait
            } else {
                NativeHostWake::ServiceAfterCurrent
            },
        )
    }

    fn service_pending_prefixes(
        &mut self,
        context: &egui::Context,
        token: Option<NativeOutputToken>,
        surface: SurfaceId,
        budget: &mut NativeInputBudget,
    ) -> Result<bool, NativeRuntimeError> {
        if self.owned_update.is_some() {
            self.pending_service = true;
            return Ok(false);
        }
        self.pending_service = false;
        let bridge = self.coordinator.dispatch_bridge();
        self.abandon_unfinalized_output_passes(context)?;
        self.coordinator
            .drive_pointer_capture(context, budget, self.shutdown.is_none())?;
        if self.coordinator.drive_close_policy(
            context,
            self.close_policy,
            self.root_surface,
            budget,
        )? {
            self.pass_actions
                .retire_unavailable(self.coordinator.session());
            self.settle_internal_application_transitions()?;
        }
        loop {
            let before = budget.remaining_records();
            let pending_output = self.coordinator.has_pending_output_prefix();
            let effect = self.coordinator.try_report_pending_effect_results()?;
            let quiescence = token.is_none_or(|token| token.viewport_id() == ViewportId::ROOT)
                && self.coordinator.try_report_retirement_quiescence()?;
            let reduced = self
                .coordinator
                .reduce_callback_prefix(context, budget)?
                .is_some();
            if let Some(error) = self.coordinator.take_callback_error() {
                return Err(error);
            }
            let progress = pending_output
                || effect
                || quiescence
                || reduced
                || matches!(bridge.admission_dispatch(), NativeDispatch::Cancel { .. })
                || self.coordinator.session().has_pending_native_input()
                || bridge.callback_boundary_pending();
            let pending_create = self.shutdown.is_none()
                && bridge.accepts_new_work()
                && self.coordinator.session().has_pending_native_create();
            if !progress {
                if !pending_create {
                    break;
                }
                if budget.is_exhausted() {
                    return Ok(false);
                }
            }
            let Some(mut frame) = self.coordinator.poll_resolved_host_frame(context, budget)?
            else {
                return Ok(false);
            };
            let advanced_create = if self.shutdown.is_none()
                && bridge.accepts_new_work()
                && !frame.native_input_pending()
                && !budget.is_exhausted()
            {
                frame.frame.advance_next_native_surface_create()?
            } else {
                false
            };
            if advanced_create {
                budget.complete_unsequenced_records(1);
            } else if !progress {
                // Waiting for fresh facts or cleanup must not publish empty frames
                // or keep the service loop polling an unchanged create intent.
                drop(frame);
                break;
            }
            frame.complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)?;
            let report = frame.commit()?;
            self.settle_published_report(context, token, surface, None, report, budget, false)?;
            self.settle_internal_application_transitions()?;
            let pending_create = self.shutdown.is_none()
                && bridge.accepts_new_work()
                && self.coordinator.session().has_pending_native_create();
            if !self.coordinator.has_pending_callback_records()
                && !self.coordinator.session().has_pending_native_input()
                && !self.coordinator.has_pending_output_prefix()
                && !bridge.callback_boundary_pending()
                && !matches!(bridge.admission_dispatch(), NativeDispatch::Cancel { .. })
                && !pending_create
            {
                break;
            }
            if budget.is_exhausted() || budget.remaining_records() == before {
                return Ok(false);
            }
        }
        if self.coordinator.has_pending_callback_records()
            || self.coordinator.session().has_pending_native_input()
            || bridge.callback_boundary_pending()
            || matches!(bridge.admission_dispatch(), NativeDispatch::Cancel { .. })
        {
            return Ok(false);
        }
        if token.is_none_or(|token| token.viewport_id() == ViewportId::ROOT)
            && let Some(prepared) = self.coordinator.prepare_committed_retirements()?
        {
            for abandoned in self.coordinator.commit_retirements(prepared) {
                self.pass_actions
                    .abandon(abandoned)
                    .map_err(map_pass_action_error)?;
            }
            context.request_repaint_of(ViewportId::ROOT);
        }
        Ok(true)
    }

    fn defer_output_input(
        &mut self,
        token: NativeOutputToken,
    ) -> Result<eframe::NativeOutputInputAdmission, NativeRuntimeError> {
        self.coordinator.abandon_stationary_output(token);
        if !self.coordinator.dispatch_bridge().defer_output_input(token) {
            return Err(NativeHostProtocolError::OutputRouteAttachmentFailed.into());
        }
        Ok(eframe::NativeOutputInputAdmission::Defer(
            NativeHostWake::RepaintRootAfterCurrent,
        ))
    }

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
        if storage.is_some_and(|limit| {
            limit.combined.owners.resource_slots == 0 || limit.combined.owners.close_slots == 0
        }) {
            return Err(NativeHostProtocolError::NativeOwnerStorageConfiguration.into());
        }
        if let Some(field) = egui_dockspace::native_support::style_presentation_mismatch(
            &style,
            session.presentation_config(),
        )? {
            return Err(NativeHostProtocolError::StylePresentationMismatch { field }.into());
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
            owned_update: None,
            pending_service: false,
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

    pub(crate) fn with_errors<R>(
        &self,
        inspect: impl FnOnce(Option<&NativeRuntimeError>, &[NativeRuntimeError]) -> R,
    ) -> R {
        match &self.shutdown {
            Some(shutdown) => inspect(Some(&shutdown.primary), &shutdown.cleanup_errors),
            None => inspect(None, &[]),
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

    pub(crate) fn output_backlog(&self) -> NativeOutputBacklog {
        self.coordinator.output_backlog()
    }

    pub(crate) fn current_output_is_retained(&self) -> bool {
        eframe::current_native_output_token()
            .is_some_and(|token| self.pass_actions.is_retained(token))
    }

    pub(crate) fn owned_input_capacity_bytes(&self) -> Result<usize, NativeRuntimeError> {
        self.coordinator.owned_input_capacity_bytes()
    }

    pub(crate) fn is_quiescent(&self) -> bool {
        // Queries made while painting observe the committed boundary. An
        // ordinary private encoding pass is not pending semantic or OS work.
        !self.pending_service
            && committed_boundary_is_quiescent(
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

    pub(crate) fn is_shutdown_drained(&self) -> bool {
        self.shutdown.is_some()
            && !self.application_actions.is_occupied()
            && !self.pass_actions.has_retained_action_work()
            && self.coordinator.is_quiescent()
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
        self.update_surface_budgeted(ui, surface, token, &mut budget)
    }

    fn update_surface_budgeted(
        &mut self,
        ui: &mut egui::Ui,
        surface: SurfaceId,
        token: NativeOutputToken,
        budget: &mut NativeInputBudget,
    ) -> Result<NativeSurfaceUpdate, NativeRuntimeError> {
        let context = ui.ctx().clone();
        if surface == self.root_surface {
            self.root_context = Some(context.clone());
        }
        let bridge = self.coordinator.dispatch_bridge();
        if self
            .owned_update
            .as_ref()
            .is_some_and(|update| update.run.token != token)
        {
            self.pending_service = true;
            self.pass_actions
                .stage_retain_previous(token, surface)
                .map_err(map_pass_action_error)?;
            require_durable_root_cycle(&self.coordinator, token, true)?;
            return Ok(NativeSurfaceUpdate::RetainPrevious);
        }

        let (mut host_frame, run, mut publication) = if let Some(update) = self.owned_update.take()
        {
            let NativeOwnedUpdate {
                host,
                mut run,
                publication,
            } = update;
            if publication.surface != surface {
                return Err(NativeHostProtocolError::MultipassOutputChanged.into());
            }
            run.begin_repeated_pass(&context, token)?;
            (self.coordinator.resume_host_update(host)?, run, publication)
        } else {
            self.abandon_unfinalized_output_passes(&context)?;
            self.coordinator
                .stage_stationary_observation(&context, token, budget);
            let output_prefix_pending = self.coordinator.has_pending_output_prefix();
            let pending_effect_reported = self.coordinator.try_report_pending_effect_results()?;
            let quiescence_recorded = surface == self.root_surface
                && self.coordinator.try_report_retirement_quiescence()?;
            let reduced_callback = self
                .coordinator
                .reduce_callback_prefix(&context, budget)?
                .is_some();
            let internal_action_settled = self.settle_internal_application_transitions()?;
            if let Some(error) = self.coordinator.take_callback_error() {
                return Err(error);
            }
            let retirements = if surface == self.root_surface {
                self.coordinator.prepare_committed_retirements()?
            } else {
                None
            };
            let mut invalidations = self.coordinator.prepare_no_frame_invalidations()?;
            let no_frame_invalidation_pending =
                self.coordinator.has_pending_no_frame_invalidation(surface);
            let published = self.coordinator.session().version();
            let Some(mut host_frame) = self
                .coordinator
                .poll_resolved_host_frame(&context, budget)?
            else {
                if surface != self.root_surface {
                    bridge.block_semantic_viewport(token.viewport_id());
                }
                self.pass_actions
                    .stage_retain_previous(token, surface)
                    .map_err(map_pass_action_error)?;
                require_durable_root_cycle(&self.coordinator, token, true)?;
                context.request_repaint_once_of(ViewportId::ROOT);
                context.request_repaint_once_of(token.viewport_id());
                return Ok(NativeSurfaceUpdate::RetainPrevious);
            };
            let native_input_pending = host_frame.native_input_pending();
            let control_output = bridge.is_control_output(token);
            let mut ui_actions = None;
            if !native_input_pending && !control_output {
                let dock_rect = ui.available_rect_before_wrap();
                let focus_route_matches = token.viewport_id() == context.viewport_id()
                    && context.input(|input| input.focused)
                    && bridge
                        .output_reservation(token)
                        .and_then(|reservation| reservation.binding())
                        .is_some_and(|binding| binding.surface() == surface);
                let actual_focus = host_frame
                    .frame
                    .view()
                    .surface(surface)
                    .filter(|_| focus_route_matches)
                    .map(|view| {
                        egui_dockspace::native_support::sample_host_focus(
                            &context,
                            view,
                            &self.panes,
                            ui.make_persistent_id((
                                self.instance_id.with(surface.get()),
                                "workspace-focus",
                                surface,
                            )),
                            context
                                .layer_transform_to_global(ui.layer_id())
                                .map_or(dock_rect, |transform| transform * dock_rect),
                            host_frame.presented_focus_bindings(surface, token.viewport_id()),
                        )
                    });
                let (actions, mut acknowledgement) =
                    self.pass_actions.peek_deferred_actions(token, surface);
                if let Some(observation) = self.pass_actions.peek_pane_focus(
                    token,
                    host_frame.frame.presented_surface(surface),
                    &mut acknowledgement,
                ) {
                    host_frame.submit_pane_focus_observation(observation)?;
                }
                if let Some(actual_focus) = actual_focus {
                    host_frame.frame.report_host_focus(surface, actual_focus)?;
                }
                for action in actions {
                    host_frame.frame.submit_deferred_surface_action(action)?;
                }
                ui_actions = Some(acknowledgement);
            }
            let application_submission = if !native_input_pending
                && !control_output
                && surface == self.root_surface
                && self.application_actions.has_queued()
            {
                Some(
                    self.application_actions
                        .take()
                        .ok_or(NativeHostProtocolError::MultipassOutputChanged)?
                        .submit(&mut host_frame.frame, published)?,
                )
            } else {
                None
            };
            let application_action = application_submission
                .map(|submission| submission.finish(&mut host_frame.frame))
                .transpose()?;
            let surfaces = host_frame.frame.surfaces();
            invalidations.select_for_surfaces(&surfaces, true);
            for viewport in &invalidations.repaint_viewports {
                context.request_repaint_once_of(*viewport);
            }
            let retain_previous = control_output
                || (surface == self.root_surface
                    && context.input(|input| input.viewport().close_requested()))
                || bridge
                    .output_reservation(token)
                    .is_some_and(|reservation| reservation.is_abandoned())
                || !surfaces.contains(&surface)
                || native_input_pending
                || should_retain_previous_output(output_prefix_pending, reduced_callback)
                || no_frame_invalidation_pending
                || !invalidations.ready.is_empty();
            host_frame.frame.begin_encoding_pass()?;
            (
                host_frame,
                NativeUpdateRun::begin(&context, token),
                NativeUpdatePublication {
                    surface,
                    no_frame_invalidations: invalidations.ready,
                    retirements: if native_input_pending {
                        None
                    } else {
                        retirements
                    },
                    application_action,
                    ui_actions,
                    native_input_pending,
                    retain_previous,
                    follow_up: output_prefix_pending
                        || pending_effect_reported
                        || quiescence_recorded
                        || reduced_callback
                        || internal_action_settled
                        || native_input_pending,
                },
            )
        };

        let mut retain_previous = publication.retain_previous;
        let active_gesture = host_frame.frame.has_active_gesture();
        let should_measure = if retain_previous {
            false
        } else {
            match host_frame.frame.surface_work(surface)? {
                dockspace::runtime::SurfaceWork::Ready(plan) => {
                    !(active_gesture || plan.has_manipulation_preview())
                }
                dockspace::runtime::SurfaceWork::Deferred(
                    dockspace::runtime::SurfaceWorkDeferred::Unmeasured,
                ) => true,
                dockspace::runtime::SurfaceWork::Deferred(
                    dockspace::runtime::SurfaceWorkDeferred::OutputCapacity,
                ) => {
                    retain_previous = true;
                    false
                }
            }
        };
        if should_measure {
            let dock_rect = ui.available_rect_before_wrap();
            let popup_rect = context.input(egui::InputState::content_rect).round_ui();
            host_frame.measure_surface(
                surface,
                ui,
                dock_rect,
                popup_rect,
                &self.panes,
                &self.style,
            )?;
        }
        if !retain_previous
            && !matches!(
                host_frame.frame.surface_work(surface)?,
                dockspace::runtime::SurfaceWork::Ready(_)
            )
        {
            retain_previous = true;
        }
        if retain_previous {
            egui_dockspace::native_support::retain_presented_pane_focus(
                &context,
                host_frame.presented_focus_bindings(surface, token.viewport_id()),
            );
            if surface != self.root_surface {
                bridge.block_semantic_viewport(token.viewport_id());
            }
            self.pass_actions
                .stage_retain_previous(token, surface)
                .map_err(map_pass_action_error)?;
        } else {
            let paint = host_frame.paint_surface(
                self.instance_id.with(surface.get()),
                surface,
                ui,
                &mut self.panes,
                &self.style,
            )?;
            let owner = host_frame
                .frame
                .proposed_native_bindings()
                .find(|binding| binding.surface() == surface)
                .or_else(|| {
                    bridge
                        .output_reservation(token)
                        .and_then(|reservation| reservation.binding())
                });
            let paint = self
                .pass_actions
                .prepare_pass(token, owner, paint)
                .map_err(map_pass_action_error)?;
            if !paint.transient_visuals_complete() {
                return Err(NativeHostProtocolError::IncompleteTransientPaint(surface).into());
            }
            self.pass_actions
                .stage_pass(token, paint)
                .map_err(map_pass_action_error)?;
        }
        publication.follow_up |= self.pass_actions.has_retained_action_work();
        self.owned_update = Some(NativeOwnedUpdate {
            host: host_frame.suspend(),
            run,
            publication,
        });
        Ok(if retain_previous {
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
            let interrupted_update = self
                .owned_update
                .as_ref()
                .is_some_and(|update| update.run.token == token);
            if interrupted_update {
                self.owned_update = None;
            }
            self.pass_actions
                .abandon(token)
                .map_err(map_pass_action_error)?;
            self.coordinator.abandon_stationary_output(token);
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
            if interrupted_update {
                // An unwound run cannot replay application callbacks or a
                // consumed input capability through a replacement candidate.
                return Err(NativeHostProtocolError::MultipassOutputChanged.into());
            }
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
        self.settle_final_output_pass(context, output, token, discarded, budget)?;
        if !discarded && self.owned_update.is_none() {
            self.coordinator
                .schedule_stationary_observation(context, token, budget)?;
        }
        Ok(())
    }

    fn settle_final_output_pass(
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
        let owns_update = self
            .owned_update
            .as_ref()
            .is_some_and(|update| update.run.token == token);
        if owns_update {
            self.owned_update
                .as_mut()
                .expect("the run was checked")
                .run
                .finish_pass(context, token)?;
        }
        if discarded {
            self.pass_actions
                .discard_pass(token)
                .map_err(map_pass_action_error)?;
            if owns_update {
                let NativeOwnedUpdate {
                    host,
                    run,
                    publication,
                } = self.owned_update.take().expect("the run was checked");
                let mut host = self.coordinator.resume_host_update(host)?;
                host.frame.restart_encoding_pass()?;
                self.owned_update = Some(NativeOwnedUpdate {
                    host: host.suspend(),
                    run,
                    publication,
                });
            }
            return Ok(());
        }

        let final_pass = self
            .pass_actions
            .finish_pass(token)
            .map_err(map_pass_action_error)?
            .expect("the staged native pass remains available");
        let Some(NativeOwnedUpdate {
            host,
            run: _,
            publication,
        }) = self
            .owned_update
            .take_if(|update| update.run.token == token)
        else {
            let kind = match final_pass {
                NativeFinalPass::RetainPrevious => RetainedOutputKind::Skip,
                NativeFinalPass::RetainedOverlay => RetainedOutputKind::Overlay,
                NativeFinalPass::Semantic(_) => {
                    return Err(NativeHostProtocolError::MultipassOutputChanged.into());
                }
            };
            return self.settle_retained_egui_output(context, output, token, kind);
        };
        let surface = publication.surface;
        let mut paint = match final_pass {
            NativeFinalPass::Semantic(paint) => Some(paint),
            NativeFinalPass::RetainPrevious | NativeFinalPass::RetainedOverlay => None,
        };
        let mut host = self.coordinator.resume_host_update(host)?;
        let mut rejected_retention = None;
        let confirmed = if let Some(paint) = paint.as_mut() {
            if paint.validate_final_output(output) {
                if !paint.confirm_into(&mut host.frame)? {
                    return Err(NativeHostProtocolError::IncompleteTransientPaint(surface).into());
                }
                true
            } else {
                // Keep the accepted input and its queued UI actions. Only this
                // pass's private geometry and renderer payload are discarded.
                discard_unproven_output(output);
                if eframe::current_native_output_token() != Some(token) {
                    return Err(NativeHostProtocolError::MultipassOutputChanged.into());
                }
                rejected_retention = Some(RetainedOutputKind::Rejected.request());
                host.frame.restart_encoding_pass()?;
                false
            }
        } else {
            false
        };
        if !publication.native_input_pending {
            for invalidation in &publication.no_frame_invalidations {
                host.defer_surface(
                    invalidation.surface(),
                    SurfaceUnavailableReason::SurfaceUnavailable,
                )?;
            }
        }
        host.complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)?;
        // This is the only publication of the run. The bridge keeps staging,
        // route replacement and core publication under the existing lock.
        let mut report = host.commit_publishing_root(&mut self.root_surface)?;
        if let Some(acknowledged) = publication.ui_actions {
            self.pass_actions.acknowledge(acknowledged);
        }
        self.pass_actions
            .retire_unavailable(self.coordinator.session());
        if !publication.native_input_pending {
            self.coordinator
                .finish_no_frame_invalidations(&publication.no_frame_invalidations);
        }
        if let Some(retirements) = publication.retirements {
            for retired in self.coordinator.commit_retirements(retirements) {
                self.pass_actions
                    .abandon(retired)
                    .map_err(map_pass_action_error)?;
            }
        }
        let mut outputs = report.take_painted_outputs();
        if outputs.len() != usize::from(confirmed) {
            return Err(NativeHostProtocolError::PaintedOutputCountMismatch {
                expected: usize::from(confirmed),
                actual: outputs.len(),
            }
            .into());
        }
        let painted_output = outputs.pop();
        let repaint_surface = report.repaint_surfaces().contains(&surface);
        self.settle_published_report(
            context,
            Some(token),
            surface,
            publication.application_action,
            report,
            budget,
            true,
        )?;
        let mut focus_pending = false;
        if let Some(painted_output) = painted_output {
            let artifact = painted_output.presentation_artifact();
            let paint = paint
                .as_mut()
                .expect("a confirmed pass owns its paint metadata");
            if let Err(error) =
                self.coordinator
                    .bind_painted_surface(context, token, painted_output, paint)
            {
                let kind = error.kind();
                drop(error.into_output());
                let _ = self.coordinator.abandon_output_token(token);
                return Err(NativeHostProtocolError::OutputBindingFailed(kind).into());
            }
            if let Some(observation) = paint.take_pane_focus_observation() {
                self.pass_actions.retain_pane_focus(
                    token,
                    surface,
                    self.coordinator.viewport_binding(token.viewport_id()),
                    artifact,
                    observation,
                );
                focus_pending = true;
            }
        } else {
            if let Some(retained) = rejected_retention {
                self.settle_egui_retention(
                    context,
                    output,
                    token,
                    RetainedOutputKind::Rejected,
                    retained,
                )?;
            } else {
                self.settle_retained_egui_output(context, output, token, RetainedOutputKind::Skip)?;
            }
        }
        let semantic_retry = retained_pass_requires_semantic_retry(
            !confirmed,
            self.coordinator
                .session()
                .presented_surface(surface)
                .is_some(),
            self.application_actions.has_queued() || repaint_surface,
        );
        let follow_up = publication.follow_up
            || self.pending_service
            || focus_pending
            || self.pass_actions.has_retained_action_work()
            || semantic_retry
            || self.coordinator.has_pending_callback_records();
        require_durable_root_cycle(&self.coordinator, token, follow_up)?;
        if follow_up {
            context.request_repaint_of(ViewportId::ROOT);
            context.request_repaint_of(token.viewport_id());
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
        self.settle_egui_retention(context, output, token, kind, kind.request())
    }

    fn settle_egui_retention(
        &mut self,
        context: &egui::Context,
        output: &mut egui::FullOutput,
        token: NativeOutputToken,
        kind: RetainedOutputKind,
        retained: bool,
    ) -> Result<(), NativeRuntimeError> {
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

    fn settle_published_report(
        &mut self,
        context: &egui::Context,
        token: Option<NativeOutputToken>,
        surface: SurfaceId,
        submitted_application_action: Option<NativeSubmittedApplicationAction>,
        mut report: HostFrameReport,
        budget: &mut NativeInputBudget,
        from_ui_run: bool,
    ) -> Result<(), NativeRuntimeError> {
        if !from_ui_run {
            self.pass_actions
                .retire_unavailable(self.coordinator.session());
        }
        self.pass_actions
            .commit_presentations(report.surface_presentation_commits());
        let application_action_settled = self
            .application_actions
            .settle_submission(submitted_application_action, &report)
            .map_err(|()| NativeHostProtocolError::ApplicationActionOutcomeMissing)?;
        let _ = self.coordinator.settle_host_frame_report(&report)?;
        let native_close_settled = self.coordinator.settle_close_control_report(&report)?;
        let native_admission_settled = self
            .coordinator
            .settle_native_admissions(report.native_admissions())?;
        if surface == self.root_surface
            && let Some(token) = token
        {
            Self::bind_root_registration(
                &mut self.coordinator,
                self.root_surface,
                token,
                report.inputs(),
            )?;
        }
        let native_effects = report.take_native_effects();
        let native_effects_emitted = !native_effects.is_empty();
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
        // Policy decisions are input for a later update. A terminal UI pass
        // must not open another candidate after publishing its owned update.
        let native_close_progress = if from_ui_run {
            let runnable = self
                .coordinator
                .dispatch_bridge()
                .with_close_control(|control| control.has_runnable_policy_work());
            self.pending_service |= runnable;
            Ok(false)
        } else {
            self.coordinator.drive_close_policy(
                context,
                self.close_policy,
                self.root_surface,
                budget,
            )
        };
        let internal_transitions = self.coordinator.take_internal_presentation_transitions();
        let internal_action_settled = if !internal_transitions.is_empty() {
            self.application_actions
                .settle_presentation_transitions(&internal_transitions)
                .map_err(|()| NativeHostProtocolError::ApplicationActionOutcomeMissing)?
        } else {
            false
        };
        let native_close_progress = native_close_progress?;
        if native_close_progress {
            self.pass_actions
                .retire_unavailable(self.coordinator.session());
        }
        let commands_dispatched = self.dispatch_post_commit(context)?
            | self
                .coordinator
                .drive_pointer_capture(context, budget, self.shutdown.is_none())?;
        for repaint_surface in report.repaint_surfaces() {
            match self.coordinator.repaint_viewport(*repaint_surface) {
                Some(egui::ViewportId::ROOT) | None if *repaint_surface == self.root_surface => {
                    context.request_repaint_once_of(egui::ViewportId::ROOT);
                }
                Some(viewport) => context.request_repaint_once_of(viewport),
                None => {}
            }
        }
        let durable_progress = application_action_settled
            || native_admission_settled
            || internal_action_settled
            || native_effects_emitted
            || native_close_settled
            || native_close_progress
            || commands_dispatched;
        request_follow_up_root_cycle(context, false, durable_progress);
        if let Some(token) = token {
            require_durable_root_cycle(&self.coordinator, token, durable_progress)?;
        } else if durable_progress {
            context.request_repaint_of(ViewportId::ROOT);
        }
        Ok(())
    }

    fn dispatch_post_commit(
        &mut self,
        context: &egui::Context,
    ) -> Result<bool, NativeRuntimeError> {
        let progressed = self.coordinator.dispatch_viewport_commands(context)?;
        let Some(command) = self.coordinator.pointer_passthrough_command() else {
            return Ok(progressed);
        };
        if self
            .coordinator
            .dispatch_pointer_passthrough(context, command)
        {
            return Ok(true);
        }
        let removed = self.coordinator.fail_pointer_passthrough_dispatch(command);
        if !removed {
            return Err(NativeHostProtocolError::NativeInputDispatchChanged.into());
        }
        context.request_repaint_once_of(egui::ViewportId::ROOT);
        Ok(true)
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
        self.owned_update = None;
        self.pending_service = false;
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

    pub(crate) fn advance_shutdown(&mut self, ui: &egui::Ui) -> (bool, Vec<NativeRuntimeError>) {
        let context = ui.ctx();
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
    mod output_backpressure;
    mod owned_update;
    use std::error::Error as _;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use dockspace::model::{
        DockAnchor, DockPlacement, DockspaceLayout, DockspaceNode, DockspaceRootLayout,
        DockspaceSurfaceLayout, ItemId, RootId,
    };
    use dockspace::runtime::{
        DockPresentationConfig, SurfacePresentationResult, SurfaceUnavailableReason,
    };
    use eframe::egui::{Ui, WidgetText};

    use super::*;

    const SURFACE: SurfaceId = SurfaceId::new(1);

    struct TestPanes;

    impl PaneView for TestPanes {
        fn title(&self, _item: ItemId) -> Option<WidgetText> {
            Some("Pane".into())
        }

        fn ui(&mut self, _item: ItemId, _ui: &mut Ui) -> egui_dockspace::PaneResponse {
            egui_dockspace::PaneResponse::default()
        }
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
    fn native_construction_preserves_matching_style_and_core_only_settings() {
        let delay = std::time::Duration::from_millis(600);
        let config = DockPresentationConfig::builder()
            .tab_bar_height(36.0)
            .touch_long_press_delay(delay)
            .tab_strip_scroll_line_extent(22.0)
            .pointer_drag_start_distance(9.0)
            .magnetic_center_fraction(0.6)
            .magnetic_outer_band_fraction(0.18)
            .magnetic_outer_band_range(30.0, 80.0)
            .target_release_hysteresis(7.0)
            .build()
            .unwrap();
        let mut session = test_session();
        {
            let mut frame = session.begin_host_frame().unwrap();
            frame.replace_presentation_config(config.clone()).unwrap();
            frame
                .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
                .unwrap();
            frame.commit().unwrap();
        }
        let before = session.version();
        let mut style = DockStyle {
            tab_bar_height: 36.0,
            touch_long_press_delay: delay,
            ghost_offset: egui::vec2(7.0, 9.0),
            ..DockStyle::default()
        };
        style.visuals.tab_active_fill = Some(egui::Color32::RED);
        let state = NativeRuntimeState::new(
            Id::unique("native-matching-presentation-config"),
            session,
            SURFACE,
            TestPanes,
            style.clone(),
            NativeWindowClosePolicy::Cancel,
        )
        .expect("core-only configuration and adapter visuals must remain independently owned");
        assert_eq!(state.coordinator.session().presentation_config(), &config);
        assert_eq!(state.coordinator.session().version(), before);
        assert_eq!(state.style, style);
    }

    #[test]
    fn native_construction_rejects_style_geometry_and_timing_mismatches() {
        for (field, style) in [
            (
                "tab_bar_height",
                DockStyle {
                    tab_bar_height: 36.0,
                    ..DockStyle::default()
                },
            ),
            (
                "touch_long_press_delay",
                DockStyle {
                    touch_long_press_delay: std::time::Duration::from_millis(600),
                    ..DockStyle::default()
                },
            ),
        ] {
            let error = NativeRuntimeState::new(
                Id::unique(("native-style-mismatch", field)),
                test_session(),
                SURFACE,
                TestPanes,
                style,
                NativeWindowClosePolicy::Cancel,
            )
            .err()
            .expect("a valid style must not silently disagree with its session");
            assert_eq!(error.kind(), NativeRuntimeErrorKind::InvalidConfiguration);
            assert_eq!(
                error
                    .source()
                    .and_then(|source| source.downcast_ref::<NativeHostProtocolError>())
                    .copied(),
                Some(NativeHostProtocolError::StylePresentationMismatch { field })
            );
            assert!(error.source().unwrap().to_string().contains(field));
        }
    }

    #[test]
    fn native_construction_keeps_invalid_style_source() {
        let error = NativeRuntimeState::new(
            Id::unique("native-invalid-renderer-style"),
            test_session(),
            SURFACE,
            TestPanes,
            DockStyle {
                ghost_offset: egui::vec2(f32::NAN, 12.0),
                ..DockStyle::default()
            },
            NativeWindowClosePolicy::Cancel,
        )
        .err()
        .expect("adapter-only style fields still require validation");
        assert_eq!(error.kind(), NativeRuntimeErrorKind::InvalidConfiguration);
        let mut source = error.source();
        let mut style_error = None;
        while let Some(current) = source {
            if let Some(original) = current.downcast_ref::<egui_dockspace::DockStyleError>() {
                style_error = Some(original);
                break;
            }
            source = current.source();
        }
        assert_eq!(
            style_error,
            Some(&egui_dockspace::DockStyleError::NonFinite {
                field: "ghost_offset.x",
            })
        );
    }

    #[test]
    fn standalone_service_commits_overload_cancellation_without_ui_or_an_output_scope() {
        let mut state = test_state();
        let context = egui::Context::default();
        let bridge = state.coordinator.dispatch_bridge();
        bridge.freeze_ordinary_for_test();
        let pass_before = context.cumulative_pass_nr_for(ViewportId::ROOT);
        assert!(eframe::current_native_output_token().is_none());

        // Budget yielding has separate clock-driven tests. This semantic drain
        // must not depend on whether the OS deschedules one call for two milliseconds.
        let clock = dockspace::runtime::NativeMonotonicClock::new(|| std::time::Duration::ZERO);
        let mut budget = NativeInputBudget::new(&clock);
        assert!(
            state
                .service_pending_prefixes(&context, None, SURFACE, &mut budget)
                .unwrap()
        );
        assert_eq!(bridge.admission_dispatch(), NativeDispatch::Idle);
        assert_eq!(
            context.cumulative_pass_nr_for(ViewportId::ROOT),
            pass_before
        );
        assert!(eframe::current_native_output_token().is_none());
        assert_eq!(state.output_backlog().ordinary(), 0);
        assert!(state.error().is_none());

        let mut budget = NativeInputBudget::new(&clock);
        assert!(
            state
                .service_pending_prefixes(&context, None, SURFACE, &mut budget)
                .unwrap()
        );
        assert_eq!(bridge.admission_dispatch(), NativeDispatch::Idle);
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
        assert!(RetainedOutputKind::Rejected.omits_accessibility_when_retained());
        assert!(!RetainedOutputKind::Overlay.omits_accessibility_when_retained());
    }

    #[test]
    fn rejected_output_discards_hook_payload_and_delivers_nonvisual_deltas_once() {
        let context = egui::Context::default();
        context.enable_accesskit();
        let mut output = context.run_ui(egui::RawInput::default(), |ui| {
            let _ = ui.button("unconfirmed interactive payload");
            ui.ctx()
                .send_viewport_cmd(egui::ViewportCommand::Title("accepted title".into()));
        });
        output.shapes.push(egui::epaint::ClippedShape {
            clip_rect: egui::Rect::EVERYTHING,
            transform_after_tessellation: Default::default(),
            shape: egui::Shape::Callback(egui::epaint::PaintCallback {
                rect: egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1.0, 1.0)),
                callback: Arc::new(()),
            }),
        });
        let texture = egui::TextureId::Managed(u64::MAX);
        let delta = egui::epaint::ImageDelta::full(
            egui::ColorImage::filled([1, 1], egui::Color32::RED),
            egui::TextureOptions::LINEAR,
        );
        output.textures_delta.push(texture, delta.clone());
        output.textures_delta.free(texture);
        let command = egui::OutputCommand::CopyText("accepted clipboard intent".into());
        output.platform_output.commands.push(command.clone());
        let viewport_commands = output.viewport_output[&ViewportId::ROOT].commands.clone();
        assert!(output.platform_output.accesskit_update.is_some());

        discard_unproven_output(&mut output);

        assert!(
            output.shapes.is_empty(),
            "markers and hook replacements cannot reach the renderer"
        );
        assert!(output.platform_output.accesskit_update.is_none());
        assert!(output.platform_output.events.is_empty());
        assert!(output.textures_delta.set[&texture].as_slice() == [delta]);
        assert!(output.textures_delta.free.contains(&texture));
        assert_eq!(output.platform_output.commands, vec![command]);
        assert_eq!(
            output.viewport_output[&ViewportId::ROOT].commands,
            viewport_commands
        );
        let delivered = output.platform_output.take();
        assert_eq!(delivered.commands.len(), 1);
        assert!(output.platform_output.take().commands.is_empty());
        output.textures_delta.clear();
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
            .request_command(NativeCommand::DockRoot {
                root: RootId::new(1),
                placement,
            })
            .expect("the empty application-action slot accepts one request");
        assert_eq!(
            state
                .request_command(NativeCommand::DockRoot {
                    root: RootId::new(1),
                    placement
                })
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
            Some(Ok(crate::NativeActionStatus::Dock(
                DockspaceActionStatus::Rejected(
                    dockspace::model::DockspaceActionRejection::PolicyDenied
                )
            )))
        ));

        state
            .request_command(NativeCommand::DockRoot {
                root: RootId::new(1),
                placement,
            })
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
                .request_command(NativeCommand::DockRoot {
                    root: RootId::new(1),
                    placement
                })
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
            .request_command(NativeCommand::DockRoot {
                root: RootId::new(1),
                placement: DockPlacement::Center(DockAnchor::Item(ItemId::new(1))),
            })
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
            .request_command(NativeCommand::DockRoot {
                root: RootId::new(1),
                placement,
            })
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
            .request_command(NativeCommand::DockRoot {
                root: RootId::new(1),
                placement,
            })
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
            .request_command(NativeCommand::DockRoot {
                root: RootId::new(1),
                placement,
            })
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
            Some(Ok(crate::NativeActionStatus::Dock(
                DockspaceActionStatus::Rejected(
                    dockspace::model::DockspaceActionRejection::PolicyDenied
                )
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
                .request_command(NativeCommand::DockRootBack {
                    root: RootId::new(1)
                })
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
