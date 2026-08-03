//! Post-transaction edge preview projection.

use thiserror::Error;

use crate::command::{DockTarget, MovePayload, WorkspaceCommand};
use crate::drop_target::{DropTargetKind, DropTargetRecord, DropVisual};
use crate::error::CommandError;
use crate::geometry::{GeometryError, LogicalRect};
use crate::graph::Workspace;
use crate::ids::{NodeId, RootId};
use crate::policy::DockPolicySnapshot;
use crate::scene::{PresentationLayoutFacts, PresentationPlan};
use crate::scene_compiler::{
    FutureLayoutProjectionError, project_future_root, project_future_tab_gap_visual,
};
use crate::transition::WorkspaceVersion;

pub(super) fn resolved_visual(
    original: &Workspace,
    candidate: &Workspace,
    workspace_version: WorkspaceVersion,
    policy: &DockPolicySnapshot,
    target_plan: &PresentationPlan,
    source_layout_facts: Option<&PresentationLayoutFacts>,
    target_record: &DropTargetRecord,
    command: &WorkspaceCommand,
) -> Result<DropVisual, DropPreviewProjectionError> {
    if !matches!(
        target_record.id().kind(),
        DropTargetKind::TabGap
            | DropTargetKind::Center
            | DropTargetKind::InnerEdge
            | DropTargetKind::OuterEdge
    ) {
        return Ok(target_record.visual());
    }
    let WorkspaceCommand::Move { payload, target } = command else {
        return Ok(target_record.visual());
    };
    let root = target_root(target);
    let Some(projection) = project_future_root(
        original,
        candidate,
        workspace_version,
        policy,
        target_plan,
        source_layout_facts,
        root,
    )?
    else {
        // Synthetic plans exist only in crate-local resolver tests. Production
        // plans always retain their measured layout facts.
        return Ok(target_record.visual());
    };
    let rect = match target {
        DockTarget::Center(target) => projected_center_rect(
            candidate,
            policy,
            target_plan,
            target,
            &projection.node_rects,
        )?,
        DockTarget::InnerEdge(_) | DockTarget::OuterEdge(_) => {
            projected_payload_rect(original, candidate, root, payload, &projection.node_rects)?
        }
        DockTarget::TabGap { target, .. } => project_future_tab_gap_visual(
            original,
            candidate,
            policy,
            target_plan,
            source_layout_facts,
            target,
            payload,
            &projection,
        )?,
    };
    Ok(DropVisual::new(rect))
}

fn projected_center_rect(
    candidate: &Workspace,
    policy: &DockPolicySnapshot,
    target_plan: &PresentationPlan,
    target: &crate::command::TabTarget,
    node_rects: &std::collections::BTreeMap<NodeId, LogicalRect>,
) -> Result<LogicalRect, DropPreviewProjectionError> {
    let bounds = node_rects.get(&target.tabs()).copied().ok_or(
        DropPreviewProjectionError::TargetProjectionMissing {
            root: target.root(),
            node: target.tabs(),
        },
    )?;
    let crate::graph::Node::Tabs { .. } = candidate.node(target.tabs()).ok_or(
        DropPreviewProjectionError::TargetProjectionMissing {
            root: target.root(),
            node: target.tabs(),
        },
    )?
    else {
        return Err(DropPreviewProjectionError::ProjectedCenterIsNotTabs {
            root: target.root(),
            node: target.tabs(),
        });
    };
    let facts = target_plan
        .layout_facts()
        .ok_or(DropPreviewProjectionError::TargetLayoutFactsMissing)?;
    let tab_bar = policy.tab_bar_policy(crate::policy::DockTabBarPolicyRequest::new(
        target.surface(),
        Some(target.rule()),
    ));
    let tab_height = if tab_bar.visibility() == crate::policy::TabBarVisibility::Visible {
        facts.config().tab_bar_height().min(bounds.height())
    } else {
        0.0
    };
    Ok(LogicalRect::new(
        bounds.x(),
        bounds.y() + tab_height,
        bounds.width(),
        (bounds.height() - tab_height).max(0.0),
    )?)
}

fn projected_payload_rect(
    original: &Workspace,
    candidate: &Workspace,
    target_root: RootId,
    payload: &MovePayload,
    node_rects: &std::collections::BTreeMap<NodeId, LogicalRect>,
) -> Result<LogicalRect, DropPreviewProjectionError> {
    let (source_root, source_node, items) = match payload {
        MovePayload::Item(source) => (source.root(), source.tabs(), vec![source.item()]),
        MovePayload::Tabs(source) | MovePayload::Subtree(source) => (
            source.root(),
            source.node(),
            original.collect_items_in_subtree(source.node()),
        ),
    };
    if items.is_empty() {
        return Err(DropPreviewProjectionError::PayloadContainsNoItems {
            root: source_root,
            node: source_node,
        });
    }

    let mut nodes = std::collections::BTreeSet::new();
    for item in items {
        nodes.insert(destination_item_node(candidate, target_root, item)?);
    }
    let mut projected = nodes.into_iter().map(|node| {
        node_rects
            .get(&node)
            .copied()
            .ok_or(DropPreviewProjectionError::PayloadProjectionMissing {
                root: target_root,
                node,
            })
    });
    let mut bounds = projected
        .next()
        .expect("a non-empty payload resolves at least one candidate tabs node")?;
    for next in projected {
        bounds = union_rect(bounds, next?)?;
    }
    Ok(bounds)
}

fn union_rect(left: LogicalRect, right: LogicalRect) -> Result<LogicalRect, GeometryError> {
    LogicalRect::new(
        left.x().min(right.x()),
        left.y().min(right.y()),
        left.max().x().max(right.max().x()) - left.x().min(right.x()),
        left.max().y().max(right.max().y()) - left.y().min(right.y()),
    )
}

fn destination_item_node(
    candidate: &Workspace,
    target_root: RootId,
    item: crate::ids::ItemId,
) -> Result<NodeId, DropPreviewProjectionError> {
    let source = candidate
        .capture_item_source_by_id(item)?
        .ok_or(DropPreviewProjectionError::PayloadItemMissing { item })?;
    if source.root() != target_root {
        return Err(DropPreviewProjectionError::PayloadItemOutsideTarget {
            item,
            expected: target_root,
            actual: source.root(),
        });
    }
    Ok(source.tabs())
}

const fn target_root(target: &DockTarget) -> RootId {
    match target {
        DockTarget::Center(target) | DockTarget::TabGap { target, .. } => target.root(),
        DockTarget::InnerEdge(target) | DockTarget::OuterEdge(target) => target.root(),
    }
}

#[derive(Debug, Error)]
pub(super) enum DropPreviewProjectionError {
    #[error(transparent)]
    FutureLayout(#[from] FutureLayoutProjectionError),
    #[error(transparent)]
    Command(#[from] CommandError),
    #[error("drag payload subtree {root}/{node:?} contains no item")]
    PayloadContainsNoItems { root: RootId, node: NodeId },
    #[error("drag payload item {item} vanished from the transaction candidate")]
    PayloadItemMissing { item: crate::ids::ItemId },
    #[error(
        "drag payload item {item} landed in root {actual}, but the resolved target root is {expected}"
    )]
    PayloadItemOutsideTarget {
        item: crate::ids::ItemId,
        expected: RootId,
        actual: RootId,
    },
    #[error("candidate projection omitted drag payload node {root}/{node:?}")]
    PayloadProjectionMissing { root: RootId, node: NodeId },
    #[error("candidate projection omitted center target node {root}/{node:?}")]
    TargetProjectionMissing { root: RootId, node: NodeId },
    #[error("candidate center target {root}/{node:?} is not a tabs leaf")]
    ProjectedCenterIsNotTabs { root: RootId, node: NodeId },
    #[error("target presentation omitted retained layout facts")]
    TargetLayoutFactsMissing,
    #[error(transparent)]
    Geometry(#[from] GeometryError),
}
