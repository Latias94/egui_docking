//! One unpublished docking update across the actual passes of an owned egui run.

use super::*;
use crate::pass_settlement::{
    DockspaceOutput, OwnedRunOutput, PendingPaneFocusObservation, ProductPassClaim,
    ProductPassSettlementRun, ProductPointerPacket, RUN_SCOPE_TAG, pointer_packet,
};
use dockspace::runtime::{
    DeferredSurfaceAction, DockspaceContextMenuRequest, DockspaceUpdate,
    SurfaceProgramExecutionReceipt, SurfaceWork, SurfaceWorkDeferred,
};
use egui::{Context, RawInput, UiBuilder, UiStackInfo};

const UI_ACTION_CAPACITY: usize = 64;

struct UiActionRun {
    context: Context,
    viewport: egui::ViewportId,
    frame: u64,
    first_pass: u64,
    packet: Option<ProductPointerPacket>,
}

impl UiActionRun {
    fn capture(ui: &Ui) -> Self {
        Self {
            context: ui.ctx().clone(),
            viewport: ui.ctx().viewport_id(),
            frame: ui.ctx().cumulative_frame_nr(),
            first_pass: ui
                .ctx()
                .cumulative_pass_nr()
                .saturating_sub(u64::try_from(ui.ctx().current_pass_index()).unwrap_or(u64::MAX)),
            packet: pointer_packet(ui),
        }
    }

    fn same_run(&self, other: &Self) -> bool {
        self.context == other.context
            && self.viewport == other.viewport
            && match (&self.packet, &other.packet) {
                (Some(left), Some(right)) => left.same_run(right),
                (None, None) => self.frame == other.frame && self.first_pass == other.first_pass,
                _ => false,
            }
    }
}

pub(super) struct UiActionBatch {
    run: u64,
    actions: Vec<UiActionIntent>,
}

struct UiActionIntent {
    origin: Id,
    action: DeferredSurfaceAction,
}

/// UI intent enters a later original run, independently of egui pass retries.
#[derive(Default)]
pub(super) struct UiActionQueue {
    batches: std::collections::VecDeque<UiActionBatch>,
    current: Option<UiActionRun>,
    sequence: u64,
    touch_menu: Option<(u64, DockspaceContextMenuRequest)>,
}

impl UiActionQueue {
    pub(super) fn enter_run(&mut self, ui: &Ui) -> Result<u64, DockspaceError> {
        let current = UiActionRun::capture(ui);
        if self
            .current
            .as_ref()
            .is_none_or(|previous| !previous.same_run(&current))
        {
            self.sequence = self
                .sequence
                .checked_add(1)
                .ok_or(DockspaceErrorSource::PassSettlementConflict)?;
            self.current = Some(current);
        }
        Ok(self.sequence)
    }

    pub(super) fn before(&self, run: u64) -> impl Iterator<Item = DeferredSurfaceAction> + '_ {
        self.batches
            .iter()
            .filter(move |batch| batch.run < run)
            .flat_map(|batch| batch.actions.iter().map(|intent| intent.action.clone()))
    }

    /// Preserve the first witnessed batch across repeated passes. Different
    /// original runs with the same intent remain independent events.
    pub(super) fn capture(
        &self,
        pending: &mut Option<UiActionBatch>,
        run: u64,
        actions: Vec<product_render::EguiSurfaceAction>,
    ) -> Result<(), DockspaceError> {
        if actions.is_empty() {
            return Ok(());
        }
        let retained = self
            .batches
            .iter()
            .map(|batch| batch.actions.len())
            .sum::<usize>();
        let previous = self.batches.back().filter(|batch| batch.run == run);
        let batch = pending.get_or_insert_with(|| UiActionBatch {
            run,
            actions: Vec::new(),
        });
        for action in actions {
            let intent = UiActionIntent {
                origin: action.origin(),
                action: action.into_deferred(),
            };
            let old = batch
                .actions
                .iter()
                .chain(previous.into_iter().flat_map(|batch| &batch.actions))
                .find(|old| old.origin == intent.origin);
            if let Some(old) = old {
                if !old.action.same_intent(&intent.action) {
                    return Err(DockspaceErrorSource::PassSettlementConflict.into());
                }
            } else {
                // Earlier events remain owned until publication, but a successful
                // update acknowledges them before retaining this run's events.
                let surviving = self
                    .batches
                    .iter()
                    .filter(|batch| batch.run >= run)
                    .map(|batch| batch.actions.len())
                    .sum::<usize>();
                if surviving
                    .saturating_add(batch.actions.len())
                    .saturating_add(1)
                    > UI_ACTION_CAPACITY
                    || retained > UI_ACTION_CAPACITY
                {
                    return Err(DockspaceErrorSource::PassSettlementConflict.into());
                }
                batch.actions.push(intent);
            }
        }
        Ok(())
    }

    pub(super) fn publish(
        &mut self,
        pending: Option<UiActionBatch>,
        run: u64,
        report: &HostFrameReport,
    ) {
        self.batches.retain(|batch| batch.run >= run);
        if let Some(batch) = pending {
            self.batches.push_back(batch);
        }
        let same_run_menu = self.touch_menu.filter(|(source_run, _)| *source_run == run);
        self.touch_menu = report
            .inputs()
            .iter()
            .find_map(|input| match input {
                dockspace::runtime::HostInputOutcome::Touch(
                    dockspace::runtime::DockspaceTouchEvent::ContextMenuRequested(request),
                ) => Some((run, *request)),
                _ => None,
            })
            .or(same_run_menu);
    }

    pub(super) fn stage_touch_menu(
        &self,
        run: u64,
        ui: &Ui,
        instance: Id,
        surface: SurfaceId,
        version: WorkspaceVersion,
    ) {
        if let Some((source_run, request)) = self.touch_menu
            && source_run < run
            && request.surface() == surface
            && request.version() == version
        {
            crate::touch_menu::stage_touch_context_menu(ui, instance, request);
        }
    }

    pub(super) fn is_empty(&self) -> bool {
        self.batches.is_empty() && self.touch_menu.is_none()
    }
}

#[cfg(test)]
#[path = "product_owned_run_tests.rs"]
mod tests;

struct EncodedPass {
    claim: ProductPassClaim,
    receipt: Option<SurfaceProgramExecutionReceipt>,
    proof: Option<crate::output_proof::SurfaceOutputProof>,
    missing: BTreeSet<ItemId>,
    receivers: Vec<product_render::ProductReceiverBinding>,
    focus_bindings: Vec<(ItemId, Id)>,
    layer: egui::LayerId,
    completed_pass: u64,
    coordinates_supported: bool,
    pane_focus: Option<PreparedPaneFocusObservation>,
    pending_focus: Option<PendingPaneFocusObservation>,
    focus_capability: DockspaceCapability,
    defer_measurement: bool,
    had_plan: bool,
}

impl Dockspace {
    /// Owns a complete egui run for one docking surface.
    ///
    /// Raw input is reduced once. Each actual egui pass measures and encodes
    /// into the same unpublished candidate; discarded passes release their
    /// private payloads. Only the terminal output, after output hooks, can
    /// publish docking state and obtain a renderer settlement receipt. Pane UI
    /// runs once per actual pass. A failed run is never automatically replayed.
    /// A rejected final proof leaves the preceding committed gesture intact;
    /// its raw input and producer watermark remain in the discarded candidate.
    ///
    /// Chrome actions produced during UI encoding enter a bounded queue for the
    /// next run. The returned response describes this run's single publication.
    /// Renderer submission and actual presentation remain the caller's job.
    /// Use [`Self::run_single_surface_with_ui`] to compose application panels
    /// around the docking surface within the same owned run.
    pub fn run_single_surface(
        &mut self,
        context: &Context,
        input: RawInput,
        surface: SurfaceId,
        panes: &mut dyn PaneView,
    ) -> Result<(DockspaceOutput, DockspaceResponse), DockspaceError> {
        self.run_single_surface_with_ui(context, input, surface, panes, |_| {})
    }

    /// Owns a complete egui run with application UI before the docking surface.
    ///
    /// `before_docking` runs once per actual egui pass, before docking
    /// measurement and painting. It may use [`egui::Panel::top`],
    /// [`egui::Panel::bottom`], or [`egui::Panel::left`] on the supplied UI;
    /// docking uses its remaining available rectangle.
    ///
    /// Publication and renderer settlement follow [`Self::run_single_surface`].
    /// Repeated passes invoke both the composition callback and pane UI again.
    /// Their external application state and side effects are not rolled back if
    /// the final output is rejected. Record panel command intents outside the
    /// callback, account for repeated passes, and apply them only after this
    /// function returns successfully. Do not nest another egui run or re-enter
    /// Dockspace from the callback.
    pub fn run_single_surface_with_ui(
        &mut self,
        context: &Context,
        input: RawInput,
        surface: SurfaceId,
        panes: &mut dyn PaneView,
        mut before_docking: impl FnMut(&mut Ui),
    ) -> Result<(DockspaceOutput, DockspaceResponse), DockspaceError> {
        let surfaces = self
            .session
            .view()
            .surfaces()
            .map(|entry| entry.id())
            .collect::<Vec<_>>();
        if surfaces.len() != 1 {
            return Err(DockspaceErrorSource::SingleSurfaceRequiresOne {
                count: surfaces.len(),
            }
            .into());
        }
        if surfaces[0] != surface {
            return Err(DockspaceErrorSource::SurfaceOutsideRoster { surface }.into());
        }
        let run = ProductPassSettlementRun::begin(context, &input);
        let mut run_sequence = None;
        let mut pending_actions = None;
        let mut update: Option<DockspaceUpdate> = None;
        let mut pointer = self.pointer.clone();
        let mut terminal = None;
        let mut error = None;
        let mut observed_at = None;
        let mut deferred_focus = Vec::new();
        let mut accepted_focus = Vec::new();
        let mut preceding_feedback = Vec::new();
        let mut retirement_repaint = false;
        let mut pass_count = 0_usize;
        let mut output = OwnedRunOutput::new(context.run_ui(input, |ui| {
            let Some(scope) = run.begin_pass(ui) else {
                error.get_or_insert_with(|| DockspaceErrorSource::PassSettlementConflict.into());
                return;
            };
            let _callback = scope.enter(ui);
            terminal = None;
            if error.is_some() {
                return;
            }
            before_docking(ui);
            let result = ui
                .scope_builder(
                    UiBuilder::new()
                        .id_salt(RUN_SCOPE_TAG)
                        .ui_stack_info(UiStackInfo::default().with_tag_value(RUN_SCOPE_TAG, scope)),
                    |ui| -> Result<EncodedPass, DockspaceError> {
                        let ProductPassEnrollment::Acquired(claim) =
                            self.pass_settlement.enroll(ui)
                        else {
                            return Err(DockspaceErrorSource::PassSettlementConflict.into());
                        };
                        let first = pass_count == 0;
                        pass_count += 1;
                        let sequence = self.ui_actions.enter_run(ui)?;
                        run_sequence = Some(sequence);
                        if first {
                            let ready = self.pass_settlement.take_ready(ui);
                            if ready.lane_changed {
                                if let Some(retired) = self
                                    .session
                                    .disable_host_pointer()
                                    .map_err(DockspaceError::from_detail)?
                                {
                                    preceding_feedback
                                        .extend_from_slice(retired.manipulation_feedback());
                                    retirement_repaint |= retired.repaint_required();
                                }
                                pointer.clear();
                                self.pending_focus.clear();
                            }
                            for presentation in ready.presentations {
                                let artifact = presentation.output.presentation_artifact();
                                self.session
                                    .report_surface_presentation(
                                        presentation.output,
                                        presentation.result,
                                    )
                                    .map_err(DockspaceError::from_detail)?;
                                if let Some(focus) = presentation.pane_focus_observation {
                                    self.pending_focus.push((artifact, focus));
                                }
                            }
                            self.session
                                .enable_host_pointer()
                                .map_err(DockspaceError::from_detail)?;
                        }
                        let mut frame = if let Some(update) = update.take() {
                            let mut frame = self
                                .session
                                .resume_update(update)
                                .map_err(DockspaceError::from_detail)?;
                            frame
                                .restart_encoding_pass()
                                .map_err(DockspaceError::from_detail)?;
                            frame
                        } else {
                            self.session
                                .begin_host_frame()
                                .map_err(DockspaceError::from_detail)?
                        };
                        if first {
                            let dock_rect = ui.available_rect_before_wrap();
                            let actual_focus = frame.view().surface(surface).map(|view| {
                                crate::product_focus::sample_host_focus(
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
                            for (artifact, focus) in &self.pending_focus {
                                if frame.presented_surface(surface).is_some_and(|current| {
                                    current.matches_presentation_artifact(*artifact)
                                }) {
                                    frame
                                        .submit_pane_focus_observation(focus.clone())
                                        .map_err(DockspaceError::from_detail)?;
                                    accepted_focus.push(*artifact);
                                }
                            }
                            if let Some(actual_focus) = actual_focus {
                                frame
                                    .report_host_focus(surface, actual_focus)
                                    .map_err(DockspaceError::from_detail)?;
                            }
                            for action in self.ui_actions.before(sequence) {
                                frame
                                    .submit_deferred_surface_action(action)
                                    .map_err(DockspaceError::from_detail)?;
                            }
                            let input = pointer.prepare(&frame, surface, ui, false);
                            observed_at = input.observed_at();
                            // This owner admits input only on its first pass and
                            // abandons the run on error. Keep its consumed-run
                            // marker private with the candidate: leaking a rejected
                            // draft would cancel the preceding committed gesture.
                            deferred_focus = input
                                .submit_into(&mut frame)
                                .map_err(DockspaceError::from_detail)?;
                            frame
                                .begin_encoding_pass()
                                .map_err(DockspaceError::from_detail)?;
                        }
                        let active = frame.has_active_gesture();
                        let (measure, backpressured) = match frame
                            .surface_work(surface)
                            .map_err(DockspaceError::from_detail)?
                        {
                            SurfaceWork::Ready(plan) => (
                                !(active
                                    || plan.has_manipulation_preview()
                                    || self.measurement_deferred),
                                false,
                            ),
                            SurfaceWork::Deferred(SurfaceWorkDeferred::Unmeasured) => (true, false),
                            SurfaceWork::Deferred(SurfaceWorkDeferred::OutputCapacity) => {
                                (false, true)
                            }
                        };
                        let mut missing = BTreeSet::new();
                        let mut resources = product_render::PaintResources::default();
                        if measure {
                            missing.extend(product_render::measure_surface(
                                &mut frame,
                                surface,
                                ui,
                                ui.available_rect_before_wrap(),
                                ui.ctx().input(egui::InputState::content_rect).round_ui(),
                                panes,
                                &self.style,
                                Some(&mut resources),
                            )?);
                        }
                        self.ui_actions.stage_touch_menu(
                            sequence,
                            ui,
                            self.id,
                            surface,
                            frame.version(),
                        );
                        let capture = crate::output_proof::SurfaceOutputCapture::begin_multilayer(
                            ui.painter(),
                        );
                        if backpressured {
                            crate::product_focus::retain_presented_pane_focus(
                                &context,
                                pointer.presented_focus_bindings(&frame, surface),
                            );
                        }
                        let paint = if !backpressured {
                            frame
                                .paint_plan(surface)
                                .map_err(DockspaceError::from_detail)?
                                .map(|plan| {
                                    product_render::paint_surface(
                                        ui,
                                        self.id,
                                        plan,
                                        panes,
                                        &self.style,
                                        resources,
                                        product_render::PointerActionAuthority::HostJournal,
                                        #[cfg(feature = "native-render-support")]
                                        None,
                                    )
                                })
                                .transpose()
                                .map_err(DockspaceError::from_detail)?
                        } else {
                            None
                        };
                        let had_plan = paint.is_some();
                        let mut pass = EncodedPass {
                            claim,
                            receipt: None,
                            proof: capture.finish(ui.painter()),
                            missing,
                            receivers: Vec::new(),
                            focus_bindings: Vec::new(),
                            layer: ui.layer_id(),
                            completed_pass: context.cumulative_pass_nr().saturating_add(1),
                            coordinates_supported: context
                                .layer_transform_to_global(ui.layer_id())
                                .is_none_or(|transform| {
                                    transform == egui::emath::TSTransform::IDENTITY
                                }),
                            pane_focus: None,
                            pending_focus: None,
                            focus_capability: DockspaceCapability::Unavailable(
                                DockspaceUnavailableReason::SurfaceBoundsUnavailable,
                            ),
                            defer_measurement: false,
                            had_plan,
                        };
                        if let Some(paint) = paint {
                            pass.missing.extend(paint.missing_items);
                            pass.receipt = paint.execution_receipt;
                            pass.receivers = paint.receivers;
                            pass.focus_bindings = paint.pane_focus_bindings;
                            pass.pane_focus = paint.pane_focus_observation;
                            pass.pending_focus = paint.pane_focus_pending;
                            pass.focus_capability = paint.pane_focus_capability;
                            pass.defer_measurement = paint.defer_measurement;
                            self.ui_actions.capture(
                                &mut pending_actions,
                                sequence,
                                paint.local_actions,
                            )?;
                            #[cfg(test)]
                            {
                                self.drop_target_observations = paint.drop_target_observations;
                            }
                        }
                        update = Some(frame.suspend());
                        Ok(pass)
                    },
                )
                .inner;
            match result {
                Ok(pass) => terminal = Some(pass),
                Err(failure) => error = Some(failure),
            }
        }));
        let texture_guard = output.take_textures(context);
        if let Some(error) = error {
            return Err(error);
        }
        let layer_order = run.layer_order_witness();
        let Some(pass) = terminal else {
            return Err(DockspaceErrorSource::PassSettlementConflict.into());
        };
        if !run.owns_only(&self.pass_settlement)
            || (pass.had_plan && pass.receipt.is_none())
            || (pass.had_plan && !output.check(pass.proof.as_ref(), &layer_order))
        {
            return Err(DockspaceErrorSource::PassSettlementConflict.into());
        }
        let mut frame = self
            .session
            .resume_update(update.take().unwrap())
            .map_err(DockspaceError::from_detail)?;
        if let Some(receipt) = pass.receipt {
            frame
                .confirm_surface_painted(receipt)
                .map_err(DockspaceError::from_detail)?;
        }
        frame
            .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
            .map_err(DockspaceError::from_detail)?;
        let mut report = frame.commit().map_err(DockspaceError::from_detail)?;
        self.ui_actions
            .publish(pending_actions, run_sequence.unwrap(), &report);
        let painted = report.take_painted_outputs().pop();
        pointer.observe(report.surface_presentation_commits());
        if let Some(painted) = painted.as_ref() {
            pointer.emitted(
                painted.presentation_artifact(),
                pass.layer,
                pass.completed_pass,
                pass.coordinates_supported,
                pass.receivers,
                pass.focus_bindings,
            );
        }
        self.pointer = pointer;
        self.measurement_deferred = pass.defer_measurement;
        self.pending_focus.retain(|(artifact, _)| {
            !accepted_focus.contains(artifact)
                && !report
                    .surface_presentation_commits()
                    .iter()
                    .any(|commit| commit.settles(*artifact))
        });
        for id in deferred_focus {
            // run_ui has ended, so prev_pass contains its terminal registrations.
            let target = context.viewport(|viewport| viewport.prev_pass.widgets.get(id).copied());
            if let Some(target) = target
                && target.enabled
                && target.visible
                && target.sense.is_focusable()
                && context.memory(|memory| memory.allows_interaction(target.layer_id))
            {
                context.memory_mut(|memory| memory.request_focus(id));
            }
        }
        crate::product_pointer::schedule_pointer_continuation(&self.session, context, observed_at);
        let needs_repaint = pass.defer_measurement
            || pass.pane_focus.is_some()
            || pass.pending_focus.is_some()
            || self
                .session
                .pointer_continuation_awaits_presentation(surface);
        if report.repaint_surfaces().contains(&surface)
            || !self.ui_actions.is_empty()
            || retirement_repaint
        {
            context.request_repaint();
        }
        let staged = pass.claim.stage(ProductPassSettlementBatch {
            emission_incomplete: pass.had_plan && painted.is_none(),
            output: painted,
            proof: pass.proof,
            pane_focus_observation: pass.pane_focus,
            pane_focus_pending: pass.pending_focus,
            needs_repaint,
        });
        debug_assert!(staged, "the preflighted terminal enrollment remains exact");
        self.presentation_settlement_was_available = staged;
        let mut response = product_response(
            &report,
            surface,
            pass.missing,
            pass.had_plan,
            pass.had_plan,
            DockspaceCapability::Supported,
            pass.focus_capability,
        )?;
        response.include_preceding_feedback(preceding_feedback);
        output.restore_textures(texture_guard);
        let (output, receipt) = run.complete_owned(output);
        Ok((DockspaceOutput::new(output, receipt), response))
    }
}
