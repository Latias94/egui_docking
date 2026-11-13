//! Frame coordination for an existing egui UI callback.

use super::{Dockspace, product_response};
use crate::error_detail::DockspaceErrorSource;
use crate::pass_settlement::{
    PresentationSettlementFailure, ProductPassEnrollment, ProductPassSettlementBatch,
};
use crate::render;
use crate::{
    DockspaceCapability, DockspaceError, DockspaceResponse, DockspaceUnavailableReason, PaneView,
};
use dockspace::model::SurfaceId;
use dockspace::runtime::{
    HostFrameReport, SurfaceUnavailableReason, SurfaceWork, SurfaceWorkDeferred,
};
use egui::emath::GuiRounding;
use egui::{Sense, Ui};
use std::collections::BTreeSet;

impl Dockspace {
    /// Measures, paints, and publishes the sole surface from an existing egui callback.
    ///
    /// Renderer observations, focus, queued UI intent, and raw pointer input
    /// enter one frame before measurement and encoding. This callback publishes
    /// once. UI actions discovered while encoding enter a bounded queue for the
    /// next original egui run; repeated passes never replay those actions.
    ///
    /// [`Self::run_single_surface`] is the recommended entry point: it owns the
    /// complete run and can validate terminal output hooks before publication.
    /// This callback does not own that boundary. A later output hook failure or
    /// discard cannot roll back docking state already published here, or pane
    /// side effects. Inside [`crate::run_ui`], exact renderer reports can still
    /// authorize subsequent input. An unowned callback reports presentation
    /// settlement as unavailable and defers response-driven actions too.
    ///
    /// # Errors
    ///
    /// Returns an error when this is not the sole surface, measurements or UI
    /// intent admission fail, or the frame cannot publish. Before publication,
    /// an error drops the complete docking candidate.
    pub fn show_single_surface(
        &mut self,
        surface: SurfaceId,
        ui: &mut Ui,
        panes: &mut dyn PaneView,
    ) -> Result<DockspaceResponse, DockspaceError> {
        let mut surfaces = self.session.view().surfaces();
        let first = surfaces.next().map(|surface| surface.id());
        let count = usize::from(first.is_some()) + surfaces.count();
        if count != 1 {
            return Err(DockspaceErrorSource::SingleSurfaceRequiresOne { count }.into());
        }
        if first != Some(surface) {
            return Err(DockspaceErrorSource::SurfaceOutsideRoster { surface }.into());
        }
        #[cfg(test)]
        self.drop_target_observations.clear();
        let dock_rect = ui.available_rect_before_wrap();
        let popup_rect = ui.ctx().input(egui::InputState::content_rect).round_ui();
        let settlement_claim = match self.pass_settlement.enroll(ui) {
            ProductPassEnrollment::Acquired(claim) => Some(claim),
            ProductPassEnrollment::Unowned => None,
            ProductPassEnrollment::Conflict => {
                return Err(DockspaceErrorSource::PassSettlementConflict.into());
            }
        };
        let run_sequence = self.ui_actions.enter_run(ui)?;
        let ready_settlement = self.pass_settlement.take_ready(ui);
        let mut preceding_feedback = Vec::new();
        let mut retirement_repaint = false;
        if ready_settlement.lane_changed {
            if let Some(retired) = self
                .session
                .disable_host_pointer()
                .map_err(DockspaceError::from_detail)?
            {
                preceding_feedback.extend_from_slice(retired.manipulation_feedback());
                retirement_repaint |= retired.repaint_required();
            }
            self.pending_focus.clear();
            self.pointer.clear();
        }
        let presentation_failure = ready_settlement.presentation_failure;
        let presentation_settlement_available =
            settlement_claim.is_some() && presentation_failure.is_none();
        for presentation in ready_settlement.presentations {
            let artifact = presentation.output.presentation_artifact();
            if self
                .session
                .report_surface_presentation(presentation.output, presentation.result)
                .is_ok()
                && let Some(focus) = presentation.pane_focus_observation
            {
                self.pending_focus.push(artifact, focus);
            }
        }
        if presentation_settlement_available {
            self.session
                .enable_host_pointer()
                .map_err(DockspaceError::from_detail)?;
        } else {
            if let Some(retired) = self
                .session
                .disable_host_pointer()
                .map_err(DockspaceError::from_detail)?
            {
                preceding_feedback.extend_from_slice(retired.manipulation_feedback());
                retirement_repaint |= retired.repaint_required();
            }
            self.pointer.clear();
        }
        let mut pointer = self.pointer.clone();
        let mut pending_actions = None;
        let mut observed_at = None;
        let mut deferred_focus = crate::focus::DeferredTabFocus::default();
        let mut focus_before_pointer = None;
        let mut accepted_focus = Vec::new();
        let mut missing = BTreeSet::new();
        let mut defer_measurement = false;
        let mut pane_focus_observation = None;
        let mut pane_focus_pending = None;
        let mut pane_focus_needs_repaint = false;
        let mut pane_focus_capability =
            DockspaceCapability::Unavailable(DockspaceUnavailableReason::SurfaceBoundsUnavailable);
        let mut pointer_receivers = Vec::new();
        let mut focus_bindings = Vec::new();
        let mut had_plan = false;
        let mut output_backpressured = false;
        let mut proof = None;
        let result = (|| -> Result<HostFrameReport, DockspaceError> {
            #[cfg(feature = "motion")]
            let motion = self.motion_frame(ui);
            let mut frame = self
                .session
                .begin_host_frame()
                .map_err(DockspaceError::from_detail)?;
            #[cfg(feature = "motion")]
            frame
                .set_motion(motion)
                .map_err(DockspaceError::from_detail)?;
            let actual_focus = frame.view().surface(surface).map(|view| {
                crate::focus::sample_host_focus(
                    ui.ctx(),
                    view,
                    panes,
                    ui.make_persistent_id((self.id, "workspace-focus", surface)),
                    ui.ctx()
                        .layer_transform_to_global(ui.layer_id())
                        .map_or(dock_rect, |transform| transform * dock_rect),
                    pointer.presented_focus_bindings(&frame, surface),
                )
            });
            accepted_focus = self
                .pending_focus
                .submit_current(&mut frame, surface)
                .map_err(DockspaceError::from_detail)?;
            if let Some(actual_focus) = actual_focus {
                frame
                    .report_host_focus(surface, actual_focus)
                    .map_err(DockspaceError::from_detail)?;
            }
            for action in self.ui_actions.before(run_sequence) {
                frame
                    .submit_deferred_surface_action(action)
                    .map_err(DockspaceError::from_detail)?;
            }
            if presentation_settlement_available {
                let input = pointer.prepare(&frame, surface, ui);
                observed_at = input.observed_at();
                focus_before_pointer = frame.pane_focus_request(surface);
                self.pointer.record_rejected_run(&pointer);
                deferred_focus = input
                    .submit_into(&mut frame)
                    .map_err(DockspaceError::from_detail)?;
            }
            frame
                .begin_encoding_pass()
                .map_err(DockspaceError::from_detail)?;
            let (measure, backpressured) = match frame
                .surface_work(surface)
                .map_err(DockspaceError::from_detail)?
            {
                SurfaceWork::Ready(plan) => (
                    plan.measurement_refresh().is_allowed() && !self.measurement_deferred,
                    false,
                ),
                SurfaceWork::Deferred(SurfaceWorkDeferred::Unmeasured) => (true, false),
                SurfaceWork::Deferred(SurfaceWorkDeferred::OutputCapacity) => (false, true),
            };
            let mut resources = render::PaintResources::default();
            output_backpressured = backpressured;
            if measure {
                missing.extend(render::measure_surface(
                    &mut frame,
                    surface,
                    ui,
                    dock_rect,
                    popup_rect,
                    panes,
                    &self.style,
                    Some(&mut resources),
                )?);
            }
            self.ui_actions
                .stage_touch_menu(run_sequence, ui, self.id, surface, frame.version());
            let capture = settlement_claim
                .as_ref()
                .map(|_| crate::output_proof::SurfaceOutputCapture::begin_multilayer(ui.painter()));
            if backpressured {
                crate::focus::retain_presented_pane_focus(
                    ui.ctx(),
                    pointer.presented_focus_bindings(&frame, surface),
                );
            }
            if !backpressured
                && let Some(plan) = frame
                    .paint_plan(surface)
                    .map_err(DockspaceError::from_detail)?
            {
                let paint = render::paint_surface(
                    ui,
                    self.id,
                    plan,
                    panes,
                    &self.style,
                    &self.pointer.input(),
                    resources,
                    if presentation_settlement_available {
                        render::PointerActionAuthority::HostJournal
                    } else {
                        render::PointerActionAuthority::LocalResponses
                    },
                    #[cfg(feature = "native-render-support")]
                    None,
                )
                .map_err(DockspaceError::from_detail)?;
                missing.extend(paint.missing_items);
                defer_measurement = paint.defer_measurement;
                pane_focus_observation = paint.pane_focus_observation;
                pane_focus_pending = paint.pane_focus_pending;
                pane_focus_needs_repaint = paint.pane_focus_needs_repaint;
                pane_focus_capability = paint.pane_focus_capability;
                pointer_receivers = paint.receivers;
                focus_bindings = paint.pane_focus_bindings;
                self.ui_actions
                    .capture(&mut pending_actions, run_sequence, paint.local_actions)?;
                #[cfg(test)]
                {
                    self.drop_target_observations = paint.drop_target_observations;
                }
                if settlement_claim.is_some()
                    && let Some(receipt) = paint.execution_receipt
                {
                    frame
                        .confirm_surface_painted(receipt)
                        .map_err(DockspaceError::from_detail)?;
                }
                had_plan = true;
            } else {
                ui.allocate_rect(dock_rect, Sense::hover());
                let visuals = self.style.resolved_visuals(ui.visuals());
                ui.painter()
                    .rect_filled(dock_rect, 0.0, visuals.workspace_fill);
            }
            proof = capture.and_then(|capture| capture.finish(ui.painter()));
            frame
                .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
                .map_err(DockspaceError::from_detail)?;
            frame.commit().map_err(DockspaceError::from_detail)
        })();
        let mut report = match result {
            Ok(report) => report,
            Err(error) => {
                self.pointer.record_rejected_run(&pointer);
                return Err(error);
            }
        };
        self.ui_actions
            .publish(pending_actions, run_sequence, &report);
        let output = report.take_painted_outputs().pop();
        pointer.observe(report.surface_presentation_commits());
        if let Some(output) = output.as_ref() {
            pointer.emitted(
                output.presentation_artifact(),
                ui.layer_id(),
                ui.ctx().cumulative_pass_nr().saturating_add(1),
                ui.ctx()
                    .layer_transform_to_global(ui.layer_id())
                    .is_none_or(|transform| transform == egui::emath::TSTransform::IDENTITY),
                pointer_receivers,
                focus_bindings,
            );
        }
        self.pointer = pointer;
        self.pending_focus.settle(&accepted_focus, &report);
        deferred_focus.apply(ui.ctx());
        if deferred_focus.handled()
            && let Some(request) = self.session.pane_focus_request(surface)
            && Some(request) != focus_before_pointer
        {
            crate::focus::retain_tab_focus_order(
                ui.ctx(),
                self.id,
                ui.ctx().viewport_id(),
                request,
                deferred_focus.target(),
            );
        }
        crate::pointer::schedule_pointer_continuation(
            &self.session,
            ui.ctx(),
            ui.ctx().viewport_id(),
            observed_at,
        );
        self.measurement_deferred = defer_measurement;
        let needs_presentation_settlement = defer_measurement || pane_focus_needs_repaint;
        let continuation_awaits_presentation = self
            .session
            .pointer_continuation_awaits_presentation(surface);
        let motion_active = {
            #[cfg(feature = "motion")]
            {
                had_plan && self.session.surface_motion_is_active(surface)
            }
            #[cfg(not(feature = "motion"))]
            {
                false
            }
        };
        let settlement_staged = settlement_claim.is_some_and(|claim| {
            claim.stage(ProductPassSettlementBatch {
                emission_incomplete: had_plan && output.is_none(),
                output,
                proof,
                pane_focus_observation,
                pane_focus_pending,
                needs_repaint: needs_presentation_settlement
                    || continuation_awaits_presentation
                    || motion_active,
            })
        });
        let presentation_settlement_capability = match presentation_failure {
            Some(PresentationSettlementFailure::OutputMismatch) => {
                DockspaceCapability::Unavailable(
                    DockspaceUnavailableReason::PresentationOutputMismatch,
                )
            }
            Some(PresentationSettlementFailure::Abandoned) => DockspaceCapability::Unavailable(
                DockspaceUnavailableReason::PresentationSettlementAbandoned,
            ),
            None if settlement_staged => DockspaceCapability::Supported,
            None => DockspaceCapability::Unavailable(
                DockspaceUnavailableReason::PresentationSettlementRequired,
            ),
        };
        if !settlement_staged {
            pane_focus_capability = DockspaceCapability::Unavailable(
                DockspaceUnavailableReason::PresentationSettlementRequired,
            );
        }
        if !output_backpressured
            && (!had_plan
                || report.repaint_surfaces().contains(&surface)
                || !self.ui_actions.is_empty()
                || (needs_presentation_settlement && settlement_staged)
                || retirement_repaint
                || motion_active)
        {
            ui.ctx().request_repaint();
        }
        let mut response = product_response(
            &report,
            surface,
            missing,
            had_plan,
            had_plan,
            presentation_settlement_capability,
            pane_focus_capability,
        )?;
        response.include_preceding_feedback(preceding_feedback);
        Ok(response)
    }
}
