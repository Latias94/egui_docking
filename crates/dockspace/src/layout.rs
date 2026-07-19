//! Deterministic, renderer-neutral layout projection.

use std::collections::{BTreeMap, BTreeSet};

use thiserror::Error;

use crate::command::NodeSource;
use crate::error::{CommandError, ReferenceRole};
use crate::geometry::{Constraints, GeometryError, LogicalRect, LogicalSize};
use crate::graph::{Axis, NORMALIZED_WEIGHT_TOLERANCE, Node, SplitWeight, Workspace};
use crate::ids::{NodeId, RootId};

/// Minimum and maximum extent accepted by one child on a split axis.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AxisConstraint {
    min: f64,
    max: f64,
}

impl AxisConstraint {
    /// Creates a validated axis constraint.
    ///
    /// # Errors
    ///
    /// Returns an error when either bound is non-finite, the minimum is
    /// negative, or the maximum is smaller than the minimum.
    pub fn new(min: f64, max: f64) -> Result<Self, AxisConstraintError> {
        if !min.is_finite() {
            return Err(AxisConstraintError::NonFiniteMinimum { value: min });
        }
        if !max.is_finite() {
            return Err(AxisConstraintError::NonFiniteMaximum { value: max });
        }
        if min < 0.0 {
            return Err(AxisConstraintError::NegativeMinimum { value: min });
        }
        if max < min {
            return Err(AxisConstraintError::MaximumBelowMinimum { min, max });
        }

        Ok(Self { min, max })
    }

    /// Returns the minimum extent.
    pub fn min(self) -> f64 {
        self.min
    }

    /// Returns the maximum extent.
    pub fn max(self) -> f64 {
        self.max
    }
}

/// Why an [`AxisConstraint`] could not be constructed.
#[derive(Debug, Clone, Copy, PartialEq, Error)]
pub enum AxisConstraintError {
    /// The minimum is not a finite number.
    #[error("axis minimum is non-finite: {value}")]
    NonFiniteMinimum { value: f64 },
    /// The maximum is not a finite number.
    #[error("axis maximum is non-finite: {value}")]
    NonFiniteMaximum { value: f64 },
    /// The minimum is negative.
    #[error("axis minimum is negative: {value}")]
    NegativeMinimum { value: f64 },
    /// The maximum is smaller than the minimum.
    #[error("axis maximum {max} is smaller than minimum {min}")]
    MaximumBelowMinimum { min: f64, max: f64 },
}

/// A solved ordered split axis.
#[derive(Debug, Clone, PartialEq)]
pub struct AxisLayout {
    /// Extent assigned to each child in input order.
    pub sizes: Vec<f64>,
    /// Extent by which mandatory minima exceed the available extent.
    pub overflow: f64,
    /// Extent left unused because every child reached its maximum.
    pub unallocated: f64,
}

/// Explicit renderer-neutral measurements that affect layout geometry.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LayoutMetrics {
    splitter_thickness: f64,
}

impl LayoutMetrics {
    /// Creates validated layout metrics.
    ///
    /// # Errors
    ///
    /// Returns an error when `splitter_thickness` is non-finite or negative.
    pub fn new(splitter_thickness: f64) -> Result<Self, LayoutMetricsError> {
        if !splitter_thickness.is_finite() {
            return Err(LayoutMetricsError::NonFiniteSplitterThickness {
                value: splitter_thickness,
            });
        }
        if splitter_thickness < 0.0 {
            return Err(LayoutMetricsError::NegativeSplitterThickness {
                value: splitter_thickness,
            });
        }

        Ok(Self { splitter_thickness })
    }

    /// Returns the preferred extent reserved for every boundary between split children.
    ///
    /// Projection uses this exact thickness whenever it fits. If splitters alone
    /// exceed a split's available extent, they are compressed uniformly so the
    /// projection remains total for temporarily collapsed renderer surfaces.
    pub fn splitter_thickness(self) -> f64 {
        self.splitter_thickness
    }
}

/// Why [`LayoutMetrics`] could not be constructed.
#[derive(Debug, Clone, Copy, PartialEq, Error)]
pub enum LayoutMetricsError {
    /// The splitter thickness is not a finite number.
    #[error("splitter thickness is non-finite: {value}")]
    NonFiniteSplitterThickness {
        /// Rejected thickness.
        value: f64,
    },
    /// The splitter thickness is negative.
    #[error("splitter thickness is negative: {value}")]
    NegativeSplitterThickness {
        /// Rejected thickness.
        value: f64,
    },
}

/// Geometry and constraint outcomes for one split node.
#[derive(Debug, Clone, PartialEq)]
pub struct SplitProjection {
    /// Direction in which child rectangles were allocated.
    pub axis: Axis,
    /// Logical rectangles reserved between adjacent children.
    pub splitter_rects: Vec<LogicalRect>,
    /// Extent by which child minima exceed the split bounds.
    pub overflow: f64,
    /// Extent left unused after every child reached its maximum.
    pub unallocated: f64,
}

/// Complete deterministic projection of one workspace root.
#[derive(Debug, Clone, PartialEq)]
pub struct LayoutProjection {
    /// Logical rectangle assigned to every reachable node.
    pub node_rects: BTreeMap<NodeId, LogicalRect>,
    /// Split-specific outcomes keyed by split node identity.
    pub splits: BTreeMap<NodeId, SplitProjection>,
}

/// Transient split weights applied to one exact frozen split source.
///
/// Construction does not inspect a workspace. [`project_root_with_overrides`]
/// validates the source fingerprint and complete weight collection against the
/// workspace being projected before using the override.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SplitWeightOverride<'state> {
    source: &'state NodeSource,
    weights: &'state [SplitWeight],
}

impl<'state> SplitWeightOverride<'state> {
    /// Creates an override which will be validated during projection.
    #[must_use]
    pub const fn new(source: &'state NodeSource, weights: &'state [SplitWeight]) -> Self {
        Self { source, weights }
    }

    /// Returns the frozen split source.
    #[must_use]
    pub const fn source(self) -> &'state NodeSource {
        self.source
    }

    /// Returns the proposed transient weights.
    #[must_use]
    pub const fn weights(self) -> &'state [SplitWeight] {
        self.weights
    }
}

/// Failure to solve an axis or project a workspace root.
#[derive(Debug, Clone, PartialEq, Error)]
pub enum LayoutError {
    /// The available extent is not finite.
    #[error("axis extent is non-finite: {value}")]
    NonFiniteExtent { value: f64 },
    /// The available extent is negative.
    #[error("axis extent is negative: {value}")]
    NegativeExtent { value: f64 },
    /// The weight and constraint lists describe different child counts.
    #[error("axis has {weights} weights but {constraints} constraints")]
    LengthMismatch { weights: usize, constraints: usize },
    /// A child weight is not finite.
    #[error("axis weight {index} is non-finite: {value}")]
    NonFiniteWeight { index: usize, value: f64 },
    /// A child weight is negative.
    #[error("axis weight {index} is negative: {value}")]
    NegativeWeight { index: usize, value: f64 },
    /// The central child does not exist in the input list.
    #[error("central child index {index} is outside child count {child_count}")]
    CentralIndexOutOfBounds { index: usize, child_count: usize },
    /// No child can receive weighted space.
    #[error("axis weights have no positive total")]
    ZeroWeightTotal,
    /// Finite inputs overflowed while their aggregate was computed.
    #[error("{quantity} aggregate is not finite")]
    NonFiniteAggregate { quantity: &'static str },
    /// The requested stable root does not exist.
    #[error("workspace does not contain root {root}")]
    MissingRoot { root: RootId },
    /// A reachable runtime node does not exist.
    #[error("workspace does not contain node {node:?}")]
    MissingNode { node: NodeId },
    /// No explicit size constraints were supplied for a tabs leaf.
    #[error("layout constraints are missing for tabs node {node:?}")]
    MissingLeafConstraints { node: NodeId },
    /// The workspace contains a cycle and cannot be projected.
    #[error("workspace contains a cycle through node {node:?}")]
    Cycle { node: NodeId },
    /// A split's ordered children and weights do not have the same cardinality.
    #[error("split node {node:?} has {children} children but {weights} weights")]
    SplitCardinalityMismatch {
        /// Invalid split node.
        node: NodeId,
        /// Number of ordered children.
        children: usize,
        /// Number of weights.
        weights: usize,
    },
    /// The configured splitter thickness overflowed while being accumulated.
    #[error(
        "split node {node:?} cannot represent {splitter_count} splitters of thickness {thickness}"
    )]
    NonFiniteSplitterExtent {
        /// Invalid split node.
        node: NodeId,
        /// Number of splitters between the node's children.
        splitter_count: usize,
        /// Configured thickness of each splitter.
        thickness: f64,
    },
    /// A root's declared central leaf is outside that root's subtree.
    #[error("root {root} central node {central:?} is not reachable from its root node")]
    CentralNodeNotReachable { root: RootId, central: NodeId },
    /// A split-weight override belongs to a different root than the projection.
    #[error(
        "split-weight override for root {override_root} cannot be used while projecting root {projected_root}"
    )]
    SplitWeightOverrideRootMismatch {
        /// Root currently being projected.
        projected_root: RootId,
        /// Root captured by the override source.
        override_root: RootId,
    },
    /// A split-weight override source is no longer exact for this workspace.
    #[error("split-weight override source is invalid: {source}")]
    InvalidSplitWeightOverrideSource {
        /// Frozen-reference validation failure.
        #[source]
        source: CommandError,
    },
    /// More than one transient override names the same split.
    #[error("split node {node:?} has more than one transient weight override")]
    DuplicateSplitWeightOverride { node: NodeId },
    /// A transient weight override names a tabs leaf rather than a split.
    #[error("transient weight override node {node:?} is not a split")]
    SplitWeightOverrideNodeNotSplit { node: NodeId },
    /// Transient weights do not match the target split's child count.
    #[error(
        "split node {node:?} has {children} children but its transient override has {weights} weights"
    )]
    SplitWeightOverrideLengthMismatch {
        /// Target split.
        node: NodeId,
        /// Durable child count.
        children: usize,
        /// Override weight count.
        weights: usize,
    },
    /// Transient split weights are individually valid but not normalized.
    #[error("split node {node:?} transient weights are not normalized; sum is {sum}")]
    SplitWeightOverrideNotNormalized {
        /// Target split.
        node: NodeId,
        /// Accumulated override weight.
        sum: f64,
    },
    /// Validated geometry could not be constructed for the projection.
    #[error(transparent)]
    Geometry(#[from] GeometryError),
}

/// Solves one split axis with stable, bounded water filling.
///
/// Without a central child, weights are normalized over the full extent. With
/// a central child, every other weight is interpreted as a share of the full
/// extent and the central child receives the remaining share. If siblings sum
/// to more than one, they are normalized and the central child yields first.
/// The central child's own weight is intentionally ignored.
///
/// Infeasible minima and maxima are reported instead of silently changing the
/// constraints. All other invalid input is rejected before allocation begins.
///
/// # Errors
///
/// Returns an error for non-finite or negative scalar input, mismatched child
/// lists, an invalid central index, a zero total weight without a central
/// child, or an aggregate which overflows `f64`.
pub fn solve_axis(
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

/// Projects a validated workspace root into renderer-neutral logical rectangles.
///
/// `leaf_constraints` must contain an entry for every reachable tabs node.
/// Split subtree constraints are composed deterministically from those leaves;
/// callers do not provide duplicate constraints for internal nodes. `metrics`
/// explicitly includes the preferred splitter thickness in measurement. Projection
/// reserves that thickness when possible and otherwise applies the documented
/// uniform compression rule from [`LayoutMetrics::splitter_thickness`].
///
/// # Errors
///
/// Returns an error when the root or a reachable node is missing, a leaf has no
/// explicit constraints, topology is cyclic or malformed, central metadata is
/// unreachable, constraint composition is impossible, or axis allocation fails.
pub fn project_root(
    workspace: &Workspace,
    root: RootId,
    bounds: LogicalRect,
    leaf_constraints: &BTreeMap<NodeId, Constraints>,
    metrics: LayoutMetrics,
) -> Result<LayoutProjection, LayoutError> {
    project_root_with_overrides(workspace, root, bounds, leaf_constraints, metrics, &[])
}

/// Projects a root while applying explicit, validated transient split weights.
///
/// Each override must name a current split in `root` through a complete
/// [`NodeSource`] fingerprint. Overrides affect only this projection and never
/// mutate durable [`Workspace`] state. Splits without an override keep their
/// durable weights.
///
/// # Errors
///
/// Returns the same errors as [`project_root`], plus an error when an override
/// belongs to another root, is stale, duplicated, names a non-split node, has
/// the wrong cardinality, or is not normalized.
pub fn project_root_with_overrides(
    workspace: &Workspace,
    root: RootId,
    bounds: LogicalRect,
    leaf_constraints: &BTreeMap<NodeId, Constraints>,
    metrics: LayoutMetrics,
    weight_overrides: &[SplitWeightOverride<'_>],
) -> Result<LayoutProjection, LayoutError> {
    let root_record = workspace
        .root(root)
        .ok_or(LayoutError::MissingRoot { root })?;
    let weight_overrides = validate_split_weight_overrides(workspace, root, weight_overrides)?;
    let mut measurements = BTreeMap::new();
    let mut visiting = BTreeSet::new();
    let root_measurement = measure_subtree(
        workspace,
        root_record.node,
        root_record.central,
        leaf_constraints,
        metrics,
        &mut measurements,
        &mut visiting,
    )?;
    if let Some(central) = root_record.central
        && !root_measurement.contains_central
    {
        return Err(LayoutError::CentralNodeNotReachable { root, central });
    }

    let mut projection = LayoutProjection {
        node_rects: BTreeMap::new(),
        splits: BTreeMap::new(),
    };
    project_subtree(
        workspace,
        root_record.node,
        bounds,
        metrics,
        &measurements,
        &weight_overrides,
        &mut projection,
    )?;
    Ok(projection)
}

fn validate_split_weight_overrides(
    workspace: &Workspace,
    root: RootId,
    overrides: &[SplitWeightOverride<'_>],
) -> Result<BTreeMap<NodeId, Vec<f64>>, LayoutError> {
    let mut validated = BTreeMap::new();

    for weight_override in overrides {
        let source = weight_override.source;
        if source.root() != root {
            return Err(LayoutError::SplitWeightOverrideRootMismatch {
                projected_root: root,
                override_root: source.root(),
            });
        }
        workspace
            .verify_reference(
                source.root(),
                source.node(),
                source.fingerprint(),
                ReferenceRole::Source,
            )
            .map_err(|source| LayoutError::InvalidSplitWeightOverrideSource { source })?;

        if validated.contains_key(&source.node()) {
            return Err(LayoutError::DuplicateSplitWeightOverride {
                node: source.node(),
            });
        }
        let Some(Node::Split { children, .. }) = workspace.node(source.node()) else {
            return Err(LayoutError::SplitWeightOverrideNodeNotSplit {
                node: source.node(),
            });
        };
        if children.len() != weight_override.weights.len() {
            return Err(LayoutError::SplitWeightOverrideLengthMismatch {
                node: source.node(),
                children: children.len(),
                weights: weight_override.weights.len(),
            });
        }

        let weights = weight_override
            .weights
            .iter()
            .map(|weight| f64::from(weight.get()))
            .collect::<Vec<_>>();
        let sum = weights.iter().sum::<f64>();
        if !sum.is_finite() || (sum - 1.0).abs() > NORMALIZED_WEIGHT_TOLERANCE {
            return Err(LayoutError::SplitWeightOverrideNotNormalized {
                node: source.node(),
                sum,
            });
        }
        validated.insert(source.node(), weights);
    }

    Ok(validated)
}

#[derive(Debug, Clone, Copy)]
struct SubtreeMeasurement {
    constraints: Constraints,
    contains_central: bool,
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

fn measure_subtree(
    workspace: &Workspace,
    node_id: NodeId,
    central: Option<NodeId>,
    leaf_constraints: &BTreeMap<NodeId, Constraints>,
    metrics: LayoutMetrics,
    measurements: &mut BTreeMap<NodeId, SubtreeMeasurement>,
    visiting: &mut BTreeSet<NodeId>,
) -> Result<SubtreeMeasurement, LayoutError> {
    let mut stack = vec![MeasurementFrame::Enter(node_id)];

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
                            constraints: leaf_constraints
                                .get(&current)
                                .copied()
                                .ok_or(LayoutError::MissingLeafConstraints { node: current })?,
                            contains_central: central == Some(current),
                        };
                        visiting.remove(&current);
                        measurements.insert(current, measurement);
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
                let children = children
                    .iter()
                    .map(|child| {
                        measurements
                            .get(child)
                            .copied()
                            .ok_or(LayoutError::MissingNode { node: *child })
                    })
                    .collect::<Result<Vec<_>, _>>()?;
                let measurement = SubtreeMeasurement {
                    constraints: compose_constraints(current, axis, &children, metrics)?,
                    contains_central: children.iter().any(|child| child.contains_central),
                };
                visiting.remove(&current);
                measurements.insert(current, measurement);
            }
        }
    }

    measurements
        .get(&node_id)
        .copied()
        .ok_or(LayoutError::MissingNode { node: node_id })
}

fn compose_constraints(
    node: NodeId,
    axis: Axis,
    children: &[SubtreeMeasurement],
    metrics: LayoutMetrics,
) -> Result<Constraints, LayoutError> {
    if children.is_empty() {
        return Err(LayoutError::SplitCardinalityMismatch {
            node,
            children: 0,
            weights: 0,
        });
    }
    let splitter_extent = total_splitter_extent(node, children.len(), metrics)?;

    let min_widths = children.iter().map(|child| child.constraints.min().width());
    let max_widths = children.iter().map(|child| child.constraints.max().width());
    let min_heights = children
        .iter()
        .map(|child| child.constraints.min().height());
    let max_heights = children
        .iter()
        .map(|child| child.constraints.max().height());

    let (min_width, max_width, min_height, max_height) = match axis {
        Axis::Horizontal => (
            checked_sum(
                min_widths.chain(std::iter::once(splitter_extent)),
                "subtree minimum width",
            )?,
            checked_sum(
                max_widths.chain(std::iter::once(splitter_extent)),
                "subtree maximum width",
            )?,
            min_heights.fold(0.0, f64::max),
            max_heights.fold(f64::INFINITY, f64::min),
        ),
        Axis::Vertical => (
            min_widths.fold(0.0, f64::max),
            max_widths.fold(f64::INFINITY, f64::min),
            checked_sum(
                min_heights.chain(std::iter::once(splitter_extent)),
                "subtree minimum height",
            )?,
            checked_sum(
                max_heights.chain(std::iter::once(splitter_extent)),
                "subtree maximum height",
            )?,
        ),
    };

    Ok(Constraints::new(
        LogicalSize::new(min_width, min_height)?,
        LogicalSize::new(max_width, max_height)?,
    )?)
}

fn total_splitter_extent(
    node: NodeId,
    child_count: usize,
    metrics: LayoutMetrics,
) -> Result<f64, LayoutError> {
    let splitter_count = child_count.saturating_sub(1);
    let thickness = metrics.splitter_thickness();
    let extent = std::iter::repeat_n(thickness, splitter_count).sum::<f64>();
    if extent.is_finite() {
        Ok(extent)
    } else {
        Err(LayoutError::NonFiniteSplitterExtent {
            node,
            splitter_count,
            thickness,
        })
    }
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

    let child_measurements = children
        .iter()
        .map(|child| {
            measurements
                .get(child)
                .copied()
                .ok_or(LayoutError::MissingNode { node: *child })
        })
        .collect::<Result<Vec<_>, _>>()?;
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
        sizes[central] += amount;
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
        sizes[central] -= amount;
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
            apply_delta(&mut sizes[index], delta, grow);
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
        apply_delta(&mut sizes[index], delta, grow);
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

fn apply_delta(size: &mut f64, delta: f64, grow: bool) {
    if grow {
        *size += delta;
    } else {
        *size -= delta;
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
