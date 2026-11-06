//! Frozen pre-refactor traversal used only as a differential test oracle.

use super::*;
use std::collections::BTreeSet;

/// Geometry and constraint outcomes for one split node.
#[derive(Debug, Clone, PartialEq)]
pub(super) struct SplitProjection {
    /// Direction in which child rectangles were allocated.
    pub axis: Axis,
    /// Logical rectangles reserved between adjacent children.
    pub splitter_rects: Vec<LogicalRect>,
    /// Minimum projected extent for each direct child along this split's axis.
    pub child_minimum_extents: Vec<f64>,
    /// Maximum projected extent for each direct child along this split's axis.
    pub child_maximum_extents: Vec<f64>,
    /// Actual projected extent for each direct child along this split's axis.
    pub child_extents: Vec<f64>,
    /// Direct child which contains the root's declared central leaf, when any.
    pub central_index: Option<usize>,
    /// Extent by which child minima exceed the split bounds.
    pub overflow: f64,
    /// Extent left unused after every child reached its maximum.
    pub unallocated: f64,
}

/// Complete deterministic projection of one workspace root.
#[derive(Debug, Clone, PartialEq)]
pub(super) struct LayoutProjection {
    /// Logical rectangle assigned to every reachable node.
    pub node_rects: BTreeMap<NodeId, LogicalRect>,
    /// Composed minimum and maximum logical sizes for every reachable node.
    pub node_constraints: BTreeMap<NodeId, Constraints>,
    /// Split-specific outcomes keyed by split node identity.
    pub splits: BTreeMap<NodeId, SplitProjection>,
}

/// Copies actual output into the frozen oracle's independent owned value types.
pub(super) fn capture(projection: &super::LayoutProjection) -> LayoutProjection {
    LayoutProjection {
        node_rects: projection
            .node_rects
            .iter()
            .map(|(&node, &rect)| (node, rect))
            .collect(),
        node_constraints: projection
            .node_constraints
            .iter()
            .map(|(&node, &constraints)| (node, constraints))
            .collect(),
        splits: projection
            .splits
            .iter()
            .map(|(&node, split)| {
                (
                    node,
                    SplitProjection {
                        axis: split.axis,
                        splitter_rects: split.splitter_rects.to_vec(),
                        child_minimum_extents: split.child_minimum_extents.to_vec(),
                        child_maximum_extents: split.child_maximum_extents.to_vec(),
                        child_extents: split.child_extents.to_vec(),
                        central_index: split.central_index,
                        overflow: split.overflow,
                        unallocated: split.unallocated,
                    },
                )
            })
            .collect(),
    }
}

pub(super) fn capture_result(
    result: &Result<super::LayoutProjection, LayoutError>,
) -> Result<LayoutProjection, LayoutError> {
    result.as_ref().map(capture).map_err(Clone::clone)
}

/// One root's postorder measurements, bound to the workspace that produced them.
pub(crate) struct RootMeasurement<'workspace> {
    workspace: &'workspace WorkspaceData,
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
    workspace: &'workspace WorkspaceData,
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
    workspace: &WorkspaceData,
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
    workspace: &'workspace WorkspaceData,
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
            let measured = measure_root(workspace.storage(), root, leaves, metrics)?;
            let weights = validate_split_weight_overrides(workspace, root, overrides, None)?;
            measured.project_with_weights(bounds, &weights)
        }
        LeafMeasurements::Constraints(_) => {
            workspace
                .root(root)
                .ok_or(LayoutError::MissingRoot { root })?;
            let weights = validate_split_weight_overrides(workspace, root, overrides, None)?;
            measure_root(workspace.storage(), root, leaves, metrics)?
                .project_with_weights(bounds, &weights)
        }
    }
}

pub(super) fn project_draft(
    draft: &crate::graph::WorkspaceBuilder,
    root: RootId,
    bounds: LogicalRect,
    leaves: LeafMeasurements<'_>,
    metrics: LayoutMetrics,
) -> Result<LayoutProjection, LayoutError> {
    measure_root(draft.draft(), root, leaves, metrics)?
        .project_with_weights(bounds, &BTreeMap::new())
}

// Keep the numerical implementation independent from production allocation changes.
pub(super) fn solve_axis(
    extent: f64,
    weights: &[f64],
    constraints: &[AxisConstraint],
    central_index: Option<usize>,
) -> Result<AxisLayout, LayoutError> {
    validate_inputs(extent, weights, constraints, central_index)?;

    if weights.is_empty() {
        return Ok(AxisLayout {
            sizes: Vec::new(),
            overflow: 0.0,
            unallocated: extent,
        });
    }

    let min_total = checked_sum(
        constraints.iter().map(|constraint| constraint.min),
        "minimum",
    )?;
    let max_total = checked_sum(
        constraints.iter().map(|constraint| constraint.max),
        "maximum",
    )?;

    if min_total > extent {
        return Ok(AxisLayout {
            sizes: constraints
                .iter()
                .map(|constraint| constraint.min)
                .collect(),
            overflow: min_total - extent,
            unallocated: 0.0,
        });
    }
    if max_total < extent {
        return Ok(AxisLayout {
            sizes: constraints
                .iter()
                .map(|constraint| constraint.max)
                .collect(),
            overflow: 0.0,
            unallocated: extent - max_total,
        });
    }

    let desired = desired_sizes(extent, weights, central_index)?;
    let mut sizes = desired
        .into_iter()
        .zip(constraints)
        .map(|(size, constraint)| size.clamp(constraint.min, constraint.max))
        .collect::<Vec<_>>();

    let assigned = checked_sum(sizes.iter().copied(), "assigned size")?;
    if assigned < extent {
        grow_to_extent(
            &mut sizes,
            extent - assigned,
            weights,
            constraints,
            central_index,
        );
    } else if assigned > extent {
        shrink_to_extent(
            &mut sizes,
            assigned - extent,
            weights,
            constraints,
            central_index,
        );
    }

    let assigned = checked_sum(sizes.iter().copied(), "assigned size")?;
    let tolerance = allocation_tolerance(extent, min_total, max_total);
    let overflow = if assigned > extent + tolerance {
        assigned - extent
    } else {
        0.0
    };
    let unallocated = if assigned + tolerance < extent {
        extent - assigned
    } else {
        0.0
    };

    Ok(AxisLayout {
        sizes,
        overflow,
        unallocated,
    })
}

fn validate_inputs(
    extent: f64,
    weights: &[f64],
    constraints: &[AxisConstraint],
    central_index: Option<usize>,
) -> Result<(), LayoutError> {
    if !extent.is_finite() {
        return Err(LayoutError::NonFiniteExtent { value: extent });
    }
    if extent < 0.0 {
        return Err(LayoutError::NegativeExtent { value: extent });
    }
    if weights.len() != constraints.len() {
        return Err(LayoutError::LengthMismatch {
            weights: weights.len(),
            constraints: constraints.len(),
        });
    }
    if let Some(index) = central_index
        && index >= weights.len()
    {
        return Err(LayoutError::CentralIndexOutOfBounds {
            index,
            child_count: weights.len(),
        });
    }

    for (index, &value) in weights.iter().enumerate() {
        if !value.is_finite() {
            return Err(LayoutError::NonFiniteWeight { index, value });
        }
        if value < 0.0 {
            return Err(LayoutError::NegativeWeight { index, value });
        }
    }

    let weight_total = checked_sum(weights.iter().copied(), "weight")?;
    if central_index.is_none() && !weights.is_empty() && weight_total <= 0.0 {
        return Err(LayoutError::ZeroWeightTotal);
    }

    Ok(())
}

fn desired_sizes(
    extent: f64,
    weights: &[f64],
    central_index: Option<usize>,
) -> Result<Vec<f64>, LayoutError> {
    if let Some(central) = central_index {
        let sibling_total = checked_sum(
            weights
                .iter()
                .enumerate()
                .filter_map(|(index, weight)| (index != central).then_some(*weight)),
            "sibling weight",
        )?;
        let scale = if sibling_total > 1.0 {
            extent / sibling_total
        } else {
            extent
        };
        let mut sizes = weights
            .iter()
            .enumerate()
            .map(|(index, weight)| {
                if index == central {
                    0.0
                } else {
                    weight * scale
                }
            })
            .collect::<Vec<_>>();
        let siblings = checked_sum(sizes.iter().copied(), "desired size")?;
        sizes[central] = (extent - siblings).max(0.0);
        return Ok(sizes);
    }

    let weight_total = checked_sum(weights.iter().copied(), "weight")?;
    Ok(weights
        .iter()
        .map(|weight| extent * (*weight / weight_total))
        .collect())
}

fn grow_to_extent(
    sizes: &mut [f64],
    mut remainder: f64,
    weights: &[f64],
    constraints: &[AxisConstraint],
    central_index: Option<usize>,
) {
    if let Some(central) = central_index {
        let amount = remainder.min((constraints[central].max - sizes[central]).max(0.0));
        apply_bounded_delta(&mut sizes[central], amount, true, constraints[central]);
        remainder -= amount;
    }
    redistribute(sizes, remainder, weights, constraints, true, central_index);
}

fn shrink_to_extent(
    sizes: &mut [f64],
    mut excess: f64,
    weights: &[f64],
    constraints: &[AxisConstraint],
    central_index: Option<usize>,
) {
    if let Some(central) = central_index {
        let amount = excess.min((sizes[central] - constraints[central].min).max(0.0));
        apply_bounded_delta(&mut sizes[central], amount, false, constraints[central]);
        excess -= amount;
    }
    redistribute(sizes, excess, weights, constraints, false, central_index);
}

fn redistribute(
    sizes: &mut [f64],
    mut amount: f64,
    weights: &[f64],
    constraints: &[AxisConstraint],
    grow: bool,
    excluded: Option<usize>,
) {
    let tolerance = allocation_tolerance(amount, amount, amount);
    while amount > tolerance {
        let eligible = (0..sizes.len())
            .filter(|&index| {
                excluded != Some(index)
                    && capacity(sizes[index], constraints[index], grow) > tolerance
                    && weights[index] > 0.0
            })
            .collect::<Vec<_>>();
        if eligible.is_empty() {
            break;
        }
        let weight_total: f64 = eligible.iter().map(|&index| weights[index]).sum();
        let before = amount;
        for &index in &eligible {
            let share = before * (weights[index] / weight_total);
            let delta = share.min(capacity(sizes[index], constraints[index], grow));
            apply_bounded_delta(&mut sizes[index], delta, grow, constraints[index]);
            amount -= delta;
        }
        if before - amount <= tolerance {
            break;
        }
    }

    // Consume floating-point residue and zero-weight capacity in stable child order.
    for index in 0..sizes.len() {
        if amount <= tolerance {
            break;
        }
        if excluded == Some(index) {
            continue;
        }
        let delta = amount.min(capacity(sizes[index], constraints[index], grow));
        apply_bounded_delta(&mut sizes[index], delta, grow, constraints[index]);
        amount -= delta;
    }
}

fn capacity(size: f64, constraint: AxisConstraint, grow: bool) -> f64 {
    if grow {
        (constraint.max - size).max(0.0)
    } else {
        (size - constraint.min).max(0.0)
    }
}

fn apply_bounded_delta(size: &mut f64, delta: f64, grow: bool, constraint: AxisConstraint) {
    if grow {
        *size = (*size + delta).min(constraint.max);
    } else {
        *size = (*size - delta).max(constraint.min);
    }
}

fn checked_sum(
    values: impl IntoIterator<Item = f64>,
    quantity: &'static str,
) -> Result<f64, LayoutError> {
    let sum = values.into_iter().sum::<f64>();
    if sum.is_finite() {
        Ok(sum)
    } else {
        Err(LayoutError::NonFiniteAggregate { quantity })
    }
}

fn allocation_tolerance(a: f64, b: f64, c: f64) -> f64 {
    a.abs().max(b.abs()).max(c.abs()).max(1.0) * f64::EPSILON * 64.0
}
