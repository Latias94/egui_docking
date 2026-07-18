//! Deterministic canonicalization for durable docking forests.
//!
//! The topology semantics are informed by Open GPUI's `graph_canonical.rs`
//! (Apache-2.0) and Dockview's VS Code-derived grid normalization (MIT). This
//! implementation was rewritten against dockspace's renderer-neutral model;
//! no source code was copied.

use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};

use thiserror::Error;

use crate::graph::{
    InvalidSplitWeight, InvalidSplitWeights, NORMALIZED_WEIGHT_TOLERANCE, Node, SplitWeight,
    Workspace, WorkspaceBuilder,
};
use crate::ids::{ItemId, NodeId, RootId};
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
    /// An internal iterative traversal invariant was violated.
    #[error("canonicalization traversal invariant failed during {stage}")]
    TraversalInvariant {
        /// Traversal phase that could not make deterministic progress.
        stage: &'static str,
    },
}

/// Rewrites a structurally sound workspace into its unique canonical form.
///
/// The operation is atomic: every check and rewrite runs against a clone, and
/// `workspace` is replaced only after strict validation and an item-multiset
/// comparison succeed. Valid unreachable arena nodes are swept; an orphan that
/// contains an item is rejected instead of silently discarding pane ownership.
///
/// # Errors
///
/// Returns [`CanonicalizationError`] when the input graph is structurally
/// invalid, canonical rewriting would change item ownership, or the complete
/// rewritten workspace fails strict validation.
pub fn canonicalize_workspace(
    workspace: &mut Workspace,
) -> Result<CanonicalizationReport, CanonicalizationError> {
    validate_rewrite_input(workspace)?;

    let original_items = workspace.item_multiset();
    let original_node_count = workspace.nodes.len();
    let protected_central: HashSet<NodeId> = workspace
        .roots
        .values()
        .filter_map(|root| root.central)
        .collect();

    let mut candidate = workspace.clone();
    let roots: Vec<(RootId, NodeId)> = candidate
        .roots
        .iter()
        .map(|(root, record)| (*root, record.node))
        .collect();

    for (root, node) in roots {
        let next = rewrite_subtree(&mut candidate, node, &protected_central)?
            .ok_or(CanonicalizationError::EmptyRoot { root })?;
        if let Some(record) = candidate.roots.get_mut(&root) {
            record.node = next;
        }
    }

    let reachable = reachable_from_roots(&candidate)?;
    candidate.nodes.retain(|node, _| reachable.contains(&node));

    let candidate_items = candidate.item_multiset();
    if candidate_items != original_items {
        return Err(CanonicalizationError::ItemMultisetChanged {
            before: original_items,
            after: candidate_items,
        });
    }

    candidate
        .validate_candidate()
        .map_err(CanonicalizationError::InvalidOutput)?;

    let report = CanonicalizationReport {
        changed: candidate != *workspace,
        removed_nodes: original_node_count.saturating_sub(candidate.nodes.len()),
    };
    *workspace = candidate;
    Ok(report)
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
        canonicalize_workspace(&mut self.workspace)
    }
}

fn rewrite_subtree(
    workspace: &mut Workspace,
    node_id: NodeId,
    protected_central: &HashSet<NodeId>,
) -> Result<Option<NodeId>, CanonicalizationError> {
    let mut stack = vec![(node_id, false)];
    let mut results = HashMap::<NodeId, Option<NodeId>>::new();

    while let Some((current_id, exiting)) = stack.pop() {
        let current = workspace.nodes.get(current_id).cloned().ok_or(
            CanonicalizationError::MissingChild {
                owner: None,
                child: current_id,
            },
        )?;

        if !exiting {
            stack.push((current_id, true));
            if let Node::Split { children, .. } = current {
                stack.extend(children.into_iter().rev().map(|child| (child, false)));
            }
            continue;
        }

        let result = match current {
            Node::Tabs { items, .. } => {
                if items.is_empty() && !protected_central.contains(&current_id) {
                    None
                } else {
                    Some(current_id)
                }
            }
            Node::Split {
                axis,
                children,
                weights,
            } => rewrite_split(workspace, current_id, axis, children, weights, &results)?,
        };
        results.insert(current_id, result);
    }

    results
        .remove(&node_id)
        .ok_or(CanonicalizationError::MissingChild {
            owner: None,
            child: node_id,
        })
}

fn rewrite_split(
    workspace: &mut Workspace,
    node_id: NodeId,
    axis: crate::graph::Axis,
    children: Vec<NodeId>,
    weights: Vec<SplitWeight>,
    results: &HashMap<NodeId, Option<NodeId>>,
) -> Result<Option<NodeId>, CanonicalizationError> {
    let mut rewritten = Vec::with_capacity(children.len());
    for (child, weight) in children.into_iter().zip(weights) {
        let child_result =
            results
                .get(&child)
                .copied()
                .ok_or(CanonicalizationError::MissingChild {
                    owner: Some(node_id),
                    child,
                })?;
        if let Some(child) = child_result {
            rewritten.push((child, weight.get()));
        }
    }

    match rewritten.len() {
        0 => return Ok(None),
        1 => return Ok(Some(rewritten[0].0)),
        _ => {}
    }

    let mut flattened = Vec::with_capacity(rewritten.len());
    for (child, parent_weight) in rewritten {
        let same_axis = match workspace.nodes.get(child) {
            Some(Node::Split {
                axis: child_axis,
                children,
                weights,
            }) if *child_axis == axis => Some((children.clone(), weights.clone())),
            _ => None,
        };

        if let Some((grandchildren, child_weights)) = same_axis {
            flattened.extend(grandchildren.into_iter().zip(child_weights).map(
                |(grandchild, child_weight)| (grandchild, parent_weight * child_weight.get()),
            ));
        } else {
            flattened.push((child, parent_weight));
        }
    }

    match flattened.len() {
        0 => return Ok(None),
        1 => return Ok(Some(flattened[0].0)),
        _ => {}
    }

    let children: Vec<NodeId> = flattened.iter().map(|(child, _)| *child).collect();
    let weights = SplitWeight::normalize(flattened.into_iter().map(|(_, weight)| weight)).map_err(
        |source| CanonicalizationError::InvalidNormalizedWeights {
            node: node_id,
            source,
        },
    )?;

    let slot = workspace
        .nodes
        .get_mut(node_id)
        .ok_or(CanonicalizationError::MissingChild {
            owner: None,
            child: node_id,
        })?;
    *slot = Node::Split {
        axis,
        children,
        weights,
    };
    Ok(Some(node_id))
}

fn validate_rewrite_input(workspace: &Workspace) -> Result<(), CanonicalizationError> {
    let parents = validate_nodes(workspace)?;
    validate_acyclic(workspace)?;
    validate_roots(workspace, &parents)
}

fn validate_nodes(workspace: &Workspace) -> Result<HashMap<NodeId, NodeId>, CanonicalizationError> {
    let mut parents = HashMap::<NodeId, NodeId>::new();
    let mut item_owners = BTreeMap::<ItemId, NodeId>::new();

    for (node_id, node) in &workspace.nodes {
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
                if children.len() != weights.len() {
                    return Err(CanonicalizationError::SplitWeightCountMismatch {
                        node: node_id,
                        children: children.len(),
                        weights: weights.len(),
                    });
                }

                let mut sum = 0.0_f64;
                for (index, weight) in weights.iter().copied().enumerate() {
                    SplitWeight::new(weight.get()).map_err(|source| {
                        CanonicalizationError::InvalidWeight {
                            node: node_id,
                            index,
                            source,
                        }
                    })?;
                    sum += f64::from(weight.get());
                }
                if (sum - 1.0).abs() > NORMALIZED_WEIGHT_TOLERANCE {
                    return Err(CanonicalizationError::WeightsNotNormalized { node: node_id, sum });
                }

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

    Ok(parents)
}

fn validate_roots(
    workspace: &Workspace,
    parents: &HashMap<NodeId, NodeId>,
) -> Result<(), CanonicalizationError> {
    let mut root_nodes = HashMap::<NodeId, RootId>::new();
    for (root, record) in &workspace.roots {
        if !workspace.nodes.contains_key(record.node) {
            return Err(CanonicalizationError::MissingChild {
                owner: None,
                child: record.node,
            });
        }
        if parents.contains_key(&record.node) {
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

fn validate_acyclic(workspace: &Workspace) -> Result<(), CanonicalizationError> {
    let mut in_degree: HashMap<NodeId, usize> =
        workspace.nodes.keys().map(|node| (node, 0)).collect();
    for node in workspace.nodes.values() {
        if let Node::Split { children, .. } = node {
            for child in children {
                let degree =
                    in_degree
                        .get_mut(child)
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
        .filter_map(|(node, degree)| (*degree == 0).then_some(*node))
        .collect();
    let mut visited = 0_usize;
    while let Some(node_id) = ready.pop_front() {
        visited += 1;
        if let Some(Node::Split { children, .. }) = workspace.nodes.get(node_id) {
            for child in children {
                let degree =
                    in_degree
                        .get_mut(child)
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
        .find(|node| in_degree.get(node).is_some_and(|degree| *degree > 0))
        .ok_or(CanonicalizationError::TraversalInvariant {
            stage: "cycle reporting",
        })?;
    Err(CanonicalizationError::Cycle { node })
}

fn reachable_from_roots(workspace: &Workspace) -> Result<HashSet<NodeId>, CanonicalizationError> {
    let mut reachable = HashSet::new();
    for record in workspace.roots.values() {
        collect_reachable(workspace, record.node, &mut reachable)?;
    }
    Ok(reachable)
}

fn reachable_from(
    workspace: &Workspace,
    root: NodeId,
) -> Result<HashSet<NodeId>, CanonicalizationError> {
    let mut reachable = HashSet::new();
    collect_reachable(workspace, root, &mut reachable)?;
    Ok(reachable)
}

fn collect_reachable(
    workspace: &Workspace,
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
