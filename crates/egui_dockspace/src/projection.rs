//! Deterministic projection shared by egui painting and core scene publication.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use dockspace::command::{DockFraction, DockTarget, Edge, MovePayload};
use dockspace::drop_guide::{DropGuideClusterRecord, DropGuideEdgeSet, DropGuideTargetRecord};
use dockspace::drop_target::{
    DropOcclusionRecord, DropTargetAvailability, DropTargetId, DropTargetRecord, DropVisual,
    SceneLayerKey,
};
use dockspace::error::CommandError;
use dockspace::geometry::{Constraints, GeometryError, LogicalRect, LogicalSize};
use dockspace::graph::{Axis, Node, Workspace};
use dockspace::hit_region::HitRegion;
use dockspace::ids::{FloatingPresentationId, ItemId, NodeId, RootId, SurfaceId, WorkspaceEpoch};
use dockspace::layout::{
    LayoutError, LayoutMetrics, LayoutMetricsError, SplitWeightOverride,
    project_root_with_overrides,
};
use dockspace::scene::{
    NodeSceneId, ReadySurfaceScene, SemanticRect, SplitterSceneId, TabBarSceneId, TabSceneId,
};
use egui::{FontSelection, Galley, Id, Rect, TextStyle, TextWrapMode, Ui, Vec2, pos2, vec2};
use thiserror::Error;

use crate::pane::PaneView;
use crate::style::{DockStyle, DockStyleError};

#[derive(Clone)]
pub(crate) struct SurfacePlan {
    pub(crate) surface: SurfaceId,
    pub(crate) bounds: Rect,
    pub(crate) roots: Vec<RootPlan>,
    pub(crate) ready: ReadySurfaceScene,
    pub(crate) fingerprint: ProjectionFingerprint,
    pub(crate) missing_items: Vec<ItemId>,
    pub(crate) contained_placements: Vec<ContainedPlacementRequest>,
    tab_scroll_layers: Vec<TabScrollLayer>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct ContainedPlacementRequest {
    pub(crate) root: RootId,
    pub(crate) floating: FloatingPresentationId,
    pub(crate) expected_rect: LogicalRect,
    pub(crate) minimum_size: LogicalSize,
}

#[derive(Clone)]
pub(crate) struct RootPlan {
    pub(crate) root: RootId,
    pub(crate) bounds: Rect,
    pub(crate) floating: Option<FloatingPlan>,
    pub(crate) tabs: Vec<TabsPlan>,
    pub(crate) splitters: Vec<SplitterPlan>,
}

#[derive(Clone, Copy)]
pub(crate) struct FloatingPlan {
    pub(crate) id: FloatingPresentationId,
    pub(crate) outer_rect: Rect,
    pub(crate) title_rect: Rect,
    pub(crate) title_drag_rect: Rect,
    pub(crate) close_rect: Option<Rect>,
    pub(crate) resize_zones: [(FloatingResizeDirection, Rect); 8],
    pub(crate) minimum_size: LogicalSize,
    pub(crate) z_order: u64,
    pub(crate) frontmost: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum FloatingResizeDirection {
    North,
    NorthEast,
    East,
    SouthEast,
    South,
    SouthWest,
    West,
    NorthWest,
}

#[derive(Clone)]
pub(crate) struct TabsPlan {
    pub(crate) key: TabStripKey,
    pub(crate) root: RootId,
    pub(crate) node: NodeId,
    pub(crate) node_rect: Rect,
    pub(crate) tab_bar_rect: Rect,
    pub(crate) group_drag_rect: Rect,
    pub(crate) tab_viewport_rect: Rect,
    pub(crate) overflow_rect: Option<Rect>,
    pub(crate) scroll_back_rect: Option<Rect>,
    pub(crate) scroll_forward_rect: Option<Rect>,
    pub(crate) scroll_offset: f32,
    pub(crate) max_scroll_offset: f32,
    pub(crate) content_rect: Rect,
    pub(crate) selected: Option<ItemId>,
    pub(crate) reveal_identity: TabRevealIdentity,
    pub(crate) pending_keyboard_focus: Option<ItemId>,
    pub(crate) tabs: Vec<TabPlan>,
    overflow_menu_measurements: Vec<OverflowMenuItemMeasurement>,
    pub(crate) overflow_menu_open: bool,
    pub(crate) overflow_menu_geometry: Option<OverflowMenuGeometry>,
    layout_identity: Arc<TabStripLayoutIdentity>,
}

#[derive(Clone)]
pub(crate) struct TabPlan {
    pub(crate) item: ItemId,
    /// Unclipped position in the scrollable strip coordinate space.
    pub(crate) rect: Rect,
    pub(crate) visible_rect: Option<Rect>,
    pub(crate) text_rect: Rect,
    pub(crate) close_rect: Option<Rect>,
    pub(crate) drag_rect: Option<Rect>,
    pub(crate) title: String,
    pub(crate) galley: Arc<Galley>,
    pub(crate) selected: bool,
    pub(crate) missing: bool,
    pub(crate) closeable: bool,
    pub(crate) overflow_menu_size: Vec2,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct TabStripKey {
    pub(crate) surface: SurfaceId,
    pub(crate) root: RootId,
    pub(crate) node: NodeId,
}

/// Adapter-owned identities whose tab chrome must survive presentation scrolling.
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

    fn with_selected(self, selected: Option<ItemId>) -> Self {
        Self { selected, ..self }
    }

    fn with_keyboard_focused(self, keyboard_focused: Option<ItemId>) -> Self {
        Self {
            keyboard_focused,
            ..self
        }
    }

    fn prioritized_items(self) -> impl Iterator<Item = ItemId> {
        [self.active_dragged, self.keyboard_focused, self.selected]
            .into_iter()
            .flatten()
    }

    fn primary(self) -> Option<ItemId> {
        self.prioritized_items().next()
    }

    fn introduces_item_since(self, previous: Self) -> bool {
        self.selected
            .is_some_and(|item| previous.selected != Some(item))
            || self
                .keyboard_focused
                .is_some_and(|item| previous.keyboard_focused != Some(item))
            || self
                .active_dragged
                .is_some_and(|item| previous.active_dragged != Some(item))
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct TabStripLayoutIdentity {
    workspace_epoch: WorkspaceEpoch,
    style: TabStripStyleIdentity,
    overflow_menu_style: OverflowMenuStyleIdentity,
    tabs: Vec<TabStripEntryIdentity>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct TabStripStyleIdentity {
    bar_height: u32,
    horizontal_padding: u32,
    min_width: u32,
    max_width: u32,
    close_size: u32,
}

impl From<&DockStyle> for TabStripStyleIdentity {
    fn from(style: &DockStyle) -> Self {
        Self {
            bar_height: style.tab_bar_height.to_bits(),
            horizontal_padding: style.tab_horizontal_padding.to_bits(),
            min_width: style.tab_min_width.to_bits(),
            max_width: style.tab_max_width.to_bits(),
            close_size: style.tab_close_size.to_bits(),
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct OverflowMenuStyleIdentity {
    local_interact_width: u32,
    local_interact_height: u32,
    local_default_area_width: u32,
    local_default_area_height: u32,
    popup_item_spacing_x: u32,
    popup_item_spacing_y: u32,
    popup_menu_margin: [i8; 4],
    popup_default_area_width: u32,
    popup_default_area_height: u32,
    popup_window_stroke_width: u32,
    popup_scroll_floating: bool,
    popup_scroll_bar_width: u32,
    popup_scroll_bar_inner_margin: u32,
    popup_scroll_bar_outer_margin: u32,
    popup_scroll_floating_allocated_width: u32,
    popup_content_min_x: u32,
    popup_content_min_y: u32,
    popup_content_max_x: u32,
    popup_content_max_y: u32,
}

impl From<&Ui> for OverflowMenuStyleIdentity {
    fn from(ui: &Ui) -> Self {
        let local_spacing = ui.spacing();
        let popup_style = ui.ctx().global_style();
        let popup_spacing = &popup_style.spacing;
        let popup_scroll = popup_spacing.scroll;
        let popup_content_rect = ui.ctx().content_rect();
        Self {
            local_interact_width: local_spacing.interact_size.x.to_bits(),
            local_interact_height: local_spacing.interact_size.y.to_bits(),
            local_default_area_width: local_spacing.default_area_size.x.to_bits(),
            local_default_area_height: local_spacing.default_area_size.y.to_bits(),
            popup_item_spacing_x: popup_spacing.item_spacing.x.to_bits(),
            popup_item_spacing_y: popup_spacing.item_spacing.y.to_bits(),
            popup_menu_margin: [
                popup_spacing.menu_margin.left,
                popup_spacing.menu_margin.right,
                popup_spacing.menu_margin.top,
                popup_spacing.menu_margin.bottom,
            ],
            popup_default_area_width: popup_spacing.default_area_size.x.to_bits(),
            popup_default_area_height: popup_spacing.default_area_size.y.to_bits(),
            popup_window_stroke_width: popup_style.visuals.window_stroke.width.to_bits(),
            popup_scroll_floating: popup_scroll.floating,
            popup_scroll_bar_width: popup_scroll.bar_width.to_bits(),
            popup_scroll_bar_inner_margin: popup_scroll.bar_inner_margin.to_bits(),
            popup_scroll_bar_outer_margin: popup_scroll.bar_outer_margin.to_bits(),
            popup_scroll_floating_allocated_width: popup_scroll.floating_allocated_width.to_bits(),
            popup_content_min_x: popup_content_rect.min.x.to_bits(),
            popup_content_min_y: popup_content_rect.min.y.to_bits(),
            popup_content_max_x: popup_content_rect.max.x.to_bits(),
            popup_content_max_y: popup_content_rect.max.y.to_bits(),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct TabStripEntryIdentity {
    item: ItemId,
    width: u32,
    closeable: bool,
    overflow_menu_width: u32,
    overflow_menu_height: u32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct OverflowMenuItemMeasurement {
    item: ItemId,
    index: usize,
    desired_size: Vec2,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct OverflowMenuItemGeometry {
    pub(crate) item: ItemId,
    pub(crate) index: usize,
    pub(crate) actual_rect: Rect,
    pub(crate) hit_rect: Option<Rect>,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct OverflowMenuGeometry {
    pub(crate) popup_rect: Rect,
    pub(crate) viewport_rect: Rect,
    pub(crate) items: Vec<OverflowMenuItemGeometry>,
}

#[derive(Clone, Debug, PartialEq)]
struct StoredOverflowMenuGeometry {
    measurements: Vec<OverflowMenuItemMeasurement>,
    geometry: OverflowMenuGeometry,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct OverflowMenuToggle {
    frame: u64,
    opened: bool,
}

#[derive(Clone, Debug, Default, PartialEq)]
struct TabStripState {
    scroll_offset: f32,
    requested_reveal_identity: TabRevealIdentity,
    applied_reveal_identity: TabRevealIdentity,
    pending_keyboard_focus: Option<ItemId>,
    viewport_width: f32,
    layout_identity: Option<Arc<TabStripLayoutIdentity>>,
    overflow_menu_open: bool,
    overflow_menu_geometry: Option<StoredOverflowMenuGeometry>,
    overflow_menu_toggle: Option<OverflowMenuToggle>,
}

#[derive(Clone, Debug, Default)]
pub(crate) struct TabStripStateMap {
    entries: BTreeMap<TabStripKey, TabStripState>,
}

impl TabStripStateMap {
    pub(crate) fn retain_live<'a>(&mut self, plans: impl IntoIterator<Item = &'a SurfacePlan>) {
        let live = plans
            .into_iter()
            .flat_map(|plan| &plan.roots)
            .flat_map(|root| &root.tabs)
            .map(|tabs| tabs.key)
            .collect::<BTreeSet<_>>();
        self.retain_keys(&live);
    }

    fn retain_keys(&mut self, live: &BTreeSet<TabStripKey>) {
        self.entries.retain(|key, _| live.contains(key));
    }
}

#[derive(Clone, Debug, PartialEq)]
struct TabStripLayout {
    viewport_rect: Rect,
    overflow_rect: Option<Rect>,
    scroll_back_rect: Option<Rect>,
    scroll_forward_rect: Option<Rect>,
    scroll_offset: f32,
    max_scroll_offset: f32,
    tab_rects: Vec<Rect>,
    visible_rects: Vec<Option<Rect>>,
    state: TabStripState,
}

#[derive(Clone, Debug)]
struct TabScrollLayer {
    occlusion_rect: Rect,
    strips: Vec<(TabStripKey, Rect)>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct TabChromeGeometry {
    text: Rect,
    close: Option<Rect>,
    drag: Rect,
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct TabScrollRange {
    min: f32,
    max: f32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct TabGapGeometry {
    index: usize,
    region: Rect,
    visual: Rect,
}

#[derive(Clone)]
pub(crate) struct SplitterPlan {
    pub(crate) root: RootId,
    pub(crate) split: NodeId,
    pub(crate) index: usize,
    pub(crate) rect: Rect,
    pub(crate) hit_rect: Rect,
    pub(crate) horizontal: bool,
    pub(crate) before_rect: Rect,
    pub(crate) after_rect: Rect,
    pub(crate) weights: Vec<dockspace::graph::SplitWeight>,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct ProjectionFingerprint {
    surface: SurfaceId,
    bounds: Rect,
    regions: Vec<ProjectionRegion>,
    tab_reveals: Vec<(TabStripKey, TabRevealIdentity)>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct ProjectionRegion {
    id: ProjectionRegionId,
    rect: Rect,
    layer: SceneLayerKey,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ProjectionRegionId {
    Pane {
        root: RootId,
        tabs: NodeId,
        selected: Option<ItemId>,
        available: bool,
    },
    TabGroup {
        root: RootId,
        tabs: NodeId,
    },
    TabViewport {
        root: RootId,
        tabs: NodeId,
    },
    TabScrollBack {
        root: RootId,
        tabs: NodeId,
    },
    TabScrollForward {
        root: RootId,
        tabs: NodeId,
    },
    TabOverflow {
        root: RootId,
        tabs: NodeId,
    },
    TabOverflowPopup {
        root: RootId,
        tabs: NodeId,
    },
    TabOverflowViewport {
        root: RootId,
        tabs: NodeId,
    },
    TabOverflowItem {
        root: RootId,
        tabs: NodeId,
        item: ItemId,
        index: usize,
    },
    TabOverflowItemActual {
        root: RootId,
        tabs: NodeId,
        item: ItemId,
        index: usize,
    },
    TabOverflowItemMeasurement {
        root: RootId,
        tabs: NodeId,
        item: ItemId,
        index: usize,
    },
    Tab {
        root: RootId,
        tabs: NodeId,
        item: ItemId,
    },
    TabClose {
        root: RootId,
        tabs: NodeId,
        item: ItemId,
    },
    Splitter {
        root: RootId,
        split: NodeId,
        index: usize,
    },
    FloatingTitle(FloatingPresentationId),
    FloatingClose(FloatingPresentationId),
    FloatingResize {
        floating: FloatingPresentationId,
        direction: FloatingResizeDirection,
    },
}

impl SurfacePlan {
    pub(crate) fn tab_scroll_owner_at(&self, pointer: egui::Pos2) -> Option<TabStripKey> {
        tab_scroll_owner_at(&self.tab_scroll_layers, pointer)
    }
}

fn tab_scroll_owner_at(layers: &[TabScrollLayer], pointer: egui::Pos2) -> Option<TabStripKey> {
    for layer in layers.iter().rev() {
        if crate::hit::contains_half_open(layer.occlusion_rect, pointer) {
            return layer
                .strips
                .iter()
                .find(|(_, rect)| crate::hit::contains_half_open(*rect, pointer))
                .map(|(key, _)| *key);
        }
    }
    None
}

#[derive(Debug, Error)]
pub enum ProjectionError {
    #[error("dock style is invalid: {0}")]
    Style(#[from] DockStyleError),
    #[error("surface bounds are invalid: {0}")]
    Geometry(#[from] GeometryError),
    #[error("layout metrics are invalid: {0}")]
    LayoutMetrics(#[from] LayoutMetricsError),
    #[error("root layout projection failed: {0}")]
    Layout(#[from] LayoutError),
    #[error("workspace reference capture failed: {0}")]
    Command(#[from] CommandError),
    #[error("workspace does not contain surface {surface}")]
    MissingSurface { surface: SurfaceId },
    #[error("surface {surface} references missing contained presentation {floating}")]
    MissingFloating {
        surface: SurfaceId,
        floating: FloatingPresentationId,
    },
    #[error("workspace does not contain root {root}")]
    MissingRoot { root: RootId },
    #[error("workspace does not contain node {node:?}")]
    MissingNode { node: NodeId },
    #[error("node {node:?} is not projected inside root {root}")]
    MissingNodeRect { root: RootId, node: NodeId },
    #[error("dock fraction is invalid")]
    InvalidDockFraction,
    #[error("logical {component} coordinate {value} cannot be represented by egui")]
    UnrepresentableEguiCoordinate { component: &'static str, value: f64 },
    #[error("pane {item:?} returned invalid minimum size {width} x {height}")]
    InvalidPaneMinimum {
        item: Option<ItemId>,
        width: f32,
        height: f32,
    },
    #[error("split child count {count} cannot be represented exactly by the egui adapter")]
    SplitChildCountUnrepresentable { count: usize },
}

#[allow(clippy::too_many_arguments)]
#[allow(
    clippy::too_many_lines,
    reason = "surface orchestration keeps the ready scene and paint plan on one geometry path"
)]
pub(crate) fn build_surface_plan(
    ui: &Ui,
    workspace_epoch: WorkspaceEpoch,
    tab_strip_states: &mut TabStripStateMap,
    workspace: &Workspace,
    surface: SurfaceId,
    bounds: Rect,
    panes: &dyn PaneView,
    style: &DockStyle,
    resize_override: Option<(
        &dockspace::command::NodeSource,
        &[dockspace::graph::SplitWeight],
    )>,
) -> Result<SurfacePlan, ProjectionError> {
    style.validate()?;
    let logical_bounds = to_logical_rect(bounds)?;
    let presentation = workspace
        .surface(surface)
        .ok_or(ProjectionError::MissingSurface { surface })?;
    let mut ready = ReadySurfaceScene::new(surface, logical_bounds);
    let mut roots = Vec::with_capacity(1 + presentation.contained.len());
    let mut missing_items = Vec::new();
    let mut contained_placements = Vec::with_capacity(presentation.contained.len());
    let mut fingerprint_regions = Vec::new();
    let metrics = LayoutMetrics::new(f64::from(style.splitter_thickness))?;
    let fraction =
        DockFraction::new(style.dock_fraction).map_err(|_| ProjectionError::InvalidDockFraction)?;

    let overrides = resize_override
        .map(|(source, weights)| vec![SplitWeightOverride::new(source, weights)])
        .unwrap_or_default();
    roots.push(build_root_plan(
        ui,
        workspace_epoch,
        tab_strip_states,
        workspace,
        surface,
        presentation.main_root,
        bounds,
        SceneLayerKey::new(1),
        panes,
        style,
        metrics,
        fraction,
        &overrides,
        &mut ready,
        &mut missing_items,
        &mut fingerprint_regions,
    )?);

    let mut contained = presentation
        .contained
        .iter()
        .map(|id| {
            workspace
                .contained_floating(*id)
                .copied()
                .ok_or(ProjectionError::MissingFloating {
                    surface,
                    floating: *id,
                })
        })
        .collect::<Result<Vec<_>, _>>()?;
    contained.sort_unstable_by_key(|floating| floating.stacking_key());
    let frontmost = contained.last().map(|floating| floating.id);
    for (index, floating) in contained.into_iter().enumerate() {
        let minimum_size = floating_minimum_for_root(workspace, floating.root, panes, style)?;
        contained_placements.push(ContainedPlacementRequest {
            root: floating.root,
            floating: floating.id,
            expected_rect: floating.rect,
            minimum_size,
        });
        let layer = SceneLayerKey::new(u64::try_from(index).unwrap_or(u64::MAX).saturating_add(2));
        let outer_rect = intersect_rect(from_logical_rect(floating.rect)?, bounds);
        let border = style
            .floating_border_width
            .min(outer_rect.width() * 0.5)
            .min(outer_rect.height() * 0.5);
        let inner_rect = Rect::from_min_max(
            outer_rect.min + vec2(border, border),
            outer_rect.max - vec2(border, border),
        );
        let title_height = style.floating_title_height.min(inner_rect.height());
        let title_rect = Rect::from_min_max(
            inner_rect.min,
            pos2(inner_rect.max.x, inner_rect.min.y + title_height),
        );
        let content_rect =
            Rect::from_min_max(pos2(inner_rect.min.x, title_rect.max.y), inner_rect.max);
        let floating_plan = FloatingPlan {
            id: floating.id,
            outer_rect,
            title_rect,
            title_drag_rect: title_rect,
            close_rect: None,
            resize_zones: floating_resize_zones(outer_rect, style.floating_resize_extent),
            minimum_size,
            z_order: floating.z_order,
            frontmost: frontmost == Some(floating.id),
        };
        let mut root_plan = build_root_plan(
            ui,
            workspace_epoch,
            tab_strip_states,
            workspace,
            surface,
            floating.root,
            content_rect,
            layer,
            panes,
            style,
            metrics,
            fraction,
            &overrides,
            &mut ready,
            &mut missing_items,
            &mut fingerprint_regions,
        )?;
        let mut floating_plan = floating_plan;
        complete_floating_plan(&root_plan, &mut floating_plan, style);
        root_plan.floating = Some(floating_plan);
        ready.push_drop_occlusion(DropOcclusionRecord::new(
            floating.id,
            HitRegion::new(to_logical_rect(floating_plan.outer_rect)?),
            layer,
        ));
        fingerprint_regions.push(ProjectionRegion {
            id: ProjectionRegionId::FloatingTitle(floating.id),
            rect: floating_plan.title_drag_rect,
            layer,
        });
        if let Some(rect) = floating_plan.close_rect {
            fingerprint_regions.push(ProjectionRegion {
                id: ProjectionRegionId::FloatingClose(floating.id),
                rect,
                layer,
            });
        }
        fingerprint_regions.extend(floating_plan.resize_zones.map(|(direction, rect)| {
            ProjectionRegion {
                id: ProjectionRegionId::FloatingResize {
                    floating: floating.id,
                    direction,
                },
                rect,
                layer,
            }
        }));
        roots.push(root_plan);
    }

    let mut tab_scroll_layers = roots
        .iter()
        .map(|root| TabScrollLayer {
            occlusion_rect: root
                .floating
                .as_ref()
                .map_or(root.bounds, |floating| floating.outer_rect),
            strips: root
                .tabs
                .iter()
                .filter(|tabs| tabs.overflow_rect.is_some())
                .map(|tabs| (tabs.key, tabs.tab_viewport_rect))
                .collect(),
        })
        .collect::<Vec<_>>();
    tab_scroll_layers.extend(
        roots
            .iter()
            .flat_map(|root| &root.tabs)
            .filter(|tabs| tabs.overflow_menu_open)
            .map(|tabs| TabScrollLayer {
                occlusion_rect: tabs
                    .overflow_menu_geometry
                    .as_ref()
                    .map_or(bounds, |geometry| geometry.popup_rect),
                strips: Vec::new(),
            }),
    );
    let tab_reveals = roots
        .iter()
        .flat_map(|root| &root.tabs)
        .map(|tabs| (tabs.key, tabs.reveal_identity))
        .collect();

    Ok(SurfacePlan {
        surface,
        bounds,
        roots,
        ready,
        fingerprint: ProjectionFingerprint {
            surface,
            bounds,
            regions: fingerprint_regions,
            tab_reveals,
        },
        missing_items,
        contained_placements,
        tab_scroll_layers,
    })
}

fn complete_floating_plan(root: &RootPlan, floating: &mut FloatingPlan, style: &DockStyle) {
    let has_items = root
        .tabs
        .iter()
        .flat_map(|tabs| &tabs.tabs)
        .next()
        .is_some();
    let closeable = has_items
        && root
            .tabs
            .iter()
            .flat_map(|tabs| &tabs.tabs)
            .all(|tab| tab.closeable);
    let inner_title = floating.title_rect.shrink2(vec2(
        style
            .floating_resize_extent
            .min(floating.title_rect.width() * 0.5),
        style
            .floating_resize_extent
            .min(floating.title_rect.height() * 0.5),
    ));
    floating.close_rect = closeable
        .then(|| floating_close_rect(inner_title, style))
        .filter(Rect::is_positive);
    let movable_title = floating.close_rect.map_or(inner_title, |close| {
        Rect::from_min_max(
            inner_title.min,
            pos2(
                (close.min.x - style.tab_horizontal_padding).max(inner_title.min.x),
                inner_title.max.y,
            ),
        )
    });
    floating.title_drag_rect = movable_title;
}

fn floating_close_rect(title_rect: Rect, style: &DockStyle) -> Rect {
    let size = style
        .tab_close_size
        .min(title_rect.width())
        .min(title_rect.height());
    let padding = style
        .tab_horizontal_padding
        .min((title_rect.width() - size).max(0.0));
    intersect_rect(
        Rect::from_center_size(
            pos2(
                title_rect.max.x - padding - size * 0.5,
                title_rect.center().y,
            ),
            Vec2::splat(size),
        ),
        title_rect,
    )
}

pub(crate) fn floating_resize_zones(
    rect: Rect,
    configured_extent: f32,
) -> [(FloatingResizeDirection, Rect); 8] {
    let x = configured_extent.min(rect.width() * 0.5);
    let y = configured_extent.min(rect.height() * 0.5);
    let left = Rect::from_min_max(rect.min, pos2(rect.min.x + x, rect.max.y));
    let right = Rect::from_min_max(pos2(rect.max.x - x, rect.min.y), rect.max);
    let top = Rect::from_min_max(rect.min, pos2(rect.max.x, rect.min.y + y));
    let bottom = Rect::from_min_max(pos2(rect.min.x, rect.max.y - y), rect.max);
    [
        (
            FloatingResizeDirection::NorthWest,
            Rect::from_min_max(rect.min, pos2(left.max.x, top.max.y)),
        ),
        (
            FloatingResizeDirection::North,
            Rect::from_min_max(pos2(left.max.x, rect.min.y), pos2(right.min.x, top.max.y)),
        ),
        (
            FloatingResizeDirection::NorthEast,
            Rect::from_min_max(pos2(right.min.x, rect.min.y), pos2(rect.max.x, top.max.y)),
        ),
        (
            FloatingResizeDirection::East,
            Rect::from_min_max(pos2(right.min.x, top.max.y), pos2(rect.max.x, bottom.min.y)),
        ),
        (
            FloatingResizeDirection::SouthEast,
            Rect::from_min_max(pos2(right.min.x, bottom.min.y), rect.max),
        ),
        (
            FloatingResizeDirection::South,
            Rect::from_min_max(
                pos2(left.max.x, bottom.min.y),
                pos2(right.min.x, rect.max.y),
            ),
        ),
        (
            FloatingResizeDirection::SouthWest,
            Rect::from_min_max(pos2(rect.min.x, bottom.min.y), pos2(left.max.x, rect.max.y)),
        ),
        (
            FloatingResizeDirection::West,
            Rect::from_min_max(pos2(rect.min.x, top.max.y), pos2(left.max.x, bottom.min.y)),
        ),
    ]
}

#[allow(clippy::too_many_arguments)]
#[allow(
    clippy::too_many_lines,
    reason = "one traversal emits node, splitter, target, and paint geometry atomically"
)]
fn build_root_plan(
    ui: &Ui,
    workspace_epoch: WorkspaceEpoch,
    tab_strip_states: &mut TabStripStateMap,
    workspace: &Workspace,
    surface: SurfaceId,
    root: RootId,
    bounds: Rect,
    layer: SceneLayerKey,
    panes: &dyn PaneView,
    style: &DockStyle,
    metrics: LayoutMetrics,
    fraction: DockFraction,
    overrides: &[SplitWeightOverride<'_>],
    ready: &mut ReadySurfaceScene,
    missing_items: &mut Vec<ItemId>,
    fingerprint_regions: &mut Vec<ProjectionRegion>,
) -> Result<RootPlan, ProjectionError> {
    let root_record = workspace
        .root(root)
        .ok_or(ProjectionError::MissingRoot { root })?;
    let mut constraints = BTreeMap::new();
    collect_leaf_constraints(
        workspace,
        root_record.node,
        panes,
        style,
        bounds.size(),
        &mut constraints,
    )?;
    let logical_bounds = to_logical_rect(bounds)?;
    let root_overrides = overrides
        .iter()
        .copied()
        .filter(|override_| override_.source().root() == root)
        .collect::<Vec<_>>();
    let projection = project_root_with_overrides(
        workspace,
        root,
        logical_bounds,
        &constraints,
        metrics,
        &root_overrides,
    )?;
    let root_bounds = from_logical_rect(logical_bounds)?;
    let mut tabs = Vec::new();
    let mut splitters = Vec::new();

    for (node, rect) in &projection.node_rects {
        let rect = intersect_rect(from_logical_rect(*rect)?, root_bounds);
        ready.push_node(SemanticRect::new(
            NodeSceneId { root, node: *node },
            to_logical_rect(rect)?,
            layer,
        ));
        match workspace
            .node(*node)
            .ok_or(ProjectionError::MissingNode { node: *node })?
        {
            Node::Tabs { items, selected } => {
                tabs.push(build_tabs_plan(
                    ui,
                    workspace_epoch,
                    tab_strip_states,
                    workspace,
                    surface,
                    root,
                    *node,
                    rect,
                    root_record.node,
                    items,
                    *selected,
                    layer,
                    panes,
                    style,
                    fraction,
                    ready,
                    missing_items,
                    fingerprint_regions,
                )?);
            }
            Node::Split {
                axis,
                children,
                weights,
            } => {
                let split = projection
                    .splits
                    .get(node)
                    .ok_or(ProjectionError::MissingNodeRect { root, node: *node })?;
                let projected_weights = root_overrides
                    .iter()
                    .find(|override_| override_.source().node() == *node)
                    .map_or_else(|| weights.clone(), |override_| override_.weights().to_vec());
                let splitter_rects = split
                    .splitter_rects
                    .iter()
                    .map(|rect| {
                        from_logical_rect(*rect).map(|rect| intersect_rect(rect, root_bounds))
                    })
                    .collect::<Result<Vec<_>, _>>()?;
                let hit_rects = splitter_hit_rects(
                    &splitter_rects,
                    *axis == Axis::Horizontal,
                    style.splitter_hit_extent,
                    root_bounds,
                );
                for (index, (splitter_rect, hit_rect)) in
                    splitter_rects.into_iter().zip(hit_rects).enumerate()
                {
                    if splitter_rect.is_positive() {
                        ready.push_splitter(SemanticRect::new(
                            SplitterSceneId {
                                root,
                                split: *node,
                                index,
                            },
                            to_logical_rect(splitter_rect)?,
                            layer,
                        ));
                        splitters.push(SplitterPlan {
                            root,
                            split: *node,
                            index,
                            rect: splitter_rect,
                            hit_rect,
                            horizontal: *axis == dockspace::graph::Axis::Horizontal,
                            before_rect: intersect_rect(
                                from_logical_rect(
                                    *projection.node_rects.get(&children[index]).ok_or(
                                        ProjectionError::MissingNodeRect {
                                            root,
                                            node: children[index],
                                        },
                                    )?,
                                )?,
                                root_bounds,
                            ),
                            after_rect: intersect_rect(
                                from_logical_rect(
                                    *projection.node_rects.get(&children[index + 1]).ok_or(
                                        ProjectionError::MissingNodeRect {
                                            root,
                                            node: children[index + 1],
                                        },
                                    )?,
                                )?,
                                root_bounds,
                            ),
                            weights: projected_weights.clone(),
                        });
                        fingerprint_regions.push(ProjectionRegion {
                            id: ProjectionRegionId::Splitter {
                                root,
                                split: *node,
                                index,
                            },
                            rect: hit_rect,
                            layer,
                        });
                    }
                }
            }
        }
    }

    push_outer_guide(
        workspace,
        surface,
        root,
        root_record.node,
        root_bounds,
        layer,
        style,
        fraction,
        ready,
    )?;

    Ok(RootPlan {
        root,
        bounds: root_bounds,
        floating: None,
        tabs,
        splitters,
    })
}

pub(crate) fn floating_minimum_for_root(
    workspace: &Workspace,
    root: RootId,
    panes: &dyn PaneView,
    style: &DockStyle,
) -> Result<LogicalSize, ProjectionError> {
    let node = workspace
        .root(root)
        .ok_or(ProjectionError::MissingRoot { root })?
        .node;
    floating_minimum_for_node(workspace, node, panes, style)
}

pub(crate) fn floating_minimum_for_payload(
    workspace: &Workspace,
    payload: &MovePayload,
    panes: &dyn PaneView,
    style: &DockStyle,
) -> Result<LogicalSize, ProjectionError> {
    let layout = match payload {
        MovePayload::Item(source) => tabs_minimum(Some(source.item()), panes, style)?,
        MovePayload::Tabs(source) | MovePayload::Subtree(source) => {
            node_layout_minimum(workspace, source.node(), panes, style)?
        }
    };
    floating_outer_minimum(layout, style)
}

fn floating_minimum_for_node(
    workspace: &Workspace,
    node: NodeId,
    panes: &dyn PaneView,
    style: &DockStyle,
) -> Result<LogicalSize, ProjectionError> {
    floating_outer_minimum(node_layout_minimum(workspace, node, panes, style)?, style)
}

fn floating_outer_minimum(
    layout: LogicalSize,
    style: &DockStyle,
) -> Result<LogicalSize, ProjectionError> {
    let border = 2.0 * f64::from(style.floating_border_width);
    LogicalSize::new(
        (layout.width() + border).max(f64::from(style.minimum_floating_size.x)),
        (layout.height() + f64::from(style.floating_title_height) + border)
            .max(f64::from(style.minimum_floating_size.y)),
    )
    .map_err(Into::into)
}

fn node_layout_minimum(
    workspace: &Workspace,
    root: NodeId,
    panes: &dyn PaneView,
    style: &DockStyle,
) -> Result<LogicalSize, ProjectionError> {
    let mut stack = vec![(root, false)];
    let mut minimums: BTreeMap<NodeId, LogicalSize> = BTreeMap::new();
    while let Some((node, visited)) = stack.pop() {
        let record = workspace
            .node(node)
            .ok_or(ProjectionError::MissingNode { node })?;
        if !visited {
            stack.push((node, true));
            if let Node::Split { children, .. } = record {
                stack.extend(children.iter().rev().map(|child| (*child, false)));
            }
            continue;
        }

        let minimum = match record {
            Node::Tabs { selected, .. } => tabs_minimum(*selected, panes, style)?,
            Node::Split { axis, children, .. } => {
                let mut width = 0.0_f64;
                let mut height = 0.0_f64;
                for child in children {
                    let child = minimums
                        .get(child)
                        .copied()
                        .ok_or(ProjectionError::MissingNode { node: *child })?;
                    match axis {
                        Axis::Horizontal => {
                            width += child.width();
                            height = height.max(child.height());
                        }
                        Axis::Vertical => {
                            width = width.max(child.width());
                            height += child.height();
                        }
                    }
                }
                let splitter_count = children.len().saturating_sub(1);
                let splitter_count = u32::try_from(splitter_count).map_err(|_| {
                    ProjectionError::SplitChildCountUnrepresentable {
                        count: children.len(),
                    }
                })?;
                let splitters = f64::from(style.splitter_thickness) * f64::from(splitter_count);
                match axis {
                    Axis::Horizontal => width += splitters,
                    Axis::Vertical => height += splitters,
                }
                LogicalSize::new(width, height)?
            }
        };
        minimums.insert(node, minimum);
    }
    minimums
        .remove(&root)
        .ok_or(ProjectionError::MissingNode { node: root })
}

fn tabs_minimum(
    selected: Option<ItemId>,
    panes: &dyn PaneView,
    style: &DockStyle,
) -> Result<LogicalSize, ProjectionError> {
    let pane = selected.map_or(Vec2::ZERO, |item| panes.minimum_size(item));
    if !pane.x.is_finite() || !pane.y.is_finite() || pane.x < 0.0 || pane.y < 0.0 {
        return Err(ProjectionError::InvalidPaneMinimum {
            item: selected,
            width: pane.x,
            height: pane.y,
        });
    }
    LogicalSize::new(
        f64::from(style.minimum_pane_size.x.max(pane.x)),
        f64::from(style.minimum_pane_size.y.max(pane.y) + style.tab_bar_height),
    )
    .map_err(Into::into)
}

fn collect_leaf_constraints(
    workspace: &Workspace,
    root_node: NodeId,
    panes: &dyn PaneView,
    style: &DockStyle,
    root_size: Vec2,
    constraints: &mut BTreeMap<NodeId, Constraints>,
) -> Result<(), ProjectionError> {
    let mut stack = vec![root_node];
    while let Some(node) = stack.pop() {
        match workspace
            .node(node)
            .ok_or(ProjectionError::MissingNode { node })?
        {
            Node::Tabs { selected, .. } => {
                let min = tabs_minimum(*selected, panes, style)?;
                let max = LogicalSize::new(
                    f64::from(root_size.x).max(min.width()),
                    f64::from(root_size.y).max(min.height()),
                )?;
                constraints.insert(node, Constraints::new(min, max)?);
            }
            Node::Split { children, .. } => stack.extend(children.iter().rev().copied()),
        }
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
#[allow(
    clippy::too_many_lines,
    reason = "tab measurement and every semantic rectangle must share one allocation pass"
)]
fn build_tabs_plan(
    ui: &Ui,
    workspace_epoch: WorkspaceEpoch,
    tab_strip_states: &mut TabStripStateMap,
    workspace: &Workspace,
    surface: SurfaceId,
    root: RootId,
    node: NodeId,
    node_rect: Rect,
    root_node: NodeId,
    items: &[ItemId],
    selected: Option<ItemId>,
    layer: SceneLayerKey,
    panes: &dyn PaneView,
    style: &DockStyle,
    fraction: DockFraction,
    ready: &mut ReadySurfaceScene,
    missing_items: &mut Vec<ItemId>,
    fingerprint_regions: &mut Vec<ProjectionRegion>,
) -> Result<TabsPlan, ProjectionError> {
    let bar_height = style.tab_bar_height.min(node_rect.height());
    let tab_bar_rect = Rect::from_min_max(
        node_rect.min,
        pos2(node_rect.max.x, node_rect.min.y + bar_height),
    );
    let group_drag_width = if items.is_empty() {
        0.0
    } else {
        bar_height.min(tab_bar_rect.width())
    };
    let group_drag_rect = Rect::from_min_size(
        tab_bar_rect.min,
        vec2(group_drag_width, tab_bar_rect.height()),
    );
    let content_rect = Rect::from_min_max(pos2(node_rect.min.x, tab_bar_rect.max.y), node_rect.max);
    ready.push_tab_bar(SemanticRect::new(
        TabBarSceneId { root, tabs: node },
        to_logical_rect(tab_bar_rect)?,
        layer,
    ));

    let mut measured = Vec::with_capacity(items.len());
    for item in items {
        let title = panes.title(*item);
        let missing = title.is_none();
        if missing {
            missing_items.push(*item);
        }
        let widget_text = title.unwrap_or_else(|| format!("Missing pane {}", item.get()).into());
        let title = widget_text.text().to_owned();
        let galley = widget_text.into_galley(
            ui,
            Some(TextWrapMode::Truncate),
            style.tab_max_width,
            FontSelection::Style(TextStyle::Button),
        );
        let closeable = panes.closeable(*item);
        let close_extent = if closeable {
            style.tab_close_size + style.tab_horizontal_padding
        } else {
            0.0
        };
        let desired = (galley.size().x + 2.0 * style.tab_horizontal_padding + close_extent)
            .clamp(style.tab_min_width, style.tab_max_width);
        let overflow_menu_size = vec2(
            (galley.size().x + 2.0 * style.tab_horizontal_padding)
                .clamp(style.tab_min_width, style.tab_max_width),
            ui.spacing()
                .interact_size
                .y
                .max(galley.size().y + style.tab_horizontal_padding),
        );
        measured.push((
            *item,
            title,
            galley,
            missing,
            closeable,
            desired,
            overflow_menu_size,
        ));
    }
    let widths = allocate_tab_widths(
        measured.iter().map(|entry| entry.5),
        tab_bar_rect.width() - group_drag_rect.width(),
        style.tab_min_width,
    );
    let selected_index = selected.and_then(|selected| {
        measured
            .iter()
            .position(|(item, ..)| *item == selected)
            .map(|index| (index, selected))
    });
    let key = TabStripKey {
        surface,
        root,
        node,
    };
    let layout_identity = Arc::new(TabStripLayoutIdentity {
        workspace_epoch,
        style: style.into(),
        overflow_menu_style: ui.into(),
        tabs: measured
            .iter()
            .zip(&widths)
            .map(
                |((item, _, _, _, closeable, _, overflow_menu_size), width)| {
                    TabStripEntryIdentity {
                        item: *item,
                        width: width.to_bits(),
                        closeable: *closeable,
                        overflow_menu_width: overflow_menu_size.x.to_bits(),
                        overflow_menu_height: overflow_menu_size.y.to_bits(),
                    }
                },
            )
            .collect(),
    });
    let stored_state = tab_strip_states
        .entries
        .get(&key)
        .cloned()
        .unwrap_or_default();
    let layout = layout_tab_strip(
        tab_bar_rect,
        group_drag_rect,
        &widths,
        selected_index,
        &stored_state,
        layout_identity.clone(),
        style,
    );
    let mut tabs = Vec::with_capacity(measured.len());
    let mut overflow_menu_measurements = Vec::new();
    for (
        index,
        (
            ((item, title, galley, missing, closeable, _, overflow_menu_size), rect),
            mut visible_rect,
        ),
    ) in measured
        .into_iter()
        .zip(layout.tab_rects.iter().copied())
        .zip(layout.visible_rects.iter().copied())
        .enumerate()
    {
        let chrome =
            visible_rect.and_then(|visible| tab_chrome_geometry(rect, visible, closeable, style));
        if chrome.is_none() {
            visible_rect = None;
        }
        let text_rect = chrome.map_or(Rect::NOTHING, |chrome| chrome.text);
        let close_rect = chrome.and_then(|chrome| chrome.close);
        let drag_rect = chrome.map(|chrome| chrome.drag);
        if let Some(visible_rect) = visible_rect {
            ready.push_tab(SemanticRect::new(
                TabSceneId {
                    root,
                    tabs: node,
                    item,
                },
                to_logical_rect(visible_rect)?,
                layer,
            ));
            fingerprint_regions.push(ProjectionRegion {
                id: ProjectionRegionId::Tab {
                    root,
                    tabs: node,
                    item,
                },
                rect: visible_rect,
                layer,
            });
            if let Some(close_rect) = close_rect {
                fingerprint_regions.push(ProjectionRegion {
                    id: ProjectionRegionId::TabClose {
                        root,
                        tabs: node,
                        item,
                    },
                    rect: close_rect,
                    layer,
                });
            }
        }
        if visible_rect != Some(rect) && layout.overflow_rect.is_some() {
            overflow_menu_measurements.push(OverflowMenuItemMeasurement {
                item,
                index,
                desired_size: overflow_menu_size,
            });
            fingerprint_regions.push(ProjectionRegion {
                id: ProjectionRegionId::TabOverflowItemMeasurement {
                    root,
                    tabs: node,
                    item,
                    index,
                },
                rect: Rect::from_min_size(pos2(0.0, 0.0), overflow_menu_size),
                layer,
            });
        }
        tabs.push(TabPlan {
            item,
            rect,
            visible_rect,
            text_rect,
            close_rect,
            drag_rect,
            title,
            galley,
            selected: selected == Some(item),
            missing,
            closeable,
            overflow_menu_size,
        });
    }

    let overflow_menu_geometry = layout
        .state
        .overflow_menu_geometry
        .as_ref()
        .filter(|geometry| geometry.measurements == overflow_menu_measurements)
        .map(|geometry| geometry.geometry.clone());
    if let Some(geometry) = &overflow_menu_geometry {
        let popup_layer = SceneLayerKey::new(u64::MAX);
        fingerprint_regions.push(ProjectionRegion {
            id: ProjectionRegionId::TabOverflowPopup { root, tabs: node },
            rect: geometry.popup_rect,
            layer: popup_layer,
        });
        fingerprint_regions.push(ProjectionRegion {
            id: ProjectionRegionId::TabOverflowViewport { root, tabs: node },
            rect: geometry.viewport_rect,
            layer: popup_layer,
        });
        for item in &geometry.items {
            fingerprint_regions.push(ProjectionRegion {
                id: ProjectionRegionId::TabOverflowItemActual {
                    root,
                    tabs: node,
                    item: item.item,
                    index: item.index,
                },
                rect: item.actual_rect,
                layer: popup_layer,
            });
            if let Some(hit_rect) = item.hit_rect {
                fingerprint_regions.push(ProjectionRegion {
                    id: ProjectionRegionId::TabOverflowItem {
                        root,
                        tabs: node,
                        item: item.item,
                        index: item.index,
                    },
                    rect: hit_rect,
                    layer: popup_layer,
                });
            }
        }
    }
    tab_strip_states.entries.insert(key, layout.state.clone());

    if group_drag_rect.is_positive() {
        fingerprint_regions.push(ProjectionRegion {
            id: ProjectionRegionId::TabGroup { root, tabs: node },
            rect: group_drag_rect,
            layer,
        });
    }

    fingerprint_regions.push(ProjectionRegion {
        id: ProjectionRegionId::TabViewport { root, tabs: node },
        rect: layout.viewport_rect,
        layer,
    });
    if let Some(rect) = layout.scroll_back_rect {
        fingerprint_regions.push(ProjectionRegion {
            id: ProjectionRegionId::TabScrollBack { root, tabs: node },
            rect,
            layer,
        });
    }
    if let Some(rect) = layout.scroll_forward_rect {
        fingerprint_regions.push(ProjectionRegion {
            id: ProjectionRegionId::TabScrollForward { root, tabs: node },
            rect,
            layer,
        });
    }
    if let Some(rect) = layout.overflow_rect {
        fingerprint_regions.push(ProjectionRegion {
            id: ProjectionRegionId::TabOverflow { root, tabs: node },
            rect,
            layer,
        });
    }

    fingerprint_regions.push(ProjectionRegion {
        id: ProjectionRegionId::Pane {
            root,
            tabs: node,
            selected,
            available: selected.is_some_and(|selected| {
                tabs.iter().any(|tab| tab.item == selected && !tab.missing)
            }),
        },
        rect: content_rect,
        layer,
    });

    push_tab_gap_targets(
        workspace,
        surface,
        root,
        node,
        layout.viewport_rect,
        &tabs,
        layer,
        style,
        ready,
    )?;
    push_leaf_guide(
        workspace,
        surface,
        root,
        node,
        node_rect,
        content_rect,
        root_node,
        layer,
        style,
        fraction,
        ready,
    )?;

    Ok(TabsPlan {
        key,
        root,
        node,
        node_rect,
        tab_bar_rect,
        group_drag_rect,
        tab_viewport_rect: layout.viewport_rect,
        overflow_rect: layout.overflow_rect,
        scroll_back_rect: layout.scroll_back_rect,
        scroll_forward_rect: layout.scroll_forward_rect,
        scroll_offset: layout.scroll_offset,
        max_scroll_offset: layout.max_scroll_offset,
        content_rect,
        selected,
        reveal_identity: layout.state.applied_reveal_identity,
        pending_keyboard_focus: layout.state.pending_keyboard_focus,
        tabs,
        overflow_menu_measurements,
        overflow_menu_open: layout.state.overflow_menu_open,
        overflow_menu_geometry,
        layout_identity,
    })
}

#[allow(
    clippy::cast_precision_loss,
    reason = "the tab count scales an extent already represented in egui f32 points"
)]
fn allocate_tab_widths(
    desired: impl IntoIterator<Item = f32>,
    available: f32,
    configured_minimum: f32,
) -> Vec<f32> {
    let desired = desired.into_iter().collect::<Vec<_>>();
    if desired.is_empty() {
        return Vec::new();
    }
    let available = available.max(0.0);
    let count = desired.len() as f32;
    if configured_minimum * count >= available {
        return vec![configured_minimum; desired.len()];
    }
    let desired_total = desired.iter().sum::<f32>();
    if desired_total <= available {
        return desired;
    }
    let shrinkable = desired
        .iter()
        .map(|width| width - configured_minimum)
        .sum::<f32>();
    let excess = desired_total - available;
    desired
        .into_iter()
        .map(|width| width - excess * ((width - configured_minimum) / shrinkable))
        .collect()
}

fn tab_strip_state_id(instance_id: Id) -> Id {
    instance_id.with("tab-strip-states")
}

pub(crate) fn load_tab_strip_states(ui: &Ui, instance_id: Id) -> TabStripStateMap {
    ui.ctx()
        .data_mut(|data| data.get_temp(tab_strip_state_id(instance_id)))
        .unwrap_or_default()
}

pub(crate) fn store_tab_strip_states(ui: &Ui, instance_id: Id, states: TabStripStateMap) {
    ui.ctx().data_mut(|data| {
        data.insert_temp(tab_strip_state_id(instance_id), states);
    });
}

pub(crate) fn set_tab_strip_scroll(
    ui: &Ui,
    instance_id: Id,
    plan: &TabsPlan,
    requested_offset: f32,
) -> bool {
    let requested_offset = if requested_offset.is_finite() {
        requested_offset.clamp(0.0, plan.max_scroll_offset)
    } else {
        plan.scroll_offset
    };
    ui.ctx().data_mut(|data| {
        let states: &mut TabStripStateMap =
            data.get_temp_mut_or_default(tab_strip_state_id(instance_id));
        let Some(state) = states.entries.get_mut(&plan.key) else {
            return false;
        };
        if state.layout_identity.as_ref() != Some(&plan.layout_identity)
            || (requested_offset - state.scroll_offset).abs() <= f32::EPSILON
        {
            return false;
        }
        state.scroll_offset = requested_offset;
        true
    })
}

pub(crate) fn set_tab_strip_scroll_preserving_reveals(
    ui: &Ui,
    instance_id: Id,
    plan: &TabsPlan,
    style: &DockStyle,
    reveal_identity: TabRevealIdentity,
    requested_offset: f32,
) -> bool {
    let requested_offset = if requested_offset.is_finite() {
        requested_offset.clamp(0.0, plan.max_scroll_offset)
    } else {
        plan.scroll_offset
    };
    let protected_offset = resolve_reveal_scroll_range(reveal_identity, |item| {
        let tab = plan.tabs.iter().find(|tab| tab.item == item)?;
        let content_rect =
            tab_content_rect(tab.rect, plan.tab_viewport_rect.min.x, plan.scroll_offset);
        tab_scroll_range(
            content_rect,
            tab.closeable,
            plan.tab_viewport_rect.width(),
            style,
        )
        .and_then(|range| range.bounded(0.0, plan.max_scroll_offset))
    })
    .map_or(requested_offset, |range| {
        requested_offset.clamp(range.min, range.max)
    });
    set_tab_strip_scroll(ui, instance_id, plan, protected_offset)
}

fn tab_content_rect(tab_rect: Rect, viewport_min_x: f32, scroll_offset: f32) -> Rect {
    tab_rect.translate(vec2(scroll_offset - viewport_min_x, 0.0))
}

pub(crate) fn sync_tab_reveal_identity(
    ui: &Ui,
    instance_id: Id,
    plan: &TabsPlan,
    reveal_identity: TabRevealIdentity,
) -> bool {
    ui.ctx().data_mut(|data| {
        let states: &mut TabStripStateMap =
            data.get_temp_mut_or_default(tab_strip_state_id(instance_id));
        let Some(state) = states.entries.get_mut(&plan.key) else {
            return false;
        };
        let reveal_identity = reveal_identity.with_keyboard_focused(
            state
                .pending_keyboard_focus
                .or(reveal_identity.keyboard_focused),
        );
        if state.layout_identity.as_ref() != Some(&plan.layout_identity)
            || state.requested_reveal_identity == reveal_identity
        {
            return false;
        }
        state.requested_reveal_identity = reveal_identity;
        true
    })
}

pub(crate) fn request_tab_keyboard_focus(
    ui: &Ui,
    instance_id: Id,
    plan: &TabsPlan,
    item: ItemId,
) -> bool {
    if !plan.tabs.iter().any(|tab| tab.item == item) {
        return false;
    }
    ui.ctx().data_mut(|data| {
        let states: &mut TabStripStateMap =
            data.get_temp_mut_or_default(tab_strip_state_id(instance_id));
        let Some(state) = states.entries.get_mut(&plan.key) else {
            return false;
        };
        if state.layout_identity.as_ref() != Some(&plan.layout_identity)
            || state.pending_keyboard_focus == Some(item)
        {
            return false;
        }
        state.pending_keyboard_focus = Some(item);
        state.requested_reveal_identity = state
            .requested_reveal_identity
            .with_keyboard_focused(Some(item));
        true
    })
}

pub(crate) fn complete_tab_keyboard_focus(ui: &Ui, instance_id: Id, plan: &TabsPlan, item: ItemId) {
    ui.ctx().data_mut(|data| {
        let states: &mut TabStripStateMap =
            data.get_temp_mut_or_default(tab_strip_state_id(instance_id));
        let Some(state) = states.entries.get_mut(&plan.key) else {
            return;
        };
        if state.layout_identity.as_ref() == Some(&plan.layout_identity)
            && state.pending_keyboard_focus == Some(item)
        {
            state.pending_keyboard_focus = None;
        }
    });
}

pub(crate) fn set_overflow_menu_geometry(
    ui: &Ui,
    instance_id: Id,
    plan: &TabsPlan,
    expanded: bool,
    geometry: Option<OverflowMenuGeometry>,
) -> bool {
    let geometry = expanded.then_some(geometry).flatten().and_then(|geometry| {
        let viewport = intersect_rect(geometry.viewport_rect, geometry.popup_rect);
        let complete = geometry.popup_rect.is_positive()
            && geometry.viewport_rect.is_positive()
            && viewport == geometry.viewport_rect
            && geometry.items.len() == plan.overflow_menu_measurements.len()
            && geometry
                .items
                .iter()
                .zip(&plan.overflow_menu_measurements)
                .all(|(item, measurement)| {
                    let clipped = intersect_rect(item.actual_rect, geometry.viewport_rect);
                    let expected_hit = clipped.is_positive().then_some(clipped);
                    item.item == measurement.item
                        && item.index == measurement.index
                        && item.actual_rect.is_positive()
                        && item.hit_rect == expected_hit
                });
        complete.then(|| StoredOverflowMenuGeometry {
            measurements: plan.overflow_menu_measurements.clone(),
            geometry,
        })
    });
    ui.ctx().data_mut(|data| {
        let states: &mut TabStripStateMap =
            data.get_temp_mut_or_default(tab_strip_state_id(instance_id));
        let Some(state) = states.entries.get_mut(&plan.key) else {
            return false;
        };
        if state.layout_identity.as_ref() != Some(&plan.layout_identity)
            || (state.overflow_menu_open == expanded && state.overflow_menu_geometry == geometry)
        {
            return false;
        }
        state.overflow_menu_open = expanded;
        state.overflow_menu_geometry = geometry;
        true
    })
}

pub(crate) fn consume_overflow_menu_toggle(
    ui: &Ui,
    instance_id: Id,
    plan: &TabsPlan,
    frame: u64,
    opened: bool,
) -> bool {
    ui.ctx().data_mut(|data| {
        let states: &mut TabStripStateMap =
            data.get_temp_mut_or_default(tab_strip_state_id(instance_id));
        let Some(state) = states.entries.get_mut(&plan.key) else {
            return false;
        };
        if state.layout_identity.as_ref() != Some(&plan.layout_identity)
            || state
                .overflow_menu_toggle
                .is_some_and(|toggle| toggle.frame == frame)
        {
            return false;
        }
        state.overflow_menu_toggle = Some(OverflowMenuToggle { frame, opened });
        state.overflow_menu_open = opened;
        if !opened {
            state.overflow_menu_geometry = None;
        }
        true
    })
}

pub(crate) fn overflow_menu_opened_in_frame(
    ui: &Ui,
    instance_id: Id,
    plan: &TabsPlan,
    frame: u64,
) -> bool {
    ui.ctx().data_mut(|data| {
        let states: &mut TabStripStateMap =
            data.get_temp_mut_or_default(tab_strip_state_id(instance_id));
        states.entries.get(&plan.key).is_some_and(|state| {
            state
                .overflow_menu_toggle
                .is_some_and(|toggle| toggle.frame == frame && toggle.opened)
        })
    })
}

pub(crate) fn overflow_menu_requested_open(ui: &Ui, instance_id: Id, plan: &TabsPlan) -> bool {
    ui.ctx().data_mut(|data| {
        let states: &mut TabStripStateMap =
            data.get_temp_mut_or_default(tab_strip_state_id(instance_id));
        states.entries.get(&plan.key).is_some_and(|state| {
            state.layout_identity.as_ref() == Some(&plan.layout_identity)
                && state.overflow_menu_open
        })
    })
}

#[allow(
    clippy::too_many_lines,
    reason = "one deterministic layout pass keeps reveal constraints and hit geometry synchronized"
)]
fn layout_tab_strip(
    tab_bar_rect: Rect,
    group_drag_rect: Rect,
    widths: &[f32],
    selected: Option<(usize, ItemId)>,
    stored_state: &TabStripState,
    layout_identity: Arc<TabStripLayoutIdentity>,
    style: &DockStyle,
) -> TabStripLayout {
    let strip_min_x = group_drag_rect
        .max
        .x
        .clamp(tab_bar_rect.min.x, tab_bar_rect.max.x);
    let available_rect =
        Rect::from_min_max(pos2(strip_min_x, tab_bar_rect.min.y), tab_bar_rect.max);
    let content_width = widths.iter().sum::<f32>();
    let overflowed = content_width > available_rect.width();
    let overflow_rect = overflowed.then(|| {
        let width = tab_bar_rect.height().min(available_rect.width());
        Rect::from_min_max(
            pos2(available_rect.max.x - width, available_rect.min.y),
            available_rect.max,
        )
    });
    let viewport_rect = Rect::from_min_max(
        available_rect.min,
        pos2(
            overflow_rect.map_or(available_rect.max.x, |rect| rect.min.x),
            available_rect.max.y,
        ),
    );
    let viewport_width = viewport_rect.width();
    let max_scroll_offset = (content_width - viewport_width).max(0.0);
    let layout_changed = stored_state.layout_identity.as_ref() != Some(&layout_identity);
    let mut scroll_offset = if !layout_changed && stored_state.scroll_offset.is_finite() {
        stored_state.scroll_offset.clamp(0.0, max_scroll_offset)
    } else {
        0.0
    };
    let selected_item = selected.map(|(_, item)| item);
    let pending_keyboard_focus = stored_state
        .pending_keyboard_focus
        .filter(|item| layout_identity.tabs.iter().any(|entry| entry.item == *item));
    let reveal_identity = stored_state
        .requested_reveal_identity
        .with_selected(selected_item)
        .with_keyboard_focused(
            pending_keyboard_focus.or(stored_state.requested_reveal_identity.keyboard_focused),
        );
    let reveal_changed = reveal_identity != stored_state.applied_reveal_identity;
    let reveal_introduced = reveal_identity
        .introduces_item_since(stored_state.applied_reveal_identity)
        || reveal_identity.primary() != stored_state.applied_reveal_identity.primary();
    let viewport_changed = (stored_state.viewport_width - viewport_width).abs() > f32::EPSILON;
    if overflowed
        && viewport_width > 0.0
        && (layout_changed || reveal_introduced || viewport_changed)
        && let Some(primary) = reveal_identity.primary()
        && let Some((index, _)) = layout_identity
            .tabs
            .iter()
            .enumerate()
            .find(|(_, entry)| entry.item == primary)
    {
        let start = widths[..index].iter().sum::<f32>();
        let end = start + widths[index];
        if widths[index] > viewport_width || start < scroll_offset {
            scroll_offset = start;
        } else if end > scroll_offset + viewport_width {
            scroll_offset = end - viewport_width;
        }
    }
    if layout_changed || reveal_changed || viewport_changed {
        let range = resolve_reveal_scroll_range(reveal_identity, |item| {
            let (index, entry) = layout_identity
                .tabs
                .iter()
                .enumerate()
                .find(|(_, entry)| entry.item == item)?;
            let start = widths[..index].iter().sum::<f32>();
            tab_scroll_range(
                Rect::from_min_size(pos2(start, 0.0), vec2(widths[index], tab_bar_rect.height())),
                entry.closeable,
                viewport_width,
                style,
            )
            .and_then(|range| range.bounded(0.0, max_scroll_offset))
        });
        if let Some(range) = range {
            scroll_offset = scroll_offset.clamp(range.min, range.max);
        }
        scroll_offset = scroll_offset.clamp(0.0, max_scroll_offset);
    }

    let (scroll_back_rect, scroll_forward_rect) = overflowed
        .then(|| tab_scroll_edge_rects(viewport_rect, tab_bar_rect.height()))
        .map_or((None, None), |(back, forward)| {
            (
                back.is_positive().then_some(back),
                forward.is_positive().then_some(forward),
            )
        });

    let mut cursor = viewport_rect.min.x - scroll_offset;
    let mut tab_rects = Vec::with_capacity(widths.len());
    let mut visible_rects = Vec::with_capacity(widths.len());
    for width in widths {
        let rect = Rect::from_min_max(
            pos2(cursor, tab_bar_rect.min.y),
            pos2(cursor + *width, tab_bar_rect.max.y),
        );
        cursor = rect.max.x;
        let visible = intersect_rect(rect, viewport_rect);
        let minimum_visible_width = style
            .tab_close_size
            .min(style.tab_min_width)
            .min(rect.width());
        tab_rects.push(rect);
        visible_rects.push(
            (visible.is_positive() && visible.width() >= minimum_visible_width).then_some(visible),
        );
    }

    let state = TabStripState {
        scroll_offset,
        requested_reveal_identity: reveal_identity,
        applied_reveal_identity: reveal_identity,
        pending_keyboard_focus,
        viewport_width,
        layout_identity: Some(layout_identity),
        overflow_menu_open: overflowed && stored_state.overflow_menu_open,
        overflow_menu_geometry: if layout_changed || !overflowed {
            None
        } else {
            stored_state.overflow_menu_geometry.clone()
        },
        overflow_menu_toggle: stored_state.overflow_menu_toggle,
    };
    TabStripLayout {
        viewport_rect,
        overflow_rect,
        scroll_back_rect,
        scroll_forward_rect,
        scroll_offset,
        max_scroll_offset,
        tab_rects,
        visible_rects,
        state,
    }
}

fn tab_scroll_edge_rects(viewport: Rect, configured_extent: f32) -> (Rect, Rect) {
    let middle = viewport.center().x;
    let back_max = (viewport.min.x + configured_extent).min(middle);
    let forward_min = (viewport.max.x - configured_extent).max(middle);
    (
        Rect::from_min_max(viewport.min, pos2(back_max, viewport.max.y)),
        Rect::from_min_max(pos2(forward_min, viewport.min.y), viewport.max),
    )
}

impl TabScrollRange {
    fn bounded(self, minimum: f32, maximum: f32) -> Option<Self> {
        let bounded = Self {
            min: self.min.max(minimum),
            max: self.max.min(maximum),
        };
        (bounded.min <= bounded.max).then_some(bounded)
    }

    fn intersect(self, other: Self) -> Option<Self> {
        Self {
            min: self.min.max(other.min),
            max: self.max.min(other.max),
        }
        .bounded(f32::NEG_INFINITY, f32::INFINITY)
    }
}

fn resolve_reveal_scroll_range(
    identity: TabRevealIdentity,
    mut range_for: impl FnMut(ItemId) -> Option<TabScrollRange>,
) -> Option<TabScrollRange> {
    let mut seen = [None; 3];
    let mut seen_len = 0;
    let mut intersection: Option<TabScrollRange> = None;
    for item in identity.prioritized_items() {
        if seen[..seen_len].contains(&Some(item)) {
            continue;
        }
        seen[seen_len] = Some(item);
        seen_len += 1;
        let Some(range) = range_for(item) else {
            continue;
        };
        let next = match intersection {
            None => Some(range),
            Some(current) => current.intersect(range),
        };
        let Some(next) = next else {
            return intersection;
        };
        intersection = Some(next);
    }
    intersection
}

fn tab_scroll_range(
    content_rect: Rect,
    closeable: bool,
    viewport_width: f32,
    style: &DockStyle,
) -> Option<TabScrollRange> {
    if !content_rect.is_positive() || !viewport_width.is_finite() || viewport_width <= 0.0 {
        return None;
    }
    if closeable {
        let close = ideal_tab_close_rect(content_rect, style)?;
        let minimum_drag_width = tab_operable_drag_width(content_rect, style);
        return TabScrollRange {
            min: close.max.x - viewport_width,
            max: close.min.x - minimum_drag_width,
        }
        .bounded(f32::NEG_INFINITY, f32::INFINITY);
    }
    let guard = style
        .tab_close_size
        .min(style.tab_min_width)
        .min(content_rect.width())
        .min(viewport_width);
    TabScrollRange {
        min: content_rect.min.x + guard - viewport_width,
        max: content_rect.max.x - guard,
    }
    .bounded(f32::NEG_INFINITY, f32::INFINITY)
}

fn tab_operable_drag_width(full_rect: Rect, style: &DockStyle) -> f32 {
    style
        .tab_close_size
        .min(style.tab_min_width)
        .min(full_rect.width())
}

fn ideal_tab_close_rect(full_rect: Rect, style: &DockStyle) -> Option<Rect> {
    let size = style
        .tab_close_size
        .min(full_rect.height())
        .min(full_rect.width());
    if !size.is_finite() || size <= 0.0 {
        return None;
    }
    let padding = style
        .tab_horizontal_padding
        .min((full_rect.width() - size).max(0.0));
    Some(Rect::from_center_size(
        pos2(full_rect.max.x - padding - size * 0.5, full_rect.center().y),
        Vec2::splat(size),
    ))
}

fn tab_chrome_geometry(
    full_rect: Rect,
    visible_rect: Rect,
    closeable: bool,
    style: &DockStyle,
) -> Option<TabChromeGeometry> {
    if !visible_rect.is_positive() {
        return None;
    }
    let ideal_close = closeable
        .then(|| ideal_tab_close_rect(full_rect, style))
        .flatten()
        .filter(|close| visible_rect.contains_rect(*close));
    if closeable && ideal_close.is_none() {
        return None;
    }
    let drag_rect = ideal_close.map_or(visible_rect, |close| {
        Rect::from_min_max(visible_rect.min, pos2(close.min.x, visible_rect.max.y))
    });
    if !drag_rect.is_positive() || drag_rect.width() < tab_operable_drag_width(full_rect, style) {
        return None;
    }
    let left_padding = style.tab_horizontal_padding.min(full_rect.width());
    let text_left = (full_rect.min.x + left_padding).min(full_rect.max.x);
    let text_right = ideal_close
        .map_or(full_rect.max.x - left_padding, |close| {
            close.min.x - left_padding
        })
        .clamp(text_left, full_rect.max.x);
    let text_rect = intersect_rect(
        Rect::from_min_max(
            pos2(text_left, full_rect.min.y),
            pos2(text_right, full_rect.max.y),
        ),
        visible_rect,
    );
    Some(TabChromeGeometry {
        text: text_rect,
        close: ideal_close,
        drag: drag_rect,
    })
}

#[derive(Clone, Copy)]
struct GuideButtonGeometry {
    draw: Rect,
    hit: Rect,
}

#[derive(Clone, Copy)]
struct GuideEdgeGeometry {
    left: GuideButtonGeometry,
    right: GuideButtonGeometry,
    top: GuideButtonGeometry,
    bottom: GuideButtonGeometry,
}

impl GuideEdgeGeometry {
    const fn get(self, edge: Edge) -> GuideButtonGeometry {
        match edge {
            Edge::Left => self.left,
            Edge::Right => self.right,
            Edge::Top => self.top,
            Edge::Bottom => self.bottom,
        }
    }

    const fn buttons(self) -> [GuideButtonGeometry; 4] {
        [self.left, self.right, self.top, self.bottom]
    }
}

#[derive(Clone, Copy)]
struct InnerGuideGeometry {
    center: GuideButtonGeometry,
    edges: GuideEdgeGeometry,
}

#[allow(clippy::too_many_arguments)]
fn push_leaf_guide(
    workspace: &Workspace,
    surface: SurfaceId,
    root: RootId,
    node: NodeId,
    node_rect: Rect,
    content_rect: Rect,
    root_node: NodeId,
    layer: SceneLayerKey,
    style: &DockStyle,
    fraction: DockFraction,
    ready: &mut ReadySurfaceScene,
) -> Result<(), ProjectionError> {
    let Some(geometry) = inner_guide_geometry(content_rect, style) else {
        return Ok(());
    };
    if !rect_contains(node_rect, content_rect) {
        return Ok(());
    }

    let center = guide_target_record(
        DropTargetId::Center {
            surface,
            root,
            tabs: node,
        },
        DockTarget::Center(workspace.capture_tab_target(root, node)?),
        geometry.center,
        content_rect,
        layer,
    )?;
    let single_tabs_root = node == root_node;
    let edge_record = |edge: Edge| -> Result<DropGuideTargetRecord, ProjectionError> {
        let id = if single_tabs_root {
            DropTargetId::OuterEdge {
                surface,
                root,
                node,
                edge,
            }
        } else {
            DropTargetId::InnerEdge {
                surface,
                root,
                node,
                edge,
            }
        };
        guide_target_record(
            id,
            DockTarget::Edge(workspace.capture_edge_target(root, node, edge, fraction)?),
            geometry.edges.get(edge),
            edge_visual(node_rect, edge, fraction),
            layer,
        )
    };
    let edges = DropGuideEdgeSet::new(
        edge_record(Edge::Left)?,
        edge_record(Edge::Right)?,
        edge_record(Edge::Top)?,
        edge_record(Edge::Bottom)?,
    );
    ready.push_drop_guide_cluster(DropGuideClusterRecord::inner(
        surface,
        root,
        node,
        HitRegion::new(to_logical_rect(node_rect)?),
        layer,
        center,
        edges,
    ));
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn push_outer_guide(
    workspace: &Workspace,
    surface: SurfaceId,
    root: RootId,
    root_node: NodeId,
    bounds: Rect,
    layer: SceneLayerKey,
    style: &DockStyle,
    fraction: DockFraction,
    ready: &mut ReadySurfaceScene,
) -> Result<(), ProjectionError> {
    if matches!(workspace.node(root_node), Some(Node::Tabs { .. })) {
        return Ok(());
    }
    let Some(edges_geometry) = outer_guide_geometry(bounds, style) else {
        return Ok(());
    };
    let edge_record = |edge: Edge| -> Result<DropGuideTargetRecord, ProjectionError> {
        guide_target_record(
            DropTargetId::OuterEdge {
                surface,
                root,
                node: root_node,
                edge,
            },
            DockTarget::Edge(workspace.capture_edge_target(root, root_node, edge, fraction)?),
            edges_geometry.get(edge),
            edge_visual(bounds, edge, fraction),
            layer,
        )
    };
    let edges = DropGuideEdgeSet::new(
        edge_record(Edge::Left)?,
        edge_record(Edge::Right)?,
        edge_record(Edge::Top)?,
        edge_record(Edge::Bottom)?,
    );
    ready.push_drop_guide_cluster(DropGuideClusterRecord::outer(
        surface,
        root,
        HitRegion::new(to_logical_rect(bounds)?),
        layer,
        edges,
    ));
    Ok(())
}

fn guide_target_record(
    id: DropTargetId,
    target: DockTarget,
    geometry: GuideButtonGeometry,
    preview: Rect,
    layer: SceneLayerKey,
) -> Result<DropGuideTargetRecord, ProjectionError> {
    Ok(DropGuideTargetRecord::new(
        DropTargetRecord::new(
            id,
            target,
            DropTargetAvailability::Available,
            HitRegion::new(to_logical_rect(geometry.hit)?),
            layer,
            DropVisual::new(to_logical_rect(preview)?),
        ),
        to_logical_rect(geometry.draw)?,
    ))
}

fn inner_guide_geometry(content: Rect, style: &DockStyle) -> Option<InnerGuideGeometry> {
    if !content.is_positive() {
        return None;
    }
    let center = content.center();
    let offset = style.drop_guide_extent + style.drop_guide_gap;
    let center_button = guide_button(center, style);
    let edges = GuideEdgeGeometry {
        left: guide_button(center - vec2(offset, 0.0), style),
        right: guide_button(center + vec2(offset, 0.0), style),
        top: guide_button(center - vec2(0.0, offset), style),
        bottom: guide_button(center + vec2(0.0, offset), style),
    };
    let [left, right, top, bottom] = edges.buttons();
    let buttons = [center_button, left, right, top, bottom];
    guide_buttons_fit(content, &buttons).then_some(InnerGuideGeometry {
        center: center_button,
        edges,
    })
}

fn outer_guide_geometry(bounds: Rect, style: &DockStyle) -> Option<GuideEdgeGeometry> {
    if !bounds.is_positive() {
        return None;
    }
    let center = bounds.center();
    let inset = style.drop_guide_outer_inset;
    let edges = GuideEdgeGeometry {
        left: guide_button(pos2(bounds.min.x + inset, center.y), style),
        right: guide_button(pos2(bounds.max.x - inset, center.y), style),
        top: guide_button(pos2(center.x, bounds.min.y + inset), style),
        bottom: guide_button(pos2(center.x, bounds.max.y - inset), style),
    };
    guide_buttons_fit(bounds, &edges.buttons()).then_some(edges)
}

fn guide_button(center: egui::Pos2, style: &DockStyle) -> GuideButtonGeometry {
    let draw = Rect::from_center_size(center, Vec2::splat(style.drop_guide_extent));
    GuideButtonGeometry {
        draw,
        hit: draw.expand(style.drop_guide_hit_padding),
    }
}

fn guide_buttons_fit(bounds: Rect, buttons: &[GuideButtonGeometry]) -> bool {
    bounds.is_positive()
        && buttons.iter().all(|button| {
            button.draw.is_positive()
                && button.hit.is_positive()
                && rect_contains(bounds, button.hit)
        })
        && buttons.iter().enumerate().all(|(index, button)| {
            buttons[index + 1..]
                .iter()
                .all(|other| !button.hit.intersect(other.hit).is_positive())
        })
}

fn rect_contains(outer: Rect, inner: Rect) -> bool {
    inner.min.x >= outer.min.x
        && inner.min.y >= outer.min.y
        && inner.max.x <= outer.max.x
        && inner.max.y <= outer.max.y
}

#[allow(clippy::too_many_arguments)]
fn push_tab_gap_targets(
    workspace: &Workspace,
    surface: SurfaceId,
    root: RootId,
    node: NodeId,
    bar: Rect,
    tabs: &[TabPlan],
    layer: SceneLayerKey,
    style: &DockStyle,
    ready: &mut ReadySurfaceScene,
) -> Result<(), ProjectionError> {
    if tabs.is_empty() || !bar.is_positive() {
        return Ok(());
    }
    let target = workspace.capture_tab_target(root, node)?;
    let tab_rects = tabs.iter().map(|tab| tab.rect).collect::<Vec<_>>();
    for geometry in tab_gap_geometries(bar, &tab_rects, style.splitter_thickness) {
        ready.push_drop_target(DropTargetRecord::new(
            DropTargetId::TabGap {
                surface,
                root,
                tabs: node,
                index: geometry.index,
            },
            DockTarget::TabGap {
                target: target.clone(),
                index: geometry.index,
            },
            DropTargetAvailability::Available,
            HitRegion::new(to_logical_rect(geometry.region)?),
            layer,
            DropVisual::new(to_logical_rect(geometry.visual)?),
        ));
    }
    Ok(())
}

fn tab_gap_geometries(
    bar: Rect,
    tabs: &[Rect],
    configured_marker_width: f32,
) -> Vec<TabGapGeometry> {
    let centers = tabs.iter().map(|tab| tab.center().x).collect::<Vec<_>>();
    let mut geometries = Vec::with_capacity(tabs.len() + 1);
    for index in 0..=tabs.len() {
        let left = if index == 0 {
            bar.min.x
        } else {
            centers[index - 1]
        };
        let right = if index == tabs.len() {
            bar.max.x
        } else {
            centers[index]
        };
        let region_left = left.max(bar.min.x);
        let region_right = right.min(bar.max.x);
        if region_right <= region_left {
            continue;
        }
        let region =
            Rect::from_min_max(pos2(region_left, bar.min.y), pos2(region_right, bar.max.y));
        let marker_x = if index == tabs.len() {
            bar.max.x
        } else {
            tabs[index].min.x
        };
        let marker_width = configured_marker_width.min(bar.width());
        let marker_min = (marker_x - marker_width * 0.5)
            .clamp(bar.min.x, (bar.max.x - marker_width).max(bar.min.x));
        let visual = Rect::from_min_size(
            pos2(marker_min, bar.min.y),
            vec2(marker_width, bar.height()),
        );
        if region.is_positive() && visual.is_positive() {
            geometries.push(TabGapGeometry {
                index,
                region,
                visual,
            });
        }
    }
    geometries
}

fn edge_visual(rect: Rect, edge: Edge, fraction: DockFraction) -> Rect {
    let fraction = fraction.get();
    match edge {
        Edge::Left => Rect::from_min_max(
            rect.min,
            pos2(rect.min.x + rect.width() * fraction, rect.max.y),
        ),
        Edge::Right => Rect::from_min_max(
            pos2(rect.max.x - rect.width() * fraction, rect.min.y),
            rect.max,
        ),
        Edge::Top => Rect::from_min_max(
            rect.min,
            pos2(rect.max.x, rect.min.y + rect.height() * fraction),
        ),
        Edge::Bottom => Rect::from_min_max(
            pos2(rect.min.x, rect.max.y - rect.height() * fraction),
            rect.max,
        ),
    }
}

fn splitter_hit_rects(rects: &[Rect], horizontal: bool, extent: f32, bounds: Rect) -> Vec<Rect> {
    rects
        .iter()
        .enumerate()
        .map(|(index, rect)| {
            let center = if horizontal {
                rect.center().x
            } else {
                rect.center().y
            };
            let previous = index.checked_sub(1).map(|previous| {
                if horizontal {
                    rects[previous].center().x
                } else {
                    rects[previous].center().y
                }
            });
            let next = rects.get(index + 1).map(|next| {
                if horizontal {
                    next.center().x
                } else {
                    next.center().y
                }
            });
            let axis_min = if horizontal {
                bounds.min.x
            } else {
                bounds.min.y
            };
            let axis_max = if horizontal {
                bounds.max.x
            } else {
                bounds.max.y
            };
            let cell_min = previous.map_or(axis_min, |previous| (previous + center) * 0.5);
            let cell_max = next.map_or(axis_max, |next| (center + next) * 0.5);
            let hit_min = (center - extent * 0.5).max(cell_min);
            let hit_max = (center + extent * 0.5).min(cell_max).max(hit_min);
            let hit = if horizontal {
                Rect::from_min_max(pos2(hit_min, rect.min.y), pos2(hit_max, rect.max.y))
            } else {
                Rect::from_min_max(pos2(rect.min.x, hit_min), pos2(rect.max.x, hit_max))
            };
            intersect_rect(hit, bounds)
        })
        .collect()
}

fn intersect_rect(rect: Rect, bounds: Rect) -> Rect {
    let min = rect.min.max(bounds.min).min(bounds.max);
    let max = rect.max.min(bounds.max).max(min);
    Rect::from_min_max(min, max)
}

fn to_logical_rect(rect: Rect) -> Result<LogicalRect, GeometryError> {
    LogicalRect::new(
        f64::from(rect.min.x),
        f64::from(rect.min.y),
        f64::from(rect.width()),
        f64::from(rect.height()),
    )
}

fn from_logical_rect(rect: LogicalRect) -> Result<Rect, ProjectionError> {
    Ok(Rect::from_min_max(
        pos2(
            to_egui_coordinate("minimum x", rect.min().x())?,
            to_egui_coordinate("minimum y", rect.min().y())?,
        ),
        pos2(
            to_egui_coordinate("maximum x", rect.max().x())?,
            to_egui_coordinate("maximum y", rect.max().y())?,
        ),
    ))
}

fn to_egui_coordinate(component: &'static str, value: f64) -> Result<f32, ProjectionError> {
    #[allow(
        clippy::cast_possible_truncation,
        reason = "the finite f64 coordinate is explicitly narrowed to egui's f32 coordinate space"
    )]
    let represented = value as f32;
    if represented.is_finite() {
        Ok(represented)
    } else {
        Err(ProjectionError::UnrepresentableEguiCoordinate { component, value })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use dockspace::drop_guide::{DropGuideScope, DropGuideSlot};
    use dockspace::graph::{RootRecord, SurfacePresentation};

    const SURFACE: SurfaceId = SurfaceId::new(1);
    const ROOT: RootId = RootId::new(10);
    const ITEM_A: ItemId = ItemId::new(100);
    const ITEM_B: ItemId = ItemId::new(101);
    const ITEM_C: ItemId = ItemId::new(102);

    fn tab_layout_identity(
        epoch: u64,
        entries: impl IntoIterator<Item = (ItemId, f32, bool)>,
    ) -> Arc<TabStripLayoutIdentity> {
        Arc::new(TabStripLayoutIdentity {
            workspace_epoch: WorkspaceEpoch::new(epoch),
            style: (&DockStyle::default()).into(),
            overflow_menu_style: OverflowMenuStyleIdentity::default(),
            tabs: entries
                .into_iter()
                .map(|(item, width, closeable)| TabStripEntryIdentity {
                    item,
                    width: width.to_bits(),
                    closeable,
                    overflow_menu_width: width.to_bits(),
                    overflow_menu_height: 28.0_f32.to_bits(),
                })
                .collect(),
        })
    }

    fn three_tab_identity(epoch: u64) -> Arc<TabStripLayoutIdentity> {
        tab_layout_identity(
            epoch,
            [
                (ITEM_A, 72.0, true),
                (ITEM_B, 72.0, true),
                (ITEM_C, 72.0, true),
            ],
        )
    }

    fn single_workspace() -> (Workspace, NodeId) {
        let mut builder = Workspace::builder();
        let tabs = builder.insert_node(Node::tabs([ITEM_A]));
        builder.set_root(ROOT, RootRecord::new(tabs));
        builder.set_surface(SURFACE, SurfacePresentation::new(ROOT));
        (builder.build().expect("single-tabs fixture is valid"), tabs)
    }

    fn split_workspace() -> (Workspace, NodeId, NodeId, NodeId) {
        let mut builder = Workspace::builder();
        let left = builder.insert_node(Node::tabs([ITEM_A]));
        let right = builder.insert_node(Node::tabs([ITEM_B]));
        let split = builder.insert_node(
            Node::equal_split(Axis::Horizontal, [left, right]).expect("two children form a split"),
        );
        builder.set_root(ROOT, RootRecord::new(split));
        builder.set_surface(SURFACE, SurfacePresentation::new(ROOT));
        (
            builder.build().expect("split fixture is valid"),
            split,
            left,
            right,
        )
    }

    fn publish_leaf(
        workspace: &Workspace,
        ready: &mut ReadySurfaceScene,
        node: NodeId,
        root_node: NodeId,
        node_rect: Rect,
    ) {
        let content = Rect::from_min_max(
            pos2(
                node_rect.min.x,
                node_rect.min.y + DockStyle::default().tab_bar_height,
            ),
            node_rect.max,
        );
        push_leaf_guide(
            workspace,
            SURFACE,
            ROOT,
            node,
            node_rect,
            content,
            root_node,
            SceneLayerKey::new(1),
            &DockStyle::default(),
            DockFraction::new(0.5).expect("fixture fraction is valid"),
            ready,
        )
        .expect("leaf guide projects");
    }

    fn assert_edge_targets(
        cluster: &DropGuideClusterRecord,
        node: NodeId,
        preview_bounds: Rect,
        outer: bool,
    ) {
        let fraction = DockFraction::new(0.5).expect("fixture fraction is valid");
        for edge in [Edge::Left, Edge::Right, Edge::Top, Edge::Bottom] {
            let record = cluster
                .target(DropGuideSlot::Edge(edge))
                .expect("complete guide contains every edge");
            let expected_id = if outer {
                DropTargetId::OuterEdge {
                    surface: SURFACE,
                    root: ROOT,
                    node,
                    edge,
                }
            } else {
                DropTargetId::InnerEdge {
                    surface: SURFACE,
                    root: ROOT,
                    node,
                    edge,
                }
            };
            assert_eq!(record.id(), expected_id);
            let DockTarget::Edge(target) = record.target().target() else {
                panic!("edge slot must own an edge command target");
            };
            assert_eq!(target.node(), node);
            assert_eq!(target.edge(), edge);
            assert_eq!(target.fraction(), fraction);
            assert_eq!(
                record.target().visual().rect(),
                to_logical_rect(edge_visual(preview_bounds, edge, fraction))
                    .expect("fixture preview is finite")
            );
        }
    }

    fn assert_button_geometry(
        record: &DropGuideTargetRecord,
        expected_center: egui::Pos2,
        style: &DockStyle,
    ) {
        let expected_draw =
            Rect::from_center_size(expected_center, Vec2::splat(style.drop_guide_extent));
        assert_eq!(
            from_logical_rect(record.draw()).expect("fixture draw is representable"),
            expected_draw
        );
        assert_eq!(
            from_logical_rect(record.target().region().rect())
                .expect("fixture hit region is representable"),
            expected_draw.expand(style.drop_guide_hit_padding)
        );
    }

    #[test]
    fn tab_widths_fill_available_space_without_layout_shift() {
        assert_eq!(
            allocate_tab_widths([80.0, 120.0], 240.0, 72.0),
            [80.0, 120.0]
        );

        let widths = allocate_tab_widths([100.0, 140.0], 200.0, 72.0);
        assert!((widths.iter().sum::<f32>() - 200.0).abs() < 0.001);
        assert!(widths.iter().all(|width| *width >= 72.0));

        assert_eq!(
            allocate_tab_widths([100.0, 100.0], 100.0, 72.0),
            [72.0, 72.0]
        );
    }

    #[test]
    fn overflowing_strip_keeps_intrinsic_widths_and_reserves_a_fixed_control() {
        let bar = Rect::from_min_size(pos2(0.0, 0.0), vec2(180.0, 28.0));
        let grip = Rect::from_min_size(bar.min, vec2(28.0, 28.0));
        let layout = layout_tab_strip(
            bar,
            grip,
            &[72.0, 72.0, 72.0],
            Some((0, ITEM_A)),
            &TabStripState::default(),
            three_tab_identity(0),
            &DockStyle::default(),
        );

        assert_eq!(
            layout.overflow_rect,
            Some(Rect::from_min_max(pos2(152.0, 0.0), pos2(180.0, 28.0)))
        );
        assert_eq!(
            layout.viewport_rect,
            Rect::from_min_max(pos2(28.0, 0.0), pos2(152.0, 28.0))
        );
        assert_eq!(
            layout.tab_rects.iter().map(Rect::width).collect::<Vec<_>>(),
            [72.0, 72.0, 72.0]
        );
        assert!(
            layout
                .visible_rects
                .iter()
                .flatten()
                .all(|rect| layout.viewport_rect.contains_rect(*rect))
        );
        assert!(layout.visible_rects[2].is_none());
    }

    #[test]
    fn changed_selection_is_revealed_without_destabilizing_scroll_state() {
        let bar = Rect::from_min_size(pos2(0.0, 0.0), vec2(180.0, 28.0));
        let grip = Rect::from_min_size(bar.min, vec2(28.0, 28.0));
        let first = layout_tab_strip(
            bar,
            grip,
            &[72.0, 72.0, 72.0],
            Some((0, ITEM_A)),
            &TabStripState::default(),
            three_tab_identity(0),
            &DockStyle::default(),
        );
        let selected_last = layout_tab_strip(
            bar,
            grip,
            &[72.0, 72.0, 72.0],
            Some((2, ITEM_C)),
            &first.state,
            three_tab_identity(0),
            &DockStyle::default(),
        );

        assert!((selected_last.scroll_offset - selected_last.max_scroll_offset).abs() < 0.001);
        assert_eq!(
            selected_last.visible_rects[2],
            Some(selected_last.tab_rects[2])
        );

        let stable = layout_tab_strip(
            bar,
            grip,
            &[72.0, 72.0, 72.0],
            Some((2, ITEM_C)),
            &selected_last.state,
            three_tab_identity(0),
            &DockStyle::default(),
        );
        assert!((stable.scroll_offset - selected_last.scroll_offset).abs() < 0.001);
        assert_eq!(stable.state, selected_last.state);
    }

    #[test]
    fn layout_identity_changes_reveal_selection_while_stable_identity_keeps_manual_scroll() {
        let bar = Rect::from_min_size(pos2(0.0, 0.0), vec2(180.0, 28.0));
        let grip = Rect::from_min_size(bar.min, vec2(28.0, 28.0));
        let identity = three_tab_identity(0);
        let manually_scrolled = TabStripState {
            scroll_offset: 36.0,
            requested_reveal_identity: TabRevealIdentity::new(Some(ITEM_C), None, None),
            applied_reveal_identity: TabRevealIdentity::new(Some(ITEM_C), None, None),
            pending_keyboard_focus: None,
            viewport_width: 124.0,
            layout_identity: Some(identity.clone()),
            overflow_menu_open: false,
            overflow_menu_geometry: None,
            overflow_menu_toggle: None,
        };
        let stable = layout_tab_strip(
            bar,
            grip,
            &[72.0, 72.0, 72.0],
            Some((2, ITEM_C)),
            &manually_scrolled,
            identity,
            &DockStyle::default(),
        );
        assert!((stable.scroll_offset - 36.0).abs() < 0.001);

        let reordered = tab_layout_identity(
            0,
            [
                (ITEM_C, 72.0, true),
                (ITEM_A, 72.0, true),
                (ITEM_B, 72.0, true),
            ],
        );
        let reordered_layout = layout_tab_strip(
            bar,
            grip,
            &[72.0, 72.0, 72.0],
            Some((0, ITEM_C)),
            &manually_scrolled,
            reordered,
            &DockStyle::default(),
        );
        assert!(reordered_layout.scroll_offset.abs() < 0.001);
        assert_eq!(
            reordered_layout.visible_rects[0],
            Some(reordered_layout.tab_rects[0])
        );

        let replacement_layout = layout_tab_strip(
            bar,
            grip,
            &[72.0, 72.0, 72.0],
            Some((2, ITEM_C)),
            &manually_scrolled,
            three_tab_identity(1),
            &DockStyle::default(),
        );
        assert_eq!(
            replacement_layout.visible_rects[2],
            Some(replacement_layout.tab_rects[2])
        );

        let mut changed_style = DockStyle::default();
        changed_style.tab_horizontal_padding += 1.0;
        let style_identity = Arc::new(TabStripLayoutIdentity {
            workspace_epoch: WorkspaceEpoch::new(0),
            style: (&changed_style).into(),
            overflow_menu_style: OverflowMenuStyleIdentity::default(),
            tabs: three_tab_identity(0).tabs.clone(),
        });
        let style_layout = layout_tab_strip(
            bar,
            grip,
            &[72.0, 72.0, 72.0],
            Some((2, ITEM_C)),
            &manually_scrolled,
            style_identity,
            &changed_style,
        );
        assert_eq!(
            style_layout.visible_rects[2],
            Some(style_layout.tab_rects[2])
        );
    }

    #[test]
    fn tab_strip_state_map_prunes_dead_surface_root_node_keys() {
        let (_, _, first_node, second_node) = split_workspace();
        let first = TabStripKey {
            surface: SURFACE,
            root: ROOT,
            node: first_node,
        };
        let second = TabStripKey {
            surface: SurfaceId::new(2),
            root: RootId::new(11),
            node: second_node,
        };
        let mut states = TabStripStateMap {
            entries: BTreeMap::from([
                (first, TabStripState::default()),
                (second, TabStripState::default()),
            ]),
        };

        states.retain_keys(&BTreeSet::from([first]));

        assert_eq!(states.entries.keys().copied().collect::<Vec<_>>(), [first]);
    }

    #[test]
    fn frontmost_floating_layer_owns_scroll_and_its_outer_rect_blocks_background() {
        let (_, _, background_node, foreground_node) = split_workspace();
        let background = TabStripKey {
            surface: SURFACE,
            root: ROOT,
            node: background_node,
        };
        let foreground = TabStripKey {
            surface: SURFACE,
            root: RootId::new(11),
            node: foreground_node,
        };
        let layers = [
            TabScrollLayer {
                occlusion_rect: Rect::from_min_max(pos2(0.0, 0.0), pos2(300.0, 200.0)),
                strips: vec![(
                    background,
                    Rect::from_min_max(pos2(0.0, 40.0), pos2(180.0, 68.0)),
                )],
            },
            TabScrollLayer {
                occlusion_rect: Rect::from_min_max(pos2(20.0, 20.0), pos2(220.0, 180.0)),
                strips: vec![(
                    foreground,
                    Rect::from_min_max(pos2(20.0, 40.0), pos2(180.0, 68.0)),
                )],
            },
        ];

        assert_eq!(
            tab_scroll_owner_at(&layers, pos2(80.0, 50.0)),
            Some(foreground)
        );
        assert_eq!(tab_scroll_owner_at(&layers, pos2(200.0, 50.0)), None);
        assert_eq!(
            tab_scroll_owner_at(&layers, pos2(10.0, 50.0)),
            Some(background)
        );
    }

    #[test]
    fn closeable_partial_tab_without_complete_close_and_drag_hits_is_hidden() {
        let style = DockStyle::default();
        let full = Rect::from_min_size(pos2(28.0, 0.0), vec2(style.tab_min_width, 28.0));
        let minimum_drag_width = tab_operable_drag_width(full, &style);
        let chrome = tab_chrome_geometry(full, full, true, &style)
            .expect("a fully visible closeable tab has complete chrome");

        assert!(chrome.drag.width() >= minimum_drag_width);
        assert!(chrome.close.is_some_and(|rect| rect.is_positive()));
        assert!(
            !chrome
                .drag
                .intersect(chrome.close.expect("close exists"))
                .is_positive()
        );

        let close = ideal_tab_close_rect(full, &style).expect("close geometry is valid");
        let boundary =
            Rect::from_min_max(pos2(close.min.x - minimum_drag_width, full.min.y), full.max);
        let boundary_chrome = tab_chrome_geometry(full, boundary, true, &style)
            .expect("the explicit minimum drag width remains operable");
        assert!((boundary_chrome.drag.width() - minimum_drag_width).abs() <= f32::EPSILON);

        let narrower = Rect::from_min_max(boundary.min + vec2(1.0, 0.0), boundary.max);
        assert!(tab_chrome_geometry(full, narrower, true, &style).is_none());

        let reveal = tab_scroll_range(full, true, 124.0, &style)
            .expect("a normal closeable tab has a reveal range");
        assert!((reveal.max - (close.min.x - minimum_drag_width)).abs() <= f32::EPSILON);

        let clipped = Rect::from_min_max(pos2(28.0, 0.0), pos2(48.0, 28.0));
        let clipped_chrome = tab_chrome_geometry(full, clipped, true, &style);
        assert!(clipped_chrome.is_none());
    }

    #[test]
    fn reveal_scroll_range_protects_all_feasible_identities_and_has_stable_conflict_priority() {
        let style = DockStyle::default();
        let viewport_width = 124.0;
        let max_scroll_offset = 92.0;
        let range_for = |item| {
            let index = [ITEM_A, ITEM_B, ITEM_C]
                .iter()
                .position(|candidate| *candidate == item)?;
            tab_scroll_range(
                Rect::from_min_size(pos2([0.0, 72.0, 144.0][index], 0.0), vec2(72.0, 28.0)),
                true,
                viewport_width,
                &style,
            )
            .and_then(|range| range.bounded(0.0, max_scroll_offset))
        };

        let feasible = resolve_reveal_scroll_range(
            TabRevealIdentity::new(Some(ITEM_A), Some(ITEM_B), None),
            range_for,
        )
        .expect("adjacent selected and focused tabs have a shared range");
        assert!(feasible.min > 0.0);
        assert!(feasible.max < max_scroll_offset);

        let conflict = resolve_reveal_scroll_range(
            TabRevealIdentity::new(Some(ITEM_A), Some(ITEM_B), Some(ITEM_C)),
            range_for,
        )
        .expect("higher-priority identities remain protected after a conflict");
        let active = range_for(ITEM_C).expect("active item has a range");
        let focused = range_for(ITEM_B).expect("focused item has a range");
        assert_eq!(conflict, active.intersect(focused).expect("ranges overlap"));
    }

    #[test]
    fn reveal_conflict_retains_last_feasible_priority_intersection() {
        let resolved = resolve_reveal_scroll_range(
            TabRevealIdentity::new(Some(ITEM_A), Some(ITEM_B), Some(ITEM_C)),
            |item| match item {
                ITEM_A => Some(TabScrollRange {
                    min: 0.0,
                    max: 30.0,
                }),
                ITEM_B => Some(TabScrollRange {
                    min: 40.0,
                    max: 100.0,
                }),
                ITEM_C => Some(TabScrollRange {
                    min: 20.0,
                    max: 80.0,
                }),
                _ => None,
            },
        );

        assert_eq!(
            resolved,
            Some(TabScrollRange {
                min: 40.0,
                max: 80.0,
            })
        );
    }

    #[test]
    fn reveal_range_normalizes_nonzero_viewport_origin_to_strip_local_content() {
        let viewport_min_x = 28.0;
        let current_scroll = 36.0;
        let content = Rect::from_min_size(pos2(72.0, 0.0), vec2(72.0, 28.0));
        let screen = content.translate(vec2(viewport_min_x - current_scroll, 0.0));

        assert_eq!(
            tab_content_rect(screen, viewport_min_x, current_scroll),
            content
        );
    }

    #[test]
    fn scrolled_tab_gaps_keep_original_indices_and_never_escape_the_viewport() {
        let bar = Rect::from_min_size(pos2(0.0, 0.0), vec2(180.0, 28.0));
        let grip = Rect::from_min_size(bar.min, vec2(28.0, 28.0));
        let layout = layout_tab_strip(
            bar,
            grip,
            &[72.0, 72.0, 72.0],
            Some((2, ITEM_C)),
            &TabStripState::default(),
            three_tab_identity(0),
            &DockStyle::default(),
        );
        let gaps = tab_gap_geometries(layout.viewport_rect, &layout.tab_rects, 1.0);

        assert_eq!(
            gaps.iter().map(|gap| gap.index).collect::<Vec<_>>(),
            [1, 2, 3]
        );
        assert!(gaps.iter().all(|gap| {
            layout.viewport_rect.contains_rect(gap.region)
                && layout.viewport_rect.contains_rect(gap.visual)
        }));
    }

    #[test]
    fn single_tabs_root_publishes_one_complete_inner_cluster_without_outer_duplicate() {
        let (workspace, tabs) = single_workspace();
        let bounds = Rect::from_min_size(pos2(0.0, 0.0), vec2(320.0, 240.0));
        let mut ready = ReadySurfaceScene::new(
            SURFACE,
            to_logical_rect(bounds).expect("fixture bounds are finite"),
        );

        publish_leaf(&workspace, &mut ready, tabs, tabs, bounds);
        push_outer_guide(
            &workspace,
            SURFACE,
            ROOT,
            tabs,
            bounds,
            SceneLayerKey::new(1),
            &DockStyle::default(),
            DockFraction::new(0.5).expect("fixture fraction is valid"),
            &mut ready,
        )
        .expect("single root projection succeeds");

        assert!(ready.drop_targets().is_empty());
        let [cluster] = ready.drop_guide_clusters() else {
            panic!("single-tabs root must publish exactly one guide cluster");
        };
        assert_eq!(cluster.id().scope, DropGuideScope::Inner(tabs));
        assert_eq!(cluster.targets().count(), 5);
        assert!(cluster.target(DropGuideSlot::Center).is_some());
        assert_edge_targets(cluster, tabs, bounds, true);

        let style = DockStyle::default();
        let content = Rect::from_min_max(
            pos2(bounds.min.x, bounds.min.y + style.tab_bar_height),
            bounds.max,
        );
        let center = content.center();
        let offset = style.drop_guide_extent + style.drop_guide_gap;
        for (slot, expected_center) in [
            (DropGuideSlot::Center, center),
            (DropGuideSlot::Edge(Edge::Left), center - vec2(offset, 0.0)),
            (DropGuideSlot::Edge(Edge::Right), center + vec2(offset, 0.0)),
            (DropGuideSlot::Edge(Edge::Top), center - vec2(0.0, offset)),
            (
                DropGuideSlot::Edge(Edge::Bottom),
                center + vec2(0.0, offset),
            ),
        ] {
            assert_button_geometry(
                cluster.target(slot).expect("inner slot exists"),
                expected_center,
                &style,
            );
        }
    }

    #[test]
    fn boundary_touching_nested_leaves_keep_all_five_inner_directions() {
        let (workspace, split, left, right) = split_workspace();
        let root_bounds = Rect::from_min_size(pos2(0.0, 0.0), vec2(400.0, 240.0));
        let left_bounds = Rect::from_min_max(root_bounds.min, pos2(200.0, root_bounds.max.y));
        let right_bounds = Rect::from_min_max(pos2(200.0, root_bounds.min.y), root_bounds.max);
        let mut ready = ReadySurfaceScene::new(
            SURFACE,
            to_logical_rect(root_bounds).expect("fixture bounds are finite"),
        );

        publish_leaf(&workspace, &mut ready, left, split, left_bounds);
        publish_leaf(&workspace, &mut ready, right, split, right_bounds);

        for (node, bounds) in [(left, left_bounds), (right, right_bounds)] {
            let cluster = ready
                .drop_guide_clusters()
                .iter()
                .find(|cluster| cluster.id().scope == DropGuideScope::Inner(node))
                .expect("every tabs leaf publishes an inner guide");
            assert_eq!(cluster.targets().count(), 5);
            assert!(cluster.target(DropGuideSlot::Center).is_some());
            assert_edge_targets(cluster, node, bounds, false);
        }
    }

    #[test]
    fn nested_root_publishes_one_complete_four_direction_outer_cluster() {
        let (workspace, split, _, _) = split_workspace();
        let bounds = Rect::from_min_size(pos2(0.0, 0.0), vec2(400.0, 240.0));
        let mut ready = ReadySurfaceScene::new(
            SURFACE,
            to_logical_rect(bounds).expect("fixture bounds are finite"),
        );

        push_outer_guide(
            &workspace,
            SURFACE,
            ROOT,
            split,
            bounds,
            SceneLayerKey::new(1),
            &DockStyle::default(),
            DockFraction::new(0.5).expect("fixture fraction is valid"),
            &mut ready,
        )
        .expect("outer guide projects");

        let [cluster] = ready.drop_guide_clusters() else {
            panic!("split root must publish exactly one outer guide cluster");
        };
        assert_eq!(cluster.id().scope, DropGuideScope::Outer);
        assert_eq!(cluster.targets().count(), 4);
        assert!(cluster.target(DropGuideSlot::Center).is_none());
        assert_edge_targets(cluster, split, bounds, true);

        let style = DockStyle::default();
        for (edge, center) in [
            (
                Edge::Left,
                pos2(
                    bounds.min.x + style.drop_guide_outer_inset,
                    bounds.center().y,
                ),
            ),
            (
                Edge::Right,
                pos2(
                    bounds.max.x - style.drop_guide_outer_inset,
                    bounds.center().y,
                ),
            ),
            (
                Edge::Top,
                pos2(
                    bounds.center().x,
                    bounds.min.y + style.drop_guide_outer_inset,
                ),
            ),
            (
                Edge::Bottom,
                pos2(
                    bounds.center().x,
                    bounds.max.y - style.drop_guide_outer_inset,
                ),
            ),
        ] {
            assert_button_geometry(
                cluster
                    .target(DropGuideSlot::Edge(edge))
                    .expect("outer slot exists"),
                center,
                &style,
            );
        }
    }

    #[test]
    fn fixed_guide_geometry_is_omitted_atomically_when_it_cannot_fit() {
        let (single, tabs) = single_workspace();
        let tiny = Rect::from_min_size(pos2(0.0, 0.0), vec2(50.0, 50.0));
        let mut ready = ReadySurfaceScene::new(
            SURFACE,
            to_logical_rect(tiny).expect("fixture bounds are finite"),
        );
        publish_leaf(&single, &mut ready, tabs, tabs, tiny);
        assert!(ready.drop_guide_clusters().is_empty());

        let (split_graph, split, _, _) = split_workspace();
        let outer_bounds = Rect::from_min_size(pos2(0.0, 0.0), vec2(90.0, 90.0));
        let mut outer_ready = ReadySurfaceScene::new(
            SURFACE,
            to_logical_rect(outer_bounds).expect("fixture bounds are finite"),
        );
        push_outer_guide(
            &split_graph,
            SURFACE,
            ROOT,
            split,
            outer_bounds,
            SceneLayerKey::new(1),
            &DockStyle::default(),
            DockFraction::new(0.5).expect("fixture fraction is valid"),
            &mut outer_ready,
        )
        .expect("insufficient outer geometry is a deterministic omission");
        assert!(outer_ready.drop_guide_clusters().is_empty());
    }
}
