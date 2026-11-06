//! Frozen pre-refactor traversal used only as a differential test oracle.

use super::*;
use std::collections::BTreeSet;

/// One root's postorder measurements, bound to the workspace that produced them.
pub(crate) struct RootMeasurement<'workspace> {
    workspace: &'workspace Workspace,
    root: RootId,
    node: NodeId,
    metrics: LayoutMetrics,
    minimum: LogicalSize,
    measurements: BTreeMap<NodeId, SubtreeMeasurement>,
    /// Nonempty only when scene leaf maxima must be completed after bounds are known.
    minimum_postorder: Vec<NodeId>,
}

impl RootMeasurement<'_> {
    pub(crate) fn minimum(&self) -> LogicalSize {
        self.minimum
    }

    pub(crate) fn project(
        self,
        bounds: LogicalRect,
        weight_overrides: &[SplitWeightOverride<'_>],
    ) -> Result<LayoutProjection, LayoutError> {
        let weights =
            validate_split_weight_overrides(self.workspace, self.root, weight_overrides, None)?;
        self.project_with_weights(bounds, &weights)
    }

    pub(crate) fn project_with_index(
        self,
        bounds: LogicalRect,
        weight_overrides: &[SplitWeightOverride<'_>],
        index: &WorkspaceIndex,
        version: WorkspaceVersion,
    ) -> Result<LayoutProjection, LayoutError> {
        let weights = validate_split_weight_overrides(
            self.workspace,
            self.root,
            weight_overrides,
            Some((index, version)),
        )?;
        self.project_with_weights(bounds, &weights)
    }

    fn project_with_weights(
        mut self,
        bounds: LogicalRect,
        weight_overrides: &BTreeMap<NodeId, Vec<f64>>,
    ) -> Result<LayoutProjection, LayoutError> {
        if !self.minimum_postorder.is_empty() {
            // Contained chrome fixes bounds after measurement; only maxima need completion.
            let minimum = self.minimum();
            let leaf_maximum = LogicalSize::new(
                bounds.width().max(minimum.width()),
                bounds.height().max(minimum.height()),
            )?;
            for node in &self.minimum_postorder {
                let maximum = match self
                    .workspace
                    .node(*node)
                    .ok_or(LayoutError::MissingNode { node: *node })?
                {
                    Node::Tabs { .. } => leaf_maximum,
                    Node::Split { axis, children, .. } => {
                        let children = child_measurements(children, &self.measurements)?;
                        compose_maximum(
                            *axis,
                            &children,
                            total_splitter_extent(*node, children.len(), self.metrics)?,
                        )?
                    }
                };
                let measurement = self
                    .measurements
                    .get_mut(node)
                    .ok_or(LayoutError::MissingNode { node: *node })?;
                measurement.constraints = Constraints::new(measurement.constraints.min(), maximum)?;
            }
        }

        let mut projection = LayoutProjection {
            node_rects: BTreeMap::new(),
            node_constraints: BTreeMap::new(),
            splits: BTreeMap::new(),
        };
        project_subtree(
            self.workspace,
            self.node,
            bounds,
            self.metrics,
            &self.measurements,
            weight_overrides,
            &mut projection,
        )?;
        projection.node_constraints = self
            .measurements
            .into_iter()
            .map(|(node, measurement)| (node, measurement.constraints))
            .collect();
        Ok(projection)
    }
}

#[derive(Debug, Clone, Copy)]
enum MeasurementFrame<'workspace> {
    Enter(NodeId),
    Exit {
        node_id: NodeId,
        axis: Axis,
        children: &'workspace [NodeId],
    },
}

fn measure_root<'workspace>(
    workspace: &'workspace Workspace,
    root: RootId,
    leaves: LeafMeasurements<'_>,
    metrics: LayoutMetrics,
) -> Result<RootMeasurement<'workspace>, LayoutError> {
    let root_record = workspace
        .root(root)
        .ok_or(LayoutError::MissingRoot { root })?;
    let minimums_only = matches!(leaves, LeafMeasurements::Minimums(_));
    let mut measurements = BTreeMap::new();
    let mut visiting = BTreeSet::new();
    let mut minimum_postorder = Vec::new();
    let mut stack = vec![MeasurementFrame::Enter(root_record.node)];

    while let Some(frame) = stack.pop() {
        match frame {
            MeasurementFrame::Enter(current) => {
                if measurements.contains_key(&current) {
                    continue;
                }
                if !visiting.insert(current) {
                    return Err(LayoutError::Cycle { node: current });
                }

                let node = workspace
                    .node(current)
                    .ok_or(LayoutError::MissingNode { node: current })?;
                match node {
                    Node::Tabs { .. } => {
                        let measurement = SubtreeMeasurement {
                            constraints: leaves.get(current)?,
                            contains_central: root_record.central == Some(current),
                        };
                        visiting.remove(&current);
                        measurements.insert(current, measurement);
                        if minimums_only {
                            minimum_postorder.push(current);
                        }
                    }
                    Node::Split {
                        axis,
                        children,
                        weights,
                    } => {
                        if children.len() != weights.len() {
                            return Err(LayoutError::SplitCardinalityMismatch {
                                node: current,
                                children: children.len(),
                                weights: weights.len(),
                            });
                        }
                        stack.push(MeasurementFrame::Exit {
                            node_id: current,
                            axis: *axis,
                            children,
                        });
                        stack.extend(children.iter().rev().copied().map(MeasurementFrame::Enter));
                    }
                }
            }
            MeasurementFrame::Exit {
                node_id: current,
                axis,
                children,
            } => {
                let children = child_measurements(children, &measurements)?;
                let measurement = SubtreeMeasurement {
                    constraints: compose_constraints(
                        current,
                        axis,
                        &children,
                        metrics,
                        minimums_only,
                    )?,
                    contains_central: children.iter().any(|child| child.contains_central),
                };
                visiting.remove(&current);
                measurements.insert(current, measurement);
                if minimums_only {
                    minimum_postorder.push(current);
                }
            }
        }
    }

    let root_measurement = measurements
        .get(&root_record.node)
        .ok_or(LayoutError::MissingNode {
            node: root_record.node,
        })?;
    if let Some(central) = root_record.central
        && !root_measurement.contains_central
    {
        return Err(LayoutError::CentralNodeNotReachable { root, central });
    }
    let minimum = root_measurement.constraints.min();
    Ok(RootMeasurement {
        workspace,
        root,
        node: root_record.node,
        metrics,
        minimum,
        measurements,
        minimum_postorder,
    })
}

fn child_measurements(
    children: &[NodeId],
    measurements: &BTreeMap<NodeId, SubtreeMeasurement>,
) -> Result<Vec<SubtreeMeasurement>, LayoutError> {
    children
        .iter()
        .map(|child| {
            measurements
                .get(child)
                .copied()
                .ok_or(LayoutError::MissingNode { node: *child })
        })
        .collect()
}

struct SplitProjectionState<'workspace> {
    node_id: NodeId,
    axis: Axis,
    children: &'workspace [NodeId],
    sizes: Vec<f64>,
    next_child: usize,
    pending_child_size: Option<f64>,
    bounds: LogicalRect,
    cursor: f64,
    splitter_rects: Vec<LogicalRect>,
    child_minimum_extents: Vec<f64>,
    child_maximum_extents: Vec<f64>,
    central_index: Option<usize>,
    overflow: f64,
    unallocated: f64,
    splitter_thickness: f64,
}

enum ProjectionFrame<'workspace> {
    Enter {
        node_id: NodeId,
        bounds: LogicalRect,
    },
    ContinueSplit(SplitProjectionState<'workspace>),
}

fn project_subtree(
    workspace: &Workspace,
    node_id: NodeId,
    bounds: LogicalRect,
    metrics: LayoutMetrics,
    measurements: &BTreeMap<NodeId, SubtreeMeasurement>,
    weight_overrides: &BTreeMap<NodeId, Vec<f64>>,
    projection: &mut LayoutProjection,
) -> Result<(), LayoutError> {
    let mut stack = vec![ProjectionFrame::Enter { node_id, bounds }];

    while let Some(frame) = stack.pop() {
        match frame {
            ProjectionFrame::Enter {
                node_id: current,
                bounds: current_bounds,
            } => {
                if let Some(state) = prepare_split_projection(
                    workspace,
                    current,
                    current_bounds,
                    metrics,
                    measurements,
                    weight_overrides,
                    projection,
                )? {
                    stack.push(ProjectionFrame::ContinueSplit(state));
                }
            }
            ProjectionFrame::ContinueSplit(state) => {
                advance_split_projection(state, &mut stack, projection)?;
            }
        }
    }

    Ok(())
}

fn prepare_split_projection<'workspace>(
    workspace: &'workspace Workspace,
    node_id: NodeId,
    bounds: LogicalRect,
    metrics: LayoutMetrics,
    measurements: &BTreeMap<NodeId, SubtreeMeasurement>,
    weight_overrides: &BTreeMap<NodeId, Vec<f64>>,
    projection: &mut LayoutProjection,
) -> Result<Option<SplitProjectionState<'workspace>>, LayoutError> {
    projection.node_rects.insert(node_id, bounds);
    let node = workspace
        .node(node_id)
        .ok_or(LayoutError::MissingNode { node: node_id })?;
    let Node::Split {
        axis,
        children,
        weights,
    } = node
    else {
        return Ok(None);
    };
    if children.len() != weights.len() {
        return Err(LayoutError::SplitCardinalityMismatch {
            node: node_id,
            children: children.len(),
            weights: weights.len(),
        });
    }

    let child_measurements = child_measurements(children, measurements)?;
    let axis_constraints = child_measurements
        .iter()
        .map(|child| match axis {
            Axis::Horizontal => AxisConstraint {
                min: child.constraints.min().width(),
                max: child.constraints.max().width(),
            },
            Axis::Vertical => AxisConstraint {
                min: child.constraints.min().height(),
                max: child.constraints.max().height(),
            },
        })
        .collect::<Vec<_>>();
    let child_minimum_extents = axis_constraints
        .iter()
        .map(|constraint| constraint.min())
        .collect();
    let child_maximum_extents = axis_constraints
        .iter()
        .map(|constraint| constraint.max())
        .collect();
    let axis_weights = weight_overrides.get(&node_id).cloned().unwrap_or_else(|| {
        weights
            .iter()
            .map(|weight| f64::from(weight.get()))
            .collect()
    });
    let central_index = child_measurements
        .iter()
        .position(|child| child.contains_central);
    let extent = match axis {
        Axis::Horizontal => bounds.width(),
        Axis::Vertical => bounds.height(),
    };
    let (splitter_extent, splitter_thickness) =
        projected_splitter_geometry(node_id, children.len(), extent, metrics)?;
    let solved = solve_axis(
        extent - splitter_extent,
        &axis_weights,
        &axis_constraints,
        central_index,
    )?;
    let cursor = match axis {
        Axis::Horizontal => bounds.x(),
        Axis::Vertical => bounds.y(),
    };
    Ok(Some(SplitProjectionState {
        node_id,
        axis: *axis,
        children,
        sizes: solved.sizes,
        next_child: 0,
        pending_child_size: None,
        bounds,
        cursor,
        splitter_rects: Vec::with_capacity(children.len().saturating_sub(1)),
        child_minimum_extents,
        child_maximum_extents,
        central_index,
        overflow: solved.overflow,
        unallocated: solved.unallocated,
        splitter_thickness,
    }))
}

fn projected_splitter_geometry(
    node: NodeId,
    child_count: usize,
    available: f64,
    metrics: LayoutMetrics,
) -> Result<(f64, f64), LayoutError> {
    let configured_extent = total_splitter_extent(node, child_count, metrics)?;
    if configured_extent <= available {
        return Ok((configured_extent, metrics.splitter_thickness()));
    }

    let splitter_count = child_count.saturating_sub(1);
    debug_assert!(splitter_count > 0);
    #[allow(
        clippy::cast_precision_loss,
        reason = "an in-memory child count is exactly representable throughout any feasible workspace"
    )]
    let splitter_count = splitter_count as f64;
    Ok((available, available / splitter_count))
}

fn advance_split_projection<'workspace>(
    mut state: SplitProjectionState<'workspace>,
    stack: &mut Vec<ProjectionFrame<'workspace>>,
    projection: &mut LayoutProjection,
) -> Result<(), LayoutError> {
    if let Some(size) = state.pending_child_size.take() {
        state.cursor += size;
        if state.next_child < state.children.len() {
            let splitter = match state.axis {
                Axis::Horizontal => LogicalRect::new(
                    state.cursor,
                    state.bounds.y(),
                    state.splitter_thickness,
                    state.bounds.height(),
                )?,
                Axis::Vertical => LogicalRect::new(
                    state.bounds.x(),
                    state.cursor,
                    state.bounds.width(),
                    state.splitter_thickness,
                )?,
            };
            state.splitter_rects.push(splitter);
            state.cursor += state.splitter_thickness;
        }
        stack.push(ProjectionFrame::ContinueSplit(state));
        return Ok(());
    }

    if state.next_child == state.children.len() {
        projection.splits.insert(
            state.node_id,
            SplitProjection {
                axis: state.axis,
                splitter_rects: state.splitter_rects,
                child_minimum_extents: state.child_minimum_extents,
                child_maximum_extents: state.child_maximum_extents,
                child_extents: state.sizes,
                central_index: state.central_index,
                overflow: state.overflow,
                unallocated: state.unallocated,
            },
        );
        return Ok(());
    }

    let child = state.children.get(state.next_child).copied().ok_or(
        LayoutError::SplitCardinalityMismatch {
            node: state.node_id,
            children: state.children.len(),
            weights: state.sizes.len(),
        },
    )?;
    let size = state.sizes.get(state.next_child).copied().ok_or(
        LayoutError::SplitCardinalityMismatch {
            node: state.node_id,
            children: state.children.len(),
            weights: state.sizes.len(),
        },
    )?;
    let child_bounds = match state.axis {
        Axis::Horizontal => {
            LogicalRect::new(state.cursor, state.bounds.y(), size, state.bounds.height())?
        }
        Axis::Vertical => {
            LogicalRect::new(state.bounds.x(), state.cursor, state.bounds.width(), size)?
        }
    };
    state.next_child += 1;
    state.pending_child_size = Some(size);
    stack.push(ProjectionFrame::ContinueSplit(state));
    stack.push(ProjectionFrame::Enter {
        node_id: child,
        bounds: child_bounds,
    });
    Ok(())
}

pub(super) fn project(
    workspace: &Workspace,
    root: RootId,
    bounds: LogicalRect,
    leaves: LeafMeasurements<'_>,
    metrics: LayoutMetrics,
    overrides: &[SplitWeightOverride<'_>],
) -> Result<LayoutProjection, LayoutError> {
    match leaves {
        LeafMeasurements::Minimums(_) => {
            measure_root(workspace, root, leaves, metrics)?.project(bounds, overrides)
        }
        LeafMeasurements::Constraints(_) => {
            workspace
                .root(root)
                .ok_or(LayoutError::MissingRoot { root })?;
            let weights = validate_split_weight_overrides(workspace, root, overrides, None)?;
            measure_root(workspace, root, leaves, metrics)?.project_with_weights(bounds, &weights)
        }
    }
}
