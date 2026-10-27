//! Default single-surface renderer over the headless product session.

use std::collections::BTreeSet;
#[cfg(not(feature = "native-render-support"))]
use std::marker::PhantomData;

mod actions;
mod contained;
mod drag_feedback;
mod geometry;
mod guides;
mod measurement;
mod presentation_menu;
mod splitters;
mod tab_chrome;
mod tabs;

use dockspace::model::{FloatingPresentationId, ItemId};
#[cfg(feature = "native-render-support")]
use dockspace::runtime::SurfaceProgramExecutionReceipt;
use dockspace::runtime::{
    ContainedPaintRecord, DockspaceContainedTransformPreview, DockspaceDragPreview,
    DockspacePaneFocusObservation, DockspacePaneFocusRequest, DockspacePreviewVisual,
    DockspaceReceiverDescriptor, DockspaceRuntimeError, PreparedPaneFocusObservation,
    PreparedSurfaceAction, SurfaceGesturePhase, SurfacePaintDisposition, SurfacePaintPlan,
    SurfacePaintProgramOperation, SurfacePaintProgramRoot, SurfacePaintProgramRootKind,
    SurfacePaintProgramStep, SurfacePaintProgramVisitError,
};
#[cfg(test)]
use dockspace::runtime::{DockspaceDropDirection, DockspaceVisualId};
use egui::{Color32, Id, Key, Modifiers, Response, Sense, Shape, Stroke, StrokeKind, Ui};

use crate::pane::{PaneFocusState, PaneView};
use crate::pass_settlement::{
    FinalOutputShapeProof, PendingPaneFocusObservation, PresentationEvidence,
    PresentationProofScope,
};
use crate::response::{DockspaceCapability, DockspaceUnavailableReason};
use crate::style::{DockStyle, ResolvedDockVisuals};

use measurement::PaintResources;
pub(crate) use measurement::measure_surface;

#[cfg(feature = "native-render-support")]
fn native_drag_preview_diagnostic_id() -> Id {
    Id::unique("egui-dockspace-native-drag-preview-diagnostic")
}

#[cfg(feature = "native-render-support")]
#[cfg(test)]
fn enable_native_drag_preview_diagnostic(context: &egui::Context) {
    context.data_mut(|data| data.insert_temp(native_drag_preview_diagnostic_id(), true));
}

#[cfg(feature = "native-render-support")]
fn native_drag_preview_diagnostic_enabled(context: &egui::Context) -> bool {
    context.data(|data| {
        data.get_temp::<bool>(native_drag_preview_diagnostic_id())
            .unwrap_or(false)
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PointerActionAuthority {
    LocalResponses {
        local_actions: bool,
        presentation_settlement: bool,
        cancel_active_gesture: bool,
    },
    #[cfg(feature = "native-render-support")]
    ExternalJournal,
}

impl PointerActionAuthority {
    const fn accepts_local_pointer_actions(self) -> bool {
        match self {
            Self::LocalResponses { local_actions, .. } => local_actions,
            #[cfg(feature = "native-render-support")]
            Self::ExternalJournal => false,
        }
    }

    const fn supports_preview_gestures(self) -> bool {
        match self {
            Self::LocalResponses {
                local_actions: _,
                presentation_settlement,
                cancel_active_gesture: _,
            } => presentation_settlement,
            #[cfg(feature = "native-render-support")]
            Self::ExternalJournal => true,
        }
    }

    const fn emits_local_preview_gestures(self) -> bool {
        matches!(
            self,
            Self::LocalResponses {
                local_actions: _,
                presentation_settlement: true,
                cancel_active_gesture: _,
            }
        )
    }

    const fn cancels_active_gesture(self) -> bool {
        matches!(
            self,
            Self::LocalResponses {
                cancel_active_gesture: true,
                ..
            }
        )
    }

    const fn acknowledges_previews_locally(self) -> bool {
        self.emits_local_preview_gestures()
    }
}

struct RenderContext<'ui, 'plan, 'scroll> {
    ui: &'ui mut Ui,
    instance_id: Id,
    plan: SurfacePaintPlan<'plan>,
    panes: &'ui mut dyn PaneView,
    style: &'ui DockStyle,
    visuals: ResolvedDockVisuals,
    resources: &'ui PaintResources,
    pane_focus_request: Option<DockspacePaneFocusRequest>,
    pane_focus_target_requested: &'ui mut bool,
    pane_focus_target: &'ui mut Option<Id>,
    local_actions: &'ui mut Vec<PreparedSurfaceAction>,
    presentation_evidence: &'ui mut Vec<PresentationEvidence>,
    presentation_proof_scope: Option<&'ui PresentationProofScope>,
    #[cfg(feature = "native-render-support")]
    receivers: &'ui mut Vec<ProductReceiverBinding>,
    #[cfg(test)]
    drop_target_observations: &'ui mut Vec<RegisteredDropTargetObservation>,
    #[cfg(feature = "native-render-support")]
    scroll_registrar: Option<&'scroll mut dyn FnMut(&Ui, egui::Rect, Id) -> (Id, egui::LayerId)>,
    #[cfg(feature = "native-render-support")]
    scroll_receivers: &'ui mut Vec<ProductScrollReceiverBinding>,
    #[cfg(not(feature = "native-render-support"))]
    _scroll_lifetime: PhantomData<&'scroll mut ()>,
    defer_measurement: &'ui mut bool,
    pointer_authority: PointerActionAuthority,
    local_primary_presses: Vec<actions::LocalPrimaryPress>,
    claimed_contained_activations: Vec<FloatingPresentationId>,
}

pub(crate) fn request_discard_once(ui: &Ui, marker: Id, reason: &'static str) -> bool {
    #[derive(Clone, Copy, PartialEq, Eq)]
    struct RunStamp {
        frame: u64,
        start_pass: u64,
    }

    let pass_index = u64::try_from(ui.ctx().current_pass_index()).unwrap_or(u64::MAX);
    let stamp = RunStamp {
        frame: ui.ctx().cumulative_frame_nr(),
        start_pass: ui.ctx().cumulative_pass_nr().saturating_sub(pass_index),
    };
    let first = ui.ctx().data_mut(|data| {
        if data.get_temp::<RunStamp>(marker) == Some(stamp) {
            false
        } else {
            data.insert_temp(marker, stamp);
            true
        }
    });
    if first {
        ui.ctx().request_discard(reason);
    }
    first
}

impl RenderContext<'_, '_, '_> {
    fn response_emphasized(response: &Response) -> bool {
        response.sense.interactive()
            && (response.is_pointer_button_down_on()
                || response.has_focus()
                || response.clicked()
                || response.hovered()
                || response.highlighted())
    }

    fn interaction_stroke(&self, response: &Response, idle_color: Option<Color32>) -> Stroke {
        let mut stroke = self.ui.style().interact(response).fg_stroke;
        if !Self::response_emphasized(response)
            && let Some(color) = idle_color
        {
            stroke.color = color;
        }
        stroke
    }

    fn interaction_fill(&self, response: &Response) -> Color32 {
        self.style
            .visuals
            .tab_hover_fill
            .unwrap_or_else(|| self.ui.style().interact(response).weak_bg_fill)
    }

    fn response_dragged_locally(&self, response: &Response) -> bool {
        self.pointer_authority.emits_local_preview_gestures() && response.dragged()
    }

    fn source_gap_active(&self, paint: SurfacePaintDisposition) -> bool {
        !self.pointer_authority.cancels_active_gesture() && paint.is_source_gap()
    }

    fn splitter_fill(&self, response: &Response, emphasized: bool) -> Color32 {
        if emphasized {
            self.style
                .visuals
                .splitter_hover_color
                .unwrap_or_else(|| self.ui.style().interact(response).fg_stroke.color)
        } else {
            self.visuals.splitter_color
        }
    }

    fn push_preview_gesture_action(
        &mut self,
        action: PreparedSurfaceAction,
        phase: SurfaceGesturePhase,
    ) {
        if !matches!(phase, SurfaceGesturePhase::Begin { .. }) {
            *self.defer_measurement = true;
        }
        self.local_actions.push(action);
    }

    fn accept_drag_phase_once(
        &mut self,
        widget: Id,
        phase: SurfaceGesturePhase,
        discard_reason: &'static str,
    ) -> bool {
        if !matches!(phase, SurfaceGesturePhase::Begin { .. }) {
            return true;
        }
        let marker = self
            .ui
            .make_persistent_id((self.instance_id, "drag-begin-frame", widget));
        request_discard_once(self.ui, marker, discard_reason)
    }

    fn accept_contained_activation_once(
        &mut self,
        contained: ContainedPaintRecord<'_>,
        press: actions::LocalPrimaryPress,
    ) -> bool {
        let marker = self.ui.make_persistent_id((
            self.instance_id,
            "contained-activation-frame",
            contained.visual_id(),
            press.ordinal(),
        ));
        request_discard_once(
            self.ui,
            marker,
            "egui_dockspace: settle contained activation",
        )
    }

    fn claim_contained_activation(&mut self, contained: ContainedPaintRecord<'_>) {
        if !self
            .claimed_contained_activations
            .contains(&contained.floating())
        {
            self.claimed_contained_activations
                .push(contained.floating());
        }
    }

    fn flush_contained_activation(&mut self, contained: ContainedPaintRecord<'_>) {
        if self
            .claimed_contained_activations
            .contains(&contained.floating())
        {
            return;
        }
        let activation = self
            .local_primary_presses
            .iter()
            .copied()
            .find_map(|press| {
                (self.ui.ctx().layer_id_at(press.position()) == Some(self.ui.layer_id()))
                    .then(|| {
                        self.plan
                            .prepare_contained_activation(contained.floating(), press.point())
                            .map(|action| (press, action))
                    })
                    .flatten()
            });
        let Some((press, action)) = activation else {
            return;
        };
        if self.accept_contained_activation_once(contained, press) {
            self.claim_contained_activation(contained);
            self.push_local_action(action);
        }
    }

    fn push_local_action(&mut self, action: PreparedSurfaceAction) {
        self.local_actions.push(action);
    }

    fn push_presentation_evidence(
        &mut self,
        action: PreparedSurfaceAction,
        proof: Option<FinalOutputShapeProof>,
    ) {
        self.presentation_evidence
            .push(PresentationEvidence { action, proof });
    }

    fn interact_receiver(
        &mut self,
        rect: egui::Rect,
        id: Id,
        sense: Sense,
        _receiver: Option<DockspaceReceiverDescriptor>,
    ) -> egui::Response {
        let response = self.ui.interact(rect, id, sense);
        #[cfg(feature = "native-render-support")]
        if self.pointer_authority == PointerActionAuthority::ExternalJournal
            && sense.senses_drag()
            && self.ui.ctx().dragged_id() == Some(response.id)
        {
            // The native pointer journal owns docking gestures. Retain egui's
            // completed-pass hit record, but release this widget's local owner.
            self.ui.ctx().stop_dragging();
        }
        #[cfg(feature = "native-render-support")]
        if let Some(receiver) = _receiver {
            self.receivers.push(ProductReceiverBinding {
                widget_id: response.id,
                layer_id: response.layer_id,
                receiver,
            });
        }
        response
    }

    #[cfg(test)]
    fn observe_drop_target(
        &mut self,
        direction: DockspaceDropDirection,
        visual_id: DockspaceVisualId,
        response: &Response,
        receiver: DockspaceReceiverDescriptor,
    ) {
        self.drop_target_observations
            .push(RegisteredDropTargetObservation {
                direction,
                visual_id,
                widget_id: response.id,
                layer_id: response.layer_id,
                receiver,
            });
    }

    fn register_scroll_receiver(
        &mut self,
        _id: Id,
        _receiver: Option<DockspaceReceiverDescriptor>,
    ) {
        #[cfg(feature = "native-render-support")]
        if let (Some(receiver), Some(registrar)) = (_receiver, self.scroll_registrar.as_deref_mut())
            && let Some(rect) = geometry::egui_rect(receiver.bounds())
        {
            let (widget_id, layer_id) = registrar(self.ui, rect, _id);
            self.scroll_receivers.push(ProductScrollReceiverBinding {
                widget_id,
                layer_id,
                receiver,
            });
        }
    }
}

#[cfg(feature = "native-render-support")]
#[derive(Debug, Clone, Copy)]
pub(crate) struct ProductReceiverBinding {
    pub(crate) widget_id: Id,
    pub(crate) layer_id: egui::LayerId,
    pub(crate) receiver: DockspaceReceiverDescriptor,
}

#[cfg(feature = "native-render-support")]
#[derive(Debug, Clone, Copy)]
pub(crate) struct ProductScrollReceiverBinding {
    pub(crate) widget_id: Id,
    pub(crate) layer_id: egui::LayerId,
    pub(crate) receiver: DockspaceReceiverDescriptor,
}

#[cfg(test)]
#[derive(Debug, Clone, Copy)]
pub(crate) struct RegisteredDropTargetObservation {
    pub(crate) direction: DockspaceDropDirection,
    pub(crate) visual_id: DockspaceVisualId,
    pub(crate) widget_id: Id,
    pub(crate) layer_id: egui::LayerId,
    pub(crate) receiver: DockspaceReceiverDescriptor,
}

pub(crate) struct ProductPaintOutput {
    #[cfg(feature = "native-render-support")]
    pub(crate) execution_receipt: SurfaceProgramExecutionReceipt,
    pub(crate) local_actions: Vec<PreparedSurfaceAction>,
    #[cfg(feature = "native-render-support")]
    pub(crate) presentation_actions: Vec<PreparedSurfaceAction>,
    pub(crate) presentation_evidence: Vec<PresentationEvidence>,
    pub(crate) pane_focus_observation: Option<PreparedPaneFocusObservation>,
    pub(crate) pane_focus_pending: Option<PendingPaneFocusObservation>,
    pub(crate) pane_focus_capability: DockspaceCapability,
    pub(crate) missing_items: BTreeSet<ItemId>,
    #[cfg(feature = "native-render-support")]
    pub(crate) receivers: Vec<ProductReceiverBinding>,
    #[cfg(test)]
    pub(crate) drop_target_observations: Vec<RegisteredDropTargetObservation>,
    #[cfg(feature = "native-render-support")]
    pub(crate) scroll_receivers: Vec<ProductScrollReceiverBinding>,
    pub(crate) defer_measurement: bool,
    #[cfg(feature = "native-render-support")]
    pub(crate) transient_visuals_complete: bool,
}

pub(crate) fn paint_surface(
    ui: &mut Ui,
    instance_id: Id,
    plan: SurfacePaintPlan<'_>,
    panes: &mut dyn PaneView,
    style: &DockStyle,
    pointer_authority: PointerActionAuthority,
    presentation_proof_scope: Option<&PresentationProofScope>,
    #[cfg(feature = "native-render-support")] scroll_registrar: Option<
        &mut dyn FnMut(&Ui, egui::Rect, Id) -> (Id, egui::LayerId),
    >,
) -> Result<ProductPaintOutput, DockspaceRuntimeError> {
    let visuals = style.resolved_visuals(ui.visuals());
    let resources = PaintResources::from_plan(plan, ui, panes, style)?;
    let pane_focus_request = plan.pane_focus_request();
    let mut pane_focus_target_requested = false;
    let mut pane_focus_target = None;
    let mut local_actions = Vec::new();
    let mut presentation_evidence = Vec::new();
    #[cfg(feature = "native-render-support")]
    let mut receivers = Vec::new();
    #[cfg(test)]
    let mut drop_target_observations = Vec::new();
    #[cfg(feature = "native-render-support")]
    let mut scroll_receivers = Vec::new();
    let mut defer_measurement = false;
    let mut transient_visuals_complete = true;
    let mut terminal_semantic_output = None;
    #[cfg(feature = "native-render-support")]
    let execution_receipt;
    let local_primary_presses = actions::local_primary_presses(ui, pointer_authority);
    if let Some(bounds) = geometry::egui_rect(plan.bounds()) {
        ui.allocate_rect(bounds, Sense::hover());
        ui.painter()
            .rect_filled(bounds, 0.0, visuals.workspace_fill);
    }

    {
        let mut context = RenderContext {
            ui,
            instance_id,
            plan,
            panes,
            style,
            visuals,
            resources: &resources,
            pane_focus_request,
            pane_focus_target_requested: &mut pane_focus_target_requested,
            pane_focus_target: &mut pane_focus_target,
            local_actions: &mut local_actions,
            presentation_evidence: &mut presentation_evidence,
            presentation_proof_scope,
            #[cfg(feature = "native-render-support")]
            receivers: &mut receivers,
            #[cfg(test)]
            drop_target_observations: &mut drop_target_observations,
            #[cfg(feature = "native-render-support")]
            scroll_registrar,
            #[cfg(feature = "native-render-support")]
            scroll_receivers: &mut scroll_receivers,
            #[cfg(not(feature = "native-render-support"))]
            _scroll_lifetime: PhantomData,
            defer_measurement: &mut defer_measurement,
            pointer_authority,
            local_primary_presses,
            claimed_contained_activations: Vec::new(),
        };
        let completed_program = context
            .plan
            .execute_surface_program(|step| -> Result<(), DockspaceRuntimeError> {
                match step {
                    SurfacePaintProgramStep::RootBegin { root, paint } => {
                        if let SurfacePaintProgramRootKind::Contained(record) = root.kind() {
                            let window_visuals = context.style.resolved_contained_window(
                                context.ui.style(),
                                record.is_frontmost(),
                            );
                            contained::paint_background(
                                &mut context,
                                record,
                                &root,
                                window_visuals,
                                paint,
                            );
                        }
                    }
                    SurfacePaintProgramStep::Operation {
                        root,
                        operation,
                        paint,
                    } => {
                        let visual_context = match root.kind() {
                            SurfacePaintProgramRootKind::Main => tabs::RootVisualContext::Main,
                            SurfacePaintProgramRootKind::Contained(record) => {
                                let window_visuals = context.style.resolved_contained_window(
                                    context.ui.style(),
                                    record.is_frontmost(),
                                );
                                tabs::RootVisualContext::Contained {
                                    record,
                                    pane_fill: window_visuals.frame.fill,
                                }
                            }
                        };
                        paint_surface_program_operation(
                            &mut context,
                            &root,
                            &operation,
                            visual_context,
                            paint,
                        )?;
                    }
                    SurfacePaintProgramStep::RootEnd(root) => {
                        if let Some(record) = root.contained() {
                            context.flush_contained_activation(record);
                        }
                    }
                    SurfacePaintProgramStep::PopupBackdrop(backdrop) => {
                        tab_chrome::paint_backdrop(&mut context, backdrop);
                    }
                    SurfacePaintProgramStep::PopupMenu { menu, paint } => {
                        tab_chrome::paint_menu(&mut context, menu, paint);
                    }
                    SurfacePaintProgramStep::DragPreview(preview) => {
                        let suppress_gesture_visuals =
                            context.pointer_authority.cancels_active_gesture();
                        let drag_preview_required = !suppress_gesture_visuals;
                        let (drag_preview_painted, drag_preview_proof) = if suppress_gesture_visuals
                        {
                            (false, None)
                        } else {
                            paint_preview(
                                context.ui,
                                context.plan,
                                preview,
                                context.visuals,
                                context
                                    .pointer_authority
                                    .acknowledges_previews_locally()
                                    .then_some(context.presentation_proof_scope)
                                    .flatten(),
                            )
                        };
                        transient_visuals_complete &=
                            !drag_preview_required || drag_preview_painted;
                        if context.pointer_authority.acknowledges_previews_locally()
                            && drag_preview_painted
                            && let Some(action) = context.plan.prepare_drag_preview_painted()
                        {
                            context.push_presentation_evidence(action, drag_preview_proof);
                            *context.defer_measurement = true;
                        }
                    }
                    SurfacePaintProgramStep::DropTarget(_) => {}
                    SurfacePaintProgramStep::DropGuide { affordance, .. } => {
                        if !context.pointer_authority.cancels_active_gesture()
                            && let Some(cluster) = affordance
                        {
                            guides::paint_cluster(&mut context, cluster);
                        }
                    }
                    SurfacePaintProgramStep::ContainedTransformPreview(preview) => {
                        let suppress_gesture_visuals =
                            context.pointer_authority.cancels_active_gesture();
                        let contained_preview_required = !suppress_gesture_visuals;
                        let (contained_preview_painted, contained_preview_proof) =
                            if suppress_gesture_visuals {
                                (false, None)
                            } else {
                                paint_contained_transform_preview(
                                    context.ui,
                                    context.plan,
                                    preview,
                                    context.visuals,
                                    context
                                        .pointer_authority
                                        .acknowledges_previews_locally()
                                        .then_some(context.presentation_proof_scope)
                                        .flatten(),
                                )
                            };
                        transient_visuals_complete &=
                            !contained_preview_required || contained_preview_painted;
                        if context.pointer_authority.acknowledges_previews_locally()
                            && contained_preview_painted
                            && let Some(action) =
                                context.plan.prepare_contained_transform_preview_painted()
                        {
                            context.push_presentation_evidence(action, contained_preview_proof);
                        }
                    }
                    SurfacePaintProgramStep::DragDecoration(decoration)
                        if !context.pointer_authority.cancels_active_gesture() =>
                    {
                        drag_feedback::paint(&mut context, decoration);
                    }
                    SurfacePaintProgramStep::DragDecoration(_) => {}
                    SurfacePaintProgramStep::InteractionFeedback => {
                        if context.pointer_authority.cancels_active_gesture()
                            && let Some(action) = context.plan.prepare_escape_cancel()
                        {
                            context.ui.ctx().stop_dragging();
                            context.push_local_action(action);
                            *context.defer_measurement = true;
                            let marker = context.ui.make_persistent_id((
                                context.instance_id,
                                "authority-cancel-frame",
                            ));
                            request_discard_once(
                                context.ui,
                                marker,
                                "egui_dockspace: settle presentation-authority cancellation",
                            );
                        } else if actions::effective_enabled(
                            context.ui,
                            context.pointer_authority.accepts_local_pointer_actions(),
                        ) && let Some(action) = context.plan.prepare_escape_cancel()
                            && context
                                .ui
                                .input_mut(|input| input.consume_key(Modifiers::NONE, Key::Escape))
                        {
                            context.ui.ctx().stop_dragging();
                            context.push_local_action(action);
                        }
                    }
                    SurfacePaintProgramStep::SurfaceEnd { semantic_output } => {
                        terminal_semantic_output = Some(semantic_output);
                    }
                }
                Ok(())
            })
            .map_err(SurfacePaintProgramVisitError::into_runtime_error)?;
        #[cfg(feature = "native-render-support")]
        {
            execution_receipt = completed_program;
        }
        #[cfg(not(feature = "native-render-support"))]
        let _ = completed_program;
    }
    debug_assert_eq!(
        terminal_semantic_output,
        Some(plan.semantic_output()),
        "the paint program must terminate with its exact candidate output"
    );
    let (pane_focus_observation, pane_focus_pending, pane_focus_capability) = pane_focus_request
        .map_or((None, None, DockspaceCapability::Supported), |request| {
            let (observation, capability) = if pane_focus_target_requested {
                match panes.focus_state(request.item(), ui.ctx()) {
                    PaneFocusState::Focused => (
                        DockspacePaneFocusObservation::Focused,
                        DockspaceCapability::Supported,
                    ),
                    PaneFocusState::Unfocused => (
                        DockspacePaneFocusObservation::NotFocused,
                        DockspaceCapability::Supported,
                    ),
                    PaneFocusState::Unknown => (
                        DockspacePaneFocusObservation::Unavailable,
                        DockspaceCapability::Unavailable(
                            DockspaceUnavailableReason::PaneFocusStateUnknown {
                                item: request.item(),
                            },
                        ),
                    ),
                }
            } else {
                (
                    DockspacePaneFocusObservation::Unavailable,
                    DockspaceCapability::Unavailable(
                        DockspaceUnavailableReason::PaneFocusBindingUnavailable,
                    ),
                )
            };
            let prepared = plan.prepare_pane_focus_observation(request, observation);
            let observed_during_callback = match observation {
                DockspacePaneFocusObservation::Focused => PaneFocusState::Focused,
                DockspacePaneFocusObservation::NotFocused => PaneFocusState::Unfocused,
                DockspacePaneFocusObservation::Unavailable => PaneFocusState::Unknown,
            };
            let pending =
                if pointer_authority.supports_preview_gestures() && pane_focus_target_requested {
                    match (
                        plan.prepare_pane_focus_observation(
                            request,
                            DockspacePaneFocusObservation::Focused,
                        ),
                        plan.prepare_pane_focus_observation(
                            request,
                            DockspacePaneFocusObservation::NotFocused,
                        ),
                        plan.prepare_pane_focus_observation(
                            request,
                            DockspacePaneFocusObservation::Unavailable,
                        ),
                    ) {
                        (Some(focused), Some(not_focused), Some(unavailable)) => {
                            Some(PendingPaneFocusObservation::new(
                                pane_focus_target,
                                observed_during_callback,
                                focused,
                                not_focused,
                                unavailable,
                            ))
                        }
                        _ => None,
                    }
                } else {
                    None
                };
            (prepared, pending, capability)
        });
    #[cfg(not(feature = "native-render-support"))]
    let _ = transient_visuals_complete;
    Ok(ProductPaintOutput {
        #[cfg(feature = "native-render-support")]
        execution_receipt,
        local_actions,
        #[cfg(feature = "native-render-support")]
        presentation_actions: Vec::new(),
        presentation_evidence,
        pane_focus_observation,
        pane_focus_pending,
        pane_focus_capability,
        missing_items: resources.missing_items().collect(),
        #[cfg(feature = "native-render-support")]
        receivers,
        #[cfg(test)]
        drop_target_observations,
        #[cfg(feature = "native-render-support")]
        scroll_receivers,
        defer_measurement,
        #[cfg(feature = "native-render-support")]
        transient_visuals_complete,
    })
}

fn paint_surface_program_operation(
    context: &mut RenderContext<'_, '_, '_>,
    root: &SurfacePaintProgramRoot<'_>,
    operation: &SurfacePaintProgramOperation<'_>,
    visual_context: tabs::RootVisualContext<'_>,
    paint: SurfacePaintDisposition,
) -> Result<(), DockspaceRuntimeError> {
    match *operation {
        SurfacePaintProgramOperation::Pane(pane) => {
            tabs::paint_pane(context, pane, visual_context, paint);
        }
        SurfacePaintProgramOperation::TabBar(bar) => {
            tabs::paint_tab_bar(context, bar, visual_context.contained(), paint);
        }
        SurfacePaintProgramOperation::Tab(tab) => {
            tabs::paint_tab_record(context, tab, visual_context.contained(), paint);
        }
        SurfacePaintProgramOperation::Splitter(splitter) => {
            splitters::paint_splitter(context, splitter, paint);
        }
        SurfacePaintProgramOperation::SplitterJunction(junction) => {
            splitters::paint_junction(context, junction, paint);
        }
        SurfacePaintProgramOperation::TabStripControl(control) => {
            tab_chrome::paint_control_record(context, control, paint);
        }
        SurfacePaintProgramOperation::ContainedControls(record) => {
            contained::paint_controls(context, record, root, paint);
        }
        SurfacePaintProgramOperation::PresentationMenuAnchor(anchor) => {
            presentation_menu::paint(context, anchor, root.contained(), paint)?;
        }
    }
    Ok(())
}

fn paint_preview(
    ui: &Ui,
    plan: SurfacePaintPlan<'_>,
    preview: DockspaceDragPreview<'_>,
    visuals: ResolvedDockVisuals,
    proof_scope: Option<&PresentationProofScope>,
) -> (bool, Option<FinalOutputShapeProof>) {
    let visual = preview.visual();
    let Some(bounds) = geometry::egui_rect(plan.bounds()) else {
        return (false, None);
    };
    let Some(rect) = preview_rect(plan.surface(), bounds, visual) else {
        return (false, None);
    };
    let painter = ui.painter_at(bounds);
    let proof = if let Some(proof_scope) = proof_scope {
        FinalOutputShapeProof::paint(
            proof_scope,
            &painter,
            rect,
            vec![
                Shape::rect_filled(rect, 2.0, visuals.drop_fill),
                Shape::rect_stroke(
                    rect,
                    2.0,
                    Stroke::new(1.0, visuals.drop_border_color),
                    StrokeKind::Inside,
                ),
            ],
        )
    } else {
        painter.rect_filled(rect, 2.0, visuals.drop_fill);
        painter.rect_stroke(
            rect,
            2.0,
            Stroke::new(1.0, visuals.drop_border_color),
            StrokeKind::Inside,
        );
        None
    };
    #[cfg(feature = "native-render-support")]
    publish_native_drag_preview_diagnostic(ui, plan.surface(), rect, visual);
    (true, proof)
}

#[cfg(feature = "native-render-support")]
fn publish_native_drag_preview_diagnostic(
    ui: &Ui,
    surface: dockspace::model::SurfaceId,
    rect: egui::Rect,
    visual: DockspacePreviewVisual,
) {
    if !native_drag_preview_diagnostic_enabled(ui.ctx()) {
        return;
    }
    if !matches!(
        visual,
        DockspacePreviewVisual::Native { host_surface, .. } if host_surface == surface
    ) {
        return;
    }
    let id = ui.make_persistent_id(("dockspace-native-drag-preview-painted", surface));
    ui.ctx().accesskit_node_builder(id, |node| {
        node.set_role(egui::accesskit::Role::Label);
        node.set_label("Dockspace native drag preview painted");
        node.set_value("painted");
        node.set_bounds(egui::accesskit::Rect::new(
            f64::from(rect.min.x),
            f64::from(rect.min.y),
            f64::from(rect.max.x),
            f64::from(rect.max.y),
        ));
    });
}

fn preview_rect(
    host_surface: dockspace::model::SurfaceId,
    host_bounds: egui::Rect,
    visual: DockspacePreviewVisual,
) -> Option<egui::Rect> {
    match visual {
        DockspacePreviewVisual::Dock { surface, rect }
        | DockspacePreviewVisual::Contained { surface, rect, .. }
            if surface == host_surface =>
        {
            geometry::egui_rect(rect)
        }
        DockspacePreviewVisual::Native {
            host_surface: surface,
            ..
        } if surface == host_surface => Some(host_bounds.shrink(4.0)),
        DockspacePreviewVisual::Dock { .. }
        | DockspacePreviewVisual::Contained { .. }
        | DockspacePreviewVisual::Native { .. } => None,
    }
}

fn paint_contained_transform_preview(
    ui: &Ui,
    plan: SurfacePaintPlan<'_>,
    preview: DockspaceContainedTransformPreview<'_>,
    visuals: ResolvedDockVisuals,
    proof_scope: Option<&PresentationProofScope>,
) -> (bool, Option<FinalOutputShapeProof>) {
    if preview.surface() != plan.surface() {
        return (false, None);
    }
    let Some(rect) = geometry::egui_rect(preview.rect()) else {
        return (false, None);
    };
    let painter = ui.painter();
    let proof = if let Some(proof_scope) = proof_scope {
        FinalOutputShapeProof::paint(
            proof_scope,
            &painter,
            rect,
            vec![
                Shape::rect_filled(rect, 0.0, visuals.ghost_fill),
                Shape::rect_stroke(
                    rect,
                    0.0,
                    Stroke::new(1.0, visuals.ghost_border_color),
                    StrokeKind::Inside,
                ),
            ],
        )
    } else {
        painter.rect(
            rect,
            0.0,
            visuals.ghost_fill,
            Stroke::new(1.0, visuals.ghost_border_color),
            StrokeKind::Inside,
        );
        None
    };
    (true, proof)
}

#[cfg(test)]
mod discard_tests {
    use egui::{Context, Id, RawInput, Ui, UiBuilder};

    use super::request_discard_once;

    #[test]
    fn manual_passes_each_receive_one_discard_request() {
        let context = Context::default();
        let marker = Id::unique("dockspace-manual-discard-once");

        for pass in 0..2 {
            context.begin_pass(RawInput::default());
            let ui = Ui::new(
                context.clone(),
                Id::unique(("dockspace-manual-discard-ui", pass)),
                UiBuilder::new().max_rect(context.viewport_rect()),
            );
            assert!(request_discard_once(&ui, marker, "manual pass discard"));
            assert!(!request_discard_once(&ui, marker, "manual pass discard"));
            context.end_pass().drop_without_applying_deltas();
        }
    }

    #[test]
    fn multipass_run_receives_only_one_discard_request() {
        let context = Context::default();
        context.options_mut(|options| {
            options.max_passes = 2.try_into().expect("two is non-zero");
        });
        let marker = Id::unique("dockspace-run-discard-once");
        let mut requested = Vec::new();

        let output = context.run_ui(RawInput::default(), |ui| {
            requested.push(request_discard_once(ui, marker, "multipass discard"));
        });
        output.drop_without_applying_deltas();

        assert_eq!(requested, [true, false]);
    }
}

#[cfg(all(test, feature = "native-render-support"))]
mod tests {
    use dockspace::geometry::{LogicalRect, PhysicalRect};
    use dockspace::model::SurfaceId;
    use egui::{Rect, pos2, vec2};

    use super::{
        DockspacePreviewVisual, PointerActionAuthority, enable_native_drag_preview_diagnostic,
        native_drag_preview_diagnostic_enabled, preview_rect,
    };

    #[test]
    fn external_journal_uses_renderer_settlement_for_preview_authority() {
        assert!(
            PointerActionAuthority::LocalResponses {
                local_actions: true,
                presentation_settlement: true,
                cancel_active_gesture: false,
            }
            .acknowledges_previews_locally(),
            "the official-egui product path has no renderer settlement callback"
        );
        assert!(
            !PointerActionAuthority::LocalResponses {
                local_actions: true,
                presentation_settlement: false,
                cancel_active_gesture: false,
            }
            .acknowledges_previews_locally(),
            "an unowned eframe callback cannot acknowledge terminal previews"
        );
        assert!(
            !PointerActionAuthority::ExternalJournal.acknowledges_previews_locally(),
            "native preview authority must come from its Presented output"
        );
    }

    #[test]
    fn native_preview_paints_a_source_hosted_cue() {
        let source = SurfaceId::new(1);
        let target = SurfaceId::new(2);
        let bounds = Rect::from_min_size(pos2(10.0, 20.0), vec2(300.0, 200.0));
        let placement =
            PhysicalRect::new(400.0, 100.0, 300.0, 200.0).expect("native placement is valid");
        assert_eq!(
            preview_rect(
                source,
                bounds,
                DockspacePreviewVisual::Native {
                    host_surface: source,
                    target_surface: target,
                    placement,
                },
            ),
            Some(bounds.shrink(4.0))
        );
        assert_eq!(
            preview_rect(
                target,
                bounds,
                DockspacePreviewVisual::Dock {
                    surface: source,
                    rect: LogicalRect::new(10.0, 20.0, 30.0, 40.0)
                        .expect("logical preview is valid"),
                },
            ),
            None
        );
    }

    #[test]
    fn native_preview_diagnostic_requires_explicit_smoke_opt_in() {
        let context = egui::Context::default();
        assert!(!native_drag_preview_diagnostic_enabled(&context));

        enable_native_drag_preview_diagnostic(&context);

        assert!(native_drag_preview_diagnostic_enabled(&context));
    }
}
