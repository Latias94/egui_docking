//! Current root rows sharing the graph owner's immutable split payloads.

use std::ops::Range;
use std::sync::Arc;

use slotmap::SparseSecondaryMap;

use super::authority::NodePlacement;
use super::{Axis, Node, SplitWeight, WorkspaceData};
use crate::graph::error::{CommandError, ReferenceRole};
use crate::graph::workspace::ParentLink;
use crate::ids::{NodeId, RootId};

/// One exact immutable split value, shared with its current arena handle.
#[derive(Debug)]
pub(crate) struct SharedSplit(Arc<Node>);

impl SharedSplit {
    pub(crate) fn new(node: Arc<Node>) -> Self {
        debug_assert!(matches!(node.as_ref(), Node::Split { .. }));
        Self(node)
    }

    pub(crate) fn axis(&self) -> Axis {
        let Node::Split { axis, .. } = self.0.as_ref() else {
            unreachable!("shared split contains a split payload")
        };
        *axis
    }

    pub(crate) fn weights(&self) -> &[SplitWeight] {
        let Node::Split { weights, .. } = self.0.as_ref() else {
            unreachable!("shared split contains a split payload")
        };
        weights
    }

    pub(crate) fn children(&self) -> &[NodeId] {
        let Node::Split { children, .. } = self.0.as_ref() else {
            unreachable!("shared split contains a split payload")
        };
        children
    }

    #[cfg(test)]
    pub(super) fn shares_payload(&self, other: &Arc<Node>) -> bool {
        Arc::ptr_eq(&self.0, other)
    }
}

#[derive(Debug)]
pub(crate) struct RootNode {
    pub(crate) id: NodeId,
    pub(crate) kind: RootNodeKind,
}

#[derive(Debug)]
pub(crate) enum RootNodeKind {
    Tabs,
    Split {
        payload: SharedSplit,
        children: Range<usize>,
        split: usize,
    },
}

/// Flat, root-local joins over the graph's actual immutable payloads.
/// Tabs retain identities only; selection and item ownership remain live graph data.
#[derive(Debug, Default)]
pub(crate) struct RootStructure {
    pub(crate) nodes: Vec<RootNode>,
    pub(crate) children: Vec<usize>,
    pub(crate) postorder: Vec<usize>,
    pub(crate) ordered_rows: Vec<usize>,
    pub(crate) split_rows: Vec<usize>,
}

enum BuildFrame {
    Enter {
        node: NodeId,
        parent: Option<ParentLink>,
        child_slot: Option<usize>,
    },
    Exit(usize),
}

impl RootStructure {
    /// Install actual rows and placement together at graph completion.
    /// The caller owns a checked runtime prefix or a validated Builder result.
    pub(super) fn build(
        storage: &WorkspaceData,
        root: RootId,
        placements: &mut SparseSecondaryMap<NodeId, NodePlacement>,
    ) -> Result<Self, CommandError> {
        Self::build_rows(storage, root, 0, |node, parent| {
            placements.insert(node, NodePlacement { root, parent });
        })
    }

    /// The checked Item writer has already repaired actual placements.
    pub(super) fn build_after_item_move(
        storage: &WorkspaceData,
        root: RootId,
        previous_nodes: usize,
    ) -> Result<Self, CommandError> {
        // One Item move can add a tabs node and one split wrapper to a root.
        let node_capacity_hint = if previous_nodes == 0 {
            0
        } else {
            previous_nodes.saturating_add(2)
        };
        Self::build_rows(storage, root, node_capacity_hint, |_, _| {})
    }

    fn build_rows(
        storage: &WorkspaceData,
        root: RootId,
        node_capacity_hint: usize,
        mut on_node: impl FnMut(NodeId, Option<ParentLink>),
    ) -> Result<Self, CommandError> {
        let record = storage
            .roots
            .get(&root)
            .ok_or(CommandError::MissingRoot { root })?;
        let mut structure = Self::default();
        let mut pending = vec![BuildFrame::Enter {
            node: record.node,
            parent: None,
            child_slot: None,
        }];
        while let Some(frame) = pending.pop() {
            match frame {
                BuildFrame::Enter {
                    node,
                    parent,
                    child_slot,
                } => {
                    let value = storage
                        .nodes
                        .shared(node)
                        .ok_or(CommandError::MissingNode {
                            role: ReferenceRole::Source,
                            node,
                        })?;
                    let row = structure.nodes.len();
                    if row == 0 && node_capacity_hint != 0 {
                        let capacity = match value.as_ref() {
                            Node::Tabs { .. } => 1,
                            Node::Split { .. } => {
                                let edges = node_capacity_hint.saturating_sub(1);
                                structure.children.reserve(edges);
                                structure.split_rows.reserve(edges / 2);
                                node_capacity_hint
                            }
                        };
                        structure.nodes.reserve(capacity);
                        structure.postorder.reserve(capacity);
                        structure.ordered_rows.reserve(capacity);
                    }
                    structure.nodes.push(RootNode {
                        id: node,
                        kind: RootNodeKind::Tabs,
                    });
                    on_node(node, parent);
                    if let Some(slot) = child_slot {
                        structure.children[slot] = row;
                    }
                    match value.as_ref() {
                        Node::Tabs { .. } => structure.postorder.push(row),
                        Node::Split {
                            children, weights, ..
                        } => {
                            debug_assert_eq!(children.len(), weights.len());
                            let payload = SharedSplit::new(Arc::clone(value));
                            let child_count = payload.children().len();
                            let start = structure.children.len();
                            structure.children.resize(start + child_count, 0);
                            structure.nodes[row].kind = RootNodeKind::Split {
                                payload,
                                children: start..structure.children.len(),
                                split: structure.split_rows.len(),
                            };
                            structure.split_rows.push(row);
                            pending.push(BuildFrame::Exit(row));
                            pending.extend(children.iter().copied().enumerate().rev().map(
                                |(index, child)| BuildFrame::Enter {
                                    node: child,
                                    parent: Some(ParentLink {
                                        parent: node,
                                        index,
                                    }),
                                    child_slot: Some(start + index),
                                },
                            ));
                        }
                    }
                }
                BuildFrame::Exit(row) => structure.postorder.push(row),
            }
        }
        structure.finish_directory();
        #[cfg(test)]
        crate::interaction::targeting::resolver::structural_work::record_root_structure_completed(
            structure.nodes.len(),
            structure.split_rows.len(),
        );
        Ok(structure)
    }

    pub(crate) fn finish_directory(&mut self) {
        let nodes = &self.nodes;
        self.ordered_rows.extend(0..nodes.len());
        self.ordered_rows.sort_unstable_by_key(|&row| nodes[row].id);
        self.split_rows.clear();
        self.split_rows.extend(
            self.ordered_rows
                .iter()
                .copied()
                .filter(|&row| matches!(&nodes[row].kind, RootNodeKind::Split { .. })),
        );
    }

    pub(crate) fn row(&self, node: &NodeId) -> Option<usize> {
        self.ordered_rows
            .binary_search_by_key(node, |&row| self.nodes[row].id)
            .ok()
            .map(|index| self.ordered_rows[index])
    }
}
