//! Default single-surface renderer over the headless product session.
//!
//! Pointer ownership, receiver facts, and migration contracts are documented in
//! `README.md` alongside this module.

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

use dockspace::model::ItemId;
use dockspace::runtime::{
    DeferredSurfaceAction, DockspaceContainedTransformPreview, DockspaceDragPreview,
    DockspacePaneFocusObservation, DockspacePaneFocusRequest, DockspacePreviewVisual,
    DockspaceReceiverDescriptor, DockspaceReceiverRole, DockspaceRuntimeError,
    PreparedPaneFocusObservation, PreparedSurfaceAction, SurfacePaintDisposition, SurfacePaintPlan,
    SurfacePaintProgramOperation, SurfacePaintProgramRoot, SurfacePaintProgramRootKind,
    SurfacePaintProgramStep, SurfacePaintProgramVisitError, SurfaceProgramExecutionReceipt,
};
#[cfg(test)]
use dockspace::runtime::{DockspaceDropDirection, DockspaceVisualId};
use egui::{Color32, Id, Key, Modifiers, Response, Sense, Shape, Stroke, StrokeKind, Ui};

use crate::pane::{PaneFocusState, PaneView};
use crate::pass_settlement::PendingPaneFocusObservation;
use crate::response::{DockspaceCapability, DockspaceUnavailableReason};
use crate::style::{DockStyle, ResolvedDockVisuals};

pub(crate) use measurement::{PaintResources, measure_surface};
pub(crate) use presentation_menu::prepare_layer as prepare_menu_layer;

/// One action captured from a stable egui widget and response source.
///
/// The origin identifies an event slot within one original run. It remains
/// stable when another slot produces no action in a repeated pass. Hosts must
/// keep the original run identity separately; equal origins in independent
/// runs still represent independent events.
#[must_use = "retain this action with its origin until it is submitted or discarded"]
#[derive(Debug)]
pub struct EguiSurfaceAction {
    origin: Id,
    action: PreparedSurfaceAction,
}

impl EguiSurfaceAction {
    /// Returns the stable widget and response-source slot that emitted the action.
    #[must_use]
    pub const fn origin(&self) -> Id {
        self.origin
    }

    /// Retains the exact core witness for submission in a later update.
    ///
    /// Read [`Self::origin`] first when recording the original adapter event slot.
    #[must_use]
    pub fn into_deferred(self) -> DeferredSurfaceAction {
        self.action.into_deferred()
    }
}

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
    HostJournal,
    LocalResponses,
    #[cfg(feature = "native-render-support")]
    ExternalJournal,
}

impl PointerActionAuthority {
    const fn accepts_local_pointer_actions(self) -> bool {
        matches!(self, Self::LocalResponses)
    }

    const fn supports_preview_gestures(self) -> bool {
        !matches!(self, Self::LocalResponses)
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
    pane_focus_bindings: &'ui mut Vec<(ItemId, Id)>,
    local_actions: &'ui mut Vec<EguiSurfaceAction>,
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
    paint_audit: Option<&'ui mut crate::output_proof::PaintAudit>,
    pointer_authority: PointerActionAuthority,
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
        stroke.color = idle_color
            .filter(|_| !Self::response_emphasized(response))
            .or(self.ui.visuals().override_text_color)
            .unwrap_or(stroke.color);
        stroke
    }

    fn interaction_text_color(
        &self,
        response: &Response,
        override_color: Option<Color32>,
    ) -> Color32 {
        override_color.unwrap_or_else(|| self.interaction_stroke(response, None).color)
    }

    fn interaction_fill(&self, response: &Response) -> Color32 {
        self.style
            .visuals
            .tab_hover_fill
            .unwrap_or_else(|| self.ui.style().interact(response).weak_bg_fill)
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

    fn push_local_action(&mut self, origin: Id, action: PreparedSurfaceAction) {
        self.local_actions
            .push(EguiSurfaceAction { origin, action });
    }

    fn interact_receiver(
        &mut self,
        rect: egui::Rect,
        id: Id,
        sense: Sense,
        receiver: Option<DockspaceReceiverDescriptor>,
    ) -> egui::Response {
        let response = self.ui.interact(rect, id, sense);
        if !matches!(
            self.pointer_authority,
            PointerActionAuthority::LocalResponses
        ) && sense.senses_drag()
            && receiver.is_some_and(|receiver| {
                matches!(
                    receiver.role(),
                    DockspaceReceiverRole::TabBody
                        | DockspaceReceiverRole::TabGroupGrip
                        | DockspaceReceiverRole::TabGroupTrailingEmpty
                        | DockspaceReceiverRole::Splitter
                        | DockspaceReceiverRole::SplitterJunction
                        | DockspaceReceiverRole::ContainedTitle
                        | DockspaceReceiverRole::ContainedResize
                )
            })
            && self.ui.ctx().dragged_id() == Some(response.id)
        {
            // The host pointer journal owns docking gestures. Retain egui's
            // completed-pass hit record, but release this widget's local owner.
            // Scroll widgets keep their response-driven drag ownership.
            self.ui.ctx().stop_dragging();
        }
        if let Some(receiver) = receiver {
            self.receivers.push(ProductReceiverBinding {
                interact_rect: response.interact_rect,
                interactive: sense.interactive() && response.enabled(),
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

#[derive(Debug, Clone, Copy)]
pub(crate) struct ProductReceiverBinding {
    pub(crate) interact_rect: egui::Rect,
    pub(crate) interactive: bool,
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
    pub(crate) execution_receipt: Option<SurfaceProgramExecutionReceipt>,
    pub(crate) local_actions: Vec<EguiSurfaceAction>,
    pub(crate) pane_focus_observation: Option<PreparedPaneFocusObservation>,
    pub(crate) pane_focus_pending: Option<PendingPaneFocusObservation>,
    pub(crate) pane_focus_capability: DockspaceCapability,
    pub(crate) pane_focus_bindings: Vec<(ItemId, Id)>,
    pub(crate) missing_items: BTreeSet<ItemId>,
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
    mut resources: PaintResources,
    pointer_authority: PointerActionAuthority,
    #[cfg(feature = "native-render-support")] scroll_registrar: Option<
        &mut dyn FnMut(&Ui, egui::Rect, Id) -> (Id, egui::LayerId),
    >,
) -> Result<ProductPaintOutput, DockspaceRuntimeError> {
    prepare_menu_layer(ui, instance_id, plan.surface());
    let visuals = style.resolved_visuals(ui.visuals());
    resources.prepare_for_plan(plan, ui, panes)?;
    let pane_focus_request = plan.pane_focus_request();
    let mut pane_focus_target_requested = false;
    let mut pane_focus_target = None;
    let mut pane_focus_bindings = Vec::new();
    let mut local_actions = Vec::new();
    let mut receivers = Vec::new();
    #[cfg(test)]
    let mut drop_target_observations = Vec::new();
    #[cfg(feature = "native-render-support")]
    let mut scroll_receivers = Vec::new();
    let mut defer_measurement = false;
    let mut transient_visuals_complete = true;
    let mut terminal_semantic_output = None;
    let execution_receipt;
    if let Some(bounds) = geometry::egui_rect(plan.bounds()) {
        ui.allocate_rect(bounds, Sense::hover());
        ui.painter()
            .rect_filled(bounds, 0.0, visuals.workspace_fill);
        let target = ui.make_persistent_id((instance_id, "workspace-focus", plan.surface()));
        let _ = ui.interact(bounds, target, Sense::focusable_noninteractive());
        let enabled = actions::effective_enabled(ui, true);
        ui.ctx().accesskit_node_builder(target, |node| {
            node.set_role(egui::accesskit::Role::Pane);
            node.set_label("Dockspace workspace");
            node.set_bounds(geometry::accesskit_bounds(bounds));
            if enabled {
                node.add_action(egui::accesskit::Action::Focus);
            } else {
                node.set_disabled();
                node.clear_actions();
            }
        });
        if pane_focus_request.is_some_and(|request| request.item().is_none())
            && pointer_authority.supports_preview_gestures()
            && enabled
        {
            ui.memory_mut(|memory| memory.request_focus(target));
            pane_focus_target_requested = true;
            pane_focus_target = Some(target);
        }
    }

    {
        let mut paint_audit = matches!(
            pointer_authority,
            PointerActionAuthority::LocalResponses | PointerActionAuthority::HostJournal
        )
        .then(|| crate::output_proof::PaintAudit::new(ui.ctx()));
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
            pane_focus_bindings: &mut pane_focus_bindings,
            local_actions: &mut local_actions,
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
            paint_audit: paint_audit.as_mut(),
            pointer_authority,
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
                    SurfacePaintProgramStep::RootEnd(_) => {}
                    SurfacePaintProgramStep::PopupBackdrop(backdrop) => {
                        tab_chrome::paint_backdrop(&mut context, backdrop);
                    }
                    SurfacePaintProgramStep::PopupMenu { menu, paint } => {
                        tab_chrome::paint_menu(&mut context, menu, paint);
                    }
                    SurfacePaintProgramStep::DragPreview(preview) => {
                        let drag_preview_painted =
                            paint_preview(context.ui, context.plan, preview, context.visuals);
                        transient_visuals_complete &= drag_preview_painted;
                        if context.pointer_authority.supports_preview_gestures()
                            && drag_preview_painted
                        {
                            *context.defer_measurement = true;
                        }
                    }
                    SurfacePaintProgramStep::DropTarget(_) => {}
                    SurfacePaintProgramStep::DropGuide { affordance, .. } => {
                        if let Some(cluster) = affordance {
                            guides::paint_cluster(&mut context, cluster);
                        }
                    }
                    SurfacePaintProgramStep::ContainedTransformPreview(preview) => {
                        let contained_preview_painted = paint_contained_transform_preview(
                            context.ui,
                            context.plan,
                            preview,
                            context.visuals,
                        );
                        transient_visuals_complete &= contained_preview_painted;
                        if context.pointer_authority.supports_preview_gestures()
                            && contained_preview_painted
                        {
                            *context.defer_measurement = true;
                        }
                    }
                    SurfacePaintProgramStep::DragDecoration(decoration) => {
                        drag_feedback::paint(&mut context, decoration);
                    }
                    SurfacePaintProgramStep::InteractionFeedback => {
                        if actions::effective_enabled(context.ui, true)
                            && let Some(action) = context.plan.prepare_escape_cancel()
                            && context
                                .ui
                                .input_mut(|input| input.consume_key(Modifiers::NONE, Key::Escape))
                        {
                            context.ui.ctx().stop_dragging();
                            let origin = context
                                .ui
                                .make_persistent_id((
                                    context.instance_id,
                                    "workspace-focus",
                                    context.plan.surface(),
                                ))
                                .with(("keyboard-cancel", Key::Escape));
                            context.push_local_action(origin, action);
                        }
                    }
                    SurfacePaintProgramStep::SurfaceEnd { semantic_output } => {
                        terminal_semantic_output = Some(semantic_output);
                    }
                }
                Ok(())
            })
            .map_err(SurfacePaintProgramVisitError::into_runtime_error)?;
        let pane_output_valid = paint_audit
            .as_ref()
            .is_none_or(crate::output_proof::PaintAudit::is_valid);
        execution_receipt =
            (transient_visuals_complete && pane_output_valid).then_some(completed_program);
    }
    debug_assert_eq!(
        terminal_semantic_output,
        Some(plan.semantic_output()),
        "the paint program must terminate with its exact candidate output"
    );
    let focus_output_check = (pane_focus_request.is_some()
        && pane_focus_target_requested
        && matches!(
            pointer_authority,
            PointerActionAuthority::LocalResponses | PointerActionAuthority::HostJournal
        ))
    .then(|| crate::output_proof::PaintAppendCheck::begin(ui.painter()));
    let (pane_focus_observation, pane_focus_pending, pane_focus_capability) = pane_focus_request
        .map_or((None, None, DockspaceCapability::Supported), |request| {
            let (observation, capability) = if pane_focus_target_requested {
                let state = request.item().map_or_else(
                    || {
                        if ui.ctx().input(|input| input.focused)
                            && pane_focus_target
                                .is_some_and(|target| ui.memory(|memory| memory.has_focus(target)))
                        {
                            PaneFocusState::Focused
                        } else {
                            PaneFocusState::Unfocused
                        }
                    },
                    |item| panes.focus_state(item, ui.ctx()),
                );
                match state {
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
                        DockspaceCapability::Unavailable(request.item().map_or(
                            DockspaceUnavailableReason::PaneFocusBindingUnavailable,
                            |item| DockspaceUnavailableReason::PaneFocusStateUnknown { item },
                        )),
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
    let focus_output_valid =
        focus_output_check.is_none_or(|check| check.is_preserved(ui.painter()));
    let execution_receipt = execution_receipt.filter(|_| focus_output_valid);
    Ok(ProductPaintOutput {
        execution_receipt,
        local_actions,
        pane_focus_observation,
        pane_focus_pending,
        pane_focus_capability,
        pane_focus_bindings,
        missing_items: resources.missing_items().collect(),
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
    visual_context: tabs::RootVisualContext,
    paint: SurfacePaintDisposition,
) -> Result<(), DockspaceRuntimeError> {
    match *operation {
        SurfacePaintProgramOperation::Pane(pane) => {
            tabs::paint_pane(context, pane, visual_context, paint);
        }
        SurfacePaintProgramOperation::TabBar(bar) => {
            tabs::paint_tab_bar(context, bar, paint);
        }
        SurfacePaintProgramOperation::Tab(tab) => {
            tabs::paint_tab_record(context, tab, paint);
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
) -> bool {
    let visual = preview.visual();
    let Some(bounds) = geometry::egui_rect(plan.bounds()) else {
        return false;
    };
    let Some(rect) = preview_rect(plan.surface(), bounds, visual) else {
        return false;
    };
    let painter = ui.painter_at(bounds);
    if !painter.is_visible() {
        return false;
    }
    if preview.is_layout_projected() {
        return true;
    }
    painter.extend([
        Shape::rect_filled(rect, 2.0, visuals.drop_fill),
        Shape::rect_stroke(
            rect,
            2.0,
            Stroke::new(1.0, visuals.drop_border_color),
            StrokeKind::Inside,
        ),
    ]);
    #[cfg(feature = "native-render-support")]
    publish_native_drag_preview_diagnostic(ui, plan.surface(), rect, visual);
    true
}

#[cfg(feature = "native-render-support")]
/// Reports a painted preview program emission without proving GPU work or Presented output.
fn publish_native_drag_preview_diagnostic(
    ui: &Ui,
    surface: dockspace::model::SurfaceId,
    rect: egui::Rect,
    visual: DockspacePreviewVisual,
) {
    if !native_drag_preview_diagnostic_enabled(ui.ctx()) {
        return;
    }
    let (id_source, label) = match visual {
        DockspacePreviewVisual::Dock {
            surface: preview_surface,
            ..
        } if preview_surface == surface => (
            "dockspace-dock-preview-emitted",
            "Dockspace dock preview emitted",
        ),
        DockspacePreviewVisual::Native { host_surface, .. } if host_surface == surface => (
            "dockspace-native-drag-preview-painted",
            "Dockspace native drag preview painted",
        ),
        _ => return,
    };
    let id = ui.make_persistent_id((id_source, surface));
    ui.ctx().accesskit_node_builder(id, |node| {
        node.set_role(egui::accesskit::Role::Label);
        node.set_label(label);
        node.set_value("painted");
        node.set_bounds(geometry::accesskit_bounds(rect));
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
) -> bool {
    if preview.surface() != plan.surface() {
        return false;
    }
    let Some(rect) = geometry::egui_rect(preview.rect()) else {
        return false;
    };
    let painter = ui.painter();
    if !painter.is_visible() {
        return false;
    }
    if preview.is_layout_projected() {
        return true;
    }
    painter.extend([
        Shape::rect_filled(rect, 0.0, visuals.ghost_fill),
        Shape::rect_stroke(
            rect,
            0.0,
            Stroke::new(1.0, visuals.ghost_border_color),
            StrokeKind::Inside,
        ),
    ]);
    true
}

#[cfg(test)]
mod pointer_authority_contract_tests {
    use super::PointerActionAuthority;

    #[test]
    fn host_journal_contract() {
        let authority = PointerActionAuthority::HostJournal;
        assert!(!authority.accepts_local_pointer_actions());
        assert!(authority.supports_preview_gestures());
    }

    #[test]
    fn local_responses_contract() {
        let authority = PointerActionAuthority::LocalResponses;
        assert!(authority.accepts_local_pointer_actions());
        assert!(!authority.supports_preview_gestures());
    }

    #[cfg(feature = "native-render-support")]
    #[test]
    fn external_journal_contract() {
        let authority = PointerActionAuthority::ExternalJournal;
        assert!(!authority.accepts_local_pointer_actions());
        assert!(authority.supports_preview_gestures());
    }
}

#[cfg(all(test, feature = "native-render-support"))]
mod tests {
    use dockspace::geometry::{LogicalRect, PhysicalRect};
    use dockspace::model::SurfaceId;
    use egui::{Rect, pos2, vec2};

    use super::{
        DockspacePreviewVisual, enable_native_drag_preview_diagnostic,
        native_drag_preview_diagnostic_enabled, preview_rect,
        publish_native_drag_preview_diagnostic,
    };

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

    #[test]
    fn dock_preview_diagnostic_keeps_exact_bounds_and_passive_painted_emission() {
        let surface = SurfaceId::new(1);
        let visual = DockspacePreviewVisual::Dock {
            surface,
            rect: LogicalRect::new(30.0, 40.0, 50.0, 60.0).unwrap(),
        };
        let bounds = Rect::from_min_size(pos2(10.0, 20.0), vec2(300.0, 200.0));
        let rect = preview_rect(surface, bounds, visual).unwrap();
        let context = egui::Context::default();
        context.enable_accesskit();
        enable_native_drag_preview_diagnostic(&context);
        let output = context.run_ui(egui::RawInput::default(), |ui| {
            let clip = ui.clip_rect();
            let focused = ui.memory(egui::Memory::focused);
            publish_native_drag_preview_diagnostic(ui, surface, rect, visual);
            assert_eq!(ui.clip_rect(), clip);
            assert_eq!(ui.memory(egui::Memory::focused), focused);
        });
        let update = output.platform_output.accesskit_update.as_ref().unwrap();
        let nodes: Vec<_> = update
            .nodes
            .iter()
            .filter(|(_, node)| node.label() == Some("Dockspace dock preview emitted"))
            .collect();
        assert_eq!(nodes.len(), 1);
        let node = &nodes[0].1;
        assert_eq!(node.role(), egui::accesskit::Role::Label);
        assert_eq!(node.value(), Some("painted"));
        assert_eq!(node.bounds(), Some(super::geometry::accesskit_bounds(rect)));
        assert!(!node.supports_action(egui::accesskit::Action::Focus));
        assert!(!node.supports_action(egui::accesskit::Action::Click));
        output.drop_without_applying_deltas();
    }

    #[test]
    fn preview_diagnostics_keep_opt_in_and_exact_dock_and_native_surface_guards() {
        let surface = SurfaceId::new(1);
        let other = SurfaceId::new(2);
        let rect = Rect::from_min_size(pos2(30.0, 40.0), vec2(50.0, 60.0));
        let dock = DockspacePreviewVisual::Dock {
            surface,
            rect: LogicalRect::new(30.0, 40.0, 50.0, 60.0).unwrap(),
        };
        let native = DockspacePreviewVisual::Native {
            host_surface: surface,
            target_surface: other,
            placement: PhysicalRect::new(400.0, 100.0, 300.0, 200.0).unwrap(),
        };
        for (enabled, host, visual, expected) in [
            (false, surface, dock, None),
            (true, other, dock, None),
            (false, surface, native, None),
            (true, other, native, None),
            (
                true,
                surface,
                native,
                Some("Dockspace native drag preview painted"),
            ),
        ] {
            let context = egui::Context::default();
            context.enable_accesskit();
            if enabled {
                enable_native_drag_preview_diagnostic(&context);
            }
            let output = context.run_ui(egui::RawInput::default(), |ui| {
                publish_native_drag_preview_diagnostic(ui, host, rect, visual);
            });
            let update = output.platform_output.accesskit_update.as_ref().unwrap();
            let nodes: Vec<_> = update
                .nodes
                .iter()
                .filter(|(_, node)| {
                    matches!(
                        node.label(),
                        Some(
                            "Dockspace native drag preview painted"
                                | "Dockspace dock preview emitted"
                        )
                    )
                })
                .collect();
            assert_eq!(nodes.len(), usize::from(expected.is_some()));
            if let Some(label) = expected {
                assert_eq!(nodes[0].1.label(), Some(label));
                assert_eq!(nodes[0].1.value(), Some("painted"));
            }
            output.drop_without_applying_deltas();
        }
    }
}
