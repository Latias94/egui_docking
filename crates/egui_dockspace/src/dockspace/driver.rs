//! Per-cycle egui measurement, paint, and atomic publication driver.

use std::collections::{BTreeMap, BTreeSet};

use dockspace::command::WorkspaceCommand;
use dockspace::engine::{
    CoreHostFrame, EngineInput, HostFrameView, HostPresentationDisposition, HostPresentationSlot,
    HostPresentationUnavailableReason, PreparedSurfaceContribution, PreparedSurfacePaintCandidate,
    SurfaceContributionToken,
};
use dockspace::frame::PanelFocus;
use dockspace::geometry::{LogicalRect, LogicalSize};
use dockspace::ids::{RootId, SurfaceId};
use dockspace::intent::ContainedPlacementUnavailable;
use dockspace::interaction::EscapeDelivery;
use dockspace::presentation_observation::{
    NativeStagingPresentation, SurfacePresentationOutputTicket,
};
use dockspace::scene::{SurfaceScene, SurfaceSceneStamp};
use dockspace::scene_manifest::{MeasurementUnavailableReason, SurfaceMeasurements};
use dockspace::transition::InputOutcome;
use dockspace::viewport_focus::PaneFocusIntent;
use dockspace::viewport_focus::PaneFocusObservation;
use egui::{Context, FullOutput, RawInput, Ui, ViewportId};

use super::engine_owner::{EguiPreparedHostFrameCommit, prepared_host_transition};
use super::host_frame::{EguiCoreFramePhase, EguiSurfacePass, HostFrameState, HostFrameStateSlot};
use super::native_binding::PreparedNativeBindingCommit;
use super::{
    Dockspace, EGUI_APPLICATION_INPUT_SOURCE, EGUI_RENDER_INPUT_SOURCE, EguiEngineOwner,
    EguiFrameScheduleKey, EguiHostFrameMode, EguiInputAuthority, NativeBindingError,
    requested_contained_resize_rect, should_request_presentation_follow_up_pass,
};
use super::{PaneFocusRequestFence, observe_surface_pane_focus};
use crate::error::DockspaceError;
use crate::pane::PaneView;
use crate::pointer_input::PreparedPointerInput;
use crate::presentation_settlement::EguiOuterSurfaceOutput;
use crate::projection::EguiSurfaceMeasurementSet;
use crate::projection::EguiSurfacePaintResources;
use crate::receiver::PaintReceiverRegistrations;
use crate::render::{
    EguiNativeStagingPublication, EguiSurfaceContribution, EguiSurfaceDraft,
    EguiSurfacePublicationMode, PreparedEguiFrameAcceptance, SemanticInputPosition,
    StagedSemanticInput,
};
use crate::renderer::{ContainedResizeEdge, RenderAction, RenderActionPosition, paint_surface};
use crate::response::{
    DockspaceCapability, DockspaceSurfaceStatus, DockspaceUnavailableReason, HostFrameResponse,
    SurfacePaintResponse,
};

/// Complete-roster egui frame owned by an outer renderer host.
///
/// Unlike ordinary egui callbacks, this capability is retained across every
/// surface and every repeated pass in one real host frame. Finishing it returns
/// the final [`FullOutput`] for each painted surface bound to its opaque
/// renderer-settlement capability.
pub struct EguiOuterHostFrame<'a> {
    pub(super) inner: DockspaceHostFrame<'a>,
}

impl EguiOuterHostFrame<'_> {
    /// Returns the host-owned schedule identity.
    #[must_use]
    pub const fn key(&self) -> EguiFrameScheduleKey {
        self.inner.key()
    }

    /// Returns the sealed post-observation authority for this outer frame.
    #[must_use]
    pub fn view(&self) -> HostFrameView<'_> {
        self.inner.view()
    }

    /// Returns the complete core-derived logical surface roster.
    #[must_use]
    pub fn expected_surfaces(&self) -> impl ExactSizeIterator<Item = SurfaceId> + '_ {
        self.inner.expected_surfaces()
    }

    /// Measures and paints one surface pass without reducing the core.
    pub fn show_surface(
        &mut self,
        surface: SurfaceId,
        ui: &mut Ui,
        panes: &mut dyn PaneView,
    ) -> Result<SurfacePaintResponse, DockspaceError> {
        self.inner.show_surface(surface, ui, panes)
    }

    /// Supplies an explicit unavailable fact for one frozen surface.
    pub fn mark_surface_unavailable(
        &mut self,
        surface: SurfaceId,
        reason: MeasurementUnavailableReason,
    ) -> Result<(), DockspaceError> {
        self.inner.mark_surface_unavailable(surface, reason)
    }

    /// Runs and owns one complete egui surface pass.
    ///
    /// Owning [`Context::run_ui`] inside this capability prevents an unrelated
    /// or earlier [`FullOutput`] from being asserted as final-presentation
    /// proof. Repeated egui passes replace both the surface draft and retained
    /// output; only the last completed pass is returned by [`Self::finish`].
    pub fn run_surface(
        &mut self,
        surface: SurfaceId,
        context: &Context,
        input: RawInput,
        panes: &mut dyn PaneView,
    ) -> Result<SurfacePaintResponse, DockspaceError> {
        let viewport = input.viewport_id;
        let mut paint = None;
        let mut pointer_events = None;
        let output = context.run_ui(input, |ui| {
            pointer_events.get_or_insert_with(|| ui.input(|input| input.events.clone()));
            paint = Some(self.inner.show_surface(surface, ui, panes));
        });
        let paint = paint.ok_or(DockspaceError::OuterHostSurfaceOutputUnconfirmed { surface })?;
        let paint = paint?;
        let pointer_events =
            pointer_events.ok_or(DockspaceError::OuterHostSurfaceOutputUnconfirmed { surface })?;
        self.inner
            .prepare_outer_pointer(surface, context, &pointer_events)?;
        self.inner
            .confirm_surface_output(surface, context, viewport, output)?;
        Ok(paint)
    }

    /// Reduces the complete roster once and returns renderer-bound final outputs.
    pub fn finish(self) -> Result<EguiOuterFrameCommit, DockspaceError> {
        self.inner.finish_inner()
    }
}

/// Atomic outer-host reduction plus exact renderer-bound surface outputs.
#[derive(Debug)]
#[must_use = "dropping the commit terminally drops every unsettled presentation output"]
pub struct EguiOuterFrameCommit {
    host: HostFrameResponse,
    outputs: Vec<EguiOuterSurfaceOutput>,
}

/// Fully preflighted egui/core frame awaiting an enclosing host transaction seal.
///
/// This capability owns every speculative core and adapter delta. It does not
/// borrow or mutate the live [`Dockspace`], so a native host may retain it until
/// output consolidation and platform roster commit have succeeded.
#[must_use = "a prepared egui frame must be committed or explicitly aborted"]
pub struct PreparedEguiOuterFrameCommit {
    core: EguiPreparedHostFrameCommit,
    renderer: PreparedEguiFrameAcceptance,
    native_bindings: Option<PreparedNativeBindingCommit>,
    state: HostFrameState,
    pane_focus_observations: Vec<PaneFocusObservation>,
    backend_ordered_input: bool,
}

impl EguiOuterFrameCommit {
    /// Returns the core transition and per-surface dispositions.
    #[must_use]
    pub const fn host(&self) -> &HostFrameResponse {
        &self.host
    }

    /// Iterates the exact surface outputs awaiting renderer settlement.
    #[must_use]
    pub fn outputs(&self) -> impl ExactSizeIterator<Item = &EguiOuterSurfaceOutput> {
        self.outputs.iter()
    }

    /// Separates the core response from the affine renderer-bound outputs.
    ///
    /// Each output can subsequently be split with
    /// [`EguiOuterSurfaceOutput::into_parts`] when renderer completion arrives
    /// after the host has consumed its [`FullOutput`].
    #[must_use]
    pub fn into_parts(self) -> (HostFrameResponse, Vec<EguiOuterSurfaceOutput>) {
        (self.host, self.outputs)
    }
}

impl PreparedEguiOuterFrameCommit {
    /// Returns the exact core transition awaiting publication.
    #[must_use]
    pub fn transition(&self) -> &dockspace::transition::EngineTransition {
        prepared_host_transition(&self.core)
    }

    /// Publishes the core candidate and every preflighted adapter sidecar once.
    ///
    /// # Errors
    ///
    /// Returns a typed staleness error if the destination dockspace changed after
    /// this capability was prepared. No adapter sidecar is committed before the
    /// core fence succeeds.
    pub fn commit(
        mut self,
        dockspace: &mut Dockspace,
    ) -> Result<EguiOuterFrameCommit, DockspaceError> {
        self.renderer.validate_renderer(&dockspace.renderer)?;
        if let Some(style) = self.state.staged_style_replacement() {
            dockspace
                .renderer
                .validate_style_replacement(style.prepared())?;
        }
        if let Some(prepared) = self.native_bindings.as_ref() {
            dockspace
                .native_bindings
                .as_ref()
                .ok_or(DockspaceError::NativeBindingRegistryUnavailable)?
                .validate_prepared(prepared)
                .map_err(NativeBindingError::from)?;
        }

        let transition = match EguiEngineOwner::commit_owned_host_presentation_frame(
            &mut dockspace.engine,
            self.core,
        ) {
            Ok(transition) => transition,
            Err(error) => {
                dockspace.discard_pointer_input_epoch();
                self.state.finish();
                return Err(error);
            }
        };
        dockspace.engine.reconcile_document_sidecars();
        if let Some(prepared) = self.native_bindings {
            dockspace
                .native_bindings
                .as_mut()
                .expect("a prepared native binding commit retains its registry")
                .commit_prepared(prepared);
        }
        if let Some(pointer) = self.state.automatic_pointer() {
            dockspace.pointer_input.commit(pointer);
        }
        self.state.pane_focus_mut().accept_transition(&transition);
        if let Some(watermark) =
            EguiEngineOwner::engine(&dockspace.engine).backend_ingress_commit_watermark()
        {
            self.state.pane_focus_mut().accept_backend_commit(watermark);
        }
        for observation in self.pane_focus_observations {
            if self.backend_ordered_input {
                self.state
                    .pane_focus_mut()
                    .queue_backend_observation(observation);
            } else {
                self.state.pane_focus_mut().record_enqueued(observation);
            }
        }
        let acceptance = self.renderer.commit(
            &mut dockspace.renderer,
            EguiEngineOwner::engine(&dockspace.engine),
        );
        if let Some(style) = self.state.take_staged_style_replacement() {
            dockspace
                .renderer
                .commit_style_replacement(style.into_prepared());
        }
        let (responses, actual_presentation_outputs, contribution_rejected) =
            acceptance.into_parts();

        dockspace.pane_focus = self.state.pane_focus().clone();
        dockspace.semantic_source_sequence = self.state.semantic_source_sequence();
        if let Some(outer) = self.state.take_outer_presentation() {
            if self.backend_ordered_input {
                dockspace.commit_ordered_outer_presentation_observations(&transition);
            } else {
                dockspace.commit_outer_presentation_observation(outer, &transition);
            }
        }
        let presentations = if let Some(automatic) = self.state.take_automatic_presentation() {
            if contribution_rejected
                || should_request_presentation_follow_up_pass(
                    transition.presentation_observations(),
                )
            {
                automatic
                    .context
                    .request_discard("dockspace presentation authority changed");
                automatic.context.request_repaint();
            }
            dockspace.commit_automatic_presentation(
                automatic,
                &transition,
                actual_presentation_outputs,
            );
            Vec::new()
        } else {
            dockspace.register_outer_presentation(actual_presentation_outputs)
        };
        let presentation_retention = dockspace.engine().presentation_retention_manifest();
        dockspace
            .presentation_ledger
            .retain(&presentation_retention);
        let mut presentations_by_surface = presentations
            .into_iter()
            .map(|presentation| (presentation.surface(), presentation))
            .collect::<BTreeMap<_, _>>();
        let outputs = self
            .state
            .take_confirmed_full_outputs()
            .into_iter()
            .map(|(surface, full_output)| {
                EguiOuterSurfaceOutput::new(
                    surface,
                    self.state.native_surface_pass(surface),
                    full_output,
                    presentations_by_surface.remove(&surface),
                )
            })
            .collect::<Vec<_>>();
        debug_assert!(
            presentations_by_surface.is_empty(),
            "every core presentation output belongs to one retained egui FullOutput"
        );
        dockspace.last_host_frame = Some(self.state.key());
        self.state.finish();
        Ok(EguiOuterFrameCommit {
            host: HostFrameResponse {
                transition,
                surfaces: responses,
            },
            outputs,
        })
    }

    /// Explicitly discards adapter staging after an enclosing host abort.
    pub fn abort(mut self, dockspace: &mut Dockspace) {
        dockspace.discard_pointer_input_epoch();
        self.state.finish();
    }
}

/// Mutable collection of the frozen surface roster in one real host frame.
///
/// No call to [`Self::show_surface`] mutates the core. [`Self::end_host_frame`] is the sole
/// reducer boundary, owns the core-minted frame capability, and commits adapter sidecars only
/// after the engine accepts the complete frame.
pub struct DockspaceHostFrame<'a> {
    pub(super) dockspace: &'a mut Dockspace,
    pub(super) state: HostFrameStateSlot,
}

impl DockspaceHostFrame<'_> {
    /// Returns the explicit egui render-pass schedule identity.
    #[must_use]
    pub const fn key(&self) -> EguiFrameScheduleKey {
        self.state.key()
    }

    /// Returns the sealed post-observation authority for this host frame.
    ///
    /// This view never exposes the live engine. Every measurement, hit-test fact,
    /// and semantic action prepared before [`Self::end_host_frame`] must derive
    /// from this exact rollback candidate.
    #[must_use]
    pub fn view(&self) -> HostFrameView<'_> {
        self.state.view()
    }

    /// Returns the complete post-observation sealed logical surface roster.
    #[must_use]
    pub fn expected_surfaces(&self) -> impl ExactSizeIterator<Item = SurfaceId> + '_ {
        self.state.expected_surfaces()
    }

    /// Measures and paints one frozen surface without mutating the core.
    pub fn show_surface(
        &mut self,
        surface: SurfaceId,
        ui: &mut Ui,
        panes: &mut dyn PaneView,
    ) -> Result<SurfacePaintResponse, DockspaceError> {
        let result = self.show_surface_inner(surface, ui, panes);
        if result.is_err() {
            self.state.poison();
        }
        result
    }

    fn show_surface_inner(
        &mut self,
        surface: SurfaceId,
        ui: &mut Ui,
        panes: &mut dyn PaneView,
    ) -> Result<SurfacePaintResponse, DockspaceError> {
        let pass = EguiSurfacePass::from_context(ui.ctx());
        self.state.validate_painted_surface_slot(surface, &pass)?;
        let bounds = ui.available_rect_before_wrap();
        let style = self.dockspace.renderer.style().clone();
        let (mut projection, contribution_measurements, resource_update, paint_projection) = {
            let view = self.view();
            let projection = self.dockspace.renderer.prepare_surface_projection(
                view,
                surface,
                bounds,
                ui,
                panes,
                self.state.sealed_workspace(),
            )?;
            let (contribution_measurements, resource_update) =
                self.dockspace.renderer.contribution_measurements(
                    view,
                    surface,
                    projection.measurement_set(),
                    &projection.resources,
                )?;
            let paint_projection = view
                .scene()
                .surface(surface)
                .and_then(SurfaceScene::paint_projection)
                .map(|projection| {
                    (
                        projection.plan_stamp(),
                        projection.output_ticket(),
                        projection.plan().clone(),
                        projection.hit_manifest().clone(),
                    )
                });
            (
                projection,
                contribution_measurements,
                resource_update,
                paint_projection,
            )
        };
        let interaction_output = {
            let view = self.view();
            view.interaction_projection(surface)
                .map(|projection| (projection.output_ticket(), projection.authority()))
        };
        let is_gesture_source_surface = {
            let view = self.view();
            Dockspace::is_gesture_source_surface(view, surface)
        };
        let contribution = match contribution_measurements {
            Some(measurements) => EguiSurfaceContribution::Prepared(
                self.prepare_surface_contribution(surface, measurements)?,
            ),
            None => match self.current_ready_output_ticket(surface) {
                Some(_) => {
                    EguiSurfaceContribution::Retained(self.begin_surface_contribution(surface)?)
                }
                None => EguiSurfaceContribution::Prepared(
                    self.prepare_surface_unavailable_contribution(
                        surface,
                        MeasurementUnavailableReason::Deferred,
                    )?,
                ),
            },
        };
        let presentation_authority_available = self.presentation_authority_available(surface);
        let projection_changed = !matches!(contribution, EguiSurfaceContribution::Retained(_));
        let prepared_paint_plan = match &contribution {
            EguiSurfaceContribution::Prepared(contribution) => match contribution.paint_candidate()
            {
                PreparedSurfacePaintCandidate::Ready(candidate) => Some(candidate.plan().clone()),
                PreparedSurfacePaintCandidate::Retained { .. }
                | PreparedSurfacePaintCandidate::Unavailable { .. } => None,
            },
            EguiSurfaceContribution::Retained(_)
            | EguiSurfaceContribution::Recorded
            | EguiSurfaceContribution::Submitted => None,
        };
        let surface_status = projection.status();
        let scene_ready = surface_status == DockspaceSurfaceStatus::Ready;
        let paint_plan = paint_projection
            .as_ref()
            .map(|(_, _, plan, _)| plan.clone())
            .or(prepared_paint_plan);
        let paint_resources = match paint_projection.as_ref() {
            Some((stamp, ..)) => resource_update
                .as_ref()
                .filter(|resource| resource.stamp == *stamp)
                .map(|resource| resource.resources.clone())
                .or_else(|| {
                    self.dockspace
                        .renderer
                        .retained_paint_resources(surface, *stamp)
                        .cloned()
                })
                .ok_or(DockspaceError::PaintResourceMissing {
                    surface,
                    stamp: *stamp,
                })?,
            None => projection.resources.clone(),
        };
        let adapter_measurements = projection.measurement_set().clone();
        let adapter_resources = projection.resources.clone();
        let chrome_matches_scene = paint_projection.is_some();
        let painted_authority_current = paint_projection
            .as_ref()
            .zip(interaction_output)
            .is_some_and(
                |((_, paint_ticket, _, _), (interaction_ticket, authority))| {
                    *paint_ticket == interaction_ticket && authority.matches_output(*paint_ticket)
                },
            );
        let pane_content_current = !projection_changed && scene_ready && chrome_matches_scene;
        let pointer_receivers_current =
            !projection_changed && scene_ready && chrome_matches_scene && painted_authority_current;
        let interactions_current = pointer_receivers_current;
        let framework_actions_enabled =
            self.state.input_authority() == EguiInputAuthority::FrameworkResponses;
        let interaction_scene = interactions_current.then(|| {
            paint_projection
                .as_ref()
                .expect("current interactions require a paint projection")
                .0
        });
        let semantic_scene = interaction_scene;
        let pane_focus_preparation = if pane_content_current {
            self.prepare_pane_focus_request(surface, ui.ctx(), &adapter_resources, panes)
        } else {
            Err(DockspaceUnavailableReason::SurfaceBoundsUnavailable)
        };
        let authoritative_hit_manifest = pointer_receivers_current.then(|| {
            &paint_projection
                .as_ref()
                .expect("current pointer receivers require a paint projection")
                .3
        });
        let mut output = {
            let view = self.view();
            let (measured_resources, tab_strip_states) = projection.paint_parts();
            paint_surface(
                ui,
                self.dockspace.renderer.id(),
                surface,
                bounds,
                paint_plan.as_ref(),
                &paint_resources,
                tab_strip_states,
                measured_resources,
                view.workspace(),
                panes,
                &style,
                view.interaction(),
                view.presentation_drag_preview(surface),
                view.presentation_contained_transform_preview(surface),
                pane_content_current,
                interaction_scene,
                semantic_scene,
                framework_actions_enabled,
                authoritative_hit_manifest,
                is_gesture_source_surface,
            )
        };
        let mut receiver_registrations = output.receivers.take();
        let painted_interaction = output.interaction_presentation();
        if let Some(error) = output.semantic_causality_error() {
            #[cfg(egui_backend_event_envelope)]
            if let Some(raw_event_index) = error.uncorrelated_raw_event() {
                return Err(
                    DockspaceError::SemanticActionBackendCorrelationUnavailable {
                        surface,
                        raw_event_index,
                    },
                );
            }
            return Err(DockspaceError::SemanticActionCausalityUnavailable {
                surface,
                matching_raw_events: error.matching_raw_events(),
            });
        }
        if let Some(registrations) = receiver_registrations.as_mut() {
            registrations.capture_framework_hover(ui.ctx());
        }
        if receiver_registrations
            .as_ref()
            .is_some_and(PaintReceiverRegistrations::is_conflicted)
        {
            return Err(DockspaceError::PointerReceiverRegistrationConflict);
        }
        let (pane_focus_capability, pane_focus_observation) = match pane_focus_preparation {
            Ok(()) => {
                self.observe_pane_focus_after_paint(surface, ui.ctx(), &adapter_resources, panes)?
            }
            Err(reason) => (DockspaceCapability::Unavailable(reason), None),
        };
        let mut semantic_inputs = Vec::new();
        if framework_actions_enabled && let Some(observation) = pane_focus_observation {
            semantic_inputs.push(StagedSemanticInput::post_batch_observation(
                EngineInput::PublishPaneFocusObservation {
                    expected_epoch: self.state.sealed_workspace().epoch(),
                    observation,
                },
            ));
        }
        if !framework_actions_enabled && !output.actions.is_empty() {
            return Err(DockspaceError::PresentationPhaseProducedSemanticInput {
                count: output.actions.len(),
            });
        }
        for positioned in output.actions {
            let (action, position) = positioned.into_parts();
            let input = self.semantic_render_action_input(&action)?;
            semantic_inputs.push(match position {
                RenderActionPosition::RawEvent(raw_event_index) => {
                    StagedSemanticInput::at_raw_event(raw_event_index, input)
                }
                RenderActionPosition::PostBatchContinuation => {
                    StagedSemanticInput::post_batch_continuation(input)
                }
            });
        }
        let has_semantic_actions = !semantic_inputs.is_empty();
        let response = SurfacePaintResponse {
            surface,
            missing_panes: projection.resources.missing_items().collect(),
            capture_errors: output.capture_errors,
            interactions_current,
            surface_status,
            contained_capability: self.contained_capability(surface, &adapter_measurements),
            pane_focus_capability,
        };
        let publication_mode = if presentation_authority_available && paint_projection.is_some() {
            EguiSurfacePublicationMode::EmitPresentedOutput
        } else {
            EguiSurfacePublicationMode::PaintOnly
        };
        let mut draft = EguiSurfaceDraft::painted(
            projection,
            response.clone(),
            resource_update,
            contribution,
            publication_mode,
            receiver_registrations,
            pointer_receivers_current,
            painted_interaction,
            semantic_inputs,
        );
        draft.set_pane_focus_observation(pane_focus_observation);
        self.state.record_painted_surface(surface, draft, pass)?;
        if projection_changed && framework_actions_enabled {
            ui.ctx()
                .request_discard("dockspace host-frame authority changed");
        }
        if projection_changed || has_semantic_actions {
            ui.ctx().request_repaint();
        }
        ui.advance_cursor_after_rect(bounds);
        Ok(response)
    }

    pub(super) fn show_native_staging(
        &mut self,
        presentation: NativeStagingPresentation,
        ui: &mut Ui,
    ) -> Result<(), DockspaceError> {
        let result = self.show_native_staging_inner(presentation, ui);
        if result.is_err() {
            self.state.poison();
        }
        result
    }

    fn show_native_staging_inner(
        &mut self,
        presentation: NativeStagingPresentation,
        ui: &mut Ui,
    ) -> Result<(), DockspaceError> {
        let pass = EguiSurfacePass::from_context(ui.ctx());
        let bounds = ui.available_rect_before_wrap();
        ui.painter()
            .rect_filled(bounds, 0.0, self.dockspace.renderer.style().workspace_fill);
        ui.advance_cursor_after_rect(bounds);
        self.state.record_native_staging_pass(presentation, pass)
    }

    fn prepare_outer_pointer(
        &mut self,
        surface: SurfaceId,
        context: &Context,
        events: &[egui::Event],
    ) -> Result<(), DockspaceError> {
        if self.state.mode() != EguiHostFrameMode::CompleteRoster
            || self.dockspace.pointer_input.provider().is_none()
        {
            return Ok(());
        }
        let result = (|| {
            let escape_input = self.escape_input(surface, context, events)?;
            let viewport = context.viewport_id();
            self.dockspace.pointer_input.bind_context(
                context,
                viewport,
                surface,
                self.state.sealed_workspace().epoch(),
            )?;
            let epoch = self
                .dockspace
                .pointer_input
                .current_epoch(
                    self.state.key().sequence(),
                    u64::from(self.state.key().pass()),
                    viewport,
                )
                .ok_or(DockspaceError::PointerInputBindingMissing)?;
            let registrations = self
                .state
                .drafts()
                .get(&surface)
                .and_then(EguiSurfaceDraft::receiver_registrations)
                .cloned();
            let pointer = self.dockspace.pointer_input.prepare_captured_events(
                context,
                epoch,
                registrations.as_ref(),
                events,
            )?;
            self.state.set_automatic_pointer(pointer);
            if let Some(input) = escape_input {
                self.state
                    .drafts_mut()
                    .get_mut(&surface)
                    .ok_or(DockspaceError::PointerInputBindingMissing)?
                    .stage_semantic_input(input);
            }
            Ok(())
        })();
        let result = if matches!(&result, Err(DockspaceError::PointerInputBindingStale)) {
            self.dockspace.abort_pointer_input().and(result)
        } else {
            result
        };
        if result.is_err() {
            self.state.poison();
        }
        result
    }

    /// Supplies an explicit unavailable fact for a frozen surface.
    ///
    /// Call this when the host knows why a rostered surface cannot be measured. If the host does
    /// not invoke either this method or [`Self::show_surface`] for one frozen slot,
    /// [`Self::end_host_frame`] submits the same slot with
    /// [`MeasurementUnavailableReason::Deferred`].
    pub fn mark_surface_unavailable(
        &mut self,
        surface: SurfaceId,
        reason: MeasurementUnavailableReason,
    ) -> Result<(), DockspaceError> {
        let result = self.mark_surface_unavailable_inner(surface, reason);
        if result.is_err() {
            self.state.poison();
        }
        result
    }

    fn mark_surface_unavailable_inner(
        &mut self,
        surface: SurfaceId,
        reason: MeasurementUnavailableReason,
    ) -> Result<(), DockspaceError> {
        self.state.validate_new_surface_slot(surface)?;
        let contribution = self.prepare_surface_unavailable_contribution(surface, reason)?;
        let draft = self
            .dockspace
            .renderer
            .unavailable_surface_draft(surface, contribution);
        self.state.record_unavailable_surface(surface, draft);
        Ok(())
    }

    pub(super) fn confirm_surface_output(
        &mut self,
        surface: SurfaceId,
        context: &Context,
        viewport: ViewportId,
        output: FullOutput,
    ) -> Result<(), DockspaceError> {
        self.state
            .confirm_surface_output(surface, context, viewport, output)
    }

    pub(super) fn confirm_external_surface_output(
        &mut self,
        surface: SurfaceId,
        context: &Context,
        viewport: ViewportId,
        output: &mut FullOutput,
    ) -> Result<(), DockspaceError> {
        self.state
            .confirm_external_surface_output(surface, context, viewport, output)
    }

    fn prepare_surface_contribution(
        &mut self,
        surface: SurfaceId,
        measurements: SurfaceMeasurements,
    ) -> Result<PreparedSurfaceContribution, DockspaceError> {
        let result = self
            .view()
            .begin_surface_contribution(surface)
            .map_err(DockspaceError::from)
            .and_then(|token| {
                self.view()
                    .prepare_surface_contribution(token, measurements)
                    .map_err(DockspaceError::from)
            });
        if result.is_err() {
            self.state.poison();
        }
        result
    }

    fn begin_surface_contribution(
        &mut self,
        surface: SurfaceId,
    ) -> Result<SurfaceContributionToken, DockspaceError> {
        let result = self
            .view()
            .begin_surface_contribution(surface)
            .map_err(DockspaceError::from);
        if result.is_err() {
            self.state.poison();
        }
        result
    }

    /// The public egui facade has no final-pass acknowledgement. It must not
    /// create an emission that the core cannot ever settle. Test-only providers
    /// exercise the authoritative path independently of the shipped adapter.
    fn presentation_authority_available(&self, surface: SurfaceId) -> bool {
        if self.state.mode() == EguiHostFrameMode::CompleteRoster {
            return !self
                .dockspace
                .outer_surface_has_pending_presentation(surface);
        }
        #[cfg(test)]
        {
            self.state
                .automatic_presentation()
                .is_some_and(|automatic| automatic.test_presentation_provider.is_some())
        }
        #[cfg(not(test))]
        {
            false
        }
    }

    fn prepare_surface_unavailable_contribution(
        &mut self,
        surface: SurfaceId,
        reason: MeasurementUnavailableReason,
    ) -> Result<PreparedSurfaceContribution, DockspaceError> {
        let result = self
            .view()
            .begin_surface_contribution(surface)
            .map_err(DockspaceError::from)
            .and_then(|token| {
                self.view()
                    .prepare_surface_unavailable_contribution(token, reason)
                    .map_err(DockspaceError::from)
            });
        if result.is_err() {
            self.state.poison();
        }
        result
    }

    /// Atomically reduces every staged input and contribution, then commits adapter sidecars.
    fn submit_automatic_pointer_segment(
        &self,
        core_frame: &mut CoreHostFrame,
        pointer: &PreparedPointerInput,
        journal: dockspace::pointer_journal::PointerEdgeJournal,
        pointer_receivers_current: bool,
    ) -> Result<(), DockspaceError> {
        self.dockspace.pointer_input.submit_prepared_segment(
            core_frame,
            pointer,
            journal.clone(),
        )?;
        let surface = self
            .state
            .expected_surfaces()
            .next()
            .ok_or(DockspaceError::PointerInputBindingMissing)?;
        let receipts = {
            let candidates = core_frame
                .pointer_receiver_candidates()
                .ok_or(DockspaceError::PointerReceiverCandidatesMissing)?;
            if pointer_receivers_current {
                let view = core_frame.view();
                let registrations = self
                    .state
                    .drafts()
                    .get(&surface)
                    .and_then(EguiSurfaceDraft::receiver_registrations)
                    .ok_or(DockspaceError::PointerInputBindingMissing)?;
                pointer.prepare_receipts_for(
                    candidates,
                    view,
                    surface,
                    &self.dockspace.renderer,
                    registrations,
                    &journal,
                )?
            } else {
                pointer.prepare_unavailable_receipts_for(candidates)?
            }
        };
        core_frame.submit_pointer_receiver_receipts(receipts)?;
        Ok(())
    }

    /// Atomically reduces every staged input and contribution, then commits adapter sidecars.
    pub fn end_host_frame(self) -> Result<HostFrameResponse, DockspaceError> {
        debug_assert_eq!(self.state.mode(), EguiHostFrameMode::SingleSurface);
        let finished = self.finish_inner()?;
        debug_assert!(finished.outputs.is_empty());
        Ok(finished.host)
    }

    pub(super) fn finish_inner(mut self) -> Result<EguiOuterFrameCommit, DockspaceError> {
        let prepared = self.prepare_inner()?;
        prepared.commit(self.dockspace)
    }

    pub(super) fn prepare_inner(&mut self) -> Result<PreparedEguiOuterFrameCommit, DockspaceError> {
        if let Err(error) = self.state.validate_finish() {
            return self.abort(error);
        }
        let missing = self.state.missing_surfaces();
        for surface in missing {
            if let Err(error) =
                self.mark_surface_unavailable_inner(surface, MeasurementUnavailableReason::Deferred)
            {
                return self.abort(error);
            }
        }

        let semantic_inputs = self
            .state
            .drafts_mut()
            .values_mut()
            .flat_map(EguiSurfaceDraft::take_semantic_inputs)
            .collect::<Vec<_>>();
        let event_derived_input_count = semantic_inputs
            .iter()
            .filter(|input| matches!(input.position(), SemanticInputPosition::RawEvent(_)))
            .count();
        let surface_count = self.state.expected_surfaces().len();
        if self.state.input_authority() == EguiInputAuthority::FrameworkResponses
            && surface_count > 1
            && event_derived_input_count != 0
        {
            return self.abort(
                DockspaceError::CrossViewportSemanticInputRequiresBackendAuthority {
                    surface_count,
                    input_count: event_derived_input_count,
                },
            );
        }
        if self.state.input_authority() == EguiInputAuthority::CoreBackend
            && !semantic_inputs.is_empty()
        {
            return self.abort(DockspaceError::PresentationPhaseProducedSemanticInput {
                count: semantic_inputs.len(),
            });
        }

        let pane_focus_observations = self
            .state
            .drafts()
            .values()
            .filter_map(EguiSurfaceDraft::pane_focus_observation)
            .collect::<Vec<_>>();
        let mut core_frame = self.state.take_core_frame();
        if self.state.input_authority() == EguiInputAuthority::FrameworkResponses
            && self.dockspace.pointer_input.provider().is_some()
            && self.state.automatic_pointer().is_none()
            && !self.state.has_surface_passes()
        {
            let input_frame = core_frame
                .input_mut()
                .expect("framework responses retain the core input capability");
            if let Err(error) = self
                .dockspace
                .pointer_input
                .submit_empty_interval(input_frame)
            {
                return self.abort(error);
            }
        }
        let pointer_segments = match self
            .state
            .automatic_pointer()
            .map(PreparedPointerInput::journal_segments)
            .transpose()
        {
            Ok(segments) => segments,
            Err(error) => return self.abort(error),
        };
        let pointer_receivers_current = self
            .state
            .drafts()
            .values()
            .all(EguiSurfaceDraft::pointer_receivers_current);
        let pointer_segment_count = pointer_segments.as_ref().map_or(0, Vec::len);
        let mut staged_inputs = semantic_inputs
            .into_iter()
            .enumerate()
            .map(|(stable_index, semantic)| {
                let position = semantic.position();
                let boundary = match position {
                    SemanticInputPosition::RawEvent(raw_event_index) => self
                        .state
                        .automatic_pointer()
                        .map_or(pointer_segment_count, |pointer| {
                            pointer.segment_index_before_raw_event(raw_event_index)
                        }),
                    SemanticInputPosition::PostBatchContinuation
                    | SemanticInputPosition::PostBatchObservation => pointer_segment_count,
                };
                (
                    boundary,
                    semantic_position_order(position, stable_index),
                    semantic.into_input(),
                )
            })
            .collect::<Vec<_>>();
        staged_inputs.sort_by_key(|(boundary, position, _)| (*boundary, *position));
        let mut semantic_sequence = self.state.semantic_source_sequence();
        if self.state.input_authority() == EguiInputAuthority::FrameworkResponses {
            let mut staged_inputs = staged_inputs.into_iter().peekable();
            for boundary in 0..=pointer_segment_count {
                while staged_inputs
                    .peek()
                    .is_some_and(|(input_boundary, _, _)| *input_boundary == boundary)
                {
                    let (_, _, input) = staged_inputs
                        .next()
                        .expect("a matching semantic boundary retains its input");
                    let sequence = match semantic_sequence.checked_next() {
                        Some(sequence) => sequence,
                        None => {
                            return self.abort(DockspaceError::InputSourceSequenceExhausted {
                                input_source: EGUI_RENDER_INPUT_SOURCE,
                            });
                        }
                    };
                    semantic_sequence = sequence;
                    let input_frame = core_frame
                        .input_mut()
                        .expect("framework responses retain the core input capability");
                    if let Err(error) =
                        input_frame.append_input(EGUI_RENDER_INPUT_SOURCE, sequence, input)
                    {
                        return self.abort(error.into());
                    }
                }
                if let (Some(pointer), Some(segments)) =
                    (self.state.automatic_pointer(), pointer_segments.as_ref())
                    && let Some(segment) = segments.get(boundary)
                {
                    let input_frame = core_frame
                        .input_mut()
                        .expect("framework responses retain the core input capability");
                    if let Err(error) = self.submit_automatic_pointer_segment(
                        input_frame,
                        pointer,
                        segment.clone(),
                        pointer_receivers_current,
                    ) {
                        return self.abort(error);
                    }
                }
            }
        }
        let paint_was_post_input = matches!(core_frame, EguiCoreFramePhase::Presentation(_));
        let mut core_frame = match core_frame {
            EguiCoreFramePhase::Input(frame) => match frame.into_presentation() {
                Ok(frame) => frame,
                Err(error) => return self.abort(error.into()),
            },
            EguiCoreFramePhase::Presentation(frame) => frame,
        };
        let projection_changed = core_frame.presentation_projection_changed_since_seal();
        let presentation_changed = core_frame.presentation_changed_since_seal();
        if projection_changed {
            let current_surfaces = core_frame.surfaces().collect::<BTreeSet<_>>();
            self.state
                .drafts_mut()
                .retain(|surface, _| current_surfaces.contains(surface));
            for surface in current_surfaces.iter().copied() {
                let contribution = {
                    let view = core_frame.view();
                    let token = view.begin_surface_contribution(surface)?;
                    view.prepare_surface_unavailable_contribution(
                        token,
                        MeasurementUnavailableReason::Deferred,
                    )?
                };
                if let Some(draft) = self.state.drafts_mut().get_mut(&surface) {
                    draft.supersede_with_unavailable_contribution(contribution);
                } else {
                    let draft = self
                        .dockspace
                        .renderer
                        .unavailable_surface_draft(surface, contribution);
                    self.state.record_unavailable_surface(surface, draft);
                }
            }
            self.state.replace_expected_surfaces(current_surfaces);
            if self.state.input_authority() == EguiInputAuthority::FrameworkResponses {
                self.state
                    .request_surface_repaint("dockspace post-input presentation changed");
            }
        } else if presentation_changed
            && self.state.input_authority() == EguiInputAuthority::FrameworkResponses
        {
            self.state
                .request_surface_repaint("dockspace interaction presentation changed");
        }
        let paint_matches_final_presentation = self
            .state
            .output_boundary()
            .paint_matches_final_presentation(paint_was_post_input, presentation_changed);
        if !paint_matches_final_presentation {
            for draft in self.state.drafts_mut().values_mut() {
                draft.force_paint_only();
            }
        }
        if self.state.terminal_configuration_pending() {
            for draft in self.state.drafts_mut().values_mut() {
                draft.force_paint_only();
            }
            self.state
                .request_surface_repaint("dockspace terminal configuration committed");
        }
        let mut presentation_obligations = match core_frame.take_presentation_obligations() {
            Ok(obligations) => obligations
                .into_iter()
                .map(|obligation| (obligation.slot().surface(), obligation))
                .collect::<BTreeMap<_, _>>(),
            Err(error) => return self.abort(error.into()),
        };
        if let Some(surface) = self.state.drafts().values().find_map(|draft| {
            (draft.paint().is_some()
                && draft.publication_is_pending()
                && !presentation_obligations.contains_key(&draft.surface()))
            .then_some(draft.surface())
        }) {
            return self.abort(DockspaceError::HostFrameSurfaceOutsideRoster { surface });
        }
        let mut native_staging_publications =
            BTreeMap::<SurfaceId, EguiNativeStagingPublication>::new();
        let native_staging_slots = presentation_obligations
            .iter()
            .filter_map(|(surface, obligation)| {
                obligation
                    .slot()
                    .native_staging()
                    .map(|presentation| (*surface, presentation))
            })
            .collect::<Vec<_>>();
        let stale_staging = {
            self.state
                .painted_native_staging_presentations()
                .find(|painted| {
                    !native_staging_slots
                        .iter()
                        .any(|(_, expected)| expected == painted)
                })
        };
        if let Some(presentation) = stale_staging {
            return self.abort(DockspaceError::NativeStagingRequestOutsideRoster { presentation });
        }
        for (surface, presentation) in native_staging_slots {
            let obligation = presentation_obligations
                .remove(&surface)
                .expect("the staging slot was captured from the obligation map");
            let disposition = if self.state.native_staging_was_painted(presentation)
                && self.state.confirmed_full_outputs().contains_key(&surface)
            {
                HostPresentationDisposition::Painted(
                    dockspace::presentation_observation::HostInteractionPresentation::default(),
                )
            } else {
                HostPresentationDisposition::Unavailable(
                    HostPresentationUnavailableReason::RetainedResourceUnavailable,
                )
            };
            let request = match core_frame.resolve_presentation_obligation(obligation, disposition)
            {
                Ok(request) => request,
                Err(error) => return self.abort(error.into()),
            };
            if let Some(request) = request {
                native_staging_publications.insert(
                    surface,
                    EguiNativeStagingPublication::new(presentation, request),
                );
            }
        }
        for draft in self.state.drafts_mut().values_mut() {
            if draft.paint().is_some() && draft.publication_is_pending() {
                let obligation = presentation_obligations
                    .remove(&draft.surface())
                    .expect("painted draft presentation slot was prevalidated");
                let result = draft.stage_painted_output(&mut core_frame, obligation);
                if let Err(error) = result {
                    return self.abort(error);
                }
            }
        }
        for (_, obligation) in presentation_obligations {
            let reason = match obligation.slot() {
                HostPresentationSlot::Surface { .. } if projection_changed => {
                    HostPresentationUnavailableReason::SupersededBeforePublication
                }
                HostPresentationSlot::Surface { .. } => {
                    HostPresentationUnavailableReason::OutputNotProduced
                }
                HostPresentationSlot::NativeStaging { .. } => {
                    unreachable!("native staging slots were resolved above")
                }
            };
            if let Err(error) = core_frame.resolve_presentation_obligation(
                obligation,
                HostPresentationDisposition::Unavailable(reason),
            ) {
                return self.abort(error.into());
            }
        }
        for draft in self.state.drafts_mut().values_mut() {
            let result = draft.stage_core_contribution(&mut core_frame);
            if let Err(error) = result {
                return self.abort(error);
            }
        }

        let prepared_core = EguiEngineOwner::prepare_owned_host_presentation_frame(
            &self.dockspace.engine,
            core_frame,
        );
        let prepared_core = match prepared_core {
            Ok(prepared) => prepared,
            Err(error) => return self.abort(error.into()),
        };
        if let Some(style) = self.state.staged_style_replacement() {
            let accepted = prepared_host_transition(&prepared_core)
                .reduced_inputs()
                .iter()
                .any(|input| {
                    input.source() == EGUI_APPLICATION_INPUT_SOURCE
                        && input.source_sequence() == style.source_sequence()
                        && matches!(
                            input.outcome(),
                            InputOutcome::PresentationConfigReplaced { .. }
                        )
                });
            if !accepted {
                let source_sequence = style.source_sequence();
                drop(prepared_core);
                return self.abort(DockspaceError::NativeStyleConfigurationNotAccepted {
                    source_sequence,
                });
            }
        }
        let prepared_renderer = match self.dockspace.renderer.prepare_frame(
            prepared_host_transition(&prepared_core),
            self.state.take_drafts(),
            native_staging_publications,
        ) {
            Ok(prepared) => prepared,
            Err(error) => {
                drop(prepared_core);
                return self.abort(error);
            }
        };
        let unbound_surface = (self.state.mode() == EguiHostFrameMode::CompleteRoster)
            .then(|| {
                prepared_renderer
                    .presentation_surfaces()
                    .find(|surface| !self.state.confirmed_full_outputs().contains_key(surface))
            })
            .flatten();
        if let Some(surface) = unbound_surface {
            drop(prepared_renderer);
            drop(prepared_core);
            return self.abort(DockspaceError::OuterHostSurfaceOutputUnconfirmed { surface });
        }
        let prepared_presentation_outputs =
            prepared_renderer.presentation_outputs().collect::<Vec<_>>();
        if let Some(error) = prepared_presentation_outputs
            .into_iter()
            .find_map(|output| self.state.validate_native_presentation_output(output).err())
        {
            drop(prepared_renderer);
            drop(prepared_core);
            return self.abort(error);
        }
        let prepared_native_bindings = match self.state.take_native_bindings() {
            Some(candidate) => {
                let registry = self
                    .dockspace
                    .native_bindings
                    .as_ref()
                    .expect("a staged native binding candidate retains its registry");
                match registry.prepare_commit(candidate, prepared_host_transition(&prepared_core)) {
                    Ok(prepared) => Some(prepared),
                    Err(error) => {
                        drop(prepared_renderer);
                        drop(prepared_core);
                        return self.abort(NativeBindingError::from(error).into());
                    }
                }
            }
            None => None,
        };
        self.state.set_semantic_source_sequence(semantic_sequence);
        let backend_ordered_input = self.state.input_authority() == EguiInputAuthority::CoreBackend;
        let state = self
            .state
            .take()
            .expect("a prepared host frame retains its adapter-owned state");
        Ok(PreparedEguiOuterFrameCommit {
            core: prepared_core,
            renderer: prepared_renderer,
            native_bindings: prepared_native_bindings,
            state,
            pane_focus_observations,
            backend_ordered_input,
        })
    }

    fn current_ready_output_ticket(
        &self,
        surface: SurfaceId,
    ) -> Option<SurfacePresentationOutputTicket> {
        self.state.current_ready_output_ticket(surface)
    }

    pub(super) fn escape_input(
        &self,
        surface: SurfaceId,
        context: &Context,
        events: &[egui::Event],
    ) -> Result<Option<StagedSemanticInput>, DockspaceError> {
        #[cfg(not(egui_backend_event_envelope))]
        let raw_event_index = events.iter().position(|event| {
            matches!(
                event,
                egui::Event::Key {
                    key: egui::Key::Escape,
                    pressed: true,
                    modifiers,
                    ..
                } if *modifiers == egui::Modifiers::NONE
            )
        });
        #[cfg(egui_backend_event_envelope)]
        let claim = context.input_mut(|input| {
            input.claim_event_envelope(|event| {
                matches!(
                    event,
                    egui::Event::Key {
                        key: egui::Key::Escape,
                        pressed: true,
                        modifiers,
                        ..
                    } if *modifiers == egui::Modifiers::NONE
                )
            })
        });
        let expected = self.state.sealed_workspace();
        let input = EngineInput::CancelActiveInteractionWithEscape {
            expected,
            delivery: EscapeDelivery::Surface(surface),
        };
        #[cfg(not(egui_backend_event_envelope))]
        {
            let _ = context;
            Ok(raw_event_index
                .map(|raw_event_index| StagedSemanticInput::at_raw_event(raw_event_index, input)))
        }
        #[cfg(egui_backend_event_envelope)]
        {
            let Some(claim) = claim else {
                return Ok(None);
            };
            if !claim.correlation().is_known() {
                return Err(
                    DockspaceError::SemanticActionBackendCorrelationUnavailable {
                        surface,
                        raw_event_index: claim.raw_event_index(),
                    },
                );
            }
            let _ = events;
            Ok(Some(StagedSemanticInput::at_raw_event(
                claim.raw_event_index(),
                input,
            )))
        }
    }

    fn semantic_render_action_input(
        &self,
        action: &RenderAction,
    ) -> Result<EngineInput, DockspaceError> {
        let view = self.view();
        let expected = self.state.sealed_workspace();
        let input = match action {
            RenderAction::Select(source) => EngineInput::WorkspaceCommand {
                expected,
                command: WorkspaceCommand::Select {
                    source: source.clone(),
                },
            },
            RenderAction::ActivateTabStripControl { surface, control } => {
                let prepared = view
                    .prepare_tab_strip_control_activation(*surface, *control)
                    .map_err(DockspaceError::TabInteractionUnavailable)?;
                EngineInput::ActivateTabStripControl { prepared }
            }
            RenderAction::ActivateTabListMenuRow {
                surface,
                session,
                tab,
            } => {
                let prepared = view
                    .prepare_tab_list_menu_row_activation(*surface, *session, *tab)
                    .map_err(DockspaceError::TabInteractionUnavailable)?;
                EngineInput::ActivateTabListMenuRow { prepared }
            }
            RenderAction::AdjustTabStripScroll {
                surface,
                bar,
                adjustment,
            } => {
                let prepared = view
                    .prepare_tab_strip_scroll(*surface, *bar, adjustment.clone())
                    .map_err(DockspaceError::TabInteractionUnavailable)?;
                EngineInput::AdjustTabStripScroll { prepared }
            }
            RenderAction::AdjustTabListMenuScroll {
                surface,
                session,
                adjustment,
            } => {
                let prepared = view
                    .prepare_tab_list_menu_scroll(*surface, *session, adjustment.clone())
                    .map_err(DockspaceError::TabInteractionUnavailable)?;
                EngineInput::AdjustTabListMenuScroll { prepared }
            }
            RenderAction::NavigateTabListMenu {
                surface,
                session,
                navigation,
            } => {
                let prepared = view
                    .prepare_tab_list_menu_navigation(*surface, *session, *navigation)
                    .map_err(DockspaceError::TabInteractionUnavailable)?;
                EngineInput::NavigateTabListMenu { prepared }
            }
            RenderAction::DismissTabListMenu { surface, session } => {
                let prepared = view
                    .prepare_tab_list_menu_dismiss(*surface, *session)
                    .map_err(DockspaceError::TabInteractionUnavailable)?;
                EngineInput::DismissTabListMenu { prepared }
            }
            RenderAction::RequestSemanticClose { scene, target } => {
                EngineInput::RequestSceneClose {
                    expected,
                    scene: *scene,
                    target: *target,
                }
            }
            RenderAction::AdjustSplitterResize {
                scene,
                splitter,
                delta,
            } => EngineInput::AdjustSplitterResize {
                expected,
                scene: *scene,
                splitter: *splitter,
                delta: *delta,
            },
            RenderAction::AdjustContainedResize {
                scene,
                surface,
                root,
                floating,
                expected_rect,
                minimum_size,
                edge,
                delta,
            } => self.contained_resize_adjustment_input(
                *scene,
                *surface,
                *root,
                *floating,
                *expected_rect,
                *minimum_size,
                *edge,
                *delta,
            )?,
        };
        Ok(input)
    }

    #[allow(clippy::too_many_arguments)]
    fn contained_resize_adjustment_input(
        &self,
        scene: SurfaceSceneStamp,
        surface: SurfaceId,
        root: RootId,
        floating: dockspace::ids::FloatingPresentationId,
        expected_rect: LogicalRect,
        minimum_size: LogicalSize,
        edge: ContainedResizeEdge,
        delta: f64,
    ) -> Result<EngineInput, DockspaceError> {
        let view = self.view();
        let baseline = view.contained_placement(surface, expected_rect, minimum_size)?;
        if baseline.scene() != scene {
            return Err(ContainedPlacementUnavailable::StaleScene {
                expected: scene,
                current: Some(baseline.scene()),
            }
            .into());
        }
        let requested_rect = requested_contained_resize_rect(
            surface,
            expected_rect,
            baseline.surface_bounds(),
            minimum_size,
            edge,
            delta,
        )?;
        let placement = view.contained_placement(surface, requested_rect, minimum_size)?;
        if placement.scene() != scene {
            return Err(ContainedPlacementUnavailable::StaleScene {
                expected: scene,
                current: Some(placement.scene()),
            }
            .into());
        }
        Ok(EngineInput::ApplyContainedPlacement {
            expected: self.state.sealed_workspace(),
            root,
            floating,
            expected_rect,
            placement,
        })
    }

    fn prepare_pane_focus_request(
        &mut self,
        surface: SurfaceId,
        context: &egui::Context,
        resources: &EguiSurfacePaintResources,
        panes: &dyn PaneView,
    ) -> Result<(), DockspaceUnavailableReason> {
        let view = self.view();
        let Some(intent) = view
            .pending_pane_focus_intent()
            .filter(|intent| intent.target().surface() == surface)
        else {
            return Ok(());
        };
        if view.viewport_focus_binding(surface) != Some(intent.target()) {
            return Err(DockspaceUnavailableReason::PaneFocusBindingUnavailable);
        }
        if !Dockspace::binding_has_authoritative_global_focus(view, intent.target()) {
            return Err(DockspaceUnavailableReason::PaneFocusWindowNotFocused);
        }
        if self
            .state
            .pane_focus()
            .request_fence
            .is_some_and(|fence| fence.intent == intent.id())
        {
            return Ok(());
        }

        let request_issued = match intent.focus() {
            PanelFocus::Item(item) => {
                let target = panes
                    .focus_target(item)
                    .ok_or(DockspaceUnavailableReason::PaneFocusTargetMissing { item })?;
                context.memory_mut(|memory| memory.request_focus(target));
                true
            }
            PanelFocus::None => {
                let targets = resources
                    .items()
                    .into_iter()
                    .map(|item| {
                        panes
                            .focus_target(item)
                            .ok_or(DockspaceUnavailableReason::PaneFocusTargetMissing { item })
                    })
                    .collect::<Result<Vec<_>, _>>()?;
                let request_issued = !targets.is_empty();
                if request_issued {
                    context.memory_mut(|memory| {
                        for target in targets {
                            memory.surrender_focus(target);
                        }
                    });
                }
                request_issued
            }
        };
        if request_issued {
            self.state.pane_focus_mut().request_fence = Some(PaneFocusRequestFence {
                intent: intent.id(),
                frame: self.state.key(),
            });
        }
        Ok(())
    }

    fn observe_pane_focus_after_paint(
        &self,
        surface: SurfaceId,
        context: &egui::Context,
        resources: &EguiSurfacePaintResources,
        panes: &dyn PaneView,
    ) -> Result<(DockspaceCapability, Option<PaneFocusObservation>), DockspaceError> {
        let view = self.view();
        let Some(binding) = view.viewport_focus_binding(surface) else {
            return Ok((
                DockspaceCapability::Unavailable(
                    DockspaceUnavailableReason::PaneFocusBindingUnavailable,
                ),
                None,
            ));
        };
        if !Dockspace::binding_has_authoritative_global_focus(view, binding) {
            return Ok((
                DockspaceCapability::Unavailable(
                    DockspaceUnavailableReason::PaneFocusWindowNotFocused,
                ),
                None,
            ));
        }
        let focus = match observe_surface_pane_focus(resources, panes, context) {
            Ok(focus) => focus,
            Err(reason) => return Ok((DockspaceCapability::Unavailable(reason), None)),
        };
        let intent = view
            .pending_pane_focus_intent()
            .filter(|intent| intent.target().surface() == surface);
        if intent.is_some_and(|intent| intent.target() != binding) {
            return Ok((
                DockspaceCapability::Unavailable(
                    DockspaceUnavailableReason::PaneFocusBindingUnavailable,
                ),
                None,
            ));
        }
        if intent.is_some_and(|intent| {
            self.state
                .pane_focus()
                .request_fence
                .is_some_and(|fence| fence.intent == intent.id() && fence.frame == self.state.key())
        }) {
            return Ok((DockspaceCapability::Supported, None));
        }

        let acknowledges = intent
            .filter(|intent| intent.focus() == focus)
            .map(PaneFocusIntent::id);
        if self
            .state
            .pane_focus()
            .observation_was_enqueued(binding, focus, acknowledges)
        {
            return Ok((DockspaceCapability::Supported, None));
        }
        let baseline = intent.and_then(PaneFocusIntent::pane_observation_baseline);
        let Some(generation) = self
            .state
            .pane_focus()
            .next_observation_generation(surface, baseline)
        else {
            return Ok((
                DockspaceCapability::Unavailable(
                    DockspaceUnavailableReason::PaneFocusObservationGenerationExhausted,
                ),
                None,
            ));
        };
        let mut observation = PaneFocusObservation::new(generation, binding, focus);
        if let Some(intent) = acknowledges {
            observation = observation.acknowledging(intent);
        }
        Ok((DockspaceCapability::Supported, Some(observation)))
    }

    fn contained_capability(
        &self,
        surface: SurfaceId,
        measurements: &EguiSurfaceMeasurementSet,
    ) -> DockspaceCapability {
        let view = self.view();
        if view.workspace().surface(surface).is_none() {
            return DockspaceCapability::Unavailable(DockspaceUnavailableReason::SurfaceAbsent);
        }
        if !measurements.bounds.is_positive() {
            return DockspaceCapability::Unavailable(
                DockspaceUnavailableReason::SurfaceBoundsUnavailable,
            );
        }
        if !view.policy().allows_contained_floating() {
            return DockspaceCapability::Unavailable(
                DockspaceUnavailableReason::ContainedPolicyDisabled,
            );
        }
        if !matches!(view.scene().surface(surface), Some(SurfaceScene::Ready(_))) {
            return DockspaceCapability::Unavailable(
                DockspaceUnavailableReason::SurfaceBoundsUnavailable,
            );
        }
        DockspaceCapability::Supported
    }

    fn abort<T>(&mut self, error: DockspaceError) -> Result<T, DockspaceError> {
        self.dockspace.discard_pointer_input_epoch();
        self.state.finish();
        Err(error)
    }
}

fn semantic_position_order(
    position: SemanticInputPosition,
    stable_index: usize,
) -> (u8, usize, usize) {
    match position {
        SemanticInputPosition::RawEvent(raw_event_index) => (0, raw_event_index, stable_index),
        SemanticInputPosition::PostBatchContinuation => (1, 0, stable_index),
        SemanticInputPosition::PostBatchObservation => (2, 0, stable_index),
    }
}

impl Drop for DockspaceHostFrame<'_> {
    fn drop(&mut self) {
        if let Some(state) = self.state.as_mut()
            && !state.is_finished()
        {
            self.dockspace.discard_pointer_input_epoch();
            state.finish();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn semantic_ties_follow_raw_events_before_post_batch_phases() {
        let mut positions = [
            (SemanticInputPosition::PostBatchObservation, 0),
            (SemanticInputPosition::RawEvent(7), 1),
            (SemanticInputPosition::PostBatchContinuation, 2),
            (SemanticInputPosition::RawEvent(3), 3),
        ];

        positions.sort_by_key(|(position, stable_index)| {
            semantic_position_order(*position, *stable_index)
        });

        assert_eq!(
            positions,
            [
                (SemanticInputPosition::RawEvent(3), 3),
                (SemanticInputPosition::RawEvent(7), 1),
                (SemanticInputPosition::PostBatchContinuation, 2),
                (SemanticInputPosition::PostBatchObservation, 0),
            ]
        );
    }
}
