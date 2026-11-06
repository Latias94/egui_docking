//! Dense structural rows shared by repeated measurements of one indexed root.

use std::collections::HashMap;
use std::ops::Range;

use super::*;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RootLayoutTopology {
    root: RootId,
    root_node: NodeId,
    central: Option<NodeId>,
    pub(super) nodes: Vec<LayoutNode>,
    pub(super) children: Vec<usize>,
    pub(super) weight_bits: Vec<u32>,
    pub(super) postorder: Vec<usize>,
    pub(super) ordered_rows: Vec<usize>,
    pub(super) split_rows: Vec<usize>,
    pub(super) splitter_count: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct LayoutNode {
    pub(super) id: NodeId,
    pub(super) kind: LayoutNodeKind,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum LayoutNodeKind {
    Tabs,
    Split {
        axis: Axis,
        children: Range<usize>,
        weights: Range<usize>,
        split: usize,
        splitters: Range<usize>,
    },
}

enum CompileFrame {
    Enter(NodeId),
    Exit(usize),
}

impl RootLayoutTopology {
    /// Compile while measuring, retaining the original depth-first error order.
    pub(super) fn compile(
        workspace: &WorkspaceData,
        root: RootId,
        leaves: LeafMeasurements<'_>,
        metrics: LayoutMetrics,
    ) -> Result<(Self, Vec<SubtreeMeasurement>), LayoutError> {
        let record = workspace
            .root(root)
            .ok_or(LayoutError::MissingRoot { root })?;
        let mut topology = Self {
            root,
            root_node: record.node,
            central: record.central,
            nodes: Vec::new(),
            children: Vec::new(),
            weight_bits: Vec::new(),
            postorder: Vec::new(),
            ordered_rows: Vec::new(),
            split_rows: Vec::new(),
            splitter_count: 0,
        };
        let mut rows = HashMap::<NodeId, usize>::new();
        let mut values = Vec::<Option<SubtreeMeasurement>>::new();
        let mut child_values = Vec::new();
        let mut pending = vec![CompileFrame::Enter(record.node)];
        while let Some(frame) = pending.pop() {
            match frame {
                CompileFrame::Enter(node) => {
                    if let Some(&row) = rows.get(&node) {
                        // A row is active until its measurement is complete.
                        if values[row].is_none() {
                            return Err(LayoutError::Cycle { node });
                        }
                        continue;
                    }
                    let live = workspace
                        .node(node)
                        .ok_or(LayoutError::MissingNode { node })?;
                    let row = topology.nodes.len();
                    topology.nodes.push(LayoutNode {
                        id: node,
                        kind: LayoutNodeKind::Tabs,
                    });
                    values.push(None);
                    rows.insert(node, row);
                    match live {
                        Node::Tabs { .. } => {
                            values[row] = Some(SubtreeMeasurement {
                                constraints: leaves.get(node)?,
                                contains_central: record.central == Some(node),
                            });
                            topology.postorder.push(row);
                        }
                        Node::Split {
                            children, weights, ..
                        } => {
                            if children.len() != weights.len() {
                                return Err(LayoutError::SplitCardinalityMismatch {
                                    node,
                                    children: children.len(),
                                    weights: weights.len(),
                                });
                            }
                            pending.push(CompileFrame::Exit(row));
                            pending.extend(children.iter().rev().copied().map(CompileFrame::Enter));
                        }
                    }
                }
                CompileFrame::Exit(row) => {
                    let node = topology.nodes[row].id;
                    let Some(Node::Split {
                        axis,
                        children,
                        weights,
                    }) = workspace.node(node)
                    else {
                        return Err(LayoutError::MissingNode { node });
                    };
                    child_values.clear();
                    let start = topology.children.len();
                    for child in children {
                        let child_row = *rows
                            .get(child)
                            .ok_or(LayoutError::MissingNode { node: *child })?;
                        child_values.push(
                            values[child_row].ok_or(LayoutError::MissingNode { node: *child })?,
                        );
                        topology.children.push(child_row);
                    }
                    let first_weight = topology.weight_bits.len();
                    topology
                        .weight_bits
                        .extend(weights.iter().map(|weight| weight.get().to_bits()));
                    values[row] = Some(SubtreeMeasurement {
                        constraints: compose_constraints(
                            node,
                            *axis,
                            &child_values,
                            metrics,
                            matches!(leaves, LeafMeasurements::Minimums(_)),
                        )?,
                        contains_central: child_values.iter().any(|child| child.contains_central),
                    });
                    let first_splitter = topology.splitter_count;
                    topology.splitter_count += children.len().saturating_sub(1);
                    topology.nodes[row].kind = LayoutNodeKind::Split {
                        axis: *axis,
                        children: start..topology.children.len(),
                        weights: first_weight..topology.weight_bits.len(),
                        split: topology.split_rows.len(),
                        splitters: first_splitter..topology.splitter_count,
                    };
                    topology.split_rows.push(row);
                    topology.postorder.push(row);
                }
            }
        }
        let values = values
            .into_iter()
            .zip(&topology.nodes)
            .map(|(value, node)| value.ok_or(LayoutError::MissingNode { node: node.id }))
            .collect::<Result<Vec<_>, _>>()?;
        topology.check_central(&values)?;
        topology.ordered_rows.extend(0..topology.nodes.len());
        topology
            .ordered_rows
            .sort_unstable_by_key(|&row| topology.nodes[row].id);
        topology.split_rows.clear();
        topology.split_rows.extend(
            topology
                .ordered_rows
                .iter()
                .copied()
                .filter(|&row| matches!(&topology.nodes[row].kind, LayoutNodeKind::Split { .. })),
        );
        Ok((topology, values))
    }

    pub(super) fn row(&self, node: &NodeId) -> Option<usize> {
        self.ordered_rows
            .binary_search_by_key(node, |&row| self.nodes[row].id)
            .ok()
            .map(|index| self.ordered_rows[index])
    }

    /// A version alone cannot authorize reuse after an internal same-version edit.
    pub(crate) fn matches(&self, workspace: &Workspace, root: RootId) -> bool {
        if root != self.root
            || workspace.root(root).is_none_or(|record| {
                record.node != self.root_node || record.central != self.central
            })
        {
            return false;
        }
        self.nodes
            .iter()
            .all(|row| match (&row.kind, workspace.node(row.id)) {
                (LayoutNodeKind::Tabs, Some(Node::Tabs { .. })) => true,
                (
                    LayoutNodeKind::Split {
                        axis,
                        children,
                        weights,
                        ..
                    },
                    Some(Node::Split {
                        axis: live_axis,
                        children: live_children,
                        weights: live_weights,
                    }),
                ) => {
                    axis == live_axis
                        && children.len() == live_children.len()
                        && weights.len() == live_weights.len()
                        && self.children[children.clone()]
                            .iter()
                            .zip(live_children)
                            .all(|(&row, live)| self.nodes[row].id == *live)
                        && self.weight_bits[weights.clone()]
                            .iter()
                            .zip(live_weights)
                            .all(|(bits, live)| *bits == live.get().to_bits())
                }
                _ => false,
            })
    }

    pub(super) fn measure(
        &self,
        leaves: LeafMeasurements<'_>,
        metrics: LayoutMetrics,
    ) -> Result<Vec<SubtreeMeasurement>, LayoutError> {
        let zero = LogicalSize::new(0.0, 0.0)?;
        let empty = SubtreeMeasurement {
            constraints: Constraints::new(zero, zero)?,
            contains_central: false,
        };
        let mut values = vec![empty; self.nodes.len()];
        let mut child_values = Vec::new();
        for &row in &self.postorder {
            let node = &self.nodes[row];
            values[row] = match &node.kind {
                LayoutNodeKind::Tabs => SubtreeMeasurement {
                    constraints: leaves.get(node.id)?,
                    contains_central: self.central == Some(node.id),
                },
                LayoutNodeKind::Split { axis, children, .. } => {
                    child_values.clear();
                    child_values.extend(
                        self.children[children.clone()]
                            .iter()
                            .map(|&row| values[row]),
                    );
                    SubtreeMeasurement {
                        constraints: compose_constraints(
                            node.id,
                            *axis,
                            &child_values,
                            metrics,
                            matches!(leaves, LeafMeasurements::Minimums(_)),
                        )?,
                        contains_central: child_values.iter().any(|child| child.contains_central),
                    }
                }
            };
        }
        self.check_central(&values)?;
        Ok(values)
    }

    fn check_central(&self, values: &[SubtreeMeasurement]) -> Result<(), LayoutError> {
        if let Some(central) = self.central
            && !values[0].contains_central
        {
            return Err(LayoutError::CentralNodeNotReachable {
                root: self.root,
                central,
            });
        }
        Ok(())
    }
}
