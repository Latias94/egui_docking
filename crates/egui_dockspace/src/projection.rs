//! Deterministic projection shared by egui painting and core scene publication.

use std::collections::BTreeMap;
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
use dockspace::ids::{FloatingPresentationId, ItemId, NodeId, RootId, SurfaceId};
use dockspace::layout::{
    LayoutError, LayoutMetrics, LayoutMetricsError, SplitWeightOverride,
    project_root_with_overrides,
};
use dockspace::scene::{
    NodeSceneId, ReadySurfaceScene, SemanticRect, SplitterSceneId, TabBarSceneId, TabSceneId,
};
use egui::{FontSelection, Galley, Rect, TextStyle, TextWrapMode, Ui, Vec2, pos2, vec2};
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
    pub(crate) root: RootId,
    pub(crate) node: NodeId,
    pub(crate) node_rect: Rect,
    pub(crate) tab_bar_rect: Rect,
    pub(crate) group_drag_rect: Rect,
    pub(crate) content_rect: Rect,
    pub(crate) selected: Option<ItemId>,
    pub(crate) tabs: Vec<TabPlan>,
}

#[derive(Clone)]
pub(crate) struct TabPlan {
    pub(crate) item: ItemId,
    pub(crate) rect: Rect,
    pub(crate) text_rect: Rect,
    pub(crate) close_rect: Option<Rect>,
    pub(crate) title: String,
    pub(crate) galley: Arc<Galley>,
    pub(crate) selected: bool,
    pub(crate) missing: bool,
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

#[allow(
    clippy::too_many_lines,
    reason = "surface orchestration keeps the ready scene and paint plan on one geometry path"
)]
pub(crate) fn build_surface_plan(
    ui: &Ui,
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

    Ok(SurfacePlan {
        surface,
        bounds,
        roots,
        ready,
        fingerprint: ProjectionFingerprint {
            surface,
            bounds,
            regions: fingerprint_regions,
        },
        missing_items,
        contained_placements,
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
            .all(|tab| tab.close_rect.is_some());
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
        measured.push((*item, title, galley, missing, closeable, desired));
    }
    let widths = allocate_tab_widths(
        measured.iter().map(|entry| entry.5),
        tab_bar_rect.width() - group_drag_rect.width(),
        style.tab_min_width,
    );
    let mut tabs = Vec::with_capacity(measured.len());
    let mut cursor = group_drag_rect.max.x;
    for ((item, title, galley, missing, closeable, _), width) in measured.into_iter().zip(widths) {
        let rect = Rect::from_min_max(
            pos2(cursor, tab_bar_rect.min.y),
            pos2((cursor + width).min(tab_bar_rect.max.x), tab_bar_rect.max.y),
        );
        cursor = rect.max.x;
        let close_rect = closeable
            .then(|| {
                let size = style.tab_close_size.min(rect.height()).min(rect.width());
                let padding = style
                    .tab_horizontal_padding
                    .min((rect.width() - size).max(0.0));
                intersect_rect(
                    Rect::from_center_size(
                        pos2(rect.max.x - padding - size * 0.5, rect.center().y),
                        Vec2::splat(size),
                    ),
                    rect,
                )
            })
            .filter(Rect::is_positive);
        let left_padding = style.tab_horizontal_padding.min(rect.width());
        let text_left = (rect.min.x + left_padding).min(rect.max.x);
        let text_right = close_rect
            .map_or(rect.max.x - left_padding, |close| {
                close.min.x - left_padding
            })
            .clamp(text_left, rect.max.x);
        let text_rect =
            Rect::from_min_max(pos2(text_left, rect.min.y), pos2(text_right, rect.max.y));
        ready.push_tab(SemanticRect::new(
            TabSceneId {
                root,
                tabs: node,
                item,
            },
            to_logical_rect(rect)?,
            layer,
        ));
        fingerprint_regions.push(ProjectionRegion {
            id: ProjectionRegionId::Tab {
                root,
                tabs: node,
                item,
            },
            rect,
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
        tabs.push(TabPlan {
            item,
            rect,
            text_rect,
            close_rect,
            title,
            galley,
            selected: selected == Some(item),
            missing,
        });
    }

    if group_drag_rect.is_positive() {
        fingerprint_regions.push(ProjectionRegion {
            id: ProjectionRegionId::TabGroup { root, tabs: node },
            rect: group_drag_rect,
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
        tab_bar_rect,
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
        root,
        node,
        node_rect,
        tab_bar_rect,
        group_drag_rect,
        content_rect,
        selected,
        tabs,
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
        return vec![available / count; desired.len()];
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
    let centers = tabs
        .iter()
        .map(|tab| tab.rect.center().x)
        .collect::<Vec<_>>();
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
        let region = Rect::from_min_max(pos2(left, bar.min.y), pos2(right, bar.max.y));
        let marker_x = if index == tabs.len() {
            bar.max.x
        } else {
            tabs[index].rect.min.x
        };
        let marker_width = style.splitter_thickness.min(bar.width());
        let marker_min = (marker_x - marker_width * 0.5)
            .clamp(bar.min.x, (bar.max.x - marker_width).max(bar.min.x));
        let visual = Rect::from_min_size(
            pos2(marker_min, bar.min.y),
            vec2(marker_width, bar.height()),
        );
        if region.is_positive() && visual.is_positive() {
            ready.push_drop_target(DropTargetRecord::new(
                DropTargetId::TabGap {
                    surface,
                    root,
                    tabs: node,
                    index,
                },
                DockTarget::TabGap {
                    target: target.clone(),
                    index,
                },
                DropTargetAvailability::Available,
                HitRegion::new(to_logical_rect(region)?),
                layer,
                DropVisual::new(to_logical_rect(visual)?),
            ));
        }
    }
    Ok(())
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
            [50.0, 50.0]
        );
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
