//! Tab-strip layout, optional chrome, overflow menus and pane scene records.

use std::{collections::BTreeSet, sync::Arc};

use super::guides::{
    GuideEdgeGeometry, compile_inner_targets_and_guide, compile_inner_targets_without_guide,
};
use super::{
    SceneCompilationError, checked_sum, clip_rect, intersect_rect, rect_contains, rect_has_area,
    tab_availability,
};
use crate::command::{DockFraction, Edge};
use crate::drop_target::{
    DropTargetId, DropTargetLocation, DropTargetRecord, DropVisual, SceneLayerKey,
};
use crate::geometry::{GeometryError, LogicalRect};
use crate::graph::{Node, Workspace};
use crate::hit_region::HitRegion;
use crate::ids::{NodeId, RootId, SurfaceId};
use crate::layout::{NodeMap, NodeResult};
use crate::policy::{DockPolicySnapshot, TabBarInteraction, TabBarVisibility};
use crate::presentation_config::DockPresentationConfig;
use crate::scene::{
    PaneRecord, PaneSceneId, PresentationMenuAnchorHost, PresentationMenuAnchorRecord,
    PresentationPlan, SurfaceProgramRootOperation, TabBarRecord, TabBarSceneId, TabGroupDragRecord,
    TabListMenuGeometryAvailability, TabListMenuRecord, TabListMenuRowRecord, TabRecord,
    TabSceneId, TabStripControlRecord, TabStripMemberRecord, TabStripMemberVisibility,
};
use crate::scene_compiler::tab_strip_layout::{TabStripFlow, desired_tab_width, select_menu_host};
pub(in crate::scene_compiler) use crate::scene_compiler::tab_strip_layout::{
    reveal_tab_range, tab_local_range,
};
use crate::scene_manifest::{
    AuthoritativeSurfaceMeasurements, SurfaceRequirements, TabBarRequirement, TabIntrinsicKey,
    TabStripControlMetrics, TabStripControlPlacement, TabStripKey,
};
use crate::tab_strip::{
    ActiveTabListMenu, TabStripControlId, TabStripStateKey, TabStripStateStore,
};
use crate::target_field::MagneticEdgeTargets;

#[allow(clippy::too_many_arguments)]
pub(super) fn select_presentation_menu_tab_bar(
    workspace: &Workspace,
    requirements: &SurfaceRequirements,
    measurements: AuthoritativeSurfaceMeasurements<'_>,
    config: &DockPresentationConfig,
    surface: SurfaceId,
    root: RootId,
    root_node: NodeId,
    central: Option<NodeId>,
    root_bounds: LogicalRect,
    node_results: &NodeMap<NodeResult>,
) -> Result<Option<NodeId>, SceneCompilationError> {
    let visible_menu_available = |tabs| -> Result<Option<bool>, SceneCompilationError> {
        let id = TabBarSceneId { root, tabs };
        let requirement = requirements
            .tab_bar(id)
            .ok_or(SceneCompilationError::MissingTabBarRequirement { id })?;
        if requirement.policy().visibility() == TabBarVisibility::Hidden {
            return Ok(None);
        }
        let projected = node_results
            .get(&tabs)
            .map(|result| result.rect)
            .ok_or(SceneCompilationError::MissingNodeProjection { root, node: tabs })?;
        let bounds = clip_rect(projected, root_bounds)?;
        let bar = LogicalRect::new(
            bounds.x(),
            bounds.y(),
            bounds.width(),
            config.tab_bar_height().min(bounds.height()),
        )?;
        if !rect_has_area(bar) {
            return Ok(None);
        }
        let items = match workspace.node(tabs) {
            Some(Node::Tabs { items, .. }) => items,
            Some(Node::Split { .. }) => return Ok(None),
            None => {
                return Err(SceneCompilationError::MissingNode {
                    surface,
                    root,
                    node: tabs,
                });
            }
        };
        let strip_key = TabStripKey::new(surface, id);
        let strip = measurements
            .tab_strip(strip_key)
            .ok_or(SceneCompilationError::MissingTabStripMeasurement { key: strip_key })?;
        let chrome = tab_strip_optional_chrome_layout(bar, items.len(), strip, config, true)?;
        Ok(Some(chrome.presentation_menu_bounds.is_some()))
    };

    select_menu_host(
        workspace,
        surface,
        root,
        root_node,
        central,
        visible_menu_available,
    )
}

fn tab_presentation_menu_anchor(
    bar: LogicalRect,
    has_group_grip: bool,
    requested_group_extent: f64,
    leading_reserved: f64,
    trailing_reserved: f64,
    minimum_tab_extent: f64,
    minimum_anchor_extent: f64,
) -> Result<Option<LogicalRect>, GeometryError> {
    if bar.height() < minimum_anchor_extent {
        return Ok(None);
    }
    let content_left = (bar.x() + leading_reserved).min(bar.max().x());
    let content_right = (bar.max().x() - trailing_reserved).max(content_left);
    let available = (content_right - content_left).max(0.0);
    let group_extent = if has_group_grip {
        requested_group_extent.min(available)
    } else {
        0.0
    };
    let tab_extent = if has_group_grip {
        minimum_tab_extent
    } else {
        0.0
    };
    let maximum_width = (available - group_extent - tab_extent).max(0.0);
    let width = bar.height().min(maximum_width);
    (width >= minimum_anchor_extent && minimum_anchor_extent > 0.0)
        .then(|| LogicalRect::new(content_right - width, bar.y(), width, bar.height()))
        .transpose()
}

#[derive(Debug, Clone, Copy)]
pub(in crate::scene_compiler) struct TabStripOptionalChromeLayout {
    pub(in crate::scene_compiler) group_extent: f64,
    leading_reserved_bounds: Option<LogicalRect>,
    trailing_reserved_bounds: Option<LogicalRect>,
    pub(in crate::scene_compiler) presentation_menu_bounds: Option<LogicalRect>,
    pub(in crate::scene_compiler) viewport: LogicalRect,
    pub(in crate::scene_compiler) controls: Option<TabStripControlLayout>,
}

fn tab_strip_optional_chrome_candidate(
    bar: LogicalRect,
    item_count: usize,
    strip: crate::scene_manifest::TabStripMetrics,
    config: &DockPresentationConfig,
    group_extent: f64,
    include_presentation_menu: bool,
    include_overflow_controls: bool,
) -> Result<Option<TabStripOptionalChromeLayout>, SceneCompilationError> {
    let minimum_tab_extent = if item_count > 0 {
        config.tab_min_width()
    } else {
        0.0
    };
    let reservable = (bar.width() - group_extent - minimum_tab_extent).max(0.0);
    let leading_reserved = strip.leading_reserved().min(reservable);
    let trailing_reserved = strip
        .trailing_reserved()
        .min((reservable - leading_reserved).max(0.0));
    let presentation_menu_bounds = if include_presentation_menu {
        tab_presentation_menu_anchor(
            bar,
            item_count > 0,
            group_extent,
            leading_reserved,
            trailing_reserved,
            config.tab_min_width(),
            config.tab_close_extent(),
        )?
    } else {
        None
    };
    if include_presentation_menu && presentation_menu_bounds.is_none() {
        return Ok(None);
    }

    let presentation_menu_extent = presentation_menu_bounds.map_or(0.0, LogicalRect::width);
    let content_x = bar.x() + group_extent + leading_reserved;
    let base_viewport_width = (bar.width()
        - group_extent
        - leading_reserved
        - trailing_reserved
        - presentation_menu_extent)
        .max(0.0);
    let item_count_u32 =
        u32::try_from(item_count).map_err(|_| SceneCompilationError::GeometryCountOverflow)?;
    let minimum_total = checked_sum(
        [
            config.tab_min_width() * f64::from(item_count_u32),
            strip.after_tabs_width(),
        ],
        "minimum tab flow width",
    )?;
    let overflowing = item_count > 0 && minimum_total > base_viewport_width;
    let controls = if overflowing && include_overflow_controls {
        match strip.controls().filter(|metrics| !metrics.is_empty()) {
            Some(metrics) => {
                tab_strip_control_layout(content_x, bar, base_viewport_width, metrics)?
            }
            None => None,
        }
    } else {
        None
    };
    if overflowing
        && include_overflow_controls
        && strip.controls().is_some_and(|metrics| !metrics.is_empty())
        && controls.is_none()
    {
        return Ok(None);
    }
    let reserved_leading = controls
        .as_ref()
        .map_or(0.0, |layout| layout.reserved_leading);
    let reserved_trailing = controls
        .as_ref()
        .map_or(0.0, |layout| layout.reserved_trailing);
    let viewport = LogicalRect::new(
        content_x + reserved_leading,
        bar.y(),
        (base_viewport_width - reserved_leading - reserved_trailing).max(0.0),
        bar.height(),
    )?;
    if item_count > 0 && viewport.width() < config.tab_min_width() {
        return Ok(None);
    }
    Ok(Some(TabStripOptionalChromeLayout {
        group_extent,
        leading_reserved_bounds: (leading_reserved > 0.0)
            .then(|| {
                LogicalRect::new(
                    bar.x() + group_extent,
                    bar.y(),
                    leading_reserved,
                    bar.height(),
                )
            })
            .transpose()?,
        trailing_reserved_bounds: (trailing_reserved > 0.0)
            .then(|| {
                LogicalRect::new(
                    bar.max().x() - trailing_reserved,
                    bar.y(),
                    trailing_reserved,
                    bar.height(),
                )
            })
            .transpose()?,
        presentation_menu_bounds,
        viewport,
        controls,
    }))
}

pub(in crate::scene_compiler) fn tab_strip_optional_chrome_layout(
    bar: LogicalRect,
    item_count: usize,
    strip: crate::scene_manifest::TabStripMetrics,
    config: &DockPresentationConfig,
    include_presentation_menu: bool,
) -> Result<TabStripOptionalChromeLayout, SceneCompilationError> {
    let group_extent = if item_count == 0 {
        0.0
    } else {
        config.tab_group_grip_extent().min(bar.height())
    };
    let (candidates, candidate_count) = if include_presentation_menu {
        (
            [
                (group_extent, true, true),
                (group_extent, false, true),
                (group_extent, false, false),
                (0.0, true, true),
                (0.0, false, true),
                (0.0, true, false),
                (0.0, false, false),
            ],
            7,
        )
    } else {
        (
            [
                (group_extent, false, true),
                (group_extent, false, false),
                (0.0, false, true),
                (0.0, false, false),
                (0.0, false, false),
                (0.0, false, false),
                (0.0, false, false),
            ],
            4,
        )
    };
    for (group_extent, menu, include_overflow_controls) in
        candidates.into_iter().take(candidate_count)
    {
        if let Some(layout) = tab_strip_optional_chrome_candidate(
            bar,
            item_count,
            strip,
            config,
            group_extent,
            menu,
            include_overflow_controls,
        )? {
            return Ok(layout);
        }
    }
    Ok(TabStripOptionalChromeLayout {
        group_extent: 0.0,
        leading_reserved_bounds: None,
        trailing_reserved_bounds: None,
        presentation_menu_bounds: None,
        viewport: LogicalRect::new(bar.x(), bar.y(), 0.0, bar.height())?,
        controls: None,
    })
}

#[allow(clippy::too_many_arguments)]
pub(super) fn compile_tabs_leaf(
    policy: &DockPolicySnapshot,
    config: &DockPresentationConfig,
    requirements: &SurfaceRequirements,
    measurements: AuthoritativeSurfaceMeasurements<'_>,
    surface: SurfaceId,
    root: RootId,
    tabs: NodeId,
    depth: usize,
    bounds: LogicalRect,
    items: &[crate::ids::ItemId],
    selected: Option<crate::ids::ItemId>,
    layer: SceneLayerKey,
    fraction: DockFraction,
    tab_strip_states: &TabStripStateStore,
    root_central: bool,
    presentation_menu_anchor: bool,
    outer_guides: Option<GuideEdgeGeometry>,
    ready: &mut PresentationPlan,
) -> Result<(), SceneCompilationError> {
    let bar_id = TabBarSceneId { root, tabs };
    let tab_bar_requirement = requirements
        .tab_bar(bar_id)
        .ok_or(SceneCompilationError::MissingTabBarRequirement { id: bar_id })?;
    let tab_height = if tab_bar_requirement.policy().visibility() == TabBarVisibility::Visible {
        config.tab_bar_height().min(bounds.height())
    } else {
        0.0
    };
    let tab_bar = LogicalRect::new(bounds.x(), bounds.y(), bounds.width(), tab_height)?;
    let content_height = (bounds.height() - tab_height).max(0.0);
    let content = LogicalRect::new(
        bounds.x(),
        bounds.y() + tab_height,
        bounds.width(),
        content_height,
    )?;
    let pane_id = PaneSceneId { root, tabs };
    ready.push_pane_record(PaneRecord::new(pane_id, bounds, content, layer));
    ready.push_surface_program_pane_operation(pane_id, selected)?;
    if rect_has_area(tab_bar) {
        compile_tab_strip(
            policy,
            config,
            tab_bar_requirement,
            measurements,
            surface,
            root,
            tabs,
            tab_bar,
            items,
            selected,
            layer,
            tab_strip_states,
            presentation_menu_anchor,
            ready,
        )?;
    }
    if rect_has_area(bounds) {
        let inner_guide = if let Some(outer_guides) = outer_guides {
            compile_inner_targets_and_guide(
                policy,
                config,
                outer_guides,
                surface,
                root,
                tabs,
                bounds,
                content,
                layer,
                fraction,
                root_central,
                ready,
            )?
        } else {
            None
        };
        if let Some(inner_guide) = inner_guide {
            ready.push_drop_guide_cluster(inner_guide);
        } else {
            compile_inner_targets_without_guide(
                policy,
                config,
                surface,
                root,
                tabs,
                bounds,
                content,
                layer,
                fraction,
                root_central,
                ready,
            )?;
        }
        let center = DropTargetId::Center {
            surface,
            root,
            tabs,
        };
        let edge_target = |edge| {
            if root_central {
                DropTargetId::OuterEdge {
                    surface,
                    root,
                    node: tabs,
                    edge,
                }
            } else {
                DropTargetId::InnerEdge {
                    surface,
                    root,
                    node: tabs,
                    edge,
                }
            }
        };
        ready.push_magnetic_pane_field(
            root,
            tabs,
            layer,
            depth,
            content,
            center,
            MagneticEdgeTargets::new(
                edge_target(Edge::Left),
                edge_target(Edge::Right),
                edge_target(Edge::Top),
                edge_target(Edge::Bottom),
            ),
            config,
        )?;
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn compile_tab_strip(
    policy: &DockPolicySnapshot,
    config: &DockPresentationConfig,
    requirement: &TabBarRequirement,
    measurements: AuthoritativeSurfaceMeasurements<'_>,
    surface: SurfaceId,
    root: RootId,
    tabs: NodeId,
    bar: LogicalRect,
    items: &[crate::ids::ItemId],
    selected: Option<crate::ids::ItemId>,
    layer: SceneLayerKey,
    tab_strip_states: &TabStripStateStore,
    presentation_menu_anchor: bool,
    ready: &mut PresentationPlan,
) -> Result<(), SceneCompilationError> {
    let interaction = requirement.policy().interaction();
    let bar_id = TabBarSceneId { root, tabs };
    let state_key = TabStripStateKey::new(surface, bar_id);
    let state = tab_strip_states.state(state_key);
    let active_menu = tab_strip_states.active_menu_for(state_key);
    let strip_key = TabStripKey::new(surface, bar_id);
    let strip = measurements
        .tab_strip(strip_key)
        .ok_or(SceneCompilationError::MissingTabStripMeasurement { key: strip_key })?;
    let chrome = tab_strip_optional_chrome_layout(
        bar,
        items.len(),
        strip,
        config,
        presentation_menu_anchor,
    )?;
    let presentation_menu_bounds = chrome.presentation_menu_bounds;
    let group_extent = chrome.group_extent;
    let group_grip_bounds = if group_extent > 0.0 {
        let grip = LogicalRect::new(bar.x(), bar.y(), group_extent, bar.height())?;
        Some(grip)
    } else {
        None
    };
    let mut desired_widths = Vec::with_capacity(items.len());
    let mut content_widths = Vec::with_capacity(items.len());
    let mut close_allowed = Vec::with_capacity(items.len());
    for item in items.iter().copied() {
        let key = TabIntrinsicKey::new(surface, TabSceneId { root, tabs, item });
        let intrinsic = measurements
            .tab_intrinsic(key)
            .ok_or(SceneCompilationError::MissingTabIntrinsicMeasurement { key })?;
        let item_close_allowed = requirement
            .close_capability(item)
            .ok_or(SceneCompilationError::MissingTabCloseCapability {
                id: TabSceneId { root, tabs, item },
            })?
            .allows_close();
        content_widths.push(intrinsic.content_width());
        desired_widths.push(desired_tab_width(
            intrinsic.content_width(),
            item_close_allowed,
            config,
        ));
        close_allowed.push(item_close_allowed);
    }

    let viewport = chrome.viewport;
    let viewport_width = viewport.width();
    let control_rects = chrome.controls.map(|layout| layout.rects);
    let menu_geometry = if control_rects
        .as_ref()
        .is_some_and(|rects| rects[2].is_some())
        && strip
            .tab_list_menu()
            .is_some_and(crate::scene_manifest::TabListMenuMetrics::can_allocate)
    {
        TabListMenuGeometryAvailability::Available
    } else {
        TabListMenuGeometryAvailability::Unavailable
    };
    let flow = TabStripFlow::allocate(
        &desired_widths,
        viewport_width,
        strip.after_tabs_width(),
        config.tab_min_width(),
    )?;
    let max_scroll = flow.maximum_scroll_offset(viewport_width);
    let selected_index =
        selected.and_then(|selected| items.iter().position(|item| *item == selected));
    let requested_scroll = state.and_then(|state| state.scroll_offset()).unwrap_or(0.0);
    let selection_changed = state.is_none_or(|state| state.resolved_selection() != selected);
    let viewport_shrank = state
        .and_then(|state| state.resolved_viewport_width())
        .is_some_and(|previous| viewport_width < previous);
    let scroll = flow.scroll_offset(
        requested_scroll,
        viewport_width,
        selected_index,
        selection_changed || viewport_shrank,
    );

    let mut full_tabs = Vec::with_capacity(items.len());
    let mut program_tabs = Vec::with_capacity(items.len());
    let mut cursor = viewport.x() - scroll;
    let mut visible_items = BTreeSet::new();
    let mut members = Vec::with_capacity(items.len());
    for (ordinal, ((item, width), close_allowed)) in items
        .iter()
        .copied()
        .zip(flow.widths.iter().copied())
        .zip(close_allowed.iter().copied())
        .enumerate()
    {
        let full = LogicalRect::new(cursor, bar.y(), width, bar.height())?;
        let visible_intersection = intersect_rect(full, viewport)?;
        let visibility = match visible_intersection {
            None => TabStripMemberVisibility::Hidden,
            Some(visible) if visible == full => TabStripMemberVisibility::Visible,
            Some(_) => TabStripMemberVisibility::PartiallyVisible,
        };
        members.push(TabStripMemberRecord::new(
            TabSceneId { root, tabs, item },
            ordinal,
            width,
            full,
            visibility,
        ));
        if let Some(chrome) = operable_tab_chrome(full, viewport, close_allowed, config)? {
            let id = TabSceneId { root, tabs, item };
            let drag = if interaction == TabBarInteraction::Enabled {
                chrome.drag
            } else {
                LogicalRect::new(chrome.visible.x(), chrome.visible.y(), 0.0, 0.0)?
            };
            ready.push_tab_record(TabRecord::new(
                id,
                full,
                chrome.visible,
                chrome.text_layout,
                chrome.text,
                HitRegion::new(drag),
                chrome.close,
                if interaction == TabBarInteraction::Enabled {
                    chrome.close
                } else {
                    None
                },
                ordinal,
                layer,
            ));
            program_tabs.push(id);
            visible_items.insert(item);
        }
        full_tabs.push(full);
        cursor += width;
    }
    let after_tabs_bounds = (flow.after_tabs_width > 0.0)
        .then(|| LogicalRect::new(cursor, bar.y(), flow.after_tabs_width, bar.height()))
        .transpose()?;
    cursor += flow.after_tabs_width;
    let trailing_empty_bounds = if max_scroll == 0.0 {
        let trailing_start = cursor.min(viewport.max().x());
        let trailing_width = (viewport.max().x() - trailing_start).max(0.0);
        (trailing_width > 0.0)
            .then(|| LogicalRect::new(trailing_start, bar.y(), trailing_width, bar.height()))
            .transpose()?
    } else {
        None
    };
    let group_drag = if interaction == TabBarInteraction::Enabled && !items.is_empty() {
        TabGroupDragRecord::new(group_grip_bounds, trailing_empty_bounds)
    } else {
        None
    };
    ready.push_tab_bar_record(
        TabBarRecord::new(
            bar_id,
            bar,
            viewport,
            scroll,
            max_scroll,
            items
                .iter()
                .copied()
                .filter(|item| !visible_items.contains(item))
                .collect(),
            members,
            group_grip_bounds,
            group_drag,
            interaction,
            menu_geometry,
            layer,
        )
        .with_application_bounds(
            chrome.leading_reserved_bounds,
            chrome.trailing_reserved_bounds,
            after_tabs_bounds,
        ),
    );
    if presentation_menu_anchor {
        let host = PresentationMenuAnchorHost::TabBar(bar_id);
        ready.push_presentation_menu_anchor_record(match presentation_menu_bounds {
            Some(bounds) => PresentationMenuAnchorRecord::ready(root, host, bounds, layer),
            None => PresentationMenuAnchorRecord::compacted(root, host, layer),
        });
    }

    let mut program_controls = Vec::new();
    if let Some([backward, forward, menu]) = control_rects {
        let enabled = interaction == TabBarInteraction::Enabled;
        if let Some(backward) = backward {
            let id = TabStripControlId::ScrollBackward(bar_id);
            ready.push_tab_strip_control_record(TabStripControlRecord::new(
                id,
                backward,
                enabled && scroll > 0.0,
                layer,
            ));
            program_controls.push(id);
        }
        if let Some(forward) = forward {
            let id = TabStripControlId::ScrollForward(bar_id);
            ready.push_tab_strip_control_record(TabStripControlRecord::new(
                id,
                forward,
                enabled && scroll < max_scroll,
                layer,
            ));
            program_controls.push(id);
        }
        if let Some(menu) = menu {
            let menu_enabled = enabled && menu_geometry.is_available();
            let id = TabStripControlId::TabListMenu(bar_id);
            ready.push_tab_strip_control_record(TabStripControlRecord::new(
                id,
                menu,
                menu_enabled,
                layer,
            ));
            program_controls.push(id);
        }
        if interaction == TabBarInteraction::Enabled
            && let Some(active_menu) = active_menu
            && let Some(menu) = menu
            && let Some(menu_metrics) = strip
                .tab_list_menu()
                .filter(|metrics| metrics.can_allocate())
        {
            compile_tab_list_menu(
                surface,
                bar_id,
                menu,
                layer,
                items,
                selected,
                &content_widths,
                menu_metrics,
                active_menu,
                ready,
            )?;
        }
    }

    ready.push_surface_program_root_operation(SurfaceProgramRootOperation::TabBar(bar_id))?;
    for tab in program_tabs {
        ready.push_surface_program_root_operation(SurfaceProgramRootOperation::Tab(tab))?;
    }
    for control in program_controls {
        ready.push_surface_program_root_operation(SurfaceProgramRootOperation::TabStripControl(
            control,
        ))?;
    }
    if presentation_menu_anchor && presentation_menu_bounds.is_some() {
        ready.push_surface_program_root_operation(
            SurfaceProgramRootOperation::PresentationMenuAnchor(root),
        )?;
    }

    if interaction == TabBarInteraction::Enabled {
        let availability = tab_availability(policy);
        let gap_viewport = if let Some(after_tabs) = after_tabs_bounds {
            LogicalRect::new(
                viewport.x(),
                viewport.y(),
                (after_tabs.x() - viewport.x()).clamp(0.0, viewport.width()),
                viewport.height(),
            )?
        } else {
            viewport
        };
        for gap in tab_gap_geometries(gap_viewport, &full_tabs, config.splitter_thickness())? {
            ready.push_drop_target(Arc::new(DropTargetRecord::new(
                DropTargetLocation::TabGap {
                    surface,
                    root,
                    tabs,
                    index: gap.index,
                },
                availability,
                HitRegion::new(gap.hit),
                layer,
                DropVisual::new(gap.visual),
            )));
        }
    }
    Ok(())
}

#[derive(Debug, Clone, Copy)]
pub(in crate::scene_compiler) struct TabStripControlLayout {
    pub(in crate::scene_compiler) rects: [Option<LogicalRect>; 3],
    pub(in crate::scene_compiler) reserved_leading: f64,
    pub(in crate::scene_compiler) reserved_trailing: f64,
}

pub(in crate::scene_compiler) fn tab_strip_control_layout(
    content_x: f64,
    bar: LogicalRect,
    available: f64,
    metrics: TabStripControlMetrics,
) -> Result<Option<TabStripControlLayout>, GeometryError> {
    let entries = [
        metrics.scroll_backward(),
        metrics.scroll_forward(),
        metrics.tab_list_menu(),
    ];
    let group_extent = |placement| {
        let controls = entries
            .iter()
            .flatten()
            .copied()
            .filter(|metric| metric.can_allocate() && metric.placement() == placement)
            .collect::<Vec<_>>();
        controls.iter().map(|metric| metric.extent()).sum::<f64>()
            + metrics.spacing()
                * f64::from(u32::try_from(controls.len().saturating_sub(1)).unwrap_or(u32::MAX))
    };
    let reserved_leading = group_extent(TabStripControlPlacement::ReservedLeading);
    let reserved_trailing = group_extent(TabStripControlPlacement::ReservedTrailing);
    if reserved_leading + reserved_trailing >= available {
        return Ok(None);
    }
    let viewport_x = content_x + reserved_leading;
    let viewport_width = available - reserved_leading - reserved_trailing;
    let viewport_right = viewport_x + viewport_width;
    let overlay_fits = group_extent(TabStripControlPlacement::OverlayLeading)
        + group_extent(TabStripControlPlacement::OverlayTrailing)
        <= viewport_width;
    let mut rects = [None; 3];

    for placement in [
        TabStripControlPlacement::ReservedLeading,
        TabStripControlPlacement::OverlayLeading,
    ] {
        if placement == TabStripControlPlacement::OverlayLeading && !overlay_fits {
            continue;
        }
        let mut cursor = if placement == TabStripControlPlacement::ReservedLeading {
            content_x
        } else {
            viewport_x
        };
        let total = group_extent(placement);
        if total > viewport_width && placement == TabStripControlPlacement::OverlayLeading {
            continue;
        }
        for (index, metric) in entries.iter().copied().enumerate() {
            let Some(metric) =
                metric.filter(|metric| metric.can_allocate() && metric.placement() == placement)
            else {
                continue;
            };
            rects[index] = Some(LogicalRect::new(
                cursor,
                bar.y(),
                metric.extent(),
                bar.height(),
            )?);
            cursor += metric.extent() + metrics.spacing();
        }
    }

    for placement in [
        TabStripControlPlacement::ReservedTrailing,
        TabStripControlPlacement::OverlayTrailing,
    ] {
        if placement == TabStripControlPlacement::OverlayTrailing && !overlay_fits {
            continue;
        }
        let mut cursor = if placement == TabStripControlPlacement::ReservedTrailing {
            content_x + available
        } else {
            viewport_right
        };
        let total = group_extent(placement);
        if total > viewport_width && placement == TabStripControlPlacement::OverlayTrailing {
            continue;
        }
        for (index, metric) in entries.iter().copied().enumerate().rev() {
            let Some(metric) =
                metric.filter(|metric| metric.can_allocate() && metric.placement() == placement)
            else {
                continue;
            };
            cursor -= metric.extent();
            rects[index] = Some(LogicalRect::new(
                cursor,
                bar.y(),
                metric.extent(),
                bar.height(),
            )?);
            cursor -= metrics.spacing();
        }
    }

    Ok(rects
        .iter()
        .any(Option::is_some)
        .then_some(TabStripControlLayout {
            rects,
            reserved_leading,
            reserved_trailing,
        }))
}

#[allow(clippy::too_many_arguments)]
fn compile_tab_list_menu(
    surface: SurfaceId,
    bar: TabBarSceneId,
    anchor: LogicalRect,
    layer: SceneLayerKey,
    items: &[crate::ids::ItemId],
    selected: Option<crate::ids::ItemId>,
    content_widths: &[f64],
    metrics: crate::scene_manifest::TabListMenuMetrics,
    active_menu: &ActiveTabListMenu,
    ready: &mut PresentationPlan,
) -> Result<(), SceneCompilationError> {
    if items.is_empty() || content_widths.len() != items.len() {
        return Ok(());
    }
    let key = TabStripStateKey::new(surface, bar);
    if active_menu.key() != key || active_menu.items() != items {
        return Err(SceneCompilationError::TabListMenuRosterMismatch { key });
    }
    let bounds = ready
        .popup_plane_bounds()
        .ok_or(SceneCompilationError::PopupPlaneStateMismatch)?;
    if !rect_contains(bounds, anchor) {
        return Err(SceneCompilationError::TabListMenuAnchorOutsidePopupPlane { key });
    }
    let row_gap_count = u32::try_from(items.len().saturating_sub(1))
        .map_err(|_| SceneCompilationError::GeometryCountOverflow)?;
    let row_heights = checked_sum(
        std::iter::repeat_n(metrics.row_height(), items.len()),
        "tab-list row heights",
    )?;
    let row_spacing = checked_sum(
        [metrics.row_spacing() * f64::from(row_gap_count)],
        "tab-list row spacing",
    )?;
    let rows_height = checked_sum([row_heights, row_spacing], "tab-list rows height")?;
    let vertical_padding = checked_sum(
        [metrics.vertical_padding(), metrics.vertical_padding()],
        "tab-list vertical padding",
    )?;
    let natural_height = checked_sum([rows_height, vertical_padding], "tab-list natural height")?;
    let desired_popup_height = natural_height.min(metrics.maximum_height());
    let space_below = (bounds.max().y() - anchor.max().y()).max(0.0);
    let space_above = (anchor.y() - bounds.y()).max(0.0);
    let (opens_below, available_height) = if desired_popup_height <= space_below {
        (true, space_below)
    } else if desired_popup_height <= space_above {
        (false, space_above)
    } else if space_below >= space_above {
        (true, space_below)
    } else {
        (false, space_above)
    };
    let popup_height = desired_popup_height.min(available_height);
    if popup_height <= 0.0 {
        return Err(SceneCompilationError::TabListMenuPopupSpaceUnavailable { key });
    }
    let needs_scrollbar = natural_height > popup_height;
    let horizontal_padding = checked_sum(
        [metrics.horizontal_padding(), metrics.horizontal_padding()],
        "tab-list horizontal padding",
    )?;
    let content_width = checked_sum(
        [
            content_widths.iter().copied().fold(0.0, f64::max),
            horizontal_padding,
        ],
        "tab-list content width",
    )?;
    let popup_width = checked_sum(
        [
            content_width,
            if needs_scrollbar {
                metrics.scrollbar_extent()
            } else {
                0.0
            },
        ],
        "tab-list popup width",
    )?
    .max(anchor.width())
    .min(bounds.width());
    if popup_width <= 0.0 {
        return Ok(());
    }
    let x = (anchor.max().x() - popup_width).clamp(bounds.x(), bounds.max().x() - popup_width);
    let y = if opens_below {
        anchor.max().y()
    } else {
        anchor.y() - popup_height
    };
    let popup = LogicalRect::new(x, y, popup_width, popup_height)?;
    let viewport_width = (popup.width()
        - horizontal_padding
        - if needs_scrollbar {
            metrics.scrollbar_extent()
        } else {
            0.0
        })
    .max(0.0);
    let viewport_height = (popup.height() - vertical_padding).max(0.0);
    if viewport_width <= 0.0 || viewport_height <= 0.0 {
        return Ok(());
    }
    let viewport = LogicalRect::new(
        popup.x() + metrics.horizontal_padding(),
        popup.y() + metrics.vertical_padding(),
        viewport_width,
        viewport_height,
    )?;
    let maximum_scroll = (rows_height - viewport.height()).max(0.0);
    let scroll = active_menu.scroll_offset().clamp(0.0, maximum_scroll);
    let mut cursor = viewport.y() - scroll;
    let mut rows = Vec::with_capacity(items.len());
    for (ordinal, item) in items.iter().copied().enumerate() {
        let row = LogicalRect::new(viewport.x(), cursor, viewport.width(), metrics.row_height())?;
        let hit = intersect_rect(row, viewport)?.map(HitRegion::new);
        rows.push(TabListMenuRowRecord::new(
            TabSceneId {
                root: bar.root,
                tabs: bar.tabs,
                item,
            },
            ordinal,
            row,
            hit,
            selected == Some(item),
            active_menu.focus() == item,
        ));
        cursor += metrics.row_height() + metrics.row_spacing();
    }
    ready.push_tab_list_menu_record(TabListMenuRecord::new(
        active_menu.session(),
        bar,
        popup,
        viewport,
        scroll,
        maximum_scroll,
        rows,
        layer,
    ));
    Ok(())
}

struct TabChromeGeometry {
    visible: LogicalRect,
    text_layout: LogicalRect,
    text: LogicalRect,
    drag: LogicalRect,
    close: Option<LogicalRect>,
}

fn operable_tab_chrome(
    full: LogicalRect,
    viewport: LogicalRect,
    close_allowed: bool,
    config: &DockPresentationConfig,
) -> Result<Option<TabChromeGeometry>, GeometryError> {
    let Some(visible) = intersect_rect(full, viewport)? else {
        return Ok(None);
    };
    let operable = config
        .tab_close_extent()
        .min(config.tab_min_width())
        .min(full.width());
    if visible.width() < operable {
        return Ok(None);
    }
    let allocated_close = if close_allowed {
        let size = config
            .tab_close_extent()
            .min(full.height())
            .min(full.width());
        if size <= 0.0 {
            return Ok(None);
        }
        let padding = config
            .tab_horizontal_padding()
            .min((full.width() - size).max(0.0));
        let close = LogicalRect::new(
            full.max().x() - padding - size,
            full.y() + (full.height() - size) * 0.5,
            size,
            size,
        )?;
        Some(close)
    } else {
        None
    };
    let close = if let Some(close) = allocated_close {
        if rect_contains(visible, close) {
            Some(close)
        } else if visible.max().x() < full.max().x() {
            None
        } else {
            // A left-clipped tail must retain complete close chrome before
            // it can expose selection or drag input beneath that control.
            return Ok(None);
        }
    } else {
        None
    };
    let drag = if let Some(close) = close {
        LogicalRect::new(
            visible.x(),
            visible.y(),
            (close.x() - visible.x()).max(0.0),
            visible.height(),
        )?
    } else {
        visible
    };
    if drag.width() < operable || !rect_has_area(drag) {
        return Ok(None);
    }
    let padding = config.tab_horizontal_padding().min(full.width());
    let text_left = (full.x() + padding).min(full.max().x());
    let text_right = allocated_close.map_or(full.max().x() - padding, |close| close.x() - padding);
    let text_layout = LogicalRect::new(
        text_left,
        full.y(),
        text_right.clamp(text_left, full.max().x()) - text_left,
        full.height(),
    )?;
    let text = clip_rect(text_layout, visible)?;
    Ok(Some(TabChromeGeometry {
        visible,
        text_layout,
        text,
        drag,
        close,
    }))
}

struct TabGapGeometry {
    index: usize,
    hit: LogicalRect,
    visual: LogicalRect,
}

fn tab_gap_geometries(
    viewport: LogicalRect,
    tabs: &[LogicalRect],
    marker_width: f64,
) -> Result<Vec<TabGapGeometry>, GeometryError> {
    if tabs.is_empty() {
        return Ok(Vec::new());
    }
    let centers = tabs
        .iter()
        .map(|tab| tab.x() + tab.width() * 0.5)
        .collect::<Vec<_>>();
    let mut gaps = Vec::with_capacity(tabs.len() + 1);
    for index in 0..=tabs.len() {
        let left = index
            .checked_sub(1)
            .map_or(viewport.x(), |previous| centers[previous]);
        let right = centers.get(index).copied().unwrap_or(viewport.max().x());
        let hit_left = left.max(viewport.x());
        let hit_right = right.min(viewport.max().x());
        if hit_right <= hit_left {
            continue;
        }
        let hit = LogicalRect::new(
            hit_left,
            viewport.y(),
            hit_right - hit_left,
            viewport.height(),
        )?;
        let marker_x = tabs.get(index).map_or(viewport.max().x(), |tab| tab.x());
        let width = marker_width.min(viewport.width());
        if width <= 0.0 {
            continue;
        }
        let marker_min = (marker_x - width * 0.5)
            .clamp(viewport.x(), (viewport.max().x() - width).max(viewport.x()));
        let visual = LogicalRect::new(marker_min, viewport.y(), width, viewport.height())?;
        gaps.push(TabGapGeometry { index, hit, visual });
    }
    Ok(gaps)
}
