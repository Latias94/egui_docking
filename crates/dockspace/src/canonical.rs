//! Deterministic canonicalization for durable docking forests.
//!
//! The topology semantics are informed by Open GPUI's `graph_canonical.rs`
//! (Apache-2.0) and Dockview's VS Code-derived grid normalization (MIT). This
//! implementation was rewritten against dockspace's renderer-neutral model;
//! no source code was copied.

use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};

use crate::graph::root_structure::RootStructure;
use slotmap::SecondaryMap;
use std::sync::Arc;
use thiserror::Error;

use crate::graph::{
    InvalidSplitWeight, InvalidSplitWeights, NORMALIZED_WEIGHT_TOLERANCE, Node, SplitWeight,
    Workspace, WorkspaceBuilder, WorkspaceData, is_exact_tab_mru,
};
use crate::ids::{ItemId, NodeId, RootId};
use crate::transaction::{clone_workspace_data_draft, clone_workspace_draft};
use crate::validation::WorkspaceValidationErrors;

/// Summary of one successfully published canonicalization transaction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CanonicalizationReport {
    /// Whether any observable workspace state changed.
    pub changed: bool,
    /// Number of unreachable or superseded runtime nodes removed from the arena.
    pub removed_nodes: usize,
}

/// Failure to canonicalize a workspace without losing or inventing state.
#[derive(Debug, Clone, PartialEq, Error)]
pub enum CanonicalizationError {
    /// A split or root references a stale or missing node identity.
    #[error("node {owner:?} references missing child {child:?}")]
    MissingChild {
        /// Referencing node, or `None` when a root owns the reference.
        owner: Option<NodeId>,
        /// Missing runtime child identity.
        child: NodeId,
    },
    /// A split has different child and weight cardinalities.
    #[error("split {node:?} has {children} children but {weights} weights")]
    SplitWeightCountMismatch {
        /// Invalid split node.
        node: NodeId,
        /// Number of child identities.
        children: usize,
        /// Number of split weights.
        weights: usize,
    },
    /// A split contains an invalid scalar weight.
    #[error("split {node:?} has invalid weight at index {index}: {source}")]
    InvalidWeight {
        /// Invalid split node.
        node: NodeId,
        /// Position of the invalid weight.
        index: usize,
        /// Scalar validation failure.
        source: InvalidSplitWeight,
    },
    /// Input split weights are valid scalars but do not describe normalized shares.
    #[error("split {node:?} weights must sum to one before canonicalization, got {sum}")]
    WeightsNotNormalized {
        /// Invalid split node.
        node: NodeId,
        /// Sum of the supplied weights.
        sum: f64,
    },
    /// A split could not be normalized after topology rewriting.
    #[error("split {node:?} could not be normalized: {source}")]
    InvalidNormalizedWeights {
        /// Invalid split node.
        node: NodeId,
        /// Weight collection failure.
        source: InvalidSplitWeights,
    },
    /// A runtime node has more than one structural parent.
    #[error("node {node:?} has more than one parent")]
    SharedNode {
        /// Shared runtime node identity.
        node: NodeId,
    },
    /// The arena contains a cycle, including inside an orphan component.
    #[error("docking arena contains a cycle through node {node:?}")]
    Cycle {
        /// Node at which the active traversal encountered the cycle.
        node: NodeId,
    },
    /// Two stable roots name the same runtime root node.
    #[error("roots {first} and {second} both own node {node:?}")]
    SharedRootNode {
        /// First stable root owner.
        first: RootId,
        /// Second stable root owner.
        second: RootId,
        /// Shared runtime root identity.
        node: NodeId,
    },
    /// A declared root is also structurally owned by another node.
    #[error("root {root} node {node:?} is also a child of another node")]
    RootHasParent {
        /// Stable root identity.
        root: RootId,
        /// Invalid runtime root identity.
        node: NodeId,
    },
    /// A tabs node contains the same item more than once.
    #[error("item {item} occurs in both tabs {first:?} and {second:?}")]
    DuplicateItem {
        /// Duplicate stable item identity.
        item: ItemId,
        /// First tabs node that contains the item.
        first: NodeId,
        /// Later tabs node that contains the item.
        second: NodeId,
    },
    /// A non-empty tabs node has no valid selected item.
    #[error("tabs node {node:?} has invalid selection {selected:?}")]
    InvalidSelection {
        /// Invalid tabs node.
        node: NodeId,
        /// Selected item, when one was supplied.
        selected: Option<ItemId>,
    },
    /// A tabs node does not carry one exact selected-first item permutation.
    #[error("tabs node {node:?} has invalid MRU state")]
    InvalidTabMru {
        /// Tabs node with invalid or missing history.
        node: NodeId,
    },
    /// A central-region identity does not name a tabs leaf.
    #[error("root {root} central node {central:?} is not a tabs leaf")]
    CentralNotTabs {
        /// Stable root identity.
        root: RootId,
        /// Invalid central node identity.
        central: NodeId,
    },
    /// A central tabs leaf is not reachable from the root that declares it.
    #[error("root {root} central node {central:?} is outside its tree")]
    CentralOutsideRoot {
        /// Stable root identity.
        root: RootId,
        /// Unreachable central leaf identity.
        central: NodeId,
    },
    /// Removing empty non-central leaves would leave a declared root empty.
    #[error("root {root} becomes empty and has no central leaf to preserve")]
    EmptyRoot {
        /// Stable root identity.
        root: RootId,
    },
    /// Arena cleanup would remove or duplicate application-owned items.
    #[error("canonicalization would change the workspace item multiset")]
    ItemMultisetChanged {
        /// Item occurrences before rewriting.
        before: BTreeMap<ItemId, usize>,
        /// Item occurrences in the canonical candidate.
        after: BTreeMap<ItemId, usize>,
    },
    /// The rewritten candidate does not satisfy the strict workspace contract.
    #[error("canonical workspace failed strict validation: {0}")]
    InvalidOutput(#[source] WorkspaceValidationErrors),
    /// Graph ownership could not be completed for a canonical candidate.
    #[error("canonical root authority could not be completed: {0}")]
    AuthorityCompletion(#[source] crate::error::CommandError),
    /// An internal iterative traversal invariant was violated.
    #[error("canonicalization traversal invariant failed during {stage}")]
    TraversalInvariant {
        /// Traversal phase that could not make deterministic progress.
        stage: &'static str,
    },
}

/// Rewrites a structurally sound workspace into its unique canonical form.
///
/// The operation is atomic: structural input checks run before cloning, and all
/// rewrites run against that clone. `workspace` is replaced only after the item
/// multiset comparison and strict validation succeed. Valid unreachable arena
/// nodes are swept; an orphan containing an item is rejected instead of silently
/// discarding pane ownership.
///
/// # Errors
///
/// Returns [`CanonicalizationError`] when the input graph is structurally
/// invalid, canonical rewriting would change item ownership, or the complete
/// rewritten workspace fails strict validation.
pub fn canonicalize_workspace(
    workspace: &mut Workspace,
) -> Result<CanonicalizationReport, CanonicalizationError> {
    validate_rewrite_input(workspace.storage())?;

    let mut candidate = clone_workspace_draft(workspace);
    let structures = candidate.take_root_structures();
    let effects = canonicalize_validated_draft(&mut candidate.storage)?;
    complete_runtime_rewrite(&mut candidate, effects, structures)?;

    let report = CanonicalizationReport {
        changed: candidate != *workspace,
        removed_nodes: workspace
            .storage
            .nodes
            .len()
            .saturating_sub(candidate.storage.nodes.len()),
    };
    *workspace = candidate;
    Ok(report)
}

/// Normalizes a caller-owned, discardable transaction draft in place.
///
/// All input, item-ownership and output checks still run. On failure the draft
/// may be partially rewritten and must be discarded rather than published.
/// On success it has passed the complete workspace validator.
pub(super) fn canonicalize_draft(workspace: &mut Workspace) -> Result<(), CanonicalizationError> {
    let structures = workspace.take_root_structures();
    let effects = canonicalize_data(&mut workspace.storage)?;
    complete_runtime_rewrite(workspace, effects, structures)
}

pub(super) fn build_workspace(
    mut storage: WorkspaceData,
) -> Result<Workspace, CanonicalizationError> {
    canonicalize_data(&mut storage)?;
    Workspace::issue_storage(storage).map_err(CanonicalizationError::AuthorityCompletion)
}

/// Finishes the structural changes made by one checked Item move.
pub(super) fn finish_item_move(
    workspace: &mut Workspace,
    source_root: RootId,
    source: NodeId,
    target_root: RootId,
    outer_root: Option<RootId>,
    inserted_split: Option<NodeId>,
) -> Result<(), CanonicalizationError> {
    let mut output_splits = Vec::new();
    if let Some(split) = inserted_split {
        let Some(Node::Split {
            children, weights, ..
        }) = workspace.node(split)
        else {
            return Err(CanonicalizationError::TraversalInvariant {
                stage: "locate edge insertion split",
            });
        };
        if validate_split_weights(split, children, weights).is_err() {
            // Rebuild the complete input diagnostic only on a failed frontier.
            validate_rewrite_input(&workspace.storage)?;
            return Err(CanonicalizationError::TraversalInvariant {
                stage: "diagnose edge insertion weights",
            });
        }
        output_splits.push(split);
    }
    let mut results = SecondaryMap::new();
    // Independent roots retain the full normalizer's stable root error order.
    if let Some(target_root) = outer_root.filter(|&root| root < source_root) {
        finish_item_target_root(workspace, target_root, &mut results, &mut output_splits)?;
    }
    if matches!(workspace.node(source), Some(Node::Tabs { items, .. }) if items.is_empty()) {
        if let Some(record) = workspace.root(source_root).copied() {
            if record.central != Some(source) {
                let path = workspace.path_to_node(source_root, source).ok_or(
                    CanonicalizationError::TraversalInvariant {
                        stage: "locate emptied source path",
                    },
                )?;
                workspace.begin_root_edit(source_root);
                let storage = &mut workspace.storage;
                results.insert(source, None);
                storage.remove_runtime_node(source);
                for &parent in path[..path.len() - 1].iter().rev() {
                    rewrite_local_split(storage, parent, &mut results, &mut output_splits)?;
                }
                let next = results
                    .get(record.node)
                    .copied()
                    .flatten()
                    .ok_or(CanonicalizationError::EmptyRoot { root: source_root })?;
                storage
                    .roots
                    .get_mut(&source_root)
                    .ok_or(CanonicalizationError::TraversalInvariant {
                        stage: "publish repaired root",
                    })?
                    .node = next;
            }
        } else {
            workspace.remove_runtime_node(source);
        }
    }

    if let Some(target_root) = outer_root.filter(|&root| root >= source_root) {
        finish_item_target_root(workspace, target_root, &mut results, &mut output_splits)?;
    }
    // A later ancestor can retire an earlier output. Check the final live
    // frontier after every rewrite, preserving canonical errors before output errors.
    if output_splits.iter().copied().any(|split| {
        matches!(workspace.node(split), Some(Node::Split { children, weights, .. })
            if children.len() < 2 || validate_split_weights(split, children, weights).is_err())
    }) {
        workspace
            .storage
            .validate_candidate()
            .map_err(CanonicalizationError::InvalidOutput)?;
        return Err(CanonicalizationError::TraversalInvariant {
            stage: "diagnose rewritten split weights",
        });
    }
    // The normalizer edits raw arena records. Publish only its changed direct
    // rosters and retired nodes; unrelated branches retain their placements.
    for (node, _) in &results {
        if workspace.node(node).is_none() {
            workspace.forget_placement(node);
        }
    }
    for root in [Some(source_root), outer_root].into_iter().flatten() {
        if let Some(record) = workspace.root(root).copied() {
            workspace.place_node(root, record.node, None);
        }
    }
    for split in output_splits {
        workspace.refresh_child_placements(split);
    }
    // Only the final local rosters, including outer flattening, may be projected.
    workspace
        .complete_root_structure(source_root)
        .map_err(CanonicalizationError::AuthorityCompletion)?;
    if target_root != source_root {
        workspace
            .complete_root_structure(target_root)
            .map_err(CanonicalizationError::AuthorityCompletion)?;
    }
    Ok(())
}

fn finish_item_target_root(
    workspace: &mut Workspace,
    target_root: RootId,
    results: &mut SecondaryMap<NodeId, Option<NodeId>>,
    output_splits: &mut Vec<NodeId>,
) -> Result<(), CanonicalizationError> {
    // An outer edge can wrap the old root in a split of that same axis,
    // independently of whether the source became empty.
    if let Some(record) = workspace.root(target_root).copied()
        && let Some(Node::Split { axis, children, .. }) = workspace.node(record.node)
        && children.iter().any(|&child| {
            matches!(workspace.node(child), Some(Node::Split { axis: child_axis, .. }) if child_axis == axis)
        })
    {
        workspace.begin_root_edit(target_root);
        rewrite_local_split(&mut workspace.storage, record.node, results, output_splits)?;
    }
    Ok(())
}

fn rewrite_local_split(
    storage: &mut WorkspaceData,
    parent: NodeId,
    results: &mut SecondaryMap<NodeId, Option<NodeId>>,
    output_splits: &mut Vec<NodeId>,
) -> Result<(), CanonicalizationError> {
    let Some(Node::Split { axis, children, .. }) = storage.nodes.get(parent) else {
        return Err(CanonicalizationError::TraversalInvariant {
            stage: "repair source ancestor",
        });
    };
    let axis = *axis;
    for &child in children {
        if !results.contains_key(child) {
            // The closed Item edit cannot have changed another child.
            results.insert(child, Some(child));
        }
    }
    let flattened = children.iter().filter_map(|&child| {
        let next = results.get(child).copied().flatten()?;
        matches!(storage.nodes.get(next), Some(Node::Split { axis: child_axis, .. }) if *child_axis == axis)
            .then_some(next)
    }).collect::<Vec<_>>();
    let (next, _) = rewrite_split(storage, parent, results)?;
    results.insert(parent, next);
    if next == Some(parent) {
        output_splits.push(parent);
        for child in flattened {
            storage.remove_runtime_node(child);
        }
    } else {
        storage.remove_runtime_node(parent);
    }
    Ok(())
}

/// Actual writes produced by this one canonical traversal, not predicted dirt.
#[derive(Debug, Default, PartialEq, Eq)]
struct CanonicalEffects {
    changed_roots: Vec<RootId>,
    retired_nodes: Vec<NodeId>,
}

fn complete_runtime_rewrite(
    workspace: &mut Workspace,
    effects: CanonicalEffects,
    previous: Vec<(RootId, Arc<RootStructure>)>,
) -> Result<(), CanonicalizationError> {
    // Only actual unchanged payloads may recover their previous immutable body.
    // Roots already being edited were not captured in this list and must rebuild.
    // Actual changed roots retain the normalizer's ascending root order.
    for (root, structure) in previous {
        if effects.changed_roots.binary_search(&root).is_err() {
            workspace.restore_root_structure(root, structure);
        }
    }
    for node in effects.retired_nodes {
        workspace.forget_placement(node);
    }
    for root in effects.changed_roots {
        workspace
            .root(root)
            .ok_or(CanonicalizationError::TraversalInvariant {
                stage: "complete rewritten root placement",
            })?;
        // Canonical rewriting changes no presentation carrier. The concrete
        // command writer already completed carrier ownership at its prefix.
        workspace
            .complete_root_change(root)
            .map_err(CanonicalizationError::AuthorityCompletion)?;
    }
    workspace
        .complete_root_structures()
        .map_err(CanonicalizationError::AuthorityCompletion)
}

fn canonicalize_data(
    workspace: &mut WorkspaceData,
) -> Result<CanonicalEffects, CanonicalizationError> {
    validate_rewrite_input(workspace)?;
    canonicalize_validated_draft(workspace)
}

fn canonicalize_validated_draft(
    workspace: &mut WorkspaceData,
) -> Result<CanonicalEffects, CanonicalizationError> {
    #[cfg(test)]
    crate::drop_resolver::structural_work::record_canonical_full_pass();
    let original_items = workspace.item_multiset();
    let protected_central: HashSet<NodeId> = workspace
        .roots
        .values()
        .filter_map(|root| root.central)
        .collect();

    let roots: Vec<(RootId, NodeId)> = workspace
        .roots
        .iter()
        .map(|(root, record)| (*root, record.node))
        .collect();

    let mut effects = CanonicalEffects::default();
    let mut rewritten = SecondaryMap::with_capacity(workspace.nodes.capacity());
    for (root, node) in roots {
        let (next, changed) = rewrite_subtree(workspace, node, &protected_central, &mut rewritten)?;
        let next = next.ok_or(CanonicalizationError::EmptyRoot { root })?;
        if changed {
            effects.changed_roots.push(root);
        }
        if let Some(record) = workspace.roots.get_mut(&root) {
            record.node = next;
        }
    }

    let reachable = reachable_from_roots(workspace)?;
    workspace.nodes.retain(|node, _| {
        let live = reachable.contains_key(node);
        if !live {
            effects.retired_nodes.push(node);
        }
        live
    });
    workspace
        .tab_mru
        .retain(|node, _| reachable.contains_key(*node));

    let candidate_items = workspace.item_multiset();
    if candidate_items != original_items {
        return Err(CanonicalizationError::ItemMultisetChanged {
            before: original_items,
            after: candidate_items,
        });
    }

    workspace
        .validate_candidate()
        .map_err(CanonicalizationError::InvalidOutput)?;

    Ok(effects)
}

impl WorkspaceBuilder {
    /// Canonicalizes this mutable draft without publishing a partial result.
    ///
    /// This is the public entry point for deliberately non-canonical builder
    /// input such as same-axis nesting and singleton split containers.
    ///
    /// # Errors
    ///
    /// Returns [`CanonicalizationError`] under the same conditions as
    /// [`canonicalize_workspace`]. The draft remains unchanged on failure.
    pub fn canonicalize(&mut self) -> Result<CanonicalizationReport, CanonicalizationError> {
        validate_rewrite_input(&self.workspace)?;
        let mut candidate = clone_workspace_data_draft(&self.workspace);
        canonicalize_validated_draft(&mut candidate)?;
        let report = CanonicalizationReport {
            changed: candidate != self.workspace,
            removed_nodes: self
                .workspace
                .nodes
                .len()
                .saturating_sub(candidate.nodes.len()),
        };
        self.workspace = candidate;
        Ok(report)
    }
}

fn rewrite_subtree(
    workspace: &mut WorkspaceData,
    node_id: NodeId,
    protected_central: &HashSet<NodeId>,
    results: &mut SecondaryMap<NodeId, Option<NodeId>>,
) -> Result<(Option<NodeId>, bool), CanonicalizationError> {
    let mut stack = vec![(node_id, false)];
    let mut changed = false;

    while let Some((current_id, exiting)) = stack.pop() {
        let current =
            workspace
                .nodes
                .get(current_id)
                .ok_or(CanonicalizationError::MissingChild {
                    owner: None,
                    child: current_id,
                })?;

        if !exiting {
            stack.push((current_id, true));
            if let Node::Split { children, .. } = current {
                stack.extend(children.iter().rev().copied().map(|child| (child, false)));
            }
            continue;
        }

        let (result, local_changed) = match current {
            Node::Tabs { items, .. } => {
                if items.is_empty() && !protected_central.contains(&current_id) {
                    (None, true)
                } else {
                    (Some(current_id), false)
                }
            }
            Node::Split { .. } => rewrite_split(workspace, current_id, results)?,
        };
        changed |= local_changed;
        results.insert(current_id, result);
    }

    let next = results
        .get(node_id)
        .copied()
        .ok_or(CanonicalizationError::MissingChild {
            owner: None,
            child: node_id,
        })?;
    Ok((next, changed))
}

fn rewrite_split(
    workspace: &mut WorkspaceData,
    node_id: NodeId,
    results: &SecondaryMap<NodeId, Option<NodeId>>,
) -> Result<(Option<NodeId>, bool), CanonicalizationError> {
    let Some(Node::Split {
        axis,
        children,
        weights,
    }) = workspace.nodes.get(node_id)
    else {
        return Err(CanonicalizationError::TraversalInvariant {
            stage: "split rewriting",
        });
    };
    let axis = *axis;
    let mut topology_changed = false;
    let mut needs_flattening = false;
    let mut retained_children = 0;
    let mut only_child = None;
    for &child in children {
        let child_result =
            results
                .get(child)
                .copied()
                .ok_or(CanonicalizationError::MissingChild {
                    owner: Some(node_id),
                    child,
                })?;
        topology_changed |= child_result != Some(child);
        if let Some(child) = child_result {
            retained_children += 1;
            only_child = Some(child);
            needs_flattening |= matches!(
                workspace.nodes.get(child),
                Some(Node::Split { axis: child_axis, .. }) if *child_axis == axis
            );
        }
    }

    if retained_children < 2 {
        return Ok((only_child, true));
    }

    // Input weights are already normalized. Preserve unchanged storage and bits.
    if !topology_changed && !needs_flattening {
        return Ok((Some(node_id), false));
    }

    let mut flattened = Vec::with_capacity(retained_children);
    for (&child, &weight) in children.iter().zip(weights) {
        let Some(child) = results.get(child).copied().flatten() else {
            continue;
        };
        let parent_weight = weight.get();
        if let Some(Node::Split {
            axis: child_axis,
            children: grandchildren,
            weights: child_weights,
        }) = workspace.nodes.get(child)
            && *child_axis == axis
        {
            flattened.extend(
                grandchildren
                    .iter()
                    .copied()
                    .zip(child_weights.iter().copied())
                    .map(|(grandchild, child_weight)| {
                        (grandchild, parent_weight * child_weight.get())
                    }),
            );
        } else {
            flattened.push((child, parent_weight));
        }
    }

    match flattened.len() {
        0 => return Ok((None, true)),
        1 => return Ok((Some(flattened[0].0), true)),
        _ => {}
    }

    let children: Vec<NodeId> = flattened.iter().map(|(child, _)| *child).collect();
    let weights = SplitWeight::normalize(flattened.into_iter().map(|(_, weight)| weight)).map_err(
        |source| CanonicalizationError::InvalidNormalizedWeights {
            node: node_id,
            source,
        },
    )?;

    workspace
        .nodes
        .replace(
            node_id,
            Node::Split {
                axis,
                children,
                weights,
            },
        )
        .ok_or(CanonicalizationError::MissingChild {
            owner: None,
            child: node_id,
        })?;
    Ok((Some(node_id), true))
}

fn validate_rewrite_input(workspace: &WorkspaceData) -> Result<(), CanonicalizationError> {
    let parents = validate_nodes(workspace)?;
    validate_acyclic(workspace)?;
    validate_roots(workspace, &parents)
}

fn validate_split_weights(
    node: NodeId,
    children: &[NodeId],
    weights: &[SplitWeight],
) -> Result<(), CanonicalizationError> {
    if children.len() != weights.len() {
        return Err(CanonicalizationError::SplitWeightCountMismatch {
            node,
            children: children.len(),
            weights: weights.len(),
        });
    }

    let mut sum = 0.0_f64;
    for (index, weight) in weights.iter().copied().enumerate() {
        SplitWeight::new(weight.get()).map_err(|source| CanonicalizationError::InvalidWeight {
            node,
            index,
            source,
        })?;
        sum += f64::from(weight.get());
    }
    if (sum - 1.0).abs() > NORMALIZED_WEIGHT_TOLERANCE {
        return Err(CanonicalizationError::WeightsNotNormalized { node, sum });
    }
    Ok(())
}

fn validate_nodes(
    workspace: &WorkspaceData,
) -> Result<SecondaryMap<NodeId, NodeId>, CanonicalizationError> {
    let mut parents = SecondaryMap::<NodeId, NodeId>::new();
    let mut item_owners = BTreeMap::<ItemId, NodeId>::new();

    for (node_id, node) in workspace.nodes.iter() {
        match node {
            Node::Tabs { items, selected } => {
                let selection_is_valid = if items.is_empty() {
                    selected.is_none()
                } else {
                    selected.is_some_and(|selected| items.contains(&selected))
                };
                if !selection_is_valid {
                    return Err(CanonicalizationError::InvalidSelection {
                        node: node_id,
                        selected: *selected,
                    });
                }
                if !is_exact_tab_mru(
                    items,
                    *selected,
                    workspace.tab_mru.get(&node_id).map(Vec::as_slice),
                ) {
                    return Err(CanonicalizationError::InvalidTabMru { node: node_id });
                }

                for item in items {
                    if let Some(first) = item_owners.insert(*item, node_id) {
                        return Err(CanonicalizationError::DuplicateItem {
                            item: *item,
                            first,
                            second: node_id,
                        });
                    }
                }
            }
            Node::Split {
                children, weights, ..
            } => {
                validate_split_weights(node_id, children, weights)?;

                for child in children {
                    if !workspace.nodes.contains_key(*child) {
                        return Err(CanonicalizationError::MissingChild {
                            owner: Some(node_id),
                            child: *child,
                        });
                    }
                    if parents.insert(*child, node_id).is_some() {
                        return Err(CanonicalizationError::SharedNode { node: *child });
                    }
                }
            }
        }
    }

    if let Some((&node, _)) = workspace
        .tab_mru
        .iter()
        .find(|(node, _)| !matches!(workspace.nodes.get(**node), Some(Node::Tabs { .. })))
    {
        return Err(CanonicalizationError::InvalidTabMru { node });
    }

    Ok(parents)
}

fn validate_roots(
    workspace: &WorkspaceData,
    parents: &SecondaryMap<NodeId, NodeId>,
) -> Result<(), CanonicalizationError> {
    let mut root_nodes = HashMap::<NodeId, RootId>::new();
    for (root, record) in &workspace.roots {
        if !workspace.nodes.contains_key(record.node) {
            return Err(CanonicalizationError::MissingChild {
                owner: None,
                child: record.node,
            });
        }
        if parents.contains_key(record.node) {
            return Err(CanonicalizationError::RootHasParent {
                root: *root,
                node: record.node,
            });
        }
        if let Some(first) = root_nodes.insert(record.node, *root) {
            return Err(CanonicalizationError::SharedRootNode {
                first,
                second: *root,
                node: record.node,
            });
        }

        if let Some(central) = record.central {
            let Some(node) = workspace.nodes.get(central) else {
                return Err(CanonicalizationError::MissingChild {
                    owner: None,
                    child: central,
                });
            };
            if !matches!(node, Node::Tabs { .. }) {
                return Err(CanonicalizationError::CentralNotTabs {
                    root: *root,
                    central,
                });
            }

            let reachable = reachable_from(workspace, record.node)?;
            if !reachable.contains(&central) {
                return Err(CanonicalizationError::CentralOutsideRoot {
                    root: *root,
                    central,
                });
            }
        }
    }

    Ok(())
}

fn validate_acyclic(workspace: &WorkspaceData) -> Result<(), CanonicalizationError> {
    let mut in_degree: SecondaryMap<NodeId, usize> =
        workspace.nodes.keys().map(|node| (node, 0)).collect();
    for node in workspace.nodes.values() {
        if let Node::Split { children, .. } = node {
            for child in children {
                let degree =
                    in_degree
                        .get_mut(*child)
                        .ok_or(CanonicalizationError::MissingChild {
                            owner: None,
                            child: *child,
                        })?;
                *degree += 1;
            }
        }
    }

    let mut ready: VecDeque<NodeId> = in_degree
        .iter()
        .filter_map(|(node, degree)| (*degree == 0).then_some(node))
        .collect();
    let mut visited = 0_usize;
    while let Some(node_id) = ready.pop_front() {
        visited += 1;
        if let Some(Node::Split { children, .. }) = workspace.nodes.get(node_id) {
            for child in children {
                let degree =
                    in_degree
                        .get_mut(*child)
                        .ok_or(CanonicalizationError::MissingChild {
                            owner: Some(node_id),
                            child: *child,
                        })?;
                *degree =
                    degree
                        .checked_sub(1)
                        .ok_or(CanonicalizationError::TraversalInvariant {
                            stage: "cycle detection",
                        })?;
                if *degree == 0 {
                    ready.push_back(*child);
                }
            }
        }
    }

    if visited == workspace.nodes.len() {
        return Ok(());
    }

    let node = workspace
        .nodes
        .keys()
        .find(|node| in_degree.get(*node).is_some_and(|degree| *degree > 0))
        .ok_or(CanonicalizationError::TraversalInvariant {
            stage: "cycle reporting",
        })?;
    Err(CanonicalizationError::Cycle { node })
}

fn reachable_from_roots(
    workspace: &WorkspaceData,
) -> Result<SecondaryMap<NodeId, ()>, CanonicalizationError> {
    let mut reachable = SecondaryMap::with_capacity(workspace.nodes.capacity());
    let mut stack = Vec::new();
    for record in workspace.roots.values() {
        stack.push(record.node);
        while let Some(node) = stack.pop() {
            if reachable.contains_key(node) {
                continue;
            }
            let current = workspace
                .nodes
                .get(node)
                .ok_or(CanonicalizationError::MissingChild {
                    owner: None,
                    child: node,
                })?;
            reachable.insert(node, ());
            if let Node::Split { children, .. } = current {
                stack.extend(children.iter().rev().copied());
            }
        }
    }
    Ok(reachable)
}

fn reachable_from(
    workspace: &WorkspaceData,
    root: NodeId,
) -> Result<HashSet<NodeId>, CanonicalizationError> {
    let mut reachable = HashSet::new();
    collect_reachable(workspace, root, &mut reachable)?;
    Ok(reachable)
}

fn collect_reachable(
    workspace: &WorkspaceData,
    node: NodeId,
    reachable: &mut HashSet<NodeId>,
) -> Result<(), CanonicalizationError> {
    let mut stack = vec![node];
    while let Some(current_id) = stack.pop() {
        if !reachable.insert(current_id) {
            continue;
        }
        let current =
            workspace
                .nodes
                .get(current_id)
                .ok_or(CanonicalizationError::MissingChild {
                    owner: None,
                    child: current_id,
                })?;
        if let Node::Split { children, .. } = current {
            stack.extend(children.iter().rev().copied());
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::drop_resolver::structural_work::{self, WorkspaceCloneVolume};
    use crate::graph::{Axis, SurfacePresentation};
    use crate::ids::SurfaceId;
    use crate::validation::WorkspaceValidationError;

    const ROOT: RootId = RootId::new(1);

    fn singleton_draft() -> (WorkspaceBuilder, NodeId, NodeId) {
        let mut builder = Workspace::builder();
        let leaf = builder.insert_node(Node::tabs([ItemId::new(1)]));
        let wrapper = builder.insert_node(Node::Split {
            axis: Axis::Horizontal,
            children: vec![leaf],
            weights: vec![SplitWeight::new(1.0).expect("unit weight is valid")],
        });
        builder.set_root(ROOT, crate::graph::RootRecord::new(wrapper));
        (builder, leaf, wrapper)
    }

    #[test]
    fn atomic_wrapper_counts_its_full_clone_and_preserves_idempotent_reports() {
        let (mut builder, leaf, wrapper) = singleton_draft();
        builder.set_surface(SurfaceId::new(1), SurfacePresentation::with_main(ROOT));
        for changed in [true, false] {
            let volume = WorkspaceCloneVolume::capture_data(builder.draft());
            structural_work::reset();
            let report = builder.canonicalize().expect("draft is canonicalizable");
            assert_eq!(report.changed, changed);
            assert_eq!(report.removed_nodes, usize::from(changed));
            assert_eq!(builder.draft().root(ROOT).unwrap().node, leaf);
            assert!(builder.draft().node(wrapper).is_none());
            builder.validate().expect("normalized graph is strict");
            let clones = structural_work::snapshot().workspace_deep_clones;
            assert_eq!(clones.transaction_candidates.calls, 1);
            assert_eq!(clones.transaction_candidates.volume, volume);
        }
        builder
            .build()
            .expect("only checked data becomes a workspace");
    }

    #[test]
    fn late_failures_dirty_only_the_discardable_draft_and_preserve_error_order() {
        for orphan_item in [false, true] {
            let (mut builder, leaf, wrapper) = singleton_draft();
            if orphan_item {
                builder.insert_node(Node::tabs([ItemId::new(2)]));
            }
            let original = builder.draft().clone();
            let volume = WorkspaceCloneVolume::capture_data(builder.draft());
            let mut draft = builder.draft().clone();

            structural_work::reset();
            let error = canonicalize_data(&mut draft).expect_err("late check must fail");
            assert_eq!(draft.root(ROOT).expect("root survives").node, leaf);
            assert!(draft.node(wrapper).is_none());
            assert_ne!(draft, original);
            assert_eq!(
                structural_work::snapshot()
                    .workspace_deep_clones
                    .transaction_candidates
                    .calls,
                0
            );
            if orphan_item {
                assert!(matches!(
                    &error,
                    CanonicalizationError::ItemMultisetChanged { before, after }
                        if *before == BTreeMap::from([(ItemId::new(1), 1), (ItemId::new(2), 1)])
                            && *after == BTreeMap::from([(ItemId::new(1), 1)])
                ));
            } else {
                assert!(matches!(
                    &error,
                    CanonicalizationError::InvalidOutput(errors)
                        if matches!(errors.errors(), [WorkspaceValidationError::RootNotPresented { root: ROOT }])
                ));
            }

            for _ in 0..2 {
                structural_work::reset();
                assert_eq!(builder.canonicalize(), Err(error.clone()));
                assert_eq!(builder.draft(), &original);
                let clones = structural_work::snapshot().workspace_deep_clones;
                assert_eq!(clones.transaction_candidates.calls, 1);
                assert_eq!(clones.transaction_candidates.volume, volume);
            }
        }
    }

    #[test]
    fn invalid_input_is_rejected_before_rewriting_or_atomic_cloning() {
        let (mut builder, leaf, _) = singleton_draft();
        *builder.draft_mut().nodes.get_mut(leaf).unwrap() = Node::Tabs {
            items: vec![ItemId::new(1)],
            selected: Some(ItemId::new(2)),
        };
        let original = builder.draft().clone();
        let error = CanonicalizationError::InvalidSelection {
            node: leaf,
            selected: Some(ItemId::new(2)),
        };

        structural_work::reset();
        assert_eq!(builder.canonicalize(), Err(error.clone()));
        assert_eq!(canonicalize_data(builder.draft_mut()), Err(error));
        assert_eq!(builder.draft(), &original);
        assert_eq!(
            structural_work::snapshot()
                .workspace_deep_clones
                .transaction_candidates
                .calls,
            0
        );
    }
}
