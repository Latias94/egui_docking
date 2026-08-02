//! Stateless egui painting and semantic interaction extraction.

#[cfg(test)]
use std::cell::Cell;
use std::collections::BTreeMap;

use dockspace::command::{ItemSource, MovePayload};
use dockspace::engine::{TabListMenuNavigation, TabScrollAdjustment};
use dockspace::error::CommandError;
use dockspace::geometry::{LogicalPoint, LogicalRect, LogicalSize};
use dockspace::graph::{Axis, Workspace};
use dockspace::ids::{FloatingPresentationId, RootId, SurfaceId};
use dockspace::intent::CloseSceneTarget;
use dockspace::interaction::{
    ActiveDragView, ContainedTransformPreview, ContainedTransformPreviewToken, DragPhase,
    InteractionPreview, InteractionState, InteractionStatus, PreviewToken, PreviewVisual,
};
use dockspace::presentation_hit::{
    PresentationHitManifest, PresentationHitRegionKind, PresentationPointerLane,
};
use dockspace::presentation_observation::HostInteractionPresentation;
use dockspace::scene::{
    PaneRecord, PaneSceneId, PresentationPlan, SplitterResizeTarget, SplitterSceneId,
    SurfaceSceneStamp, TabBarRecord, TabBarSceneId, TabRecord, TabSceneId, TabStripControlRecord,
};
use dockspace::tab_strip::{TabListMenuSessionId, TabStripControlId};
use egui::accesskit::Action;
use egui::{
    Align2, Event, FontSelection, Id, Key, Modifiers, Pos2, Rect, Stroke, StrokeKind, TextStyle,
    Ui, pos2, vec2,
};

use crate::drop_guides;
use crate::floating;
use crate::hit::SemanticActivation;
use crate::pane::PaneView;
use crate::projection::{EguiSurfacePaintResources, TabStripStateMap};
use crate::receiver::PaintReceiverRegistrations;
use crate::splits;
use crate::style::DockStyle;
use crate::tabs;

/// One cardinal edge moved by a discrete contained-floating adjustment.
///
/// Corners are intentionally absent: AccessKit's splitter role is one-dimensional,
/// so a diagonal resize must remain a pointer gesture until the adapter can expose
/// two independent axis actions without misrepresenting either one.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ContainedResizeEdge {
    Top,
    Right,
    Bottom,
    Left,
}

/// A semantic action observed while painting one exact surface plan.
///
/// Actions contain frozen core references or absolute coordinates. The facade
/// submits them in generation order at the current callback boundary.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum RenderAction {
    Select(ItemSource),
    ActivateTabStripControl {
        surface: SurfaceId,
        control: TabStripControlId,
    },
    ActivateTabListMenuRow {
        surface: SurfaceId,
        session: TabListMenuSessionId,
        tab: TabSceneId,
    },
    AdjustTabStripScroll {
        surface: SurfaceId,
        bar: dockspace::scene::TabBarSceneId,
        adjustment: TabScrollAdjustment,
    },
    AdjustTabListMenuScroll {
        surface: SurfaceId,
        session: TabListMenuSessionId,
        adjustment: TabScrollAdjustment,
    },
    NavigateTabListMenu {
        surface: SurfaceId,
        session: TabListMenuSessionId,
        navigation: TabListMenuNavigation,
    },
    DismissTabListMenu {
        surface: SurfaceId,
        session: TabListMenuSessionId,
    },
    RequestSemanticClose {
        scene: SurfaceSceneStamp,
        target: CloseSceneTarget,
    },
    AdjustSplitterResize {
        scene: SurfaceSceneStamp,
        splitter: SplitterSceneId,
        delta: f64,
    },
    AdjustContainedResize {
        scene: SurfaceSceneStamp,
        surface: SurfaceId,
        root: dockspace::ids::RootId,
        floating: FloatingPresentationId,
        expected_rect: LogicalRect,
        minimum_size: LogicalSize,
        edge: ContainedResizeEdge,
        delta: f64,
    },
}

/// Exact position of one renderer-observed action in the host event batch.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum RenderActionPosition {
    /// The action was derived from this exact raw egui event.
    RawEvent(usize),
    /// The action is an adapter continuation observed only after the complete input batch.
    PostBatchContinuation,
}

/// One semantic action paired with its non-inferred causal position.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct PositionedRenderAction {
    action: RenderAction,
    position: RenderActionPosition,
}

impl PositionedRenderAction {
    const fn new(action: RenderAction, position: RenderActionPosition) -> Self {
        Self { action, position }
    }

    pub(crate) fn into_parts(self) -> (RenderAction, RenderActionPosition) {
        (self.action, self.position)
    }
}

/// Failure to bind a renderer-observed action to one exact raw event.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SemanticActionCausalityError {
    MatchingRawEvents {
        matching_raw_events: usize,
    },
    #[cfg(egui_backend_event_envelope)]
    BackendCorrelationUnavailable {
        raw_event_index: usize,
    },
}

impl SemanticActionCausalityError {
    pub(crate) const fn matching_raw_events(self) -> usize {
        match self {
            Self::MatchingRawEvents {
                matching_raw_events,
            } => matching_raw_events,
            #[cfg(egui_backend_event_envelope)]
            Self::BackendCorrelationUnavailable { .. } => 0,
        }
    }

    #[cfg(egui_backend_event_envelope)]
    pub(crate) const fn uncorrelated_raw_event(self) -> Option<usize> {
        match self {
            Self::MatchingRawEvents { .. } => None,
            Self::BackendCorrelationUnavailable { raw_event_index } => Some(raw_event_index),
        }
    }
}

/// Paint result kept separate from the docking engine mutation boundary.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct RenderOutput {
    pub(crate) actions: Vec<PositionedRenderAction>,
    pub(crate) capture_errors: Vec<CommandError>,
    pub(crate) receivers: Option<PaintReceiverRegistrations>,
    capture_semantic_actions: bool,
    primary_button_event_count: usize,
    #[cfg(not(egui_backend_event_envelope))]
    raw_events: Vec<Event>,
    semantic_causality_error: Option<SemanticActionCausalityError>,
    painted_drag_preview: Option<PreviewToken>,
    painted_contained_transform_preview: Option<ContainedTransformPreviewToken>,
    #[cfg(test)]
    content_lookup_work: ContentLookupWork,
}

impl RenderOutput {
    #[cfg(all(test, not(egui_backend_event_envelope)))]
    fn with_raw_events(raw_events: Vec<Event>) -> Self {
        Self {
            raw_events,
            capture_semantic_actions: true,
            ..Self::default()
        }
    }

    #[cfg(all(test, egui_backend_event_envelope))]
    fn from_ui(ui: &Ui) -> Self {
        Self::from_ui_with_semantic_action_capture(ui, true)
    }

    fn from_ui_with_semantic_action_capture(ui: &Ui, capture_semantic_actions: bool) -> Self {
        let primary_button_event_count = ui.input(|input| {
            input
                .events
                .iter()
                .filter(|event| {
                    matches!(
                        event,
                        Event::PointerButton {
                            button: egui::PointerButton::Primary,
                            ..
                        }
                    )
                })
                .count()
        });
        #[cfg(egui_backend_event_envelope)]
        {
            let _ = ui;
            Self {
                capture_semantic_actions,
                primary_button_event_count,
                ..Self::default()
            }
        }
        #[cfg(not(egui_backend_event_envelope))]
        {
            Self {
                capture_semantic_actions,
                primary_button_event_count,
                raw_events: ui.input(|input| input.events.clone()),
                ..Self::default()
            }
        }
    }

    pub(crate) const fn interaction_presentation(&self) -> HostInteractionPresentation {
        HostInteractionPresentation::new(
            self.painted_drag_preview,
            self.painted_contained_transform_preview,
        )
    }

    pub(crate) fn push_key(&mut self, ui: &Ui, action: RenderAction, key: Key) {
        self.push_matching_raw_event(ui, action, |event| {
            matches!(event, Event::Key { key: candidate, pressed: true, .. } if *candidate == key)
        });
    }

    pub(crate) fn push_unmodified_key(&mut self, ui: &Ui, action: RenderAction, key: Key) {
        self.push_matching_raw_event(ui, action, |event| {
            matches!(
                event,
                Event::Key {
                    key: candidate,
                    pressed: true,
                    modifiers,
                    ..
                } if *candidate == key && *modifiers == Modifiers::NONE
            )
        });
    }

    pub(crate) fn push_accesskit(
        &mut self,
        ui: &Ui,
        action: RenderAction,
        id: Id,
        requested: Action,
    ) {
        let target = id.accesskit_id();
        self.push_matching_raw_event(ui, action, |event| {
            matches!(
                event,
                Event::AccessKitActionRequest(request)
                    if request.target_tree == egui::accesskit::TreeId::ROOT
                        && request.target_node == target
                        && request.action == requested
            )
        });
    }

    pub(crate) fn push_widget_activation(
        &mut self,
        ui: &Ui,
        action: RenderAction,
        id: Id,
        keyboard_focused: bool,
        activation: SemanticActivation,
    ) {
        match activation {
            SemanticActivation::Pointer(position) => {
                if self.primary_button_event_count <= 1 {
                    return;
                }
                self.push_matching_raw_event(ui, action, |event| {
                    matches!(
                        event,
                        Event::PointerButton {
                            pos,
                            button: egui::PointerButton::Primary,
                            pressed: false,
                            ..
                        } if pos.x.to_bits() == position.x.to_bits()
                            && pos.y.to_bits() == position.y.to_bits()
                    )
                });
            }
            SemanticActivation::Semantic => {
                let target = id.accesskit_id();
                self.push_matching_raw_event(ui, action, |event| match event {
                    Event::Key {
                        key: Key::Enter | Key::Space,
                        pressed: true,
                        ..
                    } => keyboard_focused,
                    Event::AccessKitActionRequest(request) => {
                        request.target_tree == egui::accesskit::TreeId::ROOT
                            && request.target_node == target
                            && request.action == Action::Click
                    }
                    _ => false,
                });
            }
        }
    }

    pub(crate) fn push_post_batch_continuation(&mut self, action: RenderAction) {
        if !self.capture_semantic_actions {
            return;
        }
        self.actions.push(PositionedRenderAction::new(
            action,
            RenderActionPosition::PostBatchContinuation,
        ));
    }

    pub(crate) const fn semantic_causality_error(&self) -> Option<SemanticActionCausalityError> {
        self.semantic_causality_error
    }

    #[cfg(not(egui_backend_event_envelope))]
    fn push_matching_raw_event(
        &mut self,
        _ui: &Ui,
        action: RenderAction,
        matches_event: impl Fn(&Event) -> bool,
    ) {
        if !self.capture_semantic_actions {
            return;
        }
        let mut matching = self
            .raw_events
            .iter()
            .enumerate()
            .filter(|(_, event)| matches_event(event));
        let first = matching.next().map(|(index, _)| index);
        let matching_raw_events = usize::from(first.is_some()) + matching.count();
        if let Some(index) = first.filter(|_| matching_raw_events == 1) {
            self.actions.push(PositionedRenderAction::new(
                action,
                RenderActionPosition::RawEvent(index),
            ));
        } else {
            self.semantic_causality_error.get_or_insert(
                SemanticActionCausalityError::MatchingRawEvents {
                    matching_raw_events,
                },
            );
        }
    }

    #[cfg(egui_backend_event_envelope)]
    fn push_matching_raw_event(
        &mut self,
        ui: &Ui,
        action: RenderAction,
        matches_event: impl Fn(&Event) -> bool,
    ) {
        if !self.capture_semantic_actions {
            return;
        }
        // egui reports one frame-aggregated widget outcome. Claim every exact contributing
        // derivative and place that aggregate at the final derivative instead of guessing one
        // event by payload equality.
        let mut last_raw_event_index = None;
        loop {
            let Some(claim) = ui.input_mut(|input| input.claim_event_envelope(&matches_event))
            else {
                break;
            };
            if !claim.correlation().is_known() {
                self.semantic_causality_error.get_or_insert(
                    SemanticActionCausalityError::BackendCorrelationUnavailable {
                        raw_event_index: claim.raw_event_index(),
                    },
                );
                return;
            }
            last_raw_event_index = Some(claim.raw_event_index());
        }
        let Some(raw_event_index) = last_raw_event_index else {
            self.semantic_causality_error.get_or_insert(
                SemanticActionCausalityError::MatchingRawEvents {
                    matching_raw_events: 0,
                },
            );
            return;
        };
        self.actions.push(PositionedRenderAction::new(
            action,
            RenderActionPosition::RawEvent(raw_event_index),
        ));
    }

    pub(crate) fn capture<T>(&mut self, result: Result<T, CommandError>) -> Option<T> {
        match result {
            Ok(value) => Some(value),
            Err(error) => {
                if !self.capture_errors.contains(&error) {
                    self.capture_errors.push(error);
                }
                None
            }
        }
    }

    pub(crate) fn register_receiver(
        &mut self,
        response: &egui::Response,
        region: PresentationHitRegionKind,
    ) {
        self.receivers
            .as_mut()
            .expect("a surface paint initializes its receiver registry")
            .register(response, region);
    }

    #[cfg(egui_backend_event_envelope)]
    pub(crate) fn register_scroll_receiver(
        &mut self,
        receiver: egui::ScrollReceiver,
        region: PresentationHitRegionKind,
    ) {
        self.receivers
            .as_mut()
            .expect("a surface paint initializes its receiver registry")
            .register_scroll(receiver, region);
    }
}

#[cfg(test)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct ContentLookupWork {
    indexed_roots: usize,
    indexed_tabs: usize,
    root_lookups: usize,
    tab_lookups: usize,
}

struct PresentationPaintIndex<'a> {
    panes_by_root: BTreeMap<RootId, Vec<&'a PaneRecord>>,
    bars: BTreeMap<TabBarSceneId, &'a TabBarRecord>,
    tabs_by_bar: BTreeMap<TabBarSceneId, Vec<&'a TabRecord>>,
    controls_by_bar: BTreeMap<TabBarSceneId, Vec<&'a TabStripControlRecord>>,
    #[cfg(test)]
    root_lookup_count: Cell<usize>,
    #[cfg(test)]
    tab_lookup_count: Cell<usize>,
}

impl<'a> PresentationPaintIndex<'a> {
    fn new(plan: &'a PresentationPlan) -> Self {
        let mut panes_by_root = BTreeMap::<RootId, Vec<&PaneRecord>>::new();
        for pane in plan.pane_records() {
            panes_by_root.entry(pane.id().root).or_default().push(pane);
        }
        let bars = plan
            .tab_bar_records()
            .iter()
            .map(|bar| (*bar.id(), bar))
            .collect();
        let mut tabs_by_bar = BTreeMap::<TabBarSceneId, Vec<&TabRecord>>::new();
        for tab in plan.tab_records() {
            let id = tab.id();
            tabs_by_bar
                .entry(TabBarSceneId {
                    root: id.root,
                    tabs: id.tabs,
                })
                .or_default()
                .push(tab);
        }
        for tabs in tabs_by_bar.values_mut() {
            tabs.sort_unstable_by_key(|tab| tab.ordinal());
        }
        let mut controls_by_bar = BTreeMap::<TabBarSceneId, Vec<&TabStripControlRecord>>::new();
        for control in plan.tab_strip_control_records() {
            let bar = match control.id() {
                TabStripControlId::ScrollBackward(bar)
                | TabStripControlId::ScrollForward(bar)
                | TabStripControlId::TabListMenu(bar) => bar,
            };
            controls_by_bar.entry(bar).or_default().push(control);
        }
        Self {
            panes_by_root,
            bars,
            tabs_by_bar,
            controls_by_bar,
            #[cfg(test)]
            root_lookup_count: Cell::new(0),
            #[cfg(test)]
            tab_lookup_count: Cell::new(0),
        }
    }

    fn panes(&self, root: RootId) -> &[&'a PaneRecord] {
        #[cfg(test)]
        self.root_lookup_count.set(self.root_lookup_count.get() + 1);
        self.panes_by_root.get(&root).map_or(&[], Vec::as_slice)
    }

    fn bar(&self, pane: PaneSceneId) -> Option<&'a TabBarRecord> {
        #[cfg(test)]
        self.tab_lookup_count.set(self.tab_lookup_count.get() + 1);
        self.bars
            .get(&TabBarSceneId {
                root: pane.root,
                tabs: pane.tabs,
            })
            .copied()
    }

    fn tabs(&self, bar: TabBarSceneId) -> &[&'a TabRecord] {
        self.tabs_by_bar.get(&bar).map_or(&[], Vec::as_slice)
    }

    fn controls(&self, bar: TabBarSceneId) -> &[&'a TabStripControlRecord] {
        self.controls_by_bar.get(&bar).map_or(&[], Vec::as_slice)
    }

    #[cfg(test)]
    fn work(&self) -> ContentLookupWork {
        ContentLookupWork {
            indexed_roots: self.panes_by_root.len(),
            indexed_tabs: self.bars.len(),
            root_lookups: self.root_lookup_count.get(),
            tab_lookups: self.tab_lookup_count.get(),
        }
    }
}

/// Paints one already validated surface plan and reports semantic input.
///
/// This function never mutates the workspace or the interaction state. When
/// `interactions_current` is false, chrome remains painted but cannot publish
/// geometry-derived response events, focus, or accessibility actions. Pane
/// callbacks resolve from the current measured plan, while stale, bootstrap,
/// and projection-changing content is painted through a disabled child UI. A
/// superseded or relocated pane is never invoked through retained geometry. A
/// current pane remains enabled even when presentation authority is
/// unavailable. Global Escape and matching release edges still terminate an
/// active core session.
#[allow(clippy::too_many_arguments)]
pub(crate) fn paint_surface(
    ui: &mut Ui,
    instance_id: Id,
    surface: SurfaceId,
    surface_bounds: Rect,
    plan: Option<&PresentationPlan>,
    resources: &EguiSurfacePaintResources,
    tab_strip_states: &mut TabStripStateMap,
    content_resources: &EguiSurfacePaintResources,
    workspace: &Workspace,
    panes: &mut dyn PaneView,
    style: &DockStyle,
    interaction: &InteractionState,
    presentation_drag_preview: Option<&InteractionPreview>,
    presentation_contained_transform_preview: Option<&ContainedTransformPreview>,
    pane_content_current: bool,
    interaction_scene: Option<SurfaceSceneStamp>,
    semantic_scene: Option<SurfaceSceneStamp>,
    capture_framework_actions: bool,
    authoritative_hit_manifest: Option<&PresentationHitManifest>,
    is_gesture_source_surface: bool,
) -> RenderOutput {
    let mut output = RenderOutput {
        receivers: Some(PaintReceiverRegistrations::new(
            ui.ctx().viewport_id(),
            ui.ctx().cumulative_pass_nr(),
        )),
        ..RenderOutput::from_ui_with_semantic_action_capture(ui, capture_framework_actions)
    };
    let interactions_current = plan.is_some() && interaction_scene.is_some();
    let escape_pressed = capture_framework_actions
        && semantic_scene.is_some()
        && consume_gesture_escape(ui, interaction.status(), is_gesture_source_surface);
    let accept_events = interactions_current && !escape_pressed;
    ui.painter()
        .rect_filled(surface_bounds, 0.0, style.workspace_fill);
    let Some(plan) = plan else {
        return output;
    };
    debug_assert_eq!(plan.surface(), surface);
    let tab_scroll_owner = if accept_events {
        ui.input(|input| input.pointer.hover_pos())
            .and_then(|pointer| to_logical_point(pointer).ok())
            .and_then(|pointer| plan.tab_scroll_owner_at(pointer))
    } else {
        None
    };
    let hovered_splitter = if accept_events {
        authoritative_splitter_target(ui, plan)
    } else {
        None
    };
    set_splitter_cursor(ui, plan, hovered_splitter);

    let index = PresentationPaintIndex::new(plan);
    if let Some(main) = workspace
        .surface(surface)
        .and_then(|presentation| presentation.main_root)
    {
        paint_root(
            ui,
            instance_id,
            surface,
            main,
            false,
            plan,
            &index,
            resources,
            tab_strip_states,
            content_resources,
            workspace,
            panes,
            style,
            interaction,
            tab_scroll_owner,
            pane_content_current,
            interaction_scene.filter(|_| accept_events),
            semantic_scene.filter(|_| accept_events),
            hovered_splitter,
            authoritative_hit_manifest,
            &mut output,
        );
    }
    for contained in plan.contained_records() {
        paint_root(
            ui,
            instance_id,
            surface,
            contained.root(),
            true,
            plan,
            &index,
            resources,
            tab_strip_states,
            content_resources,
            workspace,
            panes,
            style,
            interaction,
            tab_scroll_owner,
            pane_content_current,
            interaction_scene.filter(|_| accept_events),
            semantic_scene.filter(|_| accept_events),
            hovered_splitter,
            authoritative_hit_manifest,
            &mut output,
        );
    }

    tabs::paint_authoritative_tab_list_menu(
        ui,
        instance_id,
        surface,
        resources,
        style,
        Some(plan),
        accept_events,
        &mut output,
    );

    register_manifest_overlay_receivers(ui, instance_id, authoritative_hit_manifest, &mut output);

    output.painted_drag_preview = paint_preview(
        ui,
        surface,
        surface_bounds,
        presentation_drag_preview,
        style,
    );
    drop_guides::paint(
        &ui.painter_at(surface_bounds),
        surface,
        interaction.drop_affordance(),
        style,
    );
    output.painted_contained_transform_preview = paint_contained_transform_preview(
        ui,
        surface,
        surface_bounds,
        presentation_contained_transform_preview,
        style,
    );
    paint_drag_ghost(
        ui,
        plan,
        resources,
        workspace,
        interaction.active_drag_view(),
        style,
    );
    #[cfg(test)]
    {
        output.content_lookup_work = index.work();
    }
    output
}

fn register_manifest_overlay_receivers(
    ui: &Ui,
    instance_id: Id,
    manifest: Option<&PresentationHitManifest>,
    output: &mut RenderOutput,
) {
    let Some(manifest) = manifest else {
        return;
    };
    let mut regions = manifest
        .regions()
        .iter()
        .copied()
        .filter(|region| {
            !region.is_passive()
                && matches!(
                    region.id().kind(),
                    PresentationHitRegionKind::SplitterJunction(_)
                        | PresentationHitRegionKind::TabStripControl(
                            dockspace::tab_strip::TabStripControlId::ScrollBackward(_)
                                | dockspace::tab_strip::TabStripControlId::ScrollForward(_)
                        )
                )
        })
        .collect::<Vec<_>>();
    regions.sort_unstable_by_key(|region| (region.stack(), region.id()));
    for region in regions {
        let Some(rect) = from_logical_rect(region.hit().rect()) else {
            continue;
        };
        let sense = if region.lanes().contains(PresentationPointerLane::HoverDrop) {
            egui::Sense::hover()
        } else if region.lanes().contains(PresentationPointerLane::Click) {
            egui::Sense::click()
        } else {
            egui::Sense::drag()
        };
        let response = ui.interact(
            crate::hit::interact_rect(rect),
            Id::new((
                "egui_dockspace",
                "presentation-hit",
                instance_id,
                region.id(),
            )),
            sense,
        );
        output.register_receiver(&response, region.id().kind());
    }

    let mut hover_regions = manifest
        .regions()
        .iter()
        .copied()
        .filter(|region| {
            !region.is_passive() && region.lanes().contains(PresentationPointerLane::HoverDrop)
        })
        .collect::<Vec<_>>();
    hover_regions.sort_unstable_by_key(|region| (region.stack(), region.id()));
    for region in hover_regions {
        let Some(rect) = from_logical_rect(region.hit().rect()) else {
            continue;
        };
        let response = ui.interact(
            crate::hit::interact_rect(rect),
            Id::new((
                "egui_dockspace",
                "presentation-hover-hit",
                instance_id,
                region.id(),
            )),
            egui::Sense::hover(),
        );
        output.register_receiver(&response, region.id().kind());
    }
}

#[allow(clippy::too_many_arguments)]
fn paint_root(
    ui: &mut Ui,
    instance_id: Id,
    surface: SurfaceId,
    root: RootId,
    is_contained: bool,
    plan: &PresentationPlan,
    index: &PresentationPaintIndex<'_>,
    resources: &EguiSurfacePaintResources,
    tab_strip_states: &mut TabStripStateMap,
    content_resources: &EguiSurfacePaintResources,
    workspace: &Workspace,
    panes: &mut dyn PaneView,
    style: &DockStyle,
    interaction: &InteractionState,
    tab_scroll_owner: Option<TabBarSceneId>,
    pane_content_current: bool,
    interaction_scene: Option<SurfaceSceneStamp>,
    semantic_scene: Option<SurfaceSceneStamp>,
    hovered_splitter: Option<SplitterResizeTarget>,
    authoritative_hit_manifest: Option<&PresentationHitManifest>,
    output: &mut RenderOutput,
) {
    let interactions_current = interaction_scene.is_some();
    let contained = is_contained
        .then(|| {
            plan.contained_records()
                .iter()
                .find(|contained| contained.root() == root)
        })
        .flatten();
    if let Some(contained) = contained {
        let frontmost = plan
            .contained_records()
            .last()
            .is_some_and(|front| front.floating() == contained.floating());
        floating::paint_background(
            ui,
            instance_id,
            surface,
            contained,
            frontmost,
            style,
            output,
        );
    }

    for pane in index.panes(root) {
        let bar = index.bar(pane.id());
        let visible_tabs = bar.map_or(&[][..], |bar| index.tabs(*bar.id()));
        let controls = bar.map_or(&[][..], |bar| index.controls(*bar.id()));
        tabs::paint_tabs(
            ui,
            instance_id,
            surface,
            pane,
            bar,
            visible_tabs,
            controls,
            resources,
            tab_strip_states,
            content_resources,
            workspace,
            panes,
            style,
            interaction.active_drag_view(),
            tab_scroll_owner,
            interactions_current,
            pane_content_current,
            interaction_scene,
            plan,
            authoritative_hit_manifest,
            output,
        );
    }

    for splitter in plan
        .splitter_records()
        .iter()
        .filter(|splitter| splitter.id().root == root)
    {
        splits::paint_splitter(
            ui,
            instance_id,
            surface,
            splitter,
            style,
            interaction_scene,
            semantic_scene,
            splitter_target_contains(hovered_splitter, *splitter.id()),
            output,
        );
    }

    if let Some(floating) = contained {
        floating::paint_chrome_and_interact(
            ui,
            instance_id,
            surface,
            plan,
            floating,
            resources,
            workspace,
            style,
            interaction.status(),
            interaction_scene,
            output,
        );
    }
}

fn paint_preview(
    ui: &Ui,
    host_surface: SurfaceId,
    bounds: Rect,
    preview: Option<&InteractionPreview>,
    style: &DockStyle,
) -> Option<PreviewToken> {
    let Some(preview) = preview else {
        return None;
    };
    let visual_rect = match preview.visual() {
        PreviewVisual::Dock { surface, rect, .. }
        | PreviewVisual::Contained { surface, rect, .. }
            if *surface == host_surface =>
        {
            from_logical_rect(*rect)
        }
        // A prospective native window has no surface-local coordinate system
        // or presentation stream yet. Its exact desktop placement is carried
        // by core, while this existing source surface paints the visible cue
        // that can authorize release into the later staging lifecycle.
        PreviewVisual::Native {
            host_surface: surface,
            ..
        } if *surface == host_surface => Some(bounds.shrink(4.0)),
        PreviewVisual::Dock { .. }
        | PreviewVisual::Contained { .. }
        | PreviewVisual::Native { .. } => None,
    };
    let Some(rect) = visual_rect else {
        return None;
    };

    let canvas = ui.painter_at(bounds);
    canvas.rect(
        rect,
        0.0,
        style.drop_fill,
        Stroke::new(1.0, style.drop_border_color),
        StrokeKind::Inside,
    );
    Some(preview.token())
}

fn paint_contained_transform_preview(
    ui: &Ui,
    surface: SurfaceId,
    bounds: Rect,
    preview: Option<&ContainedTransformPreview>,
    style: &DockStyle,
) -> Option<ContainedTransformPreviewToken> {
    let Some(preview) = preview.filter(|preview| preview.surface() == surface) else {
        return None;
    };
    let Some(rect) = from_logical_rect(preview.rect()) else {
        return None;
    };
    ui.painter_at(bounds).rect(
        rect,
        0.0,
        style.ghost_fill,
        Stroke::new(1.0, style.ghost_border_color),
        StrokeKind::Inside,
    );
    Some(preview.token())
}

fn paint_drag_ghost(
    ui: &Ui,
    plan: &PresentationPlan,
    resources: &EguiSurfacePaintResources,
    workspace: &Workspace,
    active: Option<ActiveDragView<'_>>,
    style: &DockStyle,
) {
    let Some(active) = active.filter(|drag| drag.phase() == DragPhase::Dragging) else {
        return;
    };
    if is_complete_contained_root_drag(workspace, active.payload()) {
        return;
    }
    let Some(pointer) = ui.input(|input| input.pointer.interact_pos()) else {
        return;
    };
    let title = drag_title(plan, resources, active.payload());
    let Some(bounds) = from_logical_rect(plan.bounds()) else {
        return;
    };
    let canvas = ui.painter_at(bounds);
    let font_id = TextStyle::Button.resolve(ui.style());
    let galley = canvas.layout_no_wrap(title, font_id, style.tab_active_text_color);
    let size = vec2(
        (galley.size().x + 2.0 * style.tab_horizontal_padding)
            .clamp(style.tab_min_width, style.tab_max_width),
        style.tab_bar_height,
    );
    let rect = Rect::from_min_size(pointer + style.ghost_offset, size);
    canvas.rect(
        rect,
        0.0,
        style.ghost_fill,
        Stroke::new(1.0, style.ghost_border_color),
        StrokeKind::Inside,
    );
    canvas.galley(
        pos2(
            rect.min.x + style.tab_horizontal_padding,
            rect.center().y - galley.size().y * 0.5,
        ),
        galley,
        style.tab_active_text_color,
    );
}

fn is_complete_contained_root_drag(workspace: &Workspace, payload: &MovePayload) -> bool {
    let MovePayload::Subtree(source) = payload else {
        return false;
    };
    workspace
        .root(source.root())
        .is_some_and(|record| record.node == source.node())
        && matches!(
            workspace.presentation_for_root(source.root()),
            Some(dockspace::RootPresentationOwner::Contained { .. })
        )
}

fn drag_title(
    plan: &PresentationPlan,
    resources: &EguiSurfacePaintResources,
    payload: &MovePayload,
) -> String {
    let (root, node, item) = match payload {
        MovePayload::Item(source) => (source.root(), source.tabs(), Some(source.item())),
        MovePayload::Tabs(source) | MovePayload::Subtree(source) => {
            (source.root(), source.node(), None)
        }
    };
    if let Some(item) = item {
        return resources
            .tab(TabSceneId {
                root,
                tabs: node,
                item,
            })
            .map_or_else(
                || format!("Pane {}", item.get()),
                |resource| resource.title.clone(),
            );
    }
    plan.pane_records()
        .iter()
        .filter(|pane| pane.id().root == root)
        .find(|pane| pane.id().tabs == node && pane.selected().is_some())
        .or_else(|| {
            plan.pane_records()
                .iter()
                .find(|pane| pane.id().root == root && pane.selected().is_some())
        })
        .and_then(|pane| {
            resources.tab(TabSceneId {
                root,
                tabs: pane.id().tabs,
                item: pane.selected()?,
            })
        })
        .map_or_else(
            || "Dock group".to_owned(),
            |resource| resource.title.clone(),
        )
}

pub(crate) fn consume_gesture_escape(
    ui: &mut Ui,
    status: InteractionStatus,
    is_gesture_source_surface: bool,
) -> bool {
    status != InteractionStatus::Idle
        && is_gesture_source_surface
        && ui.input_mut(|input| input.consume_key(Modifiers::NONE, Key::Escape))
}

fn authoritative_splitter_target(
    ui: &Ui,
    authoritative_plan: &PresentationPlan,
) -> Option<SplitterResizeTarget> {
    let Some(position) = ui.input(|input| input.pointer.hover_pos()) else {
        return None;
    };
    if pointer_blocked_by_popup(authoritative_plan, position) {
        return None;
    }
    let Ok(point) = to_logical_point(position) else {
        return None;
    };
    authoritative_plan.splitter_resize_target_at(point).ok()?
}

fn pointer_blocked_by_popup(plan: &PresentationPlan, position: Pos2) -> bool {
    plan.tab_list_menu_backdrop_records()
        .iter()
        .filter_map(|record| from_logical_rect(record.bounds()))
        .any(|bounds| crate::hit::contains_half_open(bounds, position))
}

fn splitter_target_contains(
    target: Option<SplitterResizeTarget>,
    splitter: SplitterSceneId,
) -> bool {
    match target {
        Some(SplitterResizeTarget::Handle(target)) => target == splitter,
        Some(SplitterResizeTarget::Junction(junction)) => junction.splitters().contains(&splitter),
        None => false,
    }
}

fn set_splitter_cursor(
    ui: &Ui,
    authoritative_plan: &PresentationPlan,
    target: Option<SplitterResizeTarget>,
) {
    let Some(target) = target else {
        return;
    };
    let cursor = match target {
        SplitterResizeTarget::Junction(_) => egui::CursorIcon::ResizeNeSw,
        SplitterResizeTarget::Handle(splitter) => {
            match authoritative_plan
                .splitter_record(splitter)
                .map(|record| record.axis())
            {
                Some(Axis::Horizontal) => egui::CursorIcon::ResizeHorizontal,
                Some(Axis::Vertical) => egui::CursorIcon::ResizeVertical,
                None => return,
            }
        }
    };
    ui.ctx().set_cursor_icon(cursor);
}

pub(crate) fn to_logical_point(
    point: Pos2,
) -> Result<LogicalPoint, dockspace::geometry::GeometryError> {
    LogicalPoint::new(f64::from(point.x), f64::from(point.y))
}

pub(crate) fn from_logical_rect(rect: LogicalRect) -> Option<Rect> {
    let values = [
        rect.min().x(),
        rect.min().y(),
        rect.max().x(),
        rect.max().y(),
    ];
    if values
        .iter()
        .any(|value| !value.is_finite() || value.abs() > f64::from(f32::MAX))
    {
        return None;
    }
    #[allow(
        clippy::cast_possible_truncation,
        reason = "validated egui coordinates intentionally use f32 points"
    )]
    Some(Rect::from_min_max(
        pos2(values[0] as f32, values[1] as f32),
        pos2(values[2] as f32, values[3] as f32),
    ))
}

pub(crate) fn accesskit_bounds(rect: Rect) -> egui::accesskit::Rect {
    egui::accesskit::Rect {
        x0: f64::from(rect.min.x),
        y0: f64::from(rect.min.y),
        x1: f64::from(rect.max.x),
        y1: f64::from(rect.max.y),
    }
}

pub(crate) fn paint_centered_label(ui: &Ui, rect: Rect, text: impl ToString, color: egui::Color32) {
    ui.painter_at(rect).text(
        rect.center(),
        Align2::CENTER_CENTER,
        text,
        FontSelection::Style(TextStyle::Body).resolve(ui.style()),
        color,
    );
}

#[cfg(test)]
mod tests {
    use dockspace::engine::DockEngine;
    use dockspace::geometry::LogicalRect;
    use dockspace::graph::{ContainedFloating, Node, RootRecord, SurfacePresentation};
    use dockspace::ids::{FloatingPresentationId, ItemId, RootId, SurfaceId};
    use dockspace::policy::DockPolicy;
    use dockspace::scene::TabBarSceneId;
    use dockspace::tab_strip::TabStripControlId;
    use egui::accesskit::ActionRequest;
    use egui::{Context, Id, Modifiers, RawInput, Rect, Ui, pos2, vec2};

    use super::*;
    use crate::projection::{TabStripStateMap, build_surface_projection};

    const SURFACE: SurfaceId = SurfaceId::new(1);

    fn positioned_action_fixture() -> RenderAction {
        let mut builder = Workspace::builder();
        let tabs = builder.insert_node(Node::tabs([ItemId::new(4)]));
        RenderAction::ActivateTabStripControl {
            surface: SURFACE,
            control: TabStripControlId::TabListMenu(TabBarSceneId {
                root: RootId::new(2),
                tabs,
            }),
        }
    }

    fn pressed_key(key: Key) -> Event {
        Event::Key {
            key,
            physical_key: Some(key),
            pressed: true,
            repeat: false,
            modifiers: Modifiers::NONE,
        }
    }

    #[cfg(not(egui_backend_event_envelope))]
    fn push_test_key(output: &mut RenderOutput, key: Key) {
        let context = Context::default();
        let _ = context.run_ui(RawInput::default(), |ui| {
            output.push_key(ui, positioned_action_fixture(), key);
        });
    }

    #[cfg(not(egui_backend_event_envelope))]
    fn push_test_accesskit(output: &mut RenderOutput, id: Id, action: Action) {
        let context = Context::default();
        let _ = context.run_ui(RawInput::default(), |ui| {
            output.push_accesskit(ui, positioned_action_fixture(), id, action);
        });
    }

    #[test]
    #[cfg(not(egui_backend_event_envelope))]
    fn core_backend_does_not_capture_framework_semantic_actions() {
        let context = Context::default();
        let input = RawInput {
            events: vec![pressed_key(Key::Enter)],
            ..RawInput::default()
        };
        let _ = context.run_ui(input, |ui| {
            let mut output = RenderOutput::from_ui_with_semantic_action_capture(ui, false);
            output.push_key(ui, positioned_action_fixture(), Key::Enter);
            assert!(output.actions.is_empty());
            assert_eq!(output.semantic_causality_error(), None);
        });
    }

    #[test]
    #[cfg(egui_backend_event_envelope)]
    fn core_backend_does_not_claim_framework_event_envelopes() {
        let context = Context::default();
        let mut derivation =
            egui::BackendEventDerivation::known(egui::BackendEventSequence::new(91));
        let input = RawInput {
            events: vec![derivation.envelope(pressed_key(Key::Enter))],
            ..RawInput::default()
        };
        let mut unclaimed = None;
        let _ = context.run_ui(input, |ui| {
            let mut output = RenderOutput::from_ui_with_semantic_action_capture(ui, false);
            output.push_key(ui, positioned_action_fixture(), Key::Enter);
            assert!(output.actions.is_empty());
            assert_eq!(output.semantic_causality_error(), None);
            unclaimed = ui.input_mut(|input| {
                input.claim_event_envelope(|event| *event == pressed_key(Key::Enter))
            });
        });
        assert!(unclaimed.is_some());
    }

    #[test]
    #[cfg(egui_backend_event_envelope)]
    fn duplicate_backend_key_envelopes_are_affinely_aggregated() {
        let context = Context::default();
        let sequence = egui::BackendEventSequence::new(77);
        let mut derivation = egui::BackendEventDerivation::known(sequence);
        let input = RawInput {
            events: vec![
                derivation.envelope(pressed_key(Key::Enter)),
                derivation.envelope(pressed_key(Key::Enter)),
            ],
            ..RawInput::default()
        };
        let mut positions = Vec::new();
        let mut unclaimed_duplicate = None;
        let _ = context.run_ui(input, |ui| {
            let mut output = RenderOutput::from_ui(ui);
            output.push_key(ui, positioned_action_fixture(), Key::Enter);
            assert_eq!(output.semantic_causality_error(), None);
            positions = output
                .actions
                .into_iter()
                .map(|action| action.into_parts().1)
                .collect();
            unclaimed_duplicate = ui.input_mut(|input| {
                input.claim_event_envelope(|event| {
                    matches!(
                        event,
                        Event::Key {
                            key: Key::Enter,
                            pressed: true,
                            ..
                        }
                    )
                })
            });
        });

        assert_eq!(positions, [RenderActionPosition::RawEvent(1)]);
        assert!(unclaimed_duplicate.is_none());
    }

    #[test]
    #[cfg(egui_backend_event_envelope)]
    fn unknown_backend_key_envelope_fails_closed_after_exact_claim() {
        let context = Context::default();
        let input = RawInput {
            events: vec![egui::EventEnvelope::unknown(pressed_key(Key::Enter))],
            ..RawInput::default()
        };
        let mut error = None;
        let _ = context.run_ui(input, |ui| {
            let mut output = RenderOutput::from_ui(ui);
            output.push_key(ui, positioned_action_fixture(), Key::Enter);
            assert!(output.actions.is_empty());
            error = output.semantic_causality_error();
        });

        assert_eq!(
            error,
            Some(
                SemanticActionCausalityError::BackendCorrelationUnavailable { raw_event_index: 0 }
            )
        );
    }

    #[test]
    #[cfg(not(egui_backend_event_envelope))]
    fn keyboard_before_pointer_retains_raw_event_position() {
        let mut output = RenderOutput::with_raw_events(vec![
            pressed_key(Key::Enter),
            Event::PointerMoved(pos2(20.0, 10.0)),
        ]);

        push_test_key(&mut output, Key::Enter);

        let (_, position) = output
            .actions
            .pop()
            .expect("one exact keyboard event positions the action")
            .into_parts();
        assert_eq!(position, RenderActionPosition::RawEvent(0));
        assert_eq!(output.semantic_causality_error(), None);
    }

    #[test]
    #[cfg(not(egui_backend_event_envelope))]
    fn pointer_before_keyboard_retains_raw_event_position() {
        let mut output = RenderOutput::with_raw_events(vec![
            Event::PointerMoved(pos2(20.0, 10.0)),
            pressed_key(Key::Enter),
        ]);

        push_test_key(&mut output, Key::Enter);

        let (_, position) = output
            .actions
            .pop()
            .expect("one exact keyboard event positions the action")
            .into_parts();
        assert_eq!(position, RenderActionPosition::RawEvent(1));
        assert_eq!(output.semantic_causality_error(), None);
    }

    #[test]
    #[cfg(not(egui_backend_event_envelope))]
    fn ambiguous_keyboard_source_fails_closed() {
        let mut output =
            RenderOutput::with_raw_events(vec![pressed_key(Key::Enter), pressed_key(Key::Enter)]);

        push_test_key(&mut output, Key::Enter);

        assert!(output.actions.is_empty());
        assert_eq!(
            output
                .semantic_causality_error()
                .map(SemanticActionCausalityError::matching_raw_events),
            Some(2)
        );
    }

    #[test]
    #[cfg(not(egui_backend_event_envelope))]
    fn stale_accessibility_target_fails_closed() {
        let current = Id::new("current-accessibility-target");
        let stale = Id::new("stale-accessibility-target");
        let mut output =
            RenderOutput::with_raw_events(vec![Event::AccessKitActionRequest(ActionRequest {
                action: Action::Click,
                target_tree: egui::accesskit::TreeId::ROOT,
                target_node: stale.accesskit_id(),
                data: None,
            })]);

        push_test_accesskit(&mut output, current, Action::Click);

        assert!(output.actions.is_empty());
        assert_eq!(
            output
                .semantic_causality_error()
                .map(SemanticActionCausalityError::matching_raw_events),
            Some(0)
        );
    }

    struct TestPanes;

    impl PaneView for TestPanes {
        fn title(&self, item: ItemId) -> Option<egui::WidgetText> {
            Some(format!("Pane {}", item.get()).into())
        }

        fn ui(&mut self, _item: ItemId, _ui: &mut Ui) {}
    }

    fn many_tabs_lookup_work(tab_count: usize) -> ContentLookupWork {
        let mut builder = Workspace::builder();
        builder.set_surface(SURFACE, SurfacePresentation::rootless());
        for index in 0..tab_count {
            let ordinal = u16::try_from(index).expect("fixture identity is representable");
            let identity = u64::from(ordinal) + 1;
            let root = RootId::new(identity);
            let floating = FloatingPresentationId::new(identity);
            let tabs = builder.insert_node(Node::tabs([ItemId::new(identity)]));
            builder.set_root(root, RootRecord::new(tabs));
            builder.set_contained_floating(
                floating,
                ContainedFloating::new(
                    root,
                    LogicalRect::new(f64::from(ordinal) * 100.0, 0.0, 96.0, 160.0)
                        .expect("fixture floating bounds are valid"),
                ),
            );
            builder
                .attach_contained(SURFACE, floating)
                .expect("fixture floating attaches");
        }
        let workspace = builder.build().expect("lookup fixture workspace is valid");
        let engine = DockEngine::new(workspace, DockPolicy::default())
            .expect("lookup fixture engine is valid");
        let context = Context::default();
        let width = f32::from(
            u16::try_from(tab_count).expect("fixture tab count is representable as an f32 source"),
        ) * 100.0;
        let bounds = Rect::from_min_size(pos2(0.0, 0.0), vec2(width, 480.0));
        let mut tab_strip_states = TabStripStateMap::default();
        let mut panes = TestPanes;
        let mut work = None;
        let _ = context.run_ui(
            RawInput {
                screen_rect: Some(bounds),
                ..RawInput::default()
            },
            |ui| {
                let projection = build_surface_projection(
                    ui,
                    engine.version(),
                    engine
                        .presentation_requirements()
                        .surface(SURFACE)
                        .expect("fixture surface requirements exist"),
                    &mut tab_strip_states,
                    engine.workspace(),
                    SURFACE,
                    bounds,
                    &panes,
                    &DockStyle::default(),
                )
                .expect("lookup fixture projects");
                assert_eq!(
                    projection.resources.tab_row_height_lookup_count(),
                    tab_count,
                    "measurement performs one indexed lookup per tab bar"
                );
                let token = engine
                    .begin_surface_contribution(SURFACE)
                    .expect("fixture surface contribution begins");
                let contribution = engine
                    .prepare_surface_contribution(token, projection.measurements.values.clone())
                    .expect("fixture measurements compile");
                let core_plan = match contribution.paint_candidate() {
                    dockspace::engine::PreparedSurfacePaintCandidate::Ready(candidate) => {
                        candidate.plan().clone()
                    }
                    dockspace::engine::PreparedSurfacePaintCandidate::Retained { .. }
                    | dockspace::engine::PreparedSurfacePaintCandidate::Unavailable { .. } => {
                        panic!("fixture contribution must compile a ready plan")
                    }
                };
                let output = paint_surface(
                    ui,
                    Id::new("linear-content-index"),
                    SURFACE,
                    bounds,
                    Some(&core_plan),
                    &projection.resources,
                    &mut tab_strip_states,
                    &projection.resources,
                    engine.workspace(),
                    &mut panes,
                    &DockStyle::default(),
                    engine.interaction(),
                    None,
                    None,
                    true,
                    None,
                    None,
                    true,
                    None,
                    false,
                );
                work = Some(output.content_lookup_work);
            },
        );
        work.expect("egui invokes the lookup fixture")
    }

    #[test]
    fn retained_content_matching_has_linear_structural_lookup_counts() {
        for tab_count in [16, 128, 1_024] {
            let work = many_tabs_lookup_work(tab_count);
            assert_eq!(work.indexed_roots, tab_count);
            assert_eq!(work.indexed_tabs, tab_count);
            assert_eq!(work.root_lookups, tab_count);
            assert_eq!(work.tab_lookups, tab_count);
        }
    }
}
