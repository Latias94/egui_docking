//! Deterministic, renderer-neutral layout projection.

use std::collections::{BTreeMap, HashSet};
use std::sync::Arc;

use thiserror::Error;

use crate::command::{NodeFingerprint, NodeSource};
use crate::error::{CommandError, ReferenceRole};
use crate::geometry::{Constraints, GeometryError, LogicalRect, LogicalSize};
use crate::graph::{
    Axis, NORMALIZED_WEIGHT_TOLERANCE, Node, SplitWeight, Workspace, WorkspaceData,
};
use crate::ids::{NodeId, RootId};
use crate::transition::WorkspaceVersion;
use crate::workspace::WorkspaceIndex;

mod projection;
pub use projection::{LayoutProjection, NodeIter, NodeMap, SplitIter, SplitMap, SplitProjection};

mod topology;
use topology::LayoutNodeKind;
pub(crate) use topology::RootLayoutTopology;

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
    let mut sizes = vec![0.0; weights.len()];
    let mut eligible = Vec::new();
    let outcome = solve_validated_axis_into(
        extent,
        weights,
        constraints,
        central_index,
        &mut sizes,
        &mut eligible,
    )?;
    Ok(AxisLayout {
        sizes,
        overflow: outcome.overflow,
        unallocated: outcome.unallocated,
    })
}

#[derive(Clone, Copy)]
struct AxisOutcome {
    overflow: f64,
    unallocated: f64,
}

fn solve_axis_into(
    extent: f64,
    weights: &[f64],
    constraints: &[AxisConstraint],
    central_index: Option<usize>,
    sizes: &mut [f64],
    eligible: &mut Vec<usize>,
) -> Result<AxisOutcome, LayoutError> {
    validate_inputs(extent, weights, constraints, central_index)?;
    solve_validated_axis_into(extent, weights, constraints, central_index, sizes, eligible)
}

fn solve_validated_axis_into(
    extent: f64,
    weights: &[f64],
    constraints: &[AxisConstraint],
    central_index: Option<usize>,
    sizes: &mut [f64],
    eligible: &mut Vec<usize>,
) -> Result<AxisOutcome, LayoutError> {
    debug_assert_eq!(sizes.len(), weights.len());
    if weights.is_empty() {
        return Ok(AxisOutcome {
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
        for (size, constraint) in sizes.iter_mut().zip(constraints) {
            *size = constraint.min;
        }
        return Ok(AxisOutcome {
            overflow: min_total - extent,
            unallocated: 0.0,
        });
    }
    if max_total < extent {
        for (size, constraint) in sizes.iter_mut().zip(constraints) {
            *size = constraint.max;
        }
        return Ok(AxisOutcome {
            overflow: 0.0,
            unallocated: extent - max_total,
        });
    }

    desired_sizes_into(extent, weights, central_index, sizes)?;
    for (size, constraint) in sizes.iter_mut().zip(constraints) {
        *size = size.clamp(constraint.min, constraint.max);
    }

    let assigned = checked_sum(sizes.iter().copied(), "assigned size")?;
    if assigned < extent {
        grow_to_extent(
            sizes,
            extent - assigned,
            weights,
            constraints,
            central_index,
            eligible,
        );
    } else if assigned > extent {
        shrink_to_extent(
            sizes,
            assigned - extent,
            weights,
            constraints,
            central_index,
            eligible,
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

    Ok(AxisOutcome {
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
    workspace
        .root(root)
        .ok_or(LayoutError::MissingRoot { root })?;
    let weight_overrides =
        validate_split_weight_overrides(workspace, root, weight_overrides, None)?;
    measure_root(
        workspace,
        root,
        LeafMeasurements::Constraints(leaf_constraints),
        metrics,
    )?
    .project_with_weights(bounds, &weight_overrides)
}

/// Measures scene minima once, before root bounds determine the uniform leaf maximum.
pub(crate) fn measure_root_minimums<'workspace>(
    workspace: &'workspace Workspace,
    root: RootId,
    leaf_minimums: &BTreeMap<NodeId, LogicalSize>,
    metrics: LayoutMetrics,
) -> Result<RootMeasurement<'workspace>, LayoutError> {
    measure_root(
        workspace,
        root,
        LeafMeasurements::Minimums(leaf_minimums),
        metrics,
    )
}

/// One root's measurements over stable, dense structural rows.
pub(crate) struct RootMeasurement<'workspace> {
    workspace: &'workspace Workspace,
    root: RootId,
    metrics: LayoutMetrics,
    minimum: LogicalSize,
    topology: Arc<RootLayoutTopology>,
    measurements: Vec<SubtreeMeasurement>,
    minimums_only: bool,
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
        self,
        bounds: LogicalRect,
        weight_overrides: &BTreeMap<NodeId, Vec<f64>>,
    ) -> Result<LayoutProjection, LayoutError> {
        project_measurements(
            self.topology,
            self.measurements,
            bounds,
            self.metrics,
            self.minimums_only,
            weight_overrides,
        )
    }
}

fn project_measurements(
    topology: Arc<RootLayoutTopology>,
    mut measurements: Vec<SubtreeMeasurement>,
    bounds: LogicalRect,
    metrics: LayoutMetrics,
    minimums_only: bool,
    weight_overrides: &BTreeMap<NodeId, Vec<f64>>,
) -> Result<LayoutProjection, LayoutError> {
    if minimums_only {
        let minimum = measurements[0].constraints.min();
        let leaf_maximum = LogicalSize::new(
            bounds.width().max(minimum.width()),
            bounds.height().max(minimum.height()),
        )?;
        let mut child_values = Vec::new();
        for &row in &topology.postorder {
            let maximum = match &topology.nodes[row].kind {
                LayoutNodeKind::Tabs => leaf_maximum,
                LayoutNodeKind::Split { axis, children, .. } => {
                    child_values.clear();
                    child_values.extend(
                        topology.children[children.clone()]
                            .iter()
                            .map(|&row| measurements[row]),
                    );
                    compose_maximum(
                        *axis,
                        &child_values,
                        total_splitter_extent(topology.nodes[row].id, children.len(), metrics)?,
                    )?
                }
            };
            let measurement = &mut measurements[row];
            measurement.constraints = Constraints::new(measurement.constraints.min(), maximum)?;
        }
    }
    let mut projection = LayoutProjection {
        node_rects: NodeMap::new(Arc::clone(&topology), vec![bounds; topology.nodes.len()]),
        node_constraints: NodeMap::new(
            Arc::clone(&topology),
            measurements
                .iter()
                .map(|measurement| measurement.constraints)
                .collect(),
        ),
        splits: SplitMap::new(Arc::clone(&topology), bounds),
    };
    project_subtree(
        &topology,
        bounds,
        metrics,
        &measurements,
        weight_overrides,
        &mut projection,
    )?;
    Ok(projection)
}

#[cfg(test)]
fn project_draft(
    draft: &crate::graph::WorkspaceBuilder,
    root: RootId,
    bounds: LogicalRect,
    leaves: LeafMeasurements<'_>,
    metrics: LayoutMetrics,
) -> Result<LayoutProjection, LayoutError> {
    let (topology, measurements) =
        RootLayoutTopology::compile(draft.draft(), root, leaves, metrics)?;
    project_measurements(
        Arc::new(topology),
        measurements,
        bounds,
        metrics,
        matches!(leaves, LeafMeasurements::Minimums(_)),
        &BTreeMap::new(),
    )
}

fn validate_split_weight_overrides(
    workspace: &Workspace,
    root: RootId,
    overrides: &[SplitWeightOverride<'_>],
    index: Option<(&WorkspaceIndex, WorkspaceVersion)>,
) -> Result<BTreeMap<NodeId, Vec<f64>>, LayoutError> {
    let mut validated = BTreeMap::new();
    // These identities skip only a repeated exact comparison of one immutable
    // fingerprint. Every source still proves node existence and root membership.
    let mut verified_fingerprints = HashSet::new();
    let mut root_fingerprint: Option<NodeFingerprint> = None;

    for weight_override in overrides {
        let source = weight_override.source;
        if source.root() != root {
            return Err(LayoutError::SplitWeightOverrideRootMismatch {
                projected_root: root,
                override_root: source.root(),
            });
        }
        let fingerprint_identity = Arc::as_ptr(&source.fingerprint().0);
        let verified = verified_fingerprints.contains(&fingerprint_identity);
        verify_split_override_source(workspace, source, index, verified, &mut root_fingerprint)
            .map_err(|source| LayoutError::InvalidSplitWeightOverrideSource { source })?;
        verified_fingerprints.insert(fingerprint_identity);

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

fn verify_split_override_source(
    workspace: &Workspace,
    source: &NodeSource,
    index: Option<(&WorkspaceIndex, WorkspaceVersion)>,
    fingerprint_verified: bool,
    root_fingerprint: &mut Option<NodeFingerprint>,
) -> Result<(), CommandError> {
    let role = ReferenceRole::Source;
    let root = source.root();
    let node = source.node();
    if let Some((index, version)) = index {
        let view = index.at_version(version).ok_or(CommandError::Invariant {
            stage: "use workspace index for a different revision",
        })?;
        if view.root_node(root).is_none() {
            return Err(CommandError::MissingRoot { root });
        }
        if workspace.node(node).is_none() {
            return Err(CommandError::MissingNode { role, node });
        }
        if !view.root_contains_node(root, node) {
            return Err(CommandError::NodeOutsideRoot { role, root, node });
        }
        if !fingerprint_verified {
            index.verify_reference(workspace, version, root, node, source.fingerprint(), role)?;
        }
        return Ok(());
    }

    if workspace.root(root).is_none() {
        return Err(CommandError::MissingRoot { root });
    }
    if workspace.node(node).is_none() {
        return Err(CommandError::MissingNode { role, node });
    }
    if !workspace.root_contains_node(root, node) {
        return Err(CommandError::NodeOutsideRoot { role, root, node });
    }
    if !fingerprint_verified {
        let actual = match root_fingerprint {
            Some(fingerprint) => fingerprint,
            None => root_fingerprint.insert(workspace.root_fingerprint(root)?),
        };
        if &*actual != source.fingerprint() {
            return Err(CommandError::StaleNode {
                role,
                node,
                expected: source.fingerprint().clone(),
                actual: actual.clone(),
            });
        }
    }
    Ok(())
}

#[derive(Debug, Clone, Copy)]
struct SubtreeMeasurement {
    constraints: Constraints,
    contains_central: bool,
}

#[derive(Clone, Copy)]
enum LeafMeasurements<'leaves> {
    Constraints(&'leaves BTreeMap<NodeId, Constraints>),
    Minimums(&'leaves BTreeMap<NodeId, LogicalSize>),
}

impl LeafMeasurements<'_> {
    fn get(self, node: NodeId) -> Result<Constraints, LayoutError> {
        match self {
            Self::Constraints(constraints) => constraints
                .get(&node)
                .copied()
                .ok_or(LayoutError::MissingLeafConstraints { node }),
            Self::Minimums(minimums) => {
                let minimum = minimums
                    .get(&node)
                    .copied()
                    .ok_or(LayoutError::MissingLeafConstraints { node })?;
                // Point constraints hold minima until root bounds supply leaf maxima.
                Ok(Constraints::new(minimum, minimum)?)
            }
        }
    }
}

fn measure_root<'workspace>(
    workspace: &'workspace Workspace,
    root: RootId,
    leaves: LeafMeasurements<'_>,
    metrics: LayoutMetrics,
) -> Result<RootMeasurement<'workspace>, LayoutError> {
    let (topology, measurements) =
        RootLayoutTopology::compile(workspace.storage(), root, leaves, metrics)?;
    Ok(RootMeasurement {
        workspace,
        root,
        metrics,
        minimum: measurements[0].constraints.min(),
        topology: Arc::new(topology),
        measurements,
        minimums_only: matches!(leaves, LeafMeasurements::Minimums(_)),
    })
}

/// Reuse only exact structural rows; metrics and constraint answers remain live.
pub(crate) fn measure_root_minimums_with_index<'workspace>(
    workspace: &'workspace Workspace,
    root: RootId,
    leaf_minimums: &BTreeMap<NodeId, LogicalSize>,
    metrics: LayoutMetrics,
    index: &WorkspaceIndex,
    version: WorkspaceVersion,
) -> Result<RootMeasurement<'workspace>, LayoutError> {
    let slot = index.layout_topology_slot(root, version);
    let leaves = LeafMeasurements::Minimums(leaf_minimums);
    if let Some(topology) = slot.and_then(std::sync::OnceLock::get)
        && topology.matches(workspace, root)
    {
        let measurements = topology.measure(leaves, metrics)?;
        return Ok(RootMeasurement {
            workspace,
            root,
            metrics,
            minimum: measurements[0].constraints.min(),
            topology: Arc::clone(topology),
            measurements,
            minimums_only: true,
        });
    }
    let measured = measure_root(workspace, root, leaves, metrics)?;
    if let Some(slot) = slot {
        // A stale same-version hint never replaces the first checked topology.
        let _ = slot.set(Arc::clone(&measured.topology));
    }
    Ok(measured)
}

fn compose_constraints(
    node: NodeId,
    axis: Axis,
    children: &[SubtreeMeasurement],
    metrics: LayoutMetrics,
    minimums_only: bool,
) -> Result<Constraints, LayoutError> {
    if children.is_empty() {
        return Err(LayoutError::SplitCardinalityMismatch {
            node,
            children: 0,
            weights: 0,
        });
    }
    let splitter_extent = total_splitter_extent(node, children.len(), metrics)?;
    let minimum = compose_minimum(axis, children, splitter_extent)?;
    let maximum = if minimums_only {
        minimum
    } else {
        compose_maximum(axis, children, splitter_extent)?
    };
    Ok(Constraints::new(minimum, maximum)?)
}

fn compose_minimum(
    axis: Axis,
    children: &[SubtreeMeasurement],
    splitter_extent: f64,
) -> Result<LogicalSize, LayoutError> {
    let min_widths = children.iter().map(|child| child.constraints.min().width());
    let min_heights = children
        .iter()
        .map(|child| child.constraints.min().height());

    let (width, height) = match axis {
        Axis::Horizontal => (
            checked_sum(
                min_widths.chain(std::iter::once(splitter_extent)),
                "subtree minimum width",
            )?,
            min_heights.fold(0.0, f64::max),
        ),
        Axis::Vertical => (
            min_widths.fold(0.0, f64::max),
            checked_sum(
                min_heights.chain(std::iter::once(splitter_extent)),
                "subtree minimum height",
            )?,
        ),
    };
    Ok(LogicalSize::new(width, height)?)
}

fn compose_maximum(
    axis: Axis,
    children: &[SubtreeMeasurement],
    splitter_extent: f64,
) -> Result<LogicalSize, LayoutError> {
    let max_widths = children.iter().map(|child| child.constraints.max().width());
    let max_heights = children
        .iter()
        .map(|child| child.constraints.max().height());
    let (width, height) = match axis {
        Axis::Horizontal => (
            checked_sum(
                max_widths.chain(std::iter::once(splitter_extent)),
                "subtree maximum width",
            )?,
            max_heights.fold(f64::INFINITY, f64::min),
        ),
        Axis::Vertical => (
            max_widths.fold(f64::INFINITY, f64::min),
            checked_sum(
                max_heights.chain(std::iter::once(splitter_extent)),
                "subtree maximum height",
            )?,
        ),
    };
    Ok(LogicalSize::new(width, height)?)
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

struct SplitProjectionState<'layout> {
    node_id: NodeId,
    axis: Axis,
    children: &'layout [usize],
    child_start: usize,
    splitter_start: usize,
    next_child: usize,
    pending_child_size: Option<f64>,
    bounds: LogicalRect,
    cursor: f64,
    splitter_thickness: f64,
}

enum ProjectionFrame<'layout> {
    Enter { row: usize, bounds: LogicalRect },
    ContinueSplit(SplitProjectionState<'layout>),
}

#[derive(Default)]
struct ProjectionScratch {
    constraints: Vec<AxisConstraint>,
    weights: Vec<f64>,
    eligible: Vec<usize>,
}

fn project_subtree(
    topology: &RootLayoutTopology,
    bounds: LogicalRect,
    metrics: LayoutMetrics,
    measurements: &[SubtreeMeasurement],
    weight_overrides: &BTreeMap<NodeId, Vec<f64>>,
    projection: &mut LayoutProjection,
) -> Result<(), LayoutError> {
    let mut stack = vec![ProjectionFrame::Enter { row: 0, bounds }];
    let mut scratch = ProjectionScratch::default();
    while let Some(frame) = stack.pop() {
        match frame {
            ProjectionFrame::Enter { row, bounds } => {
                if let Some(state) = prepare_split_projection(
                    topology,
                    row,
                    bounds,
                    metrics,
                    measurements,
                    weight_overrides,
                    projection,
                    &mut scratch,
                )? {
                    stack.push(ProjectionFrame::ContinueSplit(state));
                }
            }
            ProjectionFrame::ContinueSplit(state) => {
                advance_split_projection(state, &mut stack, projection)?
            }
        }
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn prepare_split_projection<'layout>(
    topology: &'layout RootLayoutTopology,
    row: usize,
    bounds: LogicalRect,
    metrics: LayoutMetrics,
    measurements: &[SubtreeMeasurement],
    weight_overrides: &BTreeMap<NodeId, Vec<f64>>,
    projection: &mut LayoutProjection,
    scratch: &mut ProjectionScratch,
) -> Result<Option<SplitProjectionState<'layout>>, LayoutError> {
    let node = &topology.nodes[row];
    let node_id = node.id;
    projection.node_rects.values[row] = bounds;
    let LayoutNodeKind::Split {
        axis,
        children: child_range,
        weights,
        split,
        splitters,
    } = &node.kind
    else {
        return Ok(None);
    };
    let children = &topology.children[child_range.clone()];
    scratch.constraints.clear();
    scratch.constraints.extend(children.iter().map(|&row| {
        let constraints = measurements[row].constraints;
        match axis {
            Axis::Horizontal => AxisConstraint {
                min: constraints.min().width(),
                max: constraints.max().width(),
            },
            Axis::Vertical => AxisConstraint {
                min: constraints.min().height(),
                max: constraints.max().height(),
            },
        }
    }));
    for ((minimum, maximum), constraint) in projection.splits.child_minimum_extents
        [child_range.clone()]
    .iter_mut()
    .zip(&mut projection.splits.child_maximum_extents[child_range.clone()])
    .zip(&scratch.constraints)
    {
        *minimum = constraint.min();
        *maximum = constraint.max();
    }
    let axis_weights = if let Some(overrides) = weight_overrides.get(&node_id) {
        overrides.as_slice()
    } else {
        scratch.weights.clear();
        scratch.weights.extend(
            topology.weight_bits[weights.clone()]
                .iter()
                .map(|&bits| f64::from(f32::from_bits(bits))),
        );
        &scratch.weights
    };
    let central_index = children
        .iter()
        .position(|&row| measurements[row].contains_central);
    let extent = match axis {
        Axis::Horizontal => bounds.width(),
        Axis::Vertical => bounds.height(),
    };
    let (splitter_extent, splitter_thickness) =
        projected_splitter_geometry(node_id, children.len(), extent, metrics)?;
    let solved = solve_axis_into(
        extent - splitter_extent,
        axis_weights,
        &scratch.constraints,
        central_index,
        &mut projection.splits.child_extents[child_range.clone()],
        &mut scratch.eligible,
    )?;
    projection
        .splits
        .set_outcome(*split, central_index, solved.overflow, solved.unallocated);
    let cursor = match axis {
        Axis::Horizontal => bounds.x(),
        Axis::Vertical => bounds.y(),
    };
    Ok(Some(SplitProjectionState {
        node_id,
        axis: *axis,
        children,
        child_start: child_range.start,
        splitter_start: splitters.start,
        next_child: 0,
        pending_child_size: None,
        bounds,
        cursor,
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

fn advance_split_projection<'layout>(
    mut state: SplitProjectionState<'layout>,
    stack: &mut Vec<ProjectionFrame<'layout>>,
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
            projection.splits.splitter_rects[state.splitter_start + state.next_child - 1] =
                splitter;
            state.cursor += state.splitter_thickness;
        }
        stack.push(ProjectionFrame::ContinueSplit(state));
        return Ok(());
    }

    if state.next_child == state.children.len() {
        return Ok(());
    }

    let child = state.children.get(state.next_child).copied().ok_or(
        LayoutError::SplitCardinalityMismatch {
            node: state.node_id,
            children: state.children.len(),
            weights: state.children.len(),
        },
    )?;
    let size = projection.splits.child_extents[state.child_start + state.next_child];
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
        row: child,
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

fn desired_sizes_into(
    extent: f64,
    weights: &[f64],
    central_index: Option<usize>,
    sizes: &mut [f64],
) -> Result<(), LayoutError> {
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
        for (index, (size, weight)) in sizes.iter_mut().zip(weights).enumerate() {
            *size = if index == central {
                0.0
            } else {
                weight * scale
            };
        }
        let siblings = checked_sum(sizes.iter().copied(), "desired size")?;
        sizes[central] = (extent - siblings).max(0.0);
        return Ok(());
    }

    let weight_total = checked_sum(weights.iter().copied(), "weight")?;
    for (size, weight) in sizes.iter_mut().zip(weights) {
        *size = extent * (*weight / weight_total);
    }
    Ok(())
}

fn grow_to_extent(
    sizes: &mut [f64],
    mut remainder: f64,
    weights: &[f64],
    constraints: &[AxisConstraint],
    central_index: Option<usize>,
    eligible: &mut Vec<usize>,
) {
    if let Some(central) = central_index {
        let amount = remainder.min((constraints[central].max - sizes[central]).max(0.0));
        apply_bounded_delta(&mut sizes[central], amount, true, constraints[central]);
        remainder -= amount;
    }
    redistribute(
        sizes,
        remainder,
        weights,
        constraints,
        true,
        central_index,
        eligible,
    );
}

fn shrink_to_extent(
    sizes: &mut [f64],
    mut excess: f64,
    weights: &[f64],
    constraints: &[AxisConstraint],
    central_index: Option<usize>,
    eligible: &mut Vec<usize>,
) {
    if let Some(central) = central_index {
        let amount = excess.min((sizes[central] - constraints[central].min).max(0.0));
        apply_bounded_delta(&mut sizes[central], amount, false, constraints[central]);
        excess -= amount;
    }
    redistribute(
        sizes,
        excess,
        weights,
        constraints,
        false,
        central_index,
        eligible,
    );
}

fn redistribute(
    sizes: &mut [f64],
    mut amount: f64,
    weights: &[f64],
    constraints: &[AxisConstraint],
    grow: bool,
    excluded: Option<usize>,
    eligible: &mut Vec<usize>,
) {
    let tolerance = allocation_tolerance(amount, amount, amount);
    while amount > tolerance {
        eligible.clear();
        eligible.extend((0..sizes.len()).filter(|&index| {
            excluded != Some(index)
                && capacity(sizes[index], constraints[index], grow) > tolerance
                && weights[index] > 0.0
        }));
        if eligible.is_empty() {
            break;
        }
        let weight_total: f64 = eligible.iter().map(|&index| weights[index]).sum();
        let before = amount;
        for &index in eligible.iter() {
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

#[cfg(test)]
mod override_tests;

#[cfg(test)]
mod reference;

#[cfg(test)]
mod topology_tests;

#[cfg(test)]
mod layout_cost;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::{RootRecord, SurfacePresentation};
    use crate::ids::{ItemId, SurfaceId};

    #[test]
    fn measured_minimums_preserve_projection_constraints_and_resize_overrides() {
        let metrics = LayoutMetrics::new(3.0).unwrap();
        let root = RootId::new(1);
        for central in [false, true] {
            let mut builder = Workspace::builder();
            let a = builder.insert_node(Node::tabs([ItemId::new(1)]));
            let b = builder.insert_node(Node::tabs([ItemId::new(2)]));
            let c = builder.insert_node(Node::tabs([ItemId::new(3)]));
            let d = builder.insert_node(Node::tabs([ItemId::new(4)]));
            let inner =
                builder.insert_node(Node::split(Axis::Vertical, [b, c], [0.7, 0.3]).unwrap());
            let outer = builder.insert_node(
                Node::split(Axis::Horizontal, [a, inner, d], [0.2, 0.5, 0.3]).unwrap(),
            );
            let record = if central {
                RootRecord::new(outer).with_central(b)
            } else {
                RootRecord::new(outer)
            };
            builder.set_root(root, record);
            builder.set_surface(SurfaceId::new(1), SurfacePresentation::with_main(root));
            let workspace = builder.build().unwrap();
            let minimums = BTreeMap::from([
                (a, LogicalSize::new(17.0, 11.0).unwrap()),
                (b, LogicalSize::new(31.0, 7.0).unwrap()),
                (c, LogicalSize::new(13.0, 19.0).unwrap()),
                (d, LogicalSize::new(23.0, 5.0).unwrap()),
            ]);
            let root_minimum = LogicalSize::new(77.0, 29.0).unwrap();
            let source = workspace.capture_node_source(root, inner).unwrap();
            let weights = SplitWeight::normalize([0.4, 0.6]).unwrap();
            let overrides = [SplitWeightOverride::new(&source, &weights)];

            for (width, height) in [(300.0, 200.0), (77.0, 29.0), (41.0, 23.0)] {
                let bounds = LogicalRect::new(5.0, 7.0, width, height).unwrap();
                let maximum = LogicalSize::new(
                    width.max(root_minimum.width()),
                    height.max(root_minimum.height()),
                )
                .unwrap();
                let constraints = minimums
                    .iter()
                    .map(|(node, minimum)| (*node, Constraints::new(*minimum, maximum).unwrap()))
                    .collect();
                for overrides in [&[][..], &overrides[..]] {
                    let measured =
                        measure_root_minimums(&workspace, root, &minimums, metrics).unwrap();
                    assert_eq!(measured.minimum(), root_minimum);
                    let actual = measured.project(bounds, overrides).unwrap();
                    let expected = project_root_with_overrides(
                        &workspace,
                        root,
                        bounds,
                        &constraints,
                        metrics,
                        overrides,
                    )
                    .unwrap();
                    assert_eq!(actual, expected);
                    assert_eq!(actual.node_constraints.len(), 6);
                    assert_eq!(actual.node_constraints[&b], constraints[&b]);
                    assert_eq!(
                        actual.node_constraints[&inner].min(),
                        LogicalSize::new(31.0, 29.0).unwrap(),
                    );
                    assert_eq!(
                        actual.node_constraints[&inner].max(),
                        LogicalSize::new(maximum.width(), 2.0 * maximum.height() + 3.0).unwrap(),
                    );
                }
            }
        }
    }
}
