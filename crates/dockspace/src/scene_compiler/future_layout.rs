//! Source-aware projection of a transaction candidate from retained measurements.

use std::collections::{BTreeMap, BTreeSet};

use thiserror::Error;

use super::surface::{pane_outer_minimum, tab_strip_optional_chrome_layout};
use super::tab_strip_layout::{TabStripFlow, desired_tab_width, select_menu_host};
use crate::RootPresentationOwner;
use crate::command::{MovePayload, TabTarget};
use crate::error::CommandError;
use crate::geometry::{GeometryError, LogicalRect, LogicalSize};
use crate::graph::{Node, Workspace};
use crate::ids::{ItemId, NodeId, RootId};
use crate::layout::{
    LayoutError, LayoutMetrics, LayoutMetricsError, LayoutProjection, measure_root_minimums,
};
use crate::policy::{DockPolicySnapshot, DockTabBarPolicyRequest, TabBarVisibility};
use crate::scene::{PresentationLayoutFacts, PresentationPlan, TabBarSceneId, TabSceneId};
use crate::scene_manifest::{PaneMinimumKey, TabIntrinsicKey, TabStripKey};

#[cfg(test)]
mod tests;

/// Reprojects one future root using only measurements retained by presented outputs.
pub(crate) fn project_future_root(
    original: &Workspace,
    workspace: &Workspace,
    policy: &DockPolicySnapshot,
    target_plan: &PresentationPlan,
    source_layout_facts: Option<&PresentationLayoutFacts>,
    root: RootId,
) -> Result<Option<LayoutProjection>, FutureLayoutProjectionError> {
    let Some(target_facts) = target_plan.layout_facts() else {
        return Ok(None);
    };
    let target_root = target_facts
        .root(root)
        .ok_or(FutureLayoutProjectionError::TargetRootNotPresented { root })?;
    let mut pane_minimums = BTreeMap::<PaneMinimumKey, LogicalSize>::new();
    collect_pane_minimums(target_facts, &mut pane_minimums)?;
    if let Some(source_facts) = source_layout_facts {
        if source_facts.config() != target_facts.config() {
            return Err(FutureLayoutProjectionError::PresentationConfigMismatch);
        }
        collect_pane_minimums(source_facts, &mut pane_minimums)?;
    }

    let surface = match workspace.presentation_for_root(root) {
        Some(RootPresentationOwner::Main { surface })
        | Some(RootPresentationOwner::Contained { surface, .. }) => surface,
        None => return Err(FutureLayoutProjectionError::TargetRootNotPresented { root }),
    };
    let root_record = workspace
        .root(root)
        .ok_or(FutureLayoutProjectionError::MissingRoot { root })?;
    let root_node = root_record.node;
    let central = root_record.central;
    let mut leaf_minimums = BTreeMap::new();
    let mut pending = vec![root_node];
    let mut visited = BTreeSet::new();
    while let Some(node) = pending.pop() {
        if !visited.insert(node) {
            return Err(FutureLayoutProjectionError::RepeatedNode { root, node });
        }
        match workspace
            .node(node)
            .ok_or(FutureLayoutProjectionError::MissingNode { root, node })?
        {
            Node::Tabs { selected, .. } => {
                let measured = match selected {
                    Some(item) => {
                        candidate_pane_minimum(original, &pane_minimums, root, node, *item)?
                    }
                    None => LogicalSize::new(0.0, 0.0)?,
                };
                let rule = workspace.pane_local_target_rule(root, node, central == Some(node))?;
                let tab_bar =
                    policy.tab_bar_policy(DockTabBarPolicyRequest::new(surface, Some(rule)));
                leaf_minimums.insert(
                    node,
                    pane_outer_minimum(measured, target_facts.config(), tab_bar.visibility())?,
                );
            }
            Node::Split { children, .. } => {
                pending.extend(children.iter().rev().copied());
            }
        }
    }

    let measured = measure_root_minimums(
        workspace,
        root,
        &leaf_minimums,
        LayoutMetrics::new(target_facts.config().splitter_thickness())?,
    )?;
    measured
        .project(target_root.bounds(), &[])
        .map(Some)
        .map_err(FutureLayoutProjectionError::Layout)
}

/// Projects the first payload tab's insertion marker or the hidden-bar candidate's content area.
#[allow(clippy::too_many_arguments)]
pub(crate) fn project_future_tab_gap_visual(
    original: &Workspace,
    candidate: &Workspace,
    policy: &DockPolicySnapshot,
    target_plan: &PresentationPlan,
    source_layout_facts: Option<&PresentationLayoutFacts>,
    target: &TabTarget,
    payload: &MovePayload,
    projection: &LayoutProjection,
) -> Result<LogicalRect, FutureLayoutProjectionError> {
    let target_facts = target_plan
        .layout_facts()
        .ok_or(FutureLayoutProjectionError::TargetLayoutFactsMissing)?;
    let config = target_facts.config();
    if let Some(source_facts) = source_layout_facts
        && source_facts.config() != config
    {
        return Err(FutureLayoutProjectionError::PresentationConfigMismatch);
    }
    let node_bounds = projection
        .nodes
        .get(&target.tabs())
        .map(|result| result.rect)
        .ok_or(FutureLayoutProjectionError::MissingNodeProjection {
            root: target.root(),
            node: target.tabs(),
        })?;
    let Node::Tabs {
        items, selected, ..
    } = candidate
        .node(target.tabs())
        .ok_or(FutureLayoutProjectionError::MissingNode {
            root: target.root(),
            node: target.tabs(),
        })?
    else {
        return Err(FutureLayoutProjectionError::TargetIsNotTabs {
            root: target.root(),
            node: target.tabs(),
        });
    };
    let central = candidate
        .root(target.root())
        .ok_or(FutureLayoutProjectionError::MissingRoot {
            root: target.root(),
        })?
        .central;
    let rule = candidate.pane_local_target_rule(
        target.root(),
        target.tabs(),
        central == Some(target.tabs()),
    )?;
    let target_policy =
        policy.tab_bar_policy(DockTabBarPolicyRequest::new(target.surface(), Some(rule)));
    if target_policy.visibility() == TabBarVisibility::Hidden {
        return Ok(node_bounds);
    }
    let original_selected = match original.node(target.tabs()) {
        Some(Node::Tabs { selected, .. }) => *selected,
        Some(Node::Split { .. }) => {
            return Err(FutureLayoutProjectionError::TargetIsNotTabs {
                root: target.root(),
                node: target.tabs(),
            });
        }
        None => {
            return Err(FutureLayoutProjectionError::MissingNode {
                root: target.root(),
                node: target.tabs(),
            });
        }
    };

    let tab_height = config.tab_bar_height().min(node_bounds.height());
    let bar = LogicalRect::new(
        node_bounds.x(),
        node_bounds.y(),
        node_bounds.width(),
        tab_height,
    )?;
    if bar.width() <= 0.0 || bar.height() <= 0.0 {
        return Err(FutureLayoutProjectionError::TargetTabBarUnrepresentable {
            root: target.root(),
            tabs: target.tabs(),
        });
    }

    let strip_key = TabStripKey::new(
        target.surface(),
        TabBarSceneId {
            root: target.root(),
            tabs: target.tabs(),
        },
    );
    let strip = target_facts
        .tab_strip(strip_key)
        .ok_or(FutureLayoutProjectionError::TabStripMeasurementUnavailable { key: strip_key })?;
    let include_presentation_menu = matches!(
        candidate.presentation_for_root(target.root()),
        Some(RootPresentationOwner::Main { surface }) if surface == target.surface()
    ) && future_presentation_menu_tab_bar(
        candidate,
        policy,
        target_facts,
        target.surface(),
        target.root(),
        projection,
    )? == Some(target.tabs());
    let chrome = tab_strip_optional_chrome_layout(
        bar,
        items.len(),
        strip,
        config,
        include_presentation_menu,
    )?;
    let mut desired_widths = Vec::with_capacity(items.len());
    for item in items.iter().copied() {
        let intrinsic = candidate_tab_intrinsic(
            original,
            target_plan,
            source_layout_facts,
            target.surface(),
            target.root(),
            target.tabs(),
            item,
        )?;
        desired_widths.push(desired_tab_width(
            intrinsic.content_width(),
            policy.pane_close_capability(item).allows_close(),
            config,
        ));
    }

    let viewport = chrome.viewport;
    if viewport.width() <= 0.0 {
        return Err(FutureLayoutProjectionError::TargetTabBarUnrepresentable {
            root: target.root(),
            tabs: target.tabs(),
        });
    }
    let flow = TabStripFlow::allocate(
        &desired_widths,
        viewport.width(),
        strip.after_tabs_width(),
        config.tab_min_width(),
    )?;
    let bar_id = TabBarSceneId {
        root: target.root(),
        tabs: target.tabs(),
    };
    let previous_bar = target_plan
        .tab_bar_records()
        .iter()
        .find(|bar| *bar.id() == bar_id);
    let requested_scroll = previous_bar.map_or(0.0, |bar| bar.scroll_offset());
    let selected_index =
        selected.and_then(|selected| items.iter().position(|item| *item == selected));
    let viewport_shrank = previous_bar.is_some_and(|bar| viewport.width() < bar.viewport().width());
    let scroll = flow.scroll_offset(
        requested_scroll,
        viewport.width(),
        selected_index,
        original_selected != *selected || viewport_shrank,
    );

    let first_payload = payload_items(original, payload)
        .into_iter()
        .next()
        .ok_or(FutureLayoutProjectionError::PayloadContainsNoItems)?;
    let payload_index = items.iter().position(|item| *item == first_payload).ok_or(
        FutureLayoutProjectionError::PayloadItemMissing {
            item: first_payload,
        },
    )?;
    let payload_start = flow
        .tab_range(payload_index)
        .ok_or(FutureLayoutProjectionError::PayloadItemMissing {
            item: first_payload,
        })?
        .0;
    let marker_x = viewport.x() - scroll + payload_start;
    let marker_width = config.splitter_thickness().min(viewport.width());
    if marker_width <= 0.0 {
        return Err(FutureLayoutProjectionError::TargetTabBarUnrepresentable {
            root: target.root(),
            tabs: target.tabs(),
        });
    }
    let marker_min = (marker_x - marker_width * 0.5).clamp(
        viewport.x(),
        (viewport.max().x() - marker_width).max(viewport.x()),
    );
    Ok(LogicalRect::new(
        marker_min,
        viewport.y(),
        marker_width,
        viewport.height(),
    )?)
}

fn future_presentation_menu_tab_bar(
    candidate: &Workspace,
    policy: &DockPolicySnapshot,
    target_facts: &PresentationLayoutFacts,
    surface: crate::ids::SurfaceId,
    root: RootId,
    projection: &LayoutProjection,
) -> Result<Option<NodeId>, FutureLayoutProjectionError> {
    let root_record = candidate
        .root(root)
        .ok_or(FutureLayoutProjectionError::MissingRoot { root })?;
    let config = target_facts.config();
    let central = root_record.central;
    let visible_menu_available = |tabs| -> Result<Option<bool>, FutureLayoutProjectionError> {
        let Some(Node::Tabs { items, .. }) = candidate.node(tabs) else {
            return Ok(None);
        };
        let rule = candidate.pane_local_target_rule(root, tabs, central == Some(tabs))?;
        let tab_bar = policy.tab_bar_policy(DockTabBarPolicyRequest::new(surface, Some(rule)));
        if tab_bar.visibility() == TabBarVisibility::Hidden {
            return Ok(None);
        }
        let node_bounds = projection
            .nodes
            .get(&tabs)
            .map(|result| result.rect)
            .ok_or(FutureLayoutProjectionError::MissingNodeProjection { root, node: tabs })?;
        let bar = LogicalRect::new(
            node_bounds.x(),
            node_bounds.y(),
            node_bounds.width(),
            config.tab_bar_height().min(node_bounds.height()),
        )?;
        if bar.width() <= 0.0 || bar.height() <= 0.0 {
            return Ok(None);
        }
        let key = TabStripKey::new(surface, TabBarSceneId { root, tabs });
        let strip = target_facts
            .tab_strip(key)
            .ok_or(FutureLayoutProjectionError::TabStripMeasurementUnavailable { key })?;
        let chrome = tab_strip_optional_chrome_layout(bar, items.len(), strip, config, true)?;
        Ok(Some(chrome.presentation_menu_bounds.is_some()))
    };

    select_menu_host(
        candidate,
        surface,
        root,
        root_record.node,
        central,
        visible_menu_available,
    )
}

fn candidate_tab_intrinsic(
    original: &Workspace,
    target_plan: &PresentationPlan,
    source_layout_facts: Option<&PresentationLayoutFacts>,
    candidate_surface: crate::ids::SurfaceId,
    candidate_root: RootId,
    candidate_tabs: NodeId,
    item: ItemId,
) -> Result<crate::scene_manifest::TabIntrinsic, FutureLayoutProjectionError> {
    let candidate_key = TabIntrinsicKey::new(
        candidate_surface,
        TabSceneId {
            root: candidate_root,
            tabs: candidate_tabs,
            item,
        },
    );
    if let Some(intrinsic) = layout_facts(target_plan, source_layout_facts)
        .find_map(|facts| facts.tab_intrinsic(candidate_key))
    {
        return Ok(intrinsic);
    }

    let source = original
        .capture_item_source_by_id(item)?
        .ok_or(FutureLayoutProjectionError::PayloadItemMissing { item })?;
    let source_surface = presentation_surface(original, source.root()).ok_or(
        FutureLayoutProjectionError::SourceRootNotPresented {
            root: source.root(),
        },
    )?;
    let source_key = TabIntrinsicKey::new(
        source_surface,
        TabSceneId {
            root: source.root(),
            tabs: source.tabs(),
            item,
        },
    );
    layout_facts(target_plan, source_layout_facts)
        .find_map(|facts| facts.tab_intrinsic(source_key))
        .ok_or(FutureLayoutProjectionError::TabIntrinsicUnavailable { key: source_key })
}

fn layout_facts<'a>(
    target_plan: &'a PresentationPlan,
    source_layout_facts: Option<&'a PresentationLayoutFacts>,
) -> impl Iterator<Item = &'a PresentationLayoutFacts> {
    std::iter::once(target_plan.layout_facts())
        .chain(std::iter::once(source_layout_facts))
        .flatten()
}

fn presentation_surface(workspace: &Workspace, root: RootId) -> Option<crate::ids::SurfaceId> {
    match workspace.presentation_for_root(root)? {
        RootPresentationOwner::Main { surface }
        | RootPresentationOwner::Contained { surface, .. } => Some(surface),
    }
}

fn payload_items(original: &Workspace, payload: &MovePayload) -> Vec<ItemId> {
    match payload {
        MovePayload::Item(source) => vec![source.item()],
        MovePayload::Tabs(source) | MovePayload::Subtree(source) => {
            original.collect_items_in_subtree(source.node())
        }
    }
}

fn collect_pane_minimums(
    facts: &PresentationLayoutFacts,
    minimums: &mut BTreeMap<PaneMinimumKey, LogicalSize>,
) -> Result<(), FutureLayoutProjectionError> {
    for (key, minimum) in facts.roots().flat_map(|root| root.pane_minimums()) {
        if let Some(previous) = minimums.insert(key, minimum)
            && previous != minimum
        {
            return Err(FutureLayoutProjectionError::ConflictingPaneMinimum { key });
        }
    }
    Ok(())
}

fn candidate_pane_minimum(
    original: &Workspace,
    minimums: &BTreeMap<PaneMinimumKey, LogicalSize>,
    candidate_root: RootId,
    candidate_tabs: NodeId,
    item: ItemId,
) -> Result<LogicalSize, FutureLayoutProjectionError> {
    let candidate_key = PaneMinimumKey::new(candidate_root, candidate_tabs, Some(item));
    if let Some(minimum) = minimums.get(&candidate_key) {
        return Ok(*minimum);
    }

    let source = original.capture_item_source_by_id(item)?.ok_or(
        FutureLayoutProjectionError::PaneMinimumUnavailable {
            root: candidate_root,
            tabs: candidate_tabs,
            item,
        },
    )?;
    let source_key = PaneMinimumKey::new(source.root(), source.tabs(), Some(item));
    minimums
        .get(&source_key)
        .copied()
        .ok_or(FutureLayoutProjectionError::PaneMinimumUnavailable {
            root: candidate_root,
            tabs: candidate_tabs,
            item,
        })
}

/// Failure to project the post-transaction layout from retained authority.
#[derive(Debug, Error)]
pub(crate) enum FutureLayoutProjectionError {
    #[error("target root {root} has no retained layout facts")]
    TargetRootNotPresented { root: RootId },
    #[error("candidate workspace is missing target root {root}")]
    MissingRoot { root: RootId },
    #[error("candidate root {root} is missing node {node:?}")]
    MissingNode { root: RootId, node: NodeId },
    #[error("candidate root {root} repeats node {node:?}")]
    RepeatedNode { root: RootId, node: NodeId },
    #[error("candidate projection omitted node {root}/{node:?}")]
    MissingNodeProjection { root: RootId, node: NodeId },
    #[error("candidate target {root}/{node:?} is not a tabs leaf")]
    TargetIsNotTabs { root: RootId, node: NodeId },
    #[error("pane minimum for item {item} in {root}/{tabs:?} was not presented")]
    PaneMinimumUnavailable {
        root: RootId,
        tabs: NodeId,
        item: ItemId,
    },
    #[error("retained outputs disagree about the pane minimum for exact key {key:?}")]
    ConflictingPaneMinimum { key: PaneMinimumKey },
    #[error("target presentation omitted retained layout facts")]
    TargetLayoutFactsMissing,
    #[error("candidate target tab bar {root}/{tabs:?} has no representable insertion marker")]
    TargetTabBarUnrepresentable { root: RootId, tabs: NodeId },
    #[error("tab intrinsic measurement {key:?} is unavailable")]
    TabIntrinsicUnavailable { key: TabIntrinsicKey },
    #[error("tab-strip measurement {key:?} is unavailable")]
    TabStripMeasurementUnavailable { key: TabStripKey },
    #[error("source root {root} is not presented")]
    SourceRootNotPresented { root: RootId },
    #[error("drag payload contains no item")]
    PayloadContainsNoItems,
    #[error("drag payload item {item} vanished from the candidate target")]
    PayloadItemMissing { item: ItemId },
    #[error("source and target outputs were compiled with different presentation configs")]
    PresentationConfigMismatch,
    #[error(transparent)]
    Command(#[from] CommandError),
    #[error(transparent)]
    Geometry(#[from] GeometryError),
    #[error(transparent)]
    Metrics(#[from] LayoutMetricsError),
    #[error(transparent)]
    Scene(#[from] super::SceneCompilationError),
    #[error(transparent)]
    Layout(#[from] LayoutError),
}
