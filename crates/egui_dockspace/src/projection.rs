//! Egui-owned intrinsic measurements and non-geometric paint resources.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

#[cfg(test)]
use std::cell::Cell;

use dockspace::RootPresentationOwner;
use dockspace::geometry::{GeometryError, LogicalRect, LogicalSize};
use dockspace::graph::{Node, Workspace};
use dockspace::ids::{ItemId, NodeId, RootId, SurfaceId, WorkspaceEpoch};
use dockspace::policy::TabBarVisibility;
use dockspace::scene::{PaneSceneId, TabBarSceneId, TabSceneId};
use dockspace::scene_manifest::{
    Measurement, MeasurementSubmissionError, MeasurementValueError,
    PaneMinimumKey as CorePaneMinimumKey, SurfaceMeasurements, SurfaceRequirements, TabIntrinsic,
    TabIntrinsicKey, TabListMenuMetrics, TabStripControlMetric, TabStripControlMetrics,
    TabStripControlPlacement, TabStripKey as CoreTabStripKey, TabStripMetrics,
};
use dockspace::transition::WorkspaceVersion;
use egui::emath::GuiRounding as _;
use egui::{FontSelection, Galley, Id, Rect, TextStyle, TextWrapMode, Ui, Vec2};
use thiserror::Error;

use crate::pane::PaneView;
use crate::style::{DockStyle, DockStyleError};

/// Complete adapter answer for one exact core measurement manifest.
///
/// This type deliberately contains no node, tab, splitter, floating, layer, or
/// hit geometry. Once the core accepts these intrinsic facts, only its
/// `PresentationPlan` may authorize painting and interaction geometry.
#[derive(Clone)]
pub(crate) struct EguiSurfaceMeasurementSet {
    pub(crate) bounds: Rect,
    pub(crate) values: SurfaceMeasurements,
}

/// Semantic location which authorizes one application pane callback.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct PaneContentIdentity {
    workspace_epoch: WorkspaceEpoch,
    presentation: RootPresentationOwner,
    root: RootId,
    tabs: NodeId,
}

/// Egui-owned paint data keyed only by core scene identities.
///
/// Text shaping and application pane availability belong here. Rectangles,
/// layers, hit regions, selection, and policy authority do not.
#[derive(Clone, Default)]
pub(crate) struct EguiSurfacePaintResources {
    tabs: BTreeMap<TabSceneId, TabPaintResource>,
    tab_row_heights: BTreeMap<TabBarSceneId, f64>,
    panes: BTreeMap<PaneSceneId, PaneContentIdentity>,
    missing_items: BTreeSet<ItemId>,
    #[cfg(test)]
    tab_row_height_lookups: Cell<usize>,
}

#[derive(Clone)]
pub(crate) struct TabPaintResource {
    pub(crate) title: String,
    pub(crate) galley: Arc<Galley>,
    pub(crate) missing: bool,
}

impl EguiSurfacePaintResources {
    fn insert_tab(&mut self, id: TabSceneId, resource: TabPaintResource, row_height: f64) {
        self.tab_row_heights
            .entry(TabBarSceneId {
                root: id.root,
                tabs: id.tabs,
            })
            .and_modify(|height| *height = height.max(row_height))
            .or_insert(row_height);
        let replaced = self.tabs.insert(id, resource);
        debug_assert!(
            replaced.is_none(),
            "one scene tab owns one paint resource bundle"
        );
    }

    fn insert_pane(&mut self, id: PaneSceneId, identity: PaneContentIdentity) {
        let replaced = self.panes.insert(id, identity);
        debug_assert!(
            replaced.is_none(),
            "one scene pane owns one content identity"
        );
    }

    pub(crate) fn tab(&self, id: TabSceneId) -> Option<&TabPaintResource> {
        self.tabs.get(&id)
    }

    fn tab_row_height(&self, id: TabBarSceneId) -> f64 {
        #[cfg(test)]
        self.tab_row_height_lookups
            .set(self.tab_row_height_lookups.get() + 1);
        self.tab_row_heights.get(&id).copied().unwrap_or(0.0)
    }

    #[cfg(test)]
    pub(crate) fn tab_row_height_lookup_count(&self) -> usize {
        self.tab_row_height_lookups.get()
    }

    pub(crate) fn pane_identity(&self, id: PaneSceneId) -> Option<PaneContentIdentity> {
        self.panes.get(&id).copied()
    }

    pub(crate) fn missing_items(&self) -> impl ExactSizeIterator<Item = ItemId> + '_ {
        self.missing_items.iter().copied()
    }

    pub(crate) fn items(&self) -> BTreeSet<ItemId> {
        self.tabs.keys().map(|tab| tab.item).collect()
    }

    fn record_missing(&mut self, item: ItemId) {
        self.missing_items.insert(item);
    }

    fn is_exact_for(&self, requirements: &SurfaceRequirements) -> bool {
        let tabs = requirements
            .tab_bars()
            .flat_map(|bar| {
                bar.close_capabilities().map(move |(item, _)| TabSceneId {
                    root: bar.id().root,
                    tabs: bar.id().tabs,
                    item,
                })
            })
            .collect::<BTreeSet<_>>();
        let panes = requirements
            .pane_minimums()
            .map(|key| PaneSceneId {
                root: key.root(),
                tabs: key.tabs(),
            })
            .collect::<BTreeSet<_>>();
        let recorded_missing = self
            .tabs
            .iter()
            .filter_map(|(tab, resource)| resource.missing.then_some(tab.item))
            .collect::<BTreeSet<_>>();

        tabs.len() == self.tabs.len()
            && tabs.iter().all(|tab| self.tabs.contains_key(tab))
            && panes.len() == self.panes.len()
            && panes.iter().all(|pane| self.panes.contains_key(pane))
            && recorded_missing == self.missing_items
    }
}

#[derive(Clone)]
pub(crate) struct EguiSurfaceProjection {
    pub(crate) measurements: EguiSurfaceMeasurementSet,
    pub(crate) resources: EguiSurfacePaintResources,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct TabStripKey {
    pub(crate) surface: SurfaceId,
    pub(crate) root: RootId,
    pub(crate) node: NodeId,
}

impl TabStripKey {
    pub(crate) const fn new(surface: SurfaceId, bar: TabBarSceneId) -> Self {
        Self {
            surface,
            root: bar.root,
            node: bar.tabs,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct TabRevealIdentity {
    selected: Option<ItemId>,
    keyboard_focused: Option<ItemId>,
    active_dragged: Option<ItemId>,
}

impl TabRevealIdentity {
    pub(crate) const fn new(
        selected: Option<ItemId>,
        keyboard_focused: Option<ItemId>,
        active_dragged: Option<ItemId>,
    ) -> Self {
        Self {
            selected,
            keyboard_focused,
            active_dragged,
        }
    }

    fn with_keyboard_focused(self, keyboard_focused: Option<ItemId>) -> Self {
        Self {
            keyboard_focused,
            ..self
        }
    }

    pub(crate) fn prioritized_items(self) -> impl Iterator<Item = ItemId> {
        let priorities = [self.active_dragged, self.keyboard_focused, self.selected];
        priorities
            .into_iter()
            .enumerate()
            .filter_map(move |(index, item)| {
                item.filter(|item| !priorities[..index].contains(&Some(*item)))
            })
    }

    pub(crate) fn primary(self) -> Option<ItemId> {
        self.prioritized_items().next()
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
struct TabStripState {
    workspace_epoch: Option<WorkspaceEpoch>,
    requested_reveal_identity: TabRevealIdentity,
    pending_keyboard_focus: Option<ItemId>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct TabStripStateMap {
    entries: BTreeMap<TabStripKey, TabStripState>,
    keyboard_focus_continuations: Vec<TabKeyboardFocusContinuation>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct TabKeyboardFocusContinuation {
    pub(crate) key: TabStripKey,
    pub(crate) item: ItemId,
    previous_pending_keyboard_focus: Option<ItemId>,
    previous_requested_reveal_identity: TabRevealIdentity,
}

impl TabStripStateMap {
    pub(crate) fn retain_surface_inventory(
        &mut self,
        surfaces: impl IntoIterator<Item = SurfaceId>,
    ) {
        let live = surfaces.into_iter().collect::<BTreeSet<_>>();
        self.entries.retain(|key, _| live.contains(&key.surface));
    }

    fn reconcile_surface(
        &mut self,
        surface: SurfaceId,
        workspace_epoch: WorkspaceEpoch,
        requirements: &SurfaceRequirements,
    ) {
        let live = requirements
            .tab_bars()
            .map(|bar| TabStripKey::new(surface, bar.id()))
            .collect::<BTreeSet<_>>();
        self.entries
            .retain(|key, _| key.surface != surface || live.contains(key));
        for key in live {
            let state = self.entries.entry(key).or_default();
            if state.workspace_epoch != Some(workspace_epoch) {
                *state = TabStripState {
                    workspace_epoch: Some(workspace_epoch),
                    ..TabStripState::default()
                };
            }
        }
    }

    pub(crate) fn replace_surface_from(&mut self, surface: SurfaceId, staged: TabStripStateMap) {
        self.entries.retain(|key, _| key.surface != surface);
        self.entries.extend(
            staged
                .entries
                .into_iter()
                .filter(|(key, _)| key.surface == surface),
        );
    }

    pub(crate) fn take_keyboard_focus_continuations(
        &mut self,
    ) -> Vec<TabKeyboardFocusContinuation> {
        let continuations = std::mem::take(&mut self.keyboard_focus_continuations);
        for continuation in continuations.iter().rev() {
            let Some(state) = self.entries.get_mut(&continuation.key) else {
                continue;
            };
            state.pending_keyboard_focus = continuation.previous_pending_keyboard_focus;
            state.requested_reveal_identity = continuation.previous_requested_reveal_identity;
        }
        continuations
    }

    pub(crate) fn apply_keyboard_focus_continuation(
        &mut self,
        continuation: TabKeyboardFocusContinuation,
    ) -> bool {
        let Some(state) = self.entries.get_mut(&continuation.key) else {
            return false;
        };
        state.pending_keyboard_focus = Some(continuation.item);
        state.requested_reveal_identity = state
            .requested_reveal_identity
            .with_keyboard_focused(Some(continuation.item));
        true
    }
}

pub(crate) fn sync_tab_reveal_identity(
    states: &mut TabStripStateMap,
    key: TabStripKey,
    reveal_identity: TabRevealIdentity,
) -> bool {
    let Some(state) = states.entries.get_mut(&key) else {
        return false;
    };
    let reveal_identity = reveal_identity.with_keyboard_focused(
        state
            .pending_keyboard_focus
            .or(reveal_identity.keyboard_focused),
    );
    if state.requested_reveal_identity == reveal_identity {
        return false;
    }
    state.requested_reveal_identity = reveal_identity;
    true
}

pub(crate) fn request_tab_keyboard_focus(
    states: &mut TabStripStateMap,
    key: TabStripKey,
    item: ItemId,
) -> bool {
    let Some(state) = states.entries.get_mut(&key) else {
        return false;
    };
    if state.pending_keyboard_focus == Some(item) {
        return false;
    }
    let continuation = TabKeyboardFocusContinuation {
        key,
        item,
        previous_pending_keyboard_focus: state.pending_keyboard_focus,
        previous_requested_reveal_identity: state.requested_reveal_identity,
    };
    state.pending_keyboard_focus = Some(item);
    state.requested_reveal_identity = state
        .requested_reveal_identity
        .with_keyboard_focused(Some(item));
    states.keyboard_focus_continuations.push(continuation);
    true
}

pub(crate) fn continue_committed_tab_keyboard_focus(
    states: &mut TabStripStateMap,
    key: TabStripKey,
    item: ItemId,
) -> bool {
    let Some(state) = states.entries.get_mut(&key) else {
        return false;
    };
    let requested_reveal_identity = state
        .requested_reveal_identity
        .with_keyboard_focused(Some(item));
    if state.pending_keyboard_focus == Some(item)
        && state.requested_reveal_identity == requested_reveal_identity
    {
        return false;
    }
    state.pending_keyboard_focus = Some(item);
    state.requested_reveal_identity = requested_reveal_identity;
    true
}

pub(crate) fn complete_tab_keyboard_focus(
    states: &mut TabStripStateMap,
    key: TabStripKey,
    item: ItemId,
) {
    let Some(state) = states.entries.get_mut(&key) else {
        return;
    };
    if state.pending_keyboard_focus == Some(item) {
        state.pending_keyboard_focus = None;
    }
}

pub(crate) fn pending_tab_keyboard_focus(
    states: &TabStripStateMap,
    key: TabStripKey,
) -> Option<ItemId> {
    states
        .entries
        .get(&key)
        .and_then(|state| state.pending_keyboard_focus)
}

#[derive(Debug, Error)]
pub enum ProjectionError {
    #[error("dock style is invalid: {0}")]
    Style(#[from] DockStyleError),
    #[error("surface bounds are invalid: {0}")]
    Geometry(#[from] GeometryError),
    #[error("surface measurement value is invalid: {0}")]
    MeasurementValue(#[from] MeasurementValueError),
    #[error("surface measurement submission is invalid: {0}")]
    MeasurementSubmission(#[from] MeasurementSubmissionError),
    #[error("workspace does not contain surface {surface}")]
    MissingSurface { surface: SurfaceId },
    #[error("surface {surface} references missing contained presentation {floating}")]
    MissingFloating {
        surface: SurfaceId,
        floating: dockspace::ids::FloatingPresentationId,
    },
    #[error("workspace does not contain root {root}")]
    MissingRoot { root: RootId },
    #[error("workspace does not contain node {node:?}")]
    MissingNode { node: NodeId },
    #[error("root {root} reaches node {node:?} more than once")]
    RepeatedNode { root: RootId, node: NodeId },
    #[error("core-required tab {tab:?} is absent from egui paint resources")]
    MissingMeasuredTab { tab: TabSceneId },
    #[error("core-required tab strip {bar:?} is absent from egui requirements")]
    MissingMeasuredTabStrip { bar: TabBarSceneId },
    #[error("core requirements omitted tab-bar policy for {bar:?}")]
    MissingTabBarRequirement { bar: TabBarSceneId },
    #[error("core requirements omitted close policy for {tab:?}")]
    MissingTabCloseCapability { tab: TabSceneId },
    #[error("egui paint resources do not exactly match surface {surface} scene identities")]
    PaintResourceSetMismatch { surface: SurfaceId },
    #[error("pane {item:?} returned invalid minimum size {width} x {height}")]
    InvalidPaneMinimum {
        item: Option<ItemId>,
        width: f32,
        height: f32,
    },
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn build_surface_projection(
    ui: &Ui,
    source_workspace: WorkspaceVersion,
    requirements: &SurfaceRequirements,
    tab_strip_states: &mut TabStripStateMap,
    workspace: &Workspace,
    surface: SurfaceId,
    bounds: Rect,
    panes: &dyn PaneView,
    style: &DockStyle,
) -> Result<EguiSurfaceProjection, ProjectionError> {
    style.validate()?;
    let logical_bounds = to_logical_rect(bounds)?;
    let popup_plane_bounds =
        to_logical_rect(ui.ctx().input(|input| input.content_rect()).round_ui())?;
    let resources = build_surface_paint_resources(
        ui,
        source_workspace,
        requirements,
        workspace,
        surface,
        panes,
        style,
    )?;
    tab_strip_states.reconcile_surface(surface, source_workspace.epoch(), requirements);
    let values = build_surface_measurements(
        ui,
        requirements,
        logical_bounds,
        popup_plane_bounds,
        &resources,
        panes,
        style,
    )?;
    let measurements = EguiSurfaceMeasurementSet { bounds, values };
    debug_assert!(resources.is_exact_for(requirements));
    Ok(EguiSurfaceProjection {
        measurements,
        resources,
    })
}

fn build_surface_paint_resources(
    ui: &Ui,
    source_workspace: WorkspaceVersion,
    requirements: &SurfaceRequirements,
    workspace: &Workspace,
    surface: SurfaceId,
    panes: &dyn PaneView,
    style: &DockStyle,
) -> Result<EguiSurfacePaintResources, ProjectionError> {
    let presentation = workspace
        .surface(surface)
        .ok_or(ProjectionError::MissingSurface { surface })?;
    let mut resources = EguiSurfacePaintResources::default();
    if let Some(root) = presentation.main_root {
        build_root_paint_resources(
            ui,
            source_workspace.epoch(),
            requirements,
            workspace,
            root,
            RootPresentationOwner::Main { surface },
            panes,
            style,
            &mut resources,
        )?;
    }
    for floating in presentation.contained.iter().copied() {
        let record = workspace
            .contained_floating(floating)
            .ok_or(ProjectionError::MissingFloating { surface, floating })?;
        build_root_paint_resources(
            ui,
            source_workspace.epoch(),
            requirements,
            workspace,
            record.root,
            RootPresentationOwner::Contained { surface, floating },
            panes,
            style,
            &mut resources,
        )?;
    }
    if !resources.is_exact_for(requirements) {
        return Err(ProjectionError::PaintResourceSetMismatch { surface });
    }
    Ok(resources)
}

#[allow(clippy::too_many_arguments)]
fn build_root_paint_resources(
    ui: &Ui,
    workspace_epoch: WorkspaceEpoch,
    requirements: &SurfaceRequirements,
    workspace: &Workspace,
    root: RootId,
    presentation: RootPresentationOwner,
    panes: &dyn PaneView,
    style: &DockStyle,
    resources: &mut EguiSurfacePaintResources,
) -> Result<(), ProjectionError> {
    let root_node = workspace
        .root(root)
        .ok_or(ProjectionError::MissingRoot { root })?
        .node;
    let mut pending = vec![root_node];
    let mut visited = BTreeSet::new();
    while let Some(node) = pending.pop() {
        if !visited.insert(node) {
            return Err(ProjectionError::RepeatedNode { root, node });
        }
        match workspace
            .node(node)
            .ok_or(ProjectionError::MissingNode { node })?
        {
            Node::Tabs { items, .. } => {
                resources.insert_pane(
                    PaneSceneId { root, tabs: node },
                    PaneContentIdentity {
                        workspace_epoch,
                        presentation,
                        root,
                        tabs: node,
                    },
                );
                let bar = TabBarSceneId { root, tabs: node };
                let requirement = requirements
                    .tab_bar(bar)
                    .ok_or(ProjectionError::MissingTabBarRequirement { bar })?;
                for item in items.iter().copied() {
                    let id = TabSceneId {
                        root,
                        tabs: node,
                        item,
                    };
                    requirement
                        .close_capability(item)
                        .ok_or(ProjectionError::MissingTabCloseCapability { tab: id })?;
                    let title = panes.title(item);
                    let missing = title.is_none();
                    if missing {
                        resources.record_missing(item);
                    }
                    let widget_text =
                        title.unwrap_or_else(|| format!("Missing pane {}", item.get()).into());
                    let title = widget_text.text().to_owned();
                    let galley = widget_text.into_galley(
                        ui,
                        Some(TextWrapMode::Truncate),
                        style.tab_max_width,
                        FontSelection::Style(TextStyle::Button),
                    );
                    let row_height = f64::from(
                        ui.spacing()
                            .interact_size
                            .y
                            .max(galley.size().y + style.tab_horizontal_padding),
                    );
                    resources.insert_tab(
                        id,
                        TabPaintResource {
                            title,
                            galley,
                            missing,
                        },
                        row_height,
                    );
                }
            }
            Node::Split { children, .. } => {
                pending.extend(children.iter().rev().copied());
            }
        }
    }
    Ok(())
}

fn build_surface_measurements(
    ui: &Ui,
    requirements: &SurfaceRequirements,
    bounds: LogicalRect,
    popup_plane_bounds: LogicalRect,
    resources: &EguiSurfacePaintResources,
    panes: &dyn PaneView,
    style: &DockStyle,
) -> Result<SurfaceMeasurements, ProjectionError> {
    let mut measurements = SurfaceMeasurements::new(requirements.ticket());
    measurements.set_bounds(requirements.bounds(), Measurement::Measured(bounds))?;
    if let Some(key) = requirements.popup_plane_bounds() {
        measurements.set_popup_plane_bounds(key, Measurement::Measured(popup_plane_bounds))?;
    }

    for key in requirements.pane_minimums() {
        let pane = key
            .selected()
            .map_or(Vec2::ZERO, |item| panes.minimum_size(item));
        if !pane.x.is_finite() || !pane.y.is_finite() || pane.x < 0.0 || pane.y < 0.0 {
            return Err(ProjectionError::InvalidPaneMinimum {
                item: key.selected(),
                width: pane.x,
                height: pane.y,
            });
        }
        measurements.insert_pane_minimum(
            CorePaneMinimumKey::new(key.root(), key.tabs(), key.selected()),
            Measurement::Measured(LogicalSize::new(f64::from(pane.x), f64::from(pane.y))?),
        )?;
    }

    for key in requirements.tab_intrinsics() {
        let tab_id = key.tab();
        let tab = resources
            .tab(tab_id)
            .ok_or(ProjectionError::MissingMeasuredTab { tab: tab_id })?;
        measurements.insert_tab_intrinsic(
            TabIntrinsicKey::new(key.surface(), tab_id),
            Measurement::Measured(TabIntrinsic::new(f64::from(tab.galley.size().x))?),
        )?;
    }

    let menu_style = OverflowMenuStyleIdentity::from(ui);
    for key in requirements.tab_strips() {
        let bar_id = key.bar();
        let requirement = requirements
            .tab_bar(bar_id)
            .ok_or(ProjectionError::MissingMeasuredTabStrip { bar: bar_id })?;
        let control_extent = if requirement.policy().visibility() == TabBarVisibility::Visible {
            f64::from(style.tab_bar_height)
        } else {
            0.0
        };
        let controls = if control_extent > 0.0 {
            Some(
                TabStripControlMetrics::new(0.0)?
                    .with_scroll_backward(TabStripControlMetric::new(
                        control_extent,
                        TabStripControlPlacement::OverlayLeading,
                    )?)
                    .with_scroll_forward(TabStripControlMetric::new(
                        control_extent,
                        TabStripControlPlacement::OverlayTrailing,
                    )?)
                    .with_tab_list_menu(TabStripControlMetric::new(
                        control_extent,
                        TabStripControlPlacement::ReservedTrailing,
                    )?),
            )
        } else {
            None
        };
        let row_height = resources.tab_row_height(bar_id);
        let popup_stroke = f32::from_bits(menu_style.popup_window_stroke_width).max(0.0);
        let horizontal_padding = f32::from(
            i8::max(
                menu_style.popup_menu_margin[0],
                menu_style.popup_menu_margin[1],
            )
            .max(0),
        ) + popup_stroke;
        let vertical_padding = f32::from(
            i8::max(
                menu_style.popup_menu_margin[2],
                menu_style.popup_menu_margin[3],
            )
            .max(0),
        ) + popup_stroke;
        let menu = TabListMenuMetrics::new(
            row_height,
            f64::from(horizontal_padding),
            f64::from(vertical_padding),
            f64::from(f32::from_bits(menu_style.popup_item_spacing_y).max(0.0)),
            popup_plane_bounds.height(),
            f64::from(f32::from_bits(menu_style.popup_scroll_allocated_width).max(0.0)),
        )?;
        let mut strip = TabStripMetrics::new(0.0, 0.0)?;
        if let Some(controls) = controls {
            strip = strip.with_controls(controls).with_tab_list_menu(menu);
        }
        measurements.insert_tab_strip(
            CoreTabStripKey::new(key.surface(), bar_id),
            Measurement::Measured(strip),
        )?;
    }
    Ok(measurements)
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct OverflowMenuStyleIdentity {
    popup_item_spacing_y: u32,
    popup_menu_margin: [i8; 4],
    popup_window_stroke_width: u32,
    popup_scroll_allocated_width: u32,
}

impl From<&Ui> for OverflowMenuStyleIdentity {
    fn from(ui: &Ui) -> Self {
        let popup_style = ui.ctx().global_style();
        let popup_spacing = &popup_style.spacing;
        Self {
            popup_item_spacing_y: popup_spacing.item_spacing.y.to_bits(),
            popup_menu_margin: [
                popup_spacing.menu_margin.left,
                popup_spacing.menu_margin.right,
                popup_spacing.menu_margin.top,
                popup_spacing.menu_margin.bottom,
            ],
            popup_window_stroke_width: popup_style.visuals.window_stroke.width.to_bits(),
            popup_scroll_allocated_width: popup_spacing.scroll.allocated_width().to_bits(),
        }
    }
}

fn tab_strip_state_id(instance_id: Id) -> Id {
    instance_id.with("tab-strip-states")
}

pub(crate) fn load_tab_strip_states(context: &egui::Context, instance_id: Id) -> TabStripStateMap {
    context
        .data_mut(|data| data.get_temp(tab_strip_state_id(instance_id)))
        .unwrap_or_default()
}

pub(crate) fn store_tab_strip_states(
    context: &egui::Context,
    instance_id: Id,
    states: TabStripStateMap,
) {
    context.data_mut(|data| {
        data.insert_temp(tab_strip_state_id(instance_id), states);
    });
}

fn to_logical_rect(rect: Rect) -> Result<LogicalRect, GeometryError> {
    LogicalRect::new(
        f64::from(rect.min.x),
        f64::from(rect.min.y),
        f64::from(rect.width()),
        f64::from(rect.height()),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reveal_priority_deduplicates_items() {
        let first = ItemId::new(1);
        let second = ItemId::new(2);
        let identity = TabRevealIdentity::new(Some(first), Some(second), Some(first));
        assert_eq!(
            identity.prioritized_items().collect::<Vec<_>>(),
            [first, second]
        );
    }

    #[test]
    fn committed_semantic_focus_is_retained_without_a_speculative_continuation() {
        let item = ItemId::new(4);
        let mut workspace = Workspace::builder();
        let node = workspace.insert_node(Node::tabs([item]));
        let key = TabStripKey {
            surface: SurfaceId::new(1),
            root: RootId::new(2),
            node,
        };
        let mut states = TabStripStateMap::default();
        states.entries.insert(key, TabStripState::default());

        assert!(continue_committed_tab_keyboard_focus(
            &mut states,
            key,
            item
        ));
        assert_eq!(pending_tab_keyboard_focus(&states, key), Some(item));
        assert!(states.keyboard_focus_continuations.is_empty());
        assert!(!continue_committed_tab_keyboard_focus(
            &mut states,
            key,
            item
        ));
    }
}
