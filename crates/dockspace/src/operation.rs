//! Internal checked command execution against isolated workspace candidates.

use std::collections::BTreeMap;

use crate::canonical::canonicalize_workspace;
use crate::command::{
    CommandOutcome, DockTarget, Edge, EdgeTarget, ItemSource, MovePayload, NodeSource, RootContent,
    RootPresentationTarget, TabTarget, WorkspaceCommand,
};
use crate::error::{CommandError, ReferenceRole, TransactionError};
use crate::graph::{
    Axis, ContainedFloating, NORMALIZED_WEIGHT_TOLERANCE, Node, RootRecord, SplitWeight,
    SurfacePresentation, Workspace,
};
use crate::ids::{FloatingPresentationId, ItemId, NodeId, RootId, SurfaceId};
use crate::policy::{DockPolicy, TearOffPresentation};
use crate::transaction::PreparedTransaction;
use crate::workspace::RootPresentation;

#[derive(Debug, Clone, PartialEq, Eq)]
enum ItemDelta {
    None,
    Open(ItemId),
    Close(ItemId),
    CloseMany(Vec<ItemId>),
}

struct AppliedCommand {
    outcome: CommandOutcome,
    delta: ItemDelta,
}

#[derive(Debug)]
enum DetachedPayload {
    Item {
        source_root: RootId,
        item: ItemId,
    },
    Node {
        source_root: RootId,
        node: NodeId,
        items: Vec<ItemId>,
        selected: Option<ItemId>,
        is_tabs: bool,
        central: Option<NodeId>,
    },
}

impl DetachedPayload {
    fn source_root(&self) -> RootId {
        match self {
            Self::Item { source_root, .. } | Self::Node { source_root, .. } => *source_root,
        }
    }

    fn items(&self) -> Vec<ItemId> {
        match self {
            Self::Item { item, .. } => vec![*item],
            Self::Node { items, .. } => items.clone(),
        }
    }
}

pub(crate) fn prepare_transaction(
    workspace: &Workspace,
    policy: &DockPolicy,
    commands: &[WorkspaceCommand],
) -> Result<PreparedTransaction, TransactionError> {
    let mut candidate = workspace.clone();
    let mut expected_items = workspace.item_multiset();
    let mut outcomes = Vec::with_capacity(commands.len());

    for (index, command) in commands.iter().enumerate() {
        let move_baseline = (index > 0 && matches!(command, WorkspaceCommand::Move { .. }))
            .then(|| candidate.clone());
        let mut applied = apply_command(&mut candidate, policy, command)
            .map_err(|source| TransactionError::Command { index, source })?;
        if let CommandOutcome::Moved { changed, .. } = &mut applied.outcome {
            *changed = move_baseline
                .as_ref()
                .map_or_else(|| candidate != *workspace, |before| candidate != *before);
        }
        apply_item_delta(&mut expected_items, applied.delta)
            .map_err(|source| TransactionError::Command { index, source })?;
        let actual_items = candidate.item_multiset();
        if actual_items != expected_items {
            return Err(TransactionError::ItemReconciliation {
                command_index: Some(index),
                expected: expected_items,
                actual: actual_items,
            });
        }
        outcomes.push(applied.outcome);
    }

    canonicalize_workspace(&mut candidate)?;
    candidate.validate()?;
    if commands.len() == 1
        && let Some(CommandOutcome::Moved { changed, .. }) = outcomes.first_mut()
    {
        *changed = candidate != *workspace;
    }
    let actual_items = candidate.item_multiset();
    if actual_items != expected_items {
        return Err(TransactionError::ItemReconciliation {
            command_index: None,
            expected: expected_items,
            actual: actual_items,
        });
    }

    Ok(PreparedTransaction {
        candidate,
        outcomes,
    })
}

fn apply_item_delta(
    items: &mut BTreeMap<ItemId, usize>,
    delta: ItemDelta,
) -> Result<(), CommandError> {
    match delta {
        ItemDelta::None => Ok(()),
        ItemDelta::Open(item) => {
            if items.contains_key(&item) {
                return Err(CommandError::Invariant {
                    stage: "reconcile unique opened item",
                });
            }
            items.insert(item, 1);
            Ok(())
        }
        ItemDelta::Close(item) => match items.remove(&item) {
            Some(1) => Ok(()),
            _ => Err(CommandError::Invariant {
                stage: "reconcile uniquely closed item",
            }),
        },
        ItemDelta::CloseMany(closed) => {
            for item in closed {
                if items.remove(&item) != Some(1) {
                    return Err(CommandError::Invariant {
                        stage: "reconcile items closed with root",
                    });
                }
            }
            Ok(())
        }
    }
}

fn apply_command(
    workspace: &mut Workspace,
    policy: &DockPolicy,
    command: &WorkspaceCommand,
) -> Result<AppliedCommand, CommandError> {
    match command {
        WorkspaceCommand::Select { source } => select(workspace, source),
        WorkspaceCommand::Reorder {
            source,
            insertion_index,
        } => reorder(workspace, policy, source, *insertion_index),
        WorkspaceCommand::Open { item, target } => open(workspace, policy, *item, target),
        WorkspaceCommand::Close { source } => close(workspace, source),
        WorkspaceCommand::CloseRoot { source } => close_root(workspace, source),
        WorkspaceCommand::Move { payload, target } => {
            move_payload(workspace, policy, payload, target)
        }
        WorkspaceCommand::ResizeSplit { split, weights } => {
            resize_split(workspace, policy, split, weights)
        }
        WorkspaceCommand::CreateSurfaceRoot {
            surface,
            root,
            content,
        } => create_surface_root(workspace, policy, *surface, *root, content),
        WorkspaceCommand::CreateContainedRoot {
            surface,
            root,
            floating,
            rect,
            z_order,
            content,
        } => create_contained_root(
            workspace, policy, *surface, *root, *floating, *rect, *z_order, content,
        ),
        WorkspaceCommand::RehomeRoot { source, target } => {
            rehome_root(workspace, policy, source, *target)
        }
        WorkspaceCommand::UpdateContainedRect {
            surface,
            root,
            floating,
            expected_rect,
            rect,
        } => update_contained_rect(
            workspace,
            policy,
            *surface,
            *root,
            *floating,
            *expected_rect,
            *rect,
        ),
        WorkspaceCommand::RaiseContained {
            surface,
            root,
            floating,
            expected_z_order,
            expected_frontmost,
        } => raise_contained(
            workspace,
            policy,
            *surface,
            *root,
            *floating,
            *expected_z_order,
            *expected_frontmost,
        ),
        WorkspaceCommand::RemoveEmptyRoot { source } => remove_empty_root(workspace, source),
    }
}

fn select(workspace: &mut Workspace, source: &ItemSource) -> Result<AppliedCommand, CommandError> {
    validate_item_source(workspace, source)?;
    let tabs = source.tabs();
    let item = source.item();
    let node = workspace
        .nodes
        .get_mut(tabs)
        .ok_or(CommandError::MissingNode {
            role: ReferenceRole::Source,
            node: tabs,
        })?;
    let Node::Tabs { selected, .. } = node else {
        return Err(CommandError::NodeIsNotTabs { node: tabs });
    };
    let changed = *selected != Some(item);
    *selected = Some(item);
    Ok(AppliedCommand {
        outcome: CommandOutcome::Selected {
            item,
            tabs,
            changed,
        },
        delta: ItemDelta::None,
    })
}

fn reorder(
    workspace: &mut Workspace,
    policy: &DockPolicy,
    source: &ItemSource,
    insertion_index: usize,
) -> Result<AppliedCommand, CommandError> {
    policy.check_tab_merge()?;
    validate_item_source(workspace, source)?;
    let tabs = source.tabs();
    let item = source.item();
    let (from, to, changed) = reorder_item(workspace, tabs, item, insertion_index)?;
    Ok(AppliedCommand {
        outcome: CommandOutcome::Reordered {
            item,
            tabs,
            from,
            to,
            changed,
        },
        delta: ItemDelta::None,
    })
}

fn open(
    workspace: &mut Workspace,
    policy: &DockPolicy,
    item: ItemId,
    target: &DockTarget,
) -> Result<AppliedCommand, CommandError> {
    if workspace.item_multiset().contains_key(&item) {
        return Err(CommandError::ItemAlreadyOpen { item });
    }
    check_target_policy(policy, target)?;
    validate_target(workspace, target)?;
    let root = target_root(target);
    let payload = DetachedPayload::Item {
        source_root: root,
        item,
    };
    insert_payload(workspace, payload, target)?;
    Ok(AppliedCommand {
        outcome: CommandOutcome::Opened { item, root },
        delta: ItemDelta::Open(item),
    })
}

fn close(workspace: &mut Workspace, source: &ItemSource) -> Result<AppliedCommand, CommandError> {
    validate_item_source(workspace, source)?;
    ensure_item_removal_is_presentable(workspace, source.root())?;
    let root = source.root();
    let item = source.item();
    remove_item(workspace, source.tabs(), item)?;
    cleanup_item_source_root(workspace, root)?;
    Ok(AppliedCommand {
        outcome: CommandOutcome::Closed { item, root },
        delta: ItemDelta::Close(item),
    })
}

fn move_payload(
    workspace: &mut Workspace,
    policy: &DockPolicy,
    payload: &MovePayload,
    target: &DockTarget,
) -> Result<AppliedCommand, CommandError> {
    check_target_policy(policy, target)?;
    validate_move_payload(workspace, payload)?;
    validate_target(workspace, target)?;

    if let MovePayload::Item(source) = payload
        && let Some(changed) = move_item_within_same_tabs(workspace, source, target)?
    {
        return Ok(AppliedCommand {
            outcome: CommandOutcome::Moved {
                items: vec![source.item()],
                source_root: source.root(),
                target_root: target_root(target),
                changed,
            },
            delta: ItemDelta::None,
        });
    }

    let source_node = payload_node(payload);
    if let Some(source_node) = source_node {
        let target_node = target_node(target);
        if workspace.subtree_contains(source_node, target_node) {
            if source_node == target_node
                && target_is_tabs(target)
                && matches!(workspace.nodes.get(source_node), Some(Node::Tabs { .. }))
            {
                return Ok(AppliedCommand {
                    outcome: CommandOutcome::Moved {
                        items: workspace.collect_items_in_subtree(source_node),
                        source_root: payload_root(payload),
                        target_root: target_root(target),
                        changed: false,
                    },
                    delta: ItemDelta::None,
                });
            }
            return Err(CommandError::TargetInsidePayload {
                source_node,
                target: target_node,
            });
        }
        if target_is_tabs(target)
            && !matches!(workspace.nodes.get(source_node), Some(Node::Tabs { .. }))
        {
            return Err(CommandError::SplitPayloadIntoTabs { node: source_node });
        }
    }

    let detached = detach_payload(workspace, payload)?;
    let source_root = detached.source_root();
    let items = detached.items();
    let target_root = target_root(target);
    insert_payload(workspace, detached, target)?;
    Ok(AppliedCommand {
        outcome: CommandOutcome::Moved {
            items,
            source_root,
            target_root,
            changed: true,
        },
        delta: ItemDelta::None,
    })
}

fn resize_split(
    workspace: &mut Workspace,
    policy: &DockPolicy,
    split: &NodeSource,
    weights: &[SplitWeight],
) -> Result<AppliedCommand, CommandError> {
    policy.check_splitter_resize()?;
    validate_node_source(workspace, split)?;
    let node_id = split.node();
    let node = workspace
        .nodes
        .get_mut(node_id)
        .ok_or(CommandError::MissingNode {
            role: ReferenceRole::Source,
            node: node_id,
        })?;
    let Node::Split {
        children,
        weights: current,
        ..
    } = node
    else {
        return Err(CommandError::NodeIsNotSplit { node: node_id });
    };
    if children.len() != weights.len() {
        return Err(CommandError::SplitWeightCountMismatch {
            split: node_id,
            children: children.len(),
            weights: weights.len(),
        });
    }
    let sum: f64 = weights.iter().map(|weight| f64::from(weight.get())).sum();
    if (sum - 1.0).abs() > NORMALIZED_WEIGHT_TOLERANCE {
        return Err(CommandError::SplitWeightsNotNormalized {
            split: node_id,
            sum,
        });
    }
    let changed = current != weights;
    current.clone_from_slice(weights);
    Ok(AppliedCommand {
        outcome: CommandOutcome::SplitResized {
            split: node_id,
            changed,
        },
        delta: ItemDelta::None,
    })
}

fn create_surface_root(
    workspace: &mut Workspace,
    policy: &DockPolicy,
    surface: SurfaceId,
    root: RootId,
    content: &RootContent,
) -> Result<AppliedCommand, CommandError> {
    policy.check_tear_off(TearOffPresentation::Native)?;
    ensure_root_ids_available(workspace, root)?;
    if workspace.surfaces.contains_key(&surface) {
        return Err(CommandError::SurfaceIdCollision { surface });
    }
    validate_root_content(workspace, content)?;
    let (node, central, items, delta) = detach_root_content(workspace, content)?;
    workspace.roots.insert(root, RootRecord { node, central });
    workspace
        .surfaces
        .insert(surface, SurfacePresentation::new(root));
    Ok(AppliedCommand {
        outcome: CommandOutcome::SurfaceRootCreated {
            surface,
            root,
            items,
        },
        delta,
    })
}

#[allow(clippy::too_many_arguments)]
fn create_contained_root(
    workspace: &mut Workspace,
    policy: &DockPolicy,
    surface: SurfaceId,
    root: RootId,
    floating: FloatingPresentationId,
    rect: crate::geometry::LogicalRect,
    z_order: u64,
    content: &RootContent,
) -> Result<AppliedCommand, CommandError> {
    policy.check_tear_off(TearOffPresentation::Contained)?;
    ensure_root_ids_available(workspace, root)?;
    if workspace.contained_floatings.contains_key(&floating) {
        return Err(CommandError::FloatingIdCollision { floating });
    }
    if !workspace.surfaces.contains_key(&surface) {
        return Err(CommandError::MissingSurface { surface });
    }
    validate_root_content(workspace, content)?;
    ensure_contained_host_survives_move(workspace, surface, content)?;
    let (node, central, items, delta) = detach_root_content(workspace, content)?;
    if !workspace.surfaces.contains_key(&surface) {
        return Err(CommandError::MissingSurface { surface });
    }
    workspace.roots.insert(root, RootRecord { node, central });
    workspace.contained_floatings.insert(
        floating,
        ContainedFloating::new(floating, root, surface, rect, z_order),
    );
    workspace
        .surfaces
        .get_mut(&surface)
        .ok_or(CommandError::MissingSurface { surface })?
        .contained
        .push(floating);
    Ok(AppliedCommand {
        outcome: CommandOutcome::ContainedRootCreated {
            surface,
            root,
            floating,
            items,
        },
        delta,
    })
}

fn rehome_root(
    workspace: &mut Workspace,
    policy: &DockPolicy,
    source: &NodeSource,
    target: RootPresentationTarget,
) -> Result<AppliedCommand, CommandError> {
    validate_node_source(workspace, source)?;
    let root = source.root();
    let root_node = workspace
        .roots
        .get(&root)
        .ok_or(CommandError::MissingRoot { root })?
        .node;
    if source.node() != root_node {
        return Err(CommandError::NodeIsNotRoot {
            root,
            node: source.node(),
        });
    }

    let current = workspace
        .presentation_for_root(root)
        .ok_or(CommandError::Invariant {
            stage: "locate root presentation before rehome",
        })?;
    match target {
        RootPresentationTarget::Surface { surface } => {
            policy.check_tear_off(TearOffPresentation::Native)?;
            if current == (RootPresentation::Main { surface }) {
                return Ok(rehome_outcome(root, surface, None, false));
            }
            if workspace.surfaces.contains_key(&surface) {
                return Err(CommandError::SurfaceIdCollision { surface });
            }
            detach_root_presentation(workspace, root, current)?;
            workspace
                .surfaces
                .insert(surface, SurfacePresentation::new(root));
            Ok(rehome_outcome(root, surface, None, true))
        }
        RootPresentationTarget::Contained {
            surface,
            floating,
            rect,
            z_order,
        } => {
            policy.check_tear_off(TearOffPresentation::Contained)?;
            if !workspace.surfaces.contains_key(&surface) {
                return Err(CommandError::MissingSurface { surface });
            }
            if let RootPresentation::Contained {
                surface: current_surface,
                floating: current_floating,
            } = current
            {
                if floating != current_floating {
                    return Err(CommandError::FloatingIdentityWouldChange {
                        root,
                        existing: current_floating,
                        requested: floating,
                    });
                }
                if current_surface == surface {
                    let record = workspace
                        .contained_floatings
                        .get(&floating)
                        .ok_or(CommandError::MissingFloating { floating })?;
                    if record.rect == rect && record.z_order == z_order {
                        return Ok(rehome_outcome(root, surface, Some(floating), false));
                    }
                    return Err(CommandError::RehomeMetadataRequiresDedicatedCommand { floating });
                }
            } else if workspace.contained_floatings.contains_key(&floating) {
                return Err(CommandError::FloatingIdCollision { floating });
            }
            if current == (RootPresentation::Main { surface }) {
                return Err(CommandError::ContainedHostWouldBeRemoved { surface, root });
            }

            detach_root_presentation(workspace, root, current)?;
            if !workspace.surfaces.contains_key(&surface) {
                return Err(CommandError::MissingSurface { surface });
            }
            workspace.contained_floatings.insert(
                floating,
                ContainedFloating::new(floating, root, surface, rect, z_order),
            );
            workspace
                .surfaces
                .get_mut(&surface)
                .ok_or(CommandError::MissingSurface { surface })?
                .contained
                .push(floating);
            Ok(rehome_outcome(root, surface, Some(floating), true))
        }
    }
}

fn rehome_outcome(
    root: RootId,
    surface: SurfaceId,
    floating: Option<FloatingPresentationId>,
    changed: bool,
) -> AppliedCommand {
    AppliedCommand {
        outcome: CommandOutcome::RootRehomed {
            root,
            surface,
            floating,
            changed,
        },
        delta: ItemDelta::None,
    }
}

fn update_contained_rect(
    workspace: &mut Workspace,
    policy: &DockPolicy,
    surface: SurfaceId,
    root: RootId,
    floating: FloatingPresentationId,
    expected_rect: crate::geometry::LogicalRect,
    rect: crate::geometry::LogicalRect,
) -> Result<AppliedCommand, CommandError> {
    policy.check_tear_off(TearOffPresentation::Contained)?;
    validate_contained(workspace, surface, root, floating)?;
    let record = workspace
        .contained_floatings
        .get_mut(&floating)
        .ok_or(CommandError::MissingFloating { floating })?;
    if record.rect != expected_rect {
        return Err(CommandError::StaleContainedRect {
            floating,
            expected: expected_rect,
            actual: record.rect,
        });
    }
    let changed = record.rect != rect;
    record.rect = rect;
    Ok(AppliedCommand {
        outcome: CommandOutcome::ContainedRectUpdated { floating, changed },
        delta: ItemDelta::None,
    })
}

fn raise_contained(
    workspace: &mut Workspace,
    policy: &DockPolicy,
    surface: SurfaceId,
    root: RootId,
    floating: FloatingPresentationId,
    expected_z_order: u64,
    expected_frontmost: crate::graph::ContainedStackKey,
) -> Result<AppliedCommand, CommandError> {
    policy.check_tear_off(TearOffPresentation::Contained)?;
    validate_contained(workspace, surface, root, floating)?;
    let previous = workspace
        .contained_floatings
        .get(&floating)
        .ok_or(CommandError::MissingFloating { floating })?
        .z_order;
    if previous != expected_z_order {
        return Err(CommandError::StaleZOrder {
            floating,
            expected: expected_z_order,
            actual: previous,
        });
    }
    let actual_frontmost = workspace
        .contained_frontmost(surface)?
        .ok_or(CommandError::MissingFloating { floating })?;
    if actual_frontmost != expected_frontmost {
        return Err(CommandError::StaleContainedStack {
            surface,
            expected: expected_frontmost,
            actual: actual_frontmost,
        });
    }
    let previous_key = crate::graph::ContainedStackKey::new(previous, floating);
    if previous_key == actual_frontmost {
        return Ok(AppliedCommand {
            outcome: CommandOutcome::ContainedRaised {
                floating,
                previous,
                current: previous,
                changed: false,
            },
            delta: ItemDelta::None,
        });
    }
    let current = actual_frontmost
        .z_order()
        .checked_add(1)
        .ok_or(CommandError::ZOrderOverflow { floating })?;
    workspace
        .contained_floatings
        .get_mut(&floating)
        .ok_or(CommandError::MissingFloating { floating })?
        .z_order = current;
    Ok(AppliedCommand {
        outcome: CommandOutcome::ContainedRaised {
            floating,
            previous,
            current,
            changed: true,
        },
        delta: ItemDelta::None,
    })
}

fn remove_empty_root(
    workspace: &mut Workspace,
    source: &NodeSource,
) -> Result<AppliedCommand, CommandError> {
    let (root, root_node) = validate_root_source(workspace, source)?;
    let item_count = workspace.collect_items_in_subtree(root_node).len();
    if item_count != 0 {
        return Err(CommandError::RootNotEmpty {
            root,
            items: item_count,
        });
    }
    remove_root_and_presentation(workspace, root)?;
    Ok(AppliedCommand {
        outcome: CommandOutcome::EmptyRootRemoved { root },
        delta: ItemDelta::None,
    })
}

fn close_root(
    workspace: &mut Workspace,
    source: &NodeSource,
) -> Result<AppliedCommand, CommandError> {
    let (root, root_node) = validate_root_source(workspace, source)?;
    let items = workspace.collect_items_in_subtree(root_node);
    if items.is_empty() {
        return Err(CommandError::RootEmpty { root });
    }

    let record = remove_root_and_presentation(workspace, root)?;
    remove_subtree_nodes(workspace, record.node)?;
    Ok(AppliedCommand {
        outcome: CommandOutcome::RootClosed {
            root,
            items: items.clone(),
        },
        delta: ItemDelta::CloseMany(items),
    })
}

fn validate_item_source(workspace: &Workspace, source: &ItemSource) -> Result<(), CommandError> {
    workspace.verify_reference(
        source.root(),
        source.tabs(),
        source.fingerprint(),
        ReferenceRole::Source,
    )?;
    match workspace.nodes.get(source.tabs()) {
        Some(Node::Tabs { items, .. }) if items.contains(&source.item()) => Ok(()),
        Some(Node::Tabs { .. }) => Err(CommandError::ItemNotInTabs {
            tabs: source.tabs(),
            item: source.item(),
        }),
        Some(Node::Split { .. }) => Err(CommandError::NodeIsNotTabs {
            node: source.tabs(),
        }),
        None => Err(CommandError::MissingNode {
            role: ReferenceRole::Source,
            node: source.tabs(),
        }),
    }
}

fn validate_node_source(workspace: &Workspace, source: &NodeSource) -> Result<(), CommandError> {
    workspace.verify_reference(
        source.root(),
        source.node(),
        source.fingerprint(),
        ReferenceRole::Source,
    )
}

fn validate_root_source(
    workspace: &Workspace,
    source: &NodeSource,
) -> Result<(RootId, NodeId), CommandError> {
    validate_node_source(workspace, source)?;
    let root = source.root();
    let root_node = workspace
        .roots
        .get(&root)
        .ok_or(CommandError::MissingRoot { root })?
        .node;
    if root_node != source.node() {
        return Err(CommandError::NodeIsNotRoot {
            root,
            node: source.node(),
        });
    }
    Ok((root, root_node))
}

fn validate_move_payload(workspace: &Workspace, payload: &MovePayload) -> Result<(), CommandError> {
    match payload {
        MovePayload::Item(source) => validate_item_source(workspace, source),
        MovePayload::Tabs(source) => {
            validate_node_source(workspace, source)?;
            if !matches!(workspace.nodes.get(source.node()), Some(Node::Tabs { .. })) {
                return Err(CommandError::NodeIsNotTabs {
                    node: source.node(),
                });
            }
            ensure_nonempty_payload(workspace, source.node())
        }
        MovePayload::Subtree(source) => {
            validate_node_source(workspace, source)?;
            ensure_nonempty_payload(workspace, source.node())
        }
    }
}

fn ensure_nonempty_payload(workspace: &Workspace, node: NodeId) -> Result<(), CommandError> {
    if workspace.collect_items_in_subtree(node).is_empty() {
        Err(CommandError::EmptyPayload { node })
    } else {
        Ok(())
    }
}

fn validate_target(workspace: &Workspace, target: &DockTarget) -> Result<(), CommandError> {
    match target {
        DockTarget::Center(target) => validate_tab_target(workspace, target, None),
        DockTarget::TabGap { target, index } => {
            validate_tab_target(workspace, target, Some(*index))
        }
        DockTarget::Edge(target) => workspace.verify_reference(
            target.root(),
            target.node(),
            target.fingerprint(),
            ReferenceRole::Target,
        ),
    }
}

fn validate_tab_target(
    workspace: &Workspace,
    target: &TabTarget,
    insertion_index: Option<usize>,
) -> Result<(), CommandError> {
    workspace.verify_reference(
        target.root(),
        target.tabs(),
        target.fingerprint(),
        ReferenceRole::Target,
    )?;
    match workspace.nodes.get(target.tabs()) {
        Some(Node::Tabs { items, .. }) => {
            if let Some(index) = insertion_index
                && index > items.len()
            {
                return Err(CommandError::TabIndexOutOfBounds {
                    tabs: target.tabs(),
                    index,
                    len: items.len(),
                });
            }
            Ok(())
        }
        Some(Node::Split { .. }) => Err(CommandError::NodeIsNotTabs {
            node: target.tabs(),
        }),
        None => Err(CommandError::MissingNode {
            role: ReferenceRole::Target,
            node: target.tabs(),
        }),
    }
}

fn check_target_policy(policy: &DockPolicy, target: &DockTarget) -> Result<(), CommandError> {
    match target {
        DockTarget::Center(_) | DockTarget::TabGap { .. } => policy.check_tab_merge()?,
        DockTarget::Edge(_) => policy.check_edge_split()?,
    }
    Ok(())
}

fn validate_root_content(workspace: &Workspace, content: &RootContent) -> Result<(), CommandError> {
    match content {
        RootContent::OpenItem(item) => {
            if workspace.item_multiset().contains_key(item) {
                Err(CommandError::ItemAlreadyOpen { item: *item })
            } else {
                Ok(())
            }
        }
        RootContent::Move(payload) => {
            validate_move_payload(workspace, payload)?;
            if move_removes_complete_root(workspace, payload)? {
                return Err(CommandError::WholeRootRequiresRehome {
                    root: payload_root(payload),
                });
            }
            Ok(())
        }
    }
}

fn move_removes_complete_root(
    workspace: &Workspace,
    payload: &MovePayload,
) -> Result<bool, CommandError> {
    let root = payload_root(payload);
    let record = workspace
        .roots
        .get(&root)
        .ok_or(CommandError::MissingRoot { root })?;
    Ok(match payload {
        MovePayload::Item(_) => {
            record.central.is_none() && workspace.collect_items_in_subtree(record.node).len() == 1
        }
        MovePayload::Tabs(source) | MovePayload::Subtree(source) => source.node() == record.node,
    })
}

fn ensure_contained_host_survives_move(
    workspace: &Workspace,
    surface: SurfaceId,
    content: &RootContent,
) -> Result<(), CommandError> {
    let RootContent::Move(payload) = content else {
        return Ok(());
    };
    let source_root = payload_root(payload);
    if workspace.presentation_for_root(source_root) != Some(RootPresentation::Main { surface }) {
        return Ok(());
    }
    let record = workspace
        .roots
        .get(&source_root)
        .ok_or(CommandError::MissingRoot { root: source_root })?;
    let removes_main = match payload {
        MovePayload::Item(_) => {
            record.central.is_none() && workspace.collect_items_in_subtree(record.node).len() == 1
        }
        MovePayload::Tabs(source) | MovePayload::Subtree(source) => source.node() == record.node,
    };
    if removes_main {
        Err(CommandError::ContainedHostWouldBeRemoved {
            surface,
            root: source_root,
        })
    } else {
        Ok(())
    }
}

fn ensure_root_ids_available(workspace: &Workspace, root: RootId) -> Result<(), CommandError> {
    if workspace.roots.contains_key(&root) {
        Err(CommandError::RootIdCollision { root })
    } else {
        Ok(())
    }
}

fn detach_root_content(
    workspace: &mut Workspace,
    content: &RootContent,
) -> Result<(NodeId, Option<NodeId>, Vec<ItemId>, ItemDelta), CommandError> {
    match content {
        RootContent::OpenItem(item) => {
            let node = workspace.nodes.insert(Node::tabs([*item]));
            Ok((node, None, vec![*item], ItemDelta::Open(*item)))
        }
        RootContent::Move(payload) => {
            let detached = detach_payload(workspace, payload)?;
            let items = detached.items();
            match detached {
                DetachedPayload::Item { item, .. } => {
                    let node = workspace.nodes.insert(Node::tabs([item]));
                    Ok((node, None, items, ItemDelta::None))
                }
                DetachedPayload::Node { node, central, .. } => {
                    Ok((node, central, items, ItemDelta::None))
                }
            }
        }
    }
}

fn detach_payload(
    workspace: &mut Workspace,
    payload: &MovePayload,
) -> Result<DetachedPayload, CommandError> {
    match payload {
        MovePayload::Item(source) => {
            ensure_item_removal_is_presentable(workspace, source.root())?;
            remove_item(workspace, source.tabs(), source.item())?;
            cleanup_item_source_root(workspace, source.root())?;
            Ok(DetachedPayload::Item {
                source_root: source.root(),
                item: source.item(),
            })
        }
        MovePayload::Tabs(source) | MovePayload::Subtree(source) => {
            let node = source.node();
            let source_root = source.root();
            let items = workspace.collect_items_in_subtree(node);
            let (selected, is_tabs) = match workspace.nodes.get(node) {
                Some(Node::Tabs { selected, .. }) => (*selected, true),
                Some(Node::Split { .. }) => (None, false),
                None => {
                    return Err(CommandError::MissingNode {
                        role: ReferenceRole::Source,
                        node,
                    });
                }
            };
            let central = detach_node(workspace, source_root, node)?;
            Ok(DetachedPayload::Node {
                source_root,
                node,
                items,
                selected,
                is_tabs,
                central,
            })
        }
    }
}

fn detach_node(
    workspace: &mut Workspace,
    root: RootId,
    node: NodeId,
) -> Result<Option<NodeId>, CommandError> {
    let record = *workspace
        .roots
        .get(&root)
        .ok_or(CommandError::MissingRoot { root })?;
    if record.node == node {
        remove_root_and_presentation(workspace, root)?;
        return Ok(record.central);
    }
    if let Some(central) = record.central
        && workspace.subtree_contains(node, central)
    {
        return Err(CommandError::CentralNodeWouldDetach {
            root,
            source_node: node,
            central,
        });
    }
    let parent = workspace
        .parent_link(root, node)
        .ok_or(CommandError::NodeOutsideRoot {
            role: ReferenceRole::Source,
            root,
            node,
        })?;
    remove_split_child(workspace, parent.parent, parent.index)?;
    Ok(None)
}

fn remove_split_child(
    workspace: &mut Workspace,
    parent: NodeId,
    index: usize,
) -> Result<(), CommandError> {
    let node = workspace
        .nodes
        .get_mut(parent)
        .ok_or(CommandError::MissingNode {
            role: ReferenceRole::Source,
            node: parent,
        })?;
    let Node::Split {
        children, weights, ..
    } = node
    else {
        return Err(CommandError::NodeIsNotSplit { node: parent });
    };
    if index >= children.len() || index >= weights.len() {
        return Err(CommandError::Invariant {
            stage: "detach indexed split child",
        });
    }
    children.remove(index);
    weights.remove(index);
    if weights.len() == 1 {
        weights[0] = SplitWeight::new(1.0).map_err(|_| CommandError::Invariant {
            stage: "assign singleton split weight",
        })?;
    } else if !weights.is_empty() {
        *weights =
            SplitWeight::normalize(weights.iter().map(|weight| weight.get())).map_err(|_| {
                CommandError::Invariant {
                    stage: "normalize weights after detaching child",
                }
            })?;
    }
    Ok(())
}

fn insert_payload(
    workspace: &mut Workspace,
    payload: DetachedPayload,
    target: &DockTarget,
) -> Result<(), CommandError> {
    match target {
        DockTarget::Center(target) => {
            let index = tabs_len(workspace, target.tabs())?;
            merge_payload_into_tabs(workspace, payload, target.tabs(), index)
        }
        DockTarget::TabGap { target, index } => {
            merge_payload_into_tabs(workspace, payload, target.tabs(), *index)
        }
        DockTarget::Edge(target) => insert_payload_at_edge(workspace, &payload, target),
    }
}

fn merge_payload_into_tabs(
    workspace: &mut Workspace,
    payload: DetachedPayload,
    target: NodeId,
    index: usize,
) -> Result<(), CommandError> {
    let (items, selected) = match payload {
        DetachedPayload::Item { item, .. } => (vec![item], Some(item)),
        DetachedPayload::Node {
            node,
            items,
            selected,
            is_tabs,
            ..
        } => {
            if !is_tabs {
                return Err(CommandError::SplitPayloadIntoTabs { node });
            }
            let source = workspace
                .nodes
                .get_mut(node)
                .ok_or(CommandError::MissingNode {
                    role: ReferenceRole::Source,
                    node,
                })?;
            let Node::Tabs {
                items: source_items,
                selected: source_selected,
            } = source
            else {
                return Err(CommandError::NodeIsNotTabs { node });
            };
            source_items.clear();
            *source_selected = None;
            (items, selected)
        }
    };
    insert_items(workspace, target, index, &items, selected)
}

fn insert_items(
    workspace: &mut Workspace,
    tabs: NodeId,
    index: usize,
    inserted: &[ItemId],
    selected_item: Option<ItemId>,
) -> Result<(), CommandError> {
    let node = workspace
        .nodes
        .get_mut(tabs)
        .ok_or(CommandError::MissingNode {
            role: ReferenceRole::Target,
            node: tabs,
        })?;
    let Node::Tabs { items, selected } = node else {
        return Err(CommandError::NodeIsNotTabs { node: tabs });
    };
    if index > items.len() {
        return Err(CommandError::TabIndexOutOfBounds {
            tabs,
            index,
            len: items.len(),
        });
    }
    items.splice(index..index, inserted.iter().copied());
    if let Some(item) = selected_item {
        *selected = Some(item);
    }
    Ok(())
}

fn insert_payload_at_edge(
    workspace: &mut Workspace,
    payload: &DetachedPayload,
    target: &EdgeTarget,
) -> Result<(), CommandError> {
    let payload_node = match payload {
        DetachedPayload::Item { item, .. } => workspace.nodes.insert(Node::tabs([*item])),
        DetachedPayload::Node { node, .. } => *node,
    };
    let (axis, before) = edge_axis_and_order(target.edge());
    if let Some(parent) = workspace.parent_link(target.root(), target.node()) {
        let parent_axis = match workspace.nodes.get(parent.parent) {
            Some(Node::Split { axis, .. }) => *axis,
            _ => {
                return Err(CommandError::Invariant {
                    stage: "inspect edge target parent",
                });
            }
        };
        if parent_axis == axis {
            return split_existing_branch(
                workspace,
                parent.parent,
                target.node(),
                payload_node,
                before,
                target.fraction().get(),
                axis,
            );
        }
    }
    wrap_target_branch(
        workspace,
        target.root(),
        target.node(),
        payload_node,
        axis,
        before,
        target.fraction().get(),
    )
}

#[allow(clippy::too_many_arguments)]
fn split_existing_branch(
    workspace: &mut Workspace,
    parent: NodeId,
    target: NodeId,
    payload: NodeId,
    before: bool,
    fraction: f32,
    axis: Axis,
) -> Result<(), CommandError> {
    let node = workspace
        .nodes
        .get_mut(parent)
        .ok_or(CommandError::MissingNode {
            role: ReferenceRole::Target,
            node: parent,
        })?;
    let Node::Split {
        children, weights, ..
    } = node
    else {
        return Err(CommandError::NodeIsNotSplit { node: parent });
    };
    let index =
        children
            .iter()
            .position(|child| *child == target)
            .ok_or(CommandError::Invariant {
                stage: "locate edge target in same-axis parent",
            })?;
    let branch = weights
        .get(index)
        .copied()
        .ok_or(CommandError::Invariant {
            stage: "locate edge target branch weight",
        })?
        .get();
    let payload_weight = SplitWeight::new(branch * fraction)
        .map_err(|_| CommandError::InvalidEdgeWeight { axis })?;
    let target_weight = SplitWeight::new(branch - payload_weight.get())
        .map_err(|_| CommandError::InvalidEdgeWeight { axis })?;
    weights[index] = target_weight;
    let insertion = if before { index } else { index + 1 };
    children.insert(insertion, payload);
    weights.insert(insertion, payload_weight);
    Ok(())
}

fn wrap_target_branch(
    workspace: &mut Workspace,
    root: RootId,
    target: NodeId,
    payload: NodeId,
    axis: Axis,
    before: bool,
    fraction: f32,
) -> Result<(), CommandError> {
    let payload_weight =
        SplitWeight::new(fraction).map_err(|_| CommandError::InvalidEdgeWeight { axis })?;
    let target_weight =
        SplitWeight::new(1.0 - fraction).map_err(|_| CommandError::InvalidEdgeWeight { axis })?;
    let (children, weights) = if before {
        (vec![payload, target], vec![payload_weight, target_weight])
    } else {
        (vec![target, payload], vec![target_weight, payload_weight])
    };
    let wrapper = workspace.nodes.insert(Node::Split {
        axis,
        children,
        weights,
    });
    if let Some(parent) = workspace.parent_link(root, target) {
        let node = workspace
            .nodes
            .get_mut(parent.parent)
            .ok_or(CommandError::MissingNode {
                role: ReferenceRole::Target,
                node: parent.parent,
            })?;
        let Node::Split { children, .. } = node else {
            return Err(CommandError::NodeIsNotSplit {
                node: parent.parent,
            });
        };
        let slot = children
            .get_mut(parent.index)
            .ok_or(CommandError::Invariant {
                stage: "replace wrapped target branch",
            })?;
        *slot = wrapper;
    } else {
        let record = workspace
            .roots
            .get_mut(&root)
            .ok_or(CommandError::MissingRoot { root })?;
        if record.node != target {
            return Err(CommandError::NodeOutsideRoot {
                role: ReferenceRole::Target,
                root,
                node: target,
            });
        }
        record.node = wrapper;
    }
    Ok(())
}

fn move_item_within_same_tabs(
    workspace: &mut Workspace,
    source: &ItemSource,
    target: &DockTarget,
) -> Result<Option<bool>, CommandError> {
    match target {
        DockTarget::Center(target) if target.tabs() == source.tabs() => Ok(Some(false)),
        DockTarget::TabGap { target, index } if target.tabs() == source.tabs() => {
            let (_, _, changed) = reorder_item(workspace, source.tabs(), source.item(), *index)?;
            Ok(Some(changed))
        }
        DockTarget::Edge(target) if target.node() == source.tabs() => {
            let is_noncentral_singleton = matches!(
                workspace.nodes.get(source.tabs()),
                Some(Node::Tabs { items, .. }) if items.len() == 1
            ) && workspace
                .roots
                .get(&source.root())
                .is_some_and(|record| record.central != Some(source.tabs()));
            Ok(is_noncentral_singleton.then_some(false))
        }
        DockTarget::Center(_) | DockTarget::TabGap { .. } | DockTarget::Edge(_) => Ok(None),
    }
}

fn reorder_item(
    workspace: &mut Workspace,
    tabs: NodeId,
    item: ItemId,
    insertion_index: usize,
) -> Result<(usize, usize, bool), CommandError> {
    let node = workspace
        .nodes
        .get_mut(tabs)
        .ok_or(CommandError::MissingNode {
            role: ReferenceRole::Source,
            node: tabs,
        })?;
    let Node::Tabs { items, .. } = node else {
        return Err(CommandError::NodeIsNotTabs { node: tabs });
    };
    if insertion_index > items.len() {
        return Err(CommandError::TabIndexOutOfBounds {
            tabs,
            index: insertion_index,
            len: items.len(),
        });
    }
    let from = items
        .iter()
        .position(|entry| *entry == item)
        .ok_or(CommandError::ItemNotInTabs { tabs, item })?;
    let to = if insertion_index > from {
        insertion_index - 1
    } else {
        insertion_index
    };
    if from == to {
        return Ok((from, to, false));
    }
    items.remove(from);
    items.insert(to, item);
    Ok((from, to, true))
}

fn remove_item(workspace: &mut Workspace, tabs: NodeId, item: ItemId) -> Result<(), CommandError> {
    let node = workspace
        .nodes
        .get_mut(tabs)
        .ok_or(CommandError::MissingNode {
            role: ReferenceRole::Source,
            node: tabs,
        })?;
    let Node::Tabs { items, selected } = node else {
        return Err(CommandError::NodeIsNotTabs { node: tabs });
    };
    let index = items
        .iter()
        .position(|entry| *entry == item)
        .ok_or(CommandError::ItemNotInTabs { tabs, item })?;
    items.remove(index);
    if *selected == Some(item) {
        *selected = if items.is_empty() {
            None
        } else {
            Some(items[index.min(items.len() - 1)])
        };
    }
    Ok(())
}

fn cleanup_item_source_root(workspace: &mut Workspace, root: RootId) -> Result<(), CommandError> {
    let record = *workspace
        .roots
        .get(&root)
        .ok_or(CommandError::MissingRoot { root })?;
    if workspace.collect_items_in_subtree(record.node).is_empty() && record.central.is_none() {
        remove_root_and_presentation(workspace, root)?;
    }
    Ok(())
}

fn ensure_item_removal_is_presentable(
    workspace: &Workspace,
    root: RootId,
) -> Result<(), CommandError> {
    let record = workspace
        .roots
        .get(&root)
        .ok_or(CommandError::MissingRoot { root })?;
    if record.central.is_none() && workspace.collect_items_in_subtree(record.node).len() == 1 {
        ensure_root_presentation_removable(workspace, root)?;
    }
    Ok(())
}

fn ensure_root_presentation_removable(
    workspace: &Workspace,
    root: RootId,
) -> Result<(), CommandError> {
    match workspace.presentation_for_root(root) {
        Some(RootPresentation::Main { surface }) => {
            let presentation = workspace
                .surfaces
                .get(&surface)
                .ok_or(CommandError::MissingSurface { surface })?;
            if presentation.contained.is_empty() {
                Ok(())
            } else {
                Err(CommandError::SurfaceHasContainedRoots { surface })
            }
        }
        Some(RootPresentation::Contained { .. }) => Ok(()),
        None => Err(CommandError::Invariant {
            stage: "remove root presentation owner",
        }),
    }
}

fn remove_root_and_presentation(
    workspace: &mut Workspace,
    root: RootId,
) -> Result<RootRecord, CommandError> {
    let presentation = workspace
        .presentation_for_root(root)
        .ok_or(CommandError::Invariant {
            stage: "locate root presentation owner",
        })?;
    detach_root_presentation(workspace, root, presentation)?;
    workspace
        .roots
        .remove(&root)
        .ok_or(CommandError::MissingRoot { root })
}

fn remove_subtree_nodes(workspace: &mut Workspace, root: NodeId) -> Result<(), CommandError> {
    let mut stack = vec![root];
    while let Some(node_id) = stack.pop() {
        let node = workspace
            .nodes
            .remove(node_id)
            .ok_or(CommandError::MissingNode {
                role: ReferenceRole::Source,
                node: node_id,
            })?;
        if let Node::Split { children, .. } = node {
            stack.extend(children.into_iter().rev());
        }
    }
    Ok(())
}

fn detach_root_presentation(
    workspace: &mut Workspace,
    root: RootId,
    presentation: RootPresentation,
) -> Result<(), CommandError> {
    ensure_root_presentation_removable(workspace, root)?;
    match presentation {
        RootPresentation::Main { surface } => {
            if workspace
                .surfaces
                .get(&surface)
                .map(|entry| entry.main_root)
                != Some(root)
            {
                return Err(CommandError::Invariant {
                    stage: "detach matching main-root presentation",
                });
            }
            workspace
                .surfaces
                .remove(&surface)
                .ok_or(CommandError::MissingSurface { surface })?;
        }
        RootPresentation::Contained { surface, floating } => {
            let record = workspace
                .contained_floatings
                .remove(&floating)
                .ok_or(CommandError::MissingFloating { floating })?;
            if record.root != root || record.surface != surface {
                return Err(CommandError::Invariant {
                    stage: "detach matching contained-root presentation",
                });
            }
            let presentation = workspace
                .surfaces
                .get_mut(&surface)
                .ok_or(CommandError::MissingSurface { surface })?;
            let index = presentation
                .contained
                .iter()
                .position(|entry| *entry == floating)
                .ok_or(CommandError::Invariant {
                    stage: "remove contained presentation backlink",
                })?;
            presentation.contained.remove(index);
        }
    }
    Ok(())
}

fn validate_contained(
    workspace: &Workspace,
    surface: SurfaceId,
    root: RootId,
    floating: FloatingPresentationId,
) -> Result<(), CommandError> {
    let record = workspace
        .contained_floatings
        .get(&floating)
        .ok_or(CommandError::MissingFloating { floating })?;
    if record.root != root || record.surface != surface {
        return Err(CommandError::FloatingPresentationMismatch {
            floating,
            expected_root: root,
            expected_surface: surface,
            actual_root: record.root,
            actual_surface: record.surface,
        });
    }
    let presentation = workspace
        .surfaces
        .get(&surface)
        .ok_or(CommandError::MissingSurface { surface })?;
    if !presentation.contained.contains(&floating) {
        return Err(CommandError::Invariant {
            stage: "validate contained presentation backlink",
        });
    }
    Ok(())
}

fn tabs_len(workspace: &Workspace, tabs: NodeId) -> Result<usize, CommandError> {
    match workspace.nodes.get(tabs) {
        Some(Node::Tabs { items, .. }) => Ok(items.len()),
        Some(Node::Split { .. }) => Err(CommandError::NodeIsNotTabs { node: tabs }),
        None => Err(CommandError::MissingNode {
            role: ReferenceRole::Target,
            node: tabs,
        }),
    }
}

fn target_root(target: &DockTarget) -> RootId {
    match target {
        DockTarget::Center(target) | DockTarget::TabGap { target, .. } => target.root(),
        DockTarget::Edge(target) => target.root(),
    }
}

fn target_node(target: &DockTarget) -> NodeId {
    match target {
        DockTarget::Center(target) | DockTarget::TabGap { target, .. } => target.tabs(),
        DockTarget::Edge(target) => target.node(),
    }
}

fn target_is_tabs(target: &DockTarget) -> bool {
    matches!(target, DockTarget::Center(_) | DockTarget::TabGap { .. })
}

fn payload_root(payload: &MovePayload) -> RootId {
    match payload {
        MovePayload::Item(source) => source.root(),
        MovePayload::Tabs(source) | MovePayload::Subtree(source) => source.root(),
    }
}

fn payload_node(payload: &MovePayload) -> Option<NodeId> {
    match payload {
        MovePayload::Item(_) => None,
        MovePayload::Tabs(source) | MovePayload::Subtree(source) => Some(source.node()),
    }
}

fn edge_axis_and_order(edge: Edge) -> (Axis, bool) {
    match edge {
        Edge::Left => (Axis::Horizontal, true),
        Edge::Right => (Axis::Horizontal, false),
        Edge::Top => (Axis::Vertical, true),
        Edge::Bottom => (Axis::Vertical, false),
    }
}
