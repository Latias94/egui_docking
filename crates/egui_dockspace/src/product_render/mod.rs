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
mod schedule;
mod splitters;
mod tab_chrome;
mod tabs;

use dockspace::model::{FloatingPresentationId, ItemId};
use dockspace::runtime::{
    ContainedPaintRecord, DockspacePaneFocusObservation, DockspacePaneFocusRequest,
    DockspacePreviewVisual, DockspaceReceiverDescriptor, PreparedPaneFocusObservation,
    PreparedSurfaceAction, SurfaceGesturePhase, SurfacePaintPlan,
};
use egui::{Color32, Id, Key, Modifiers, Response, Sense, Stroke, StrokeKind, Ui};

use crate::pane::{PaneFocusState, PaneView};
use crate::response::{DockspaceCapability, DockspaceUnavailableReason};
use crate::style::{DockStyle, ResolvedDockVisuals};

use measurement::PaintResources;
pub(crate) use measurement::measure_surface;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PointerActionAuthority {
    LocalResponses,
    #[cfg(feature = "native-render-support")]
    ExternalJournal,
}

impl PointerActionAuthority {
    const fn accepts_local_pointer_actions(self) -> bool {
        matches!(self, Self::LocalResponses)
    }

    const fn acknowledges_previews_locally(self) -> bool {
        matches!(self, Self::LocalResponses)
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
    local_actions: &'ui mut Vec<PreparedSurfaceAction>,
    presentation_actions: &'ui mut Vec<PreparedSurfaceAction>,
    #[cfg(feature = "native-render-support")]
    receivers: &'ui mut Vec<ProductReceiverBinding>,
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
    let frame = ui.ctx().cumulative_frame_nr();
    let first = ui.ctx().data_mut(|data| {
        if data.get_temp::<u64>(marker) == Some(frame) {
            false
        } else {
            data.insert_temp(marker, frame);
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
        self.pointer_authority.accepts_local_pointer_actions() && response.dragged()
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

    fn push_presentation_action(&mut self, action: PreparedSurfaceAction) {
        self.presentation_actions.push(action);
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

pub(crate) struct ProductPaintOutput {
    pub(crate) local_actions: Vec<PreparedSurfaceAction>,
    pub(crate) presentation_actions: Vec<PreparedSurfaceAction>,
    pub(crate) pane_focus_observation: Option<PreparedPaneFocusObservation>,
    pub(crate) pane_focus_capability: DockspaceCapability,
    pub(crate) missing_items: BTreeSet<ItemId>,
    #[cfg(feature = "native-render-support")]
    pub(crate) receivers: Vec<ProductReceiverBinding>,
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
    #[cfg(feature = "native-render-support")] scroll_registrar: Option<
        &mut dyn FnMut(&Ui, egui::Rect, Id) -> (Id, egui::LayerId),
    >,
) -> ProductPaintOutput {
    let visuals = style.resolved_visuals(ui.visuals());
    let resources = PaintResources::from_plan(plan, ui, panes, style);
    let pane_focus_request = plan.pane_focus_request();
    let mut pane_focus_target_requested = false;
    let mut local_actions = Vec::new();
    let mut presentation_actions = Vec::new();
    #[cfg(feature = "native-render-support")]
    let mut receivers = Vec::new();
    #[cfg(feature = "native-render-support")]
    let mut scroll_receivers = Vec::new();
    let mut defer_measurement = false;
    let mut transient_visuals_complete = true;
    let local_primary_presses = actions::local_primary_presses(ui, pointer_authority);
    if let Some(bounds) = geometry::egui_rect(plan.bounds()) {
        ui.allocate_rect(bounds, Sense::hover());
        ui.painter()
            .rect_filled(bounds, 0.0, visuals.workspace_fill);
    }

    let schedule = schedule::SurfacePaintSchedule::from_plan(plan);

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
            local_actions: &mut local_actions,
            presentation_actions: &mut presentation_actions,
            #[cfg(feature = "native-render-support")]
            receivers: &mut receivers,
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
        for root in schedule.main_roots() {
            tabs::paint_root(&mut context, root, tabs::RootVisualContext::Main);
            splitters::paint_root(&mut context, root);
        }

        for root in schedule.contained_roots() {
            let record = root.contained();
            let window_visuals = context
                .style
                .resolved_contained_window(context.ui.style(), record.is_frontmost());
            contained::paint_background(&mut context, record, root.records(), window_visuals);
            tabs::paint_root(
                &mut context,
                root.records(),
                tabs::RootVisualContext::Contained {
                    record,
                    pane_fill: window_visuals.frame.fill,
                },
            );
            splitters::paint_root(&mut context, root.records());
            contained::paint_controls(&mut context, record, root.records());
            context.flush_contained_activation(record);
        }

        tab_chrome::paint(&mut context);

        let drag_preview_required = context.plan.drag_preview().is_some();
        let drag_preview_painted = paint_preview(context.ui, context.plan, context.visuals);
        transient_visuals_complete &= !drag_preview_required || drag_preview_painted;
        if context.pointer_authority.acknowledges_previews_locally()
            && drag_preview_painted
            && let Some(action) = context.plan.prepare_drag_preview_painted()
        {
            context.push_presentation_action(action);
            *context.defer_measurement = true;
        }
        guides::paint(&mut context);
        let contained_preview_required = context.plan.contained_transform_preview().is_some();
        let contained_preview_painted =
            paint_contained_transform_preview(context.ui, context.plan, context.visuals);
        transient_visuals_complete &= !contained_preview_required || contained_preview_painted;
        if context.pointer_authority.acknowledges_previews_locally()
            && contained_preview_painted
            && let Some(action) = context.plan.prepare_contained_transform_preview_painted()
        {
            context.push_presentation_action(action);
        }
        drag_feedback::paint(&mut context);
        if context.pointer_authority.accepts_local_pointer_actions()
            && let Some(action) = context.plan.prepare_escape_cancel()
            && context
                .ui
                .input_mut(|input| input.consume_key(Modifiers::NONE, Key::Escape))
        {
            context.ui.ctx().stop_dragging();
            context.push_local_action(action);
        }
    }
    let (pane_focus_observation, pane_focus_capability) =
        pane_focus_request.map_or((None, DockspaceCapability::Supported), |request| {
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
            (
                plan.prepare_pane_focus_observation(request, observation),
                capability,
            )
        });
    #[cfg(not(feature = "native-render-support"))]
    let _ = transient_visuals_complete;
    ProductPaintOutput {
        local_actions,
        presentation_actions,
        pane_focus_observation,
        pane_focus_capability,
        missing_items: resources.missing_items().collect(),
        #[cfg(feature = "native-render-support")]
        receivers,
        #[cfg(feature = "native-render-support")]
        scroll_receivers,
        defer_measurement,
        #[cfg(feature = "native-render-support")]
        transient_visuals_complete,
    }
}

fn paint_preview(ui: &Ui, plan: SurfacePaintPlan<'_>, visuals: ResolvedDockVisuals) -> bool {
    let Some(preview) = plan.drag_preview() else {
        return false;
    };
    let Some(bounds) = geometry::egui_rect(plan.bounds()) else {
        return false;
    };
    let Some(rect) = preview_rect(plan.surface(), bounds, preview.visual()) else {
        return false;
    };
    let painter = ui.painter_at(bounds);
    painter.rect_filled(rect, 2.0, visuals.drop_fill);
    painter.rect_stroke(
        rect,
        2.0,
        Stroke::new(1.0, visuals.drop_border_color),
        StrokeKind::Inside,
    );
    true
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
    visuals: ResolvedDockVisuals,
) -> bool {
    let Some(preview) = plan.contained_transform_preview() else {
        return false;
    };
    if preview.surface() != plan.surface() {
        return false;
    }
    let Some(rect) = geometry::egui_rect(preview.rect()) else {
        return false;
    };
    ui.painter().rect(
        rect,
        0.0,
        visuals.ghost_fill,
        Stroke::new(1.0, visuals.ghost_border_color),
        StrokeKind::Inside,
    );
    true
}

#[cfg(all(test, feature = "native-render-support"))]
mod tests {
    use dockspace::geometry::{LogicalRect, PhysicalRect};
    use dockspace::model::SurfaceId;
    use egui::{Rect, pos2, vec2};

    use super::{DockspacePreviewVisual, PointerActionAuthority, preview_rect};

    #[test]
    fn external_journal_uses_renderer_settlement_for_preview_authority() {
        assert!(
            PointerActionAuthority::LocalResponses.acknowledges_previews_locally(),
            "the official-egui product path has no renderer settlement callback"
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
}
