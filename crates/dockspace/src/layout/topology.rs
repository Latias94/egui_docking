//! Fresh numeric measurements over graph-owned rows; raw diagnostics stay checked.

#[cfg(test)]
use crate::graph::root_structure::{RootNode, SharedSplit};
#[cfg(test)]
use std::collections::HashMap;

use super::*;

#[cfg(test)]
enum CheckedCompileFrame {
    Enter(NodeId),
    Exit(usize),
}

impl RootStructure {
    /// Retain malformed draft diagnostics without weakening runtime ownership.
    #[cfg(test)]
    pub(super) fn compile_draft(
        workspace: &WorkspaceData,
        root: RootId,
        leaves: LeafMeasurements<'_>,
        metrics: LayoutMetrics,
    ) -> Result<(Self, Vec<SubtreeMeasurement>), LayoutError> {
        let record = workspace
            .root(root)
            .ok_or(LayoutError::MissingRoot { root })?;
        let mut topology = Self::default();
        let mut rows = HashMap::<NodeId, usize>::new();
        let mut values = Vec::<Option<SubtreeMeasurement>>::new();
        let mut child_values = Vec::new();
        let mut pending = vec![CheckedCompileFrame::Enter(record.node)];
        while let Some(frame) = pending.pop() {
            match frame {
                CheckedCompileFrame::Enter(node) => {
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
                    topology.nodes.push(RootNode {
                        id: node,
                        kind: RootNodeKind::Tabs,
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
                            pending.push(CheckedCompileFrame::Exit(row));
                            pending.extend(
                                children
                                    .iter()
                                    .rev()
                                    .copied()
                                    .map(CheckedCompileFrame::Enter),
                            );
                        }
                    }
                }
                CheckedCompileFrame::Exit(row) => {
                    let node = topology.nodes[row].id;
                    let Some(Node::Split { axis, children, .. }) = workspace.node(node) else {
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
                    topology.nodes[row].kind = RootNodeKind::Split {
                        payload: SharedSplit::new(Arc::clone(
                            workspace
                                .nodes
                                .shared(node)
                                .ok_or(LayoutError::MissingNode { node })?,
                        )),
                        children: start..topology.children.len(),
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
        topology.check_central(root, record.central, &values)?;
        topology.finish_directory();
        Ok((topology, values))
    }

    pub(super) fn measure_with_reader(
        &self,
        root: RootId,
        central: Option<NodeId>,
        read_leaf: &mut impl FnMut(NodeId) -> Result<Constraints, LayoutError>,
        metrics: LayoutMetrics,
        minimums_only: bool,
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
                RootNodeKind::Tabs => SubtreeMeasurement {
                    constraints: read_leaf(node.id)?,
                    contains_central: central == Some(node.id),
                },
                RootNodeKind::Split {
                    payload, children, ..
                } => {
                    child_values.clear();
                    child_values.extend(
                        self.children[children.clone()]
                            .iter()
                            .map(|&row| values[row]),
                    );
                    SubtreeMeasurement {
                        constraints: compose_constraints(
                            node.id,
                            payload.axis(),
                            &child_values,
                            metrics,
                            minimums_only,
                        )?,
                        contains_central: child_values.iter().any(|child| child.contains_central),
                    }
                }
            };
        }
        self.check_central(root, central, &values)?;
        Ok(values)
    }

    fn check_central(
        &self,
        root: RootId,
        central: Option<NodeId>,
        values: &[SubtreeMeasurement],
    ) -> Result<(), LayoutError> {
        if let Some(central) = central
            && !values[0].contains_central
        {
            return Err(LayoutError::CentralNodeNotReachable { root, central });
        }
        Ok(())
    }
}
