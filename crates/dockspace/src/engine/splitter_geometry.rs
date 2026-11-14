//! Frozen touching-boundary geometry shared by pointer and semantic resizing.

use std::collections::{BTreeMap, BTreeSet};

use crate::geometry::{LogicalPoint, LogicalRect, LogicalSize};
use crate::graph::command::{NodeSource, SplitResize};
use crate::graph::{Axis, Node, SplitWeight, Workspace};
use crate::ids::{NodeId, RootId};
use crate::interaction::{
    ActiveResize, FrozenResizeAxisGroup, FrozenResizeSplit, ResizeDeltaInterval,
};
use crate::presentation::layout::{LayoutProjection, SplitProjection};
use crate::presentation::scene::{PresentationPlan, SplitterRecord};

pub(super) fn split_resize_updates(
    resize: &ActiveResize,
    current_pointer: LogicalPoint,
) -> Option<Vec<SplitResize>> {
    resize_axis_group_updates(
        &resize.axis_groups,
        [
            current_pointer.x() - resize.initial_pointer.x(),
            current_pointer.y() - resize.initial_pointer.y(),
        ],
    )
}

/// Freezes the complete touching frontier from the exact presented projection.
/// Missing layout facts never fall back to clipped handle geometry.
pub(super) fn prepare_resize_axis_groups(
    workspace: &Workspace,
    plan: &PresentationPlan,
    handles: Vec<(NodeSource, SplitterRecord)>,
) -> Option<Vec<FrozenResizeAxisGroup>> {
    if handles.is_empty() {
        return None;
    }
    let facts = plan.layout_facts()?;
    let mut roots = BTreeMap::<RootId, NodeSource>::new();
    // Include both outer slots, and merge every incident gap before traversal.
    // No handle independently solves a child shared with another incident gap.
    let mut incidents = BTreeMap::<Axis, BTreeMap<(RootId, NodeId), Vec<Option<f64>>>>::new();
    for (source, record) in handles {
        let id = *record.id();
        if source.root() != id.root || source.node() != id.split || !record.operable() {
            return None;
        }
        match roots.entry(id.root) {
            std::collections::btree_map::Entry::Vacant(entry) => {
                workspace
                    .verify_reference(
                        id.root,
                        id.split,
                        source.reference(),
                        crate::graph::error::ReferenceRole::Source,
                    )
                    .ok()?;
                entry.insert(source.clone());
            }
            std::collections::btree_map::Entry::Occupied(entry) => {
                if entry.get().reference() != source.reference() {
                    return None;
                }
            }
        }
        let projection = facts.root(id.root)?.projection();
        let Node::Split {
            axis,
            children,
            weights,
        } = workspace.node(id.split)?
        else {
            return None;
        };
        let split = projection.splits.get(&id.split)?;
        if *axis != record.axis()
            || split.axis != *axis
            || id.index >= children.len().checked_sub(1)?
            || !record
                .child_extents()
                .iter()
                .copied()
                .eq(split.children.iter().map(|child| child.extent))
            || record.central_index() != split.central_index
            || record.weights() != weights.as_slice()
            || record.before_minimum_extent() != split.children.get(id.index)?.minimum
            || record.after_minimum_extent() != split.children.get(id.index + 1)?.minimum
            || record.before_maximum_extent() != split.children.get(id.index)?.maximum
            || record.after_maximum_extent() != split.children.get(id.index + 1)?.maximum
        {
            return None;
        }
        let edges = incidents
            .entry(*axis)
            .or_default()
            .entry((id.root, id.split))
            .or_insert_with(|| vec![None; children.len() + 1]);
        merge_edge(edges.get_mut(id.index + 1)?, 1.0)?;
    }

    incidents
        .into_iter()
        .map(|(axis, incidents)| {
            let mut group = FrozenResizeAxisGroup {
                axis,
                splits: Vec::new(),
                common_delta: ResizeDeltaInterval {
                    minimum: f64::NEG_INFINITY,
                    maximum: f64::INFINITY,
                },
            };
            let incident_roots = incidents
                .keys()
                .map(|(root, _)| *root)
                .collect::<BTreeSet<_>>();
            let mut found_incidents = 0;
            for root in incident_roots {
                let prototype = roots.get(&root)?;
                let root_facts = facts.root(root)?;
                let projection = root_facts.projection();
                let root_node = workspace.root(root)?.node;
                if projection.nodes.get(&root_node).map(|result| result.rect)
                    != Some(root_facts.bounds())
                {
                    return None;
                }
                let mut projected_nodes = projection.nodes.iter_preorder();
                let mut pending = vec![(root_node, [0.0, 0.0], false)];
                let mut visited = BTreeSet::new();
                while let Some((node, outer_edges, check_extent)) = pending.pop() {
                    if !visited.insert(node) {
                        return None;
                    }
                    let (&projected_node, result) = projected_nodes.next()?;
                    if projected_node != node {
                        return None;
                    }
                    let rect = result.rect;
                    let incident = incidents.get(&(root, node));
                    let affected = outer_edges != [0.0, 0.0] || incident.is_some();
                    if affected || check_extent {
                        let constraints = result.constraints;
                        intersect_extent_interval(
                            &mut group.common_delta,
                            rect_extent(rect, axis),
                            size_extent(constraints.min(), axis),
                            size_extent(constraints.max(), axis),
                            outer_edges[1] - outer_edges[0],
                            rect_extent_tolerance(rect, axis),
                        )?;
                    }
                    let Node::Split {
                        axis: split_axis,
                        children,
                        weights,
                    } = workspace.node(node)?
                    else {
                        if incident.is_some() {
                            return None;
                        }
                        continue;
                    };
                    let mut edges = vec![0.0; children.len() + 1];
                    if *split_axis == axis {
                        *edges.first_mut()? = outer_edges[0];
                        *edges.last_mut()? = outer_edges[1];
                        if let Some(incident) = incident {
                            if incident.len() != edges.len() {
                                return None;
                            }
                            for (edge, movement) in edges.iter_mut().zip(incident) {
                                if let Some(movement) = movement {
                                    *edge = *movement;
                                }
                            }
                            found_incidents += 1;
                        }
                    } else if incident.is_some() {
                        return None;
                    }
                    if affected {
                        let split = projection.splits.get(&node)?;
                        let bounds = rect;
                        validate_split_projection(
                            projection,
                            &split,
                            *split_axis,
                            bounds,
                            children,
                            weights,
                            facts.config().splitter_thickness(),
                        )?;
                        let coefficients = if *split_axis == axis {
                            edges
                                .windows(2)
                                .map(|pair| pair[1] - pair[0])
                                .collect::<Vec<_>>()
                        } else {
                            vec![0.0; children.len()]
                        };
                        if *split_axis == axis {
                            intersect_split_intervals(
                                &mut group.common_delta,
                                &split,
                                &coefficients,
                            )?;
                        }
                        group.splits.push(FrozenResizeSplit {
                            // Traversal proves membership; share the exact root snapshot.
                            source: NodeSource {
                                root,
                                node,
                                reference: prototype.reference().clone(),
                            },
                            axis: *split_axis,
                            bounds,
                            constraints: result.constraints,
                            weights: weights.clone(),
                            child_extents: split
                                .children
                                .iter()
                                .map(|child| child.extent)
                                .collect(),
                            child_minimum_extents: split
                                .children
                                .iter()
                                .map(|child| child.minimum)
                                .collect(),
                            child_maximum_extents: split
                                .children
                                .iter()
                                .map(|child| child.maximum)
                                .collect(),
                            child_coefficients: coefficients,
                            central_index: split.central_index,
                        });
                    }
                    for (index, child) in children.iter().copied().enumerate().rev() {
                        let child_edges = if *split_axis == axis {
                            [*edges.get(index)?, *edges.get(index + 1)?]
                        } else {
                            outer_edges
                        };
                        pending.push((child, child_edges, affected));
                    }
                }
                if projected_nodes.next().is_some() {
                    return None;
                }
            }
            if found_incidents != incidents.len()
                || !group.common_delta.minimum.is_finite()
                || !group.common_delta.maximum.is_finite()
                || group.common_delta.minimum > 0.0
                || group.common_delta.maximum < 0.0
            {
                return None;
            }
            Some(group)
        })
        .collect()
}

/// Produces one deduplicated batch. Orthogonal frontiers constrain motion and
/// continuation; only the group matching a split's own axis emits its weights.
pub(super) fn resize_axis_group_updates(
    groups: &[FrozenResizeAxisGroup],
    delta: [f64; 2],
) -> Option<Vec<SplitResize>> {
    if groups.is_empty() || delta.iter().any(|value| !value.is_finite()) {
        return None;
    }
    let mut axes = BTreeSet::new();
    let mut updates = BTreeMap::new();
    for group in groups {
        if !axes.insert(group.axis) {
            return None;
        }
        let requested = match group.axis {
            Axis::Horizontal => delta[0],
            Axis::Vertical => delta[1],
        };
        let shared = requested.clamp(group.common_delta.minimum, group.common_delta.maximum);
        for split in group.splits.iter().filter(|split| split.axis == group.axis) {
            let update = split_update(split, shared)?;
            if updates
                .insert((split.source.root(), split.source.node()), update)
                .is_some()
            {
                return None;
            }
        }
    }
    Some(updates.into_values().collect())
}

fn merge_edge(edge: &mut Option<f64>, movement: f64) -> Option<()> {
    if edge.is_some_and(|previous| previous != movement) {
        return None;
    }
    *edge = Some(movement);
    Some(())
}

fn rect_extent(rect: LogicalRect, axis: Axis) -> f64 {
    match axis {
        Axis::Horizontal => rect.width(),
        Axis::Vertical => rect.height(),
    }
}

fn size_extent(size: LogicalSize, axis: Axis) -> f64 {
    match axis {
        Axis::Horizontal => size.width(),
        Axis::Vertical => size.height(),
    }
}

fn tolerance(left: f64, right: f64) -> f64 {
    left.abs().max(right.abs()).max(1.0) * f64::EPSILON * 64.0
}

fn rect_extent_tolerance(rect: LogicalRect, axis: Axis) -> f64 {
    match axis {
        Axis::Horizontal => tolerance(rect.min().x(), rect.max().x()),
        Axis::Vertical => tolerance(rect.min().y(), rect.max().y()),
    }
}

fn intersect_extent_interval(
    interval: &mut ResizeDeltaInterval,
    extent: f64,
    minimum: f64,
    maximum: f64,
    coefficient: f64,
    coordinate_tolerance: f64,
) -> Option<()> {
    if !extent.is_finite()
        || !minimum.is_finite()
        || !maximum.is_finite()
        || minimum < 0.0
        || minimum > maximum
        || minimum - extent > tolerance(extent, minimum).max(coordinate_tolerance)
        || extent - maximum > tolerance(extent, maximum).max(coordinate_tolerance)
    {
        return None;
    }
    if coefficient == 0.0 {
        return Some(());
    }
    let lower = (minimum - extent).min(0.0) / coefficient;
    let upper = (maximum - extent).max(0.0) / coefficient;
    interval.minimum = interval.minimum.max(lower.min(upper));
    interval.maximum = interval.maximum.min(lower.max(upper));
    (interval.minimum <= interval.maximum).then_some(())
}

#[allow(clippy::too_many_arguments)]
fn validate_split_projection(
    projection: &LayoutProjection,
    split: &SplitProjection<'_>,
    axis: Axis,
    bounds: LogicalRect,
    children: &[NodeId],
    weights: &[SplitWeight],
    thickness: f64,
) -> Option<()> {
    let count = children.len();
    if count < 2
        || split.axis != axis
        || count != weights.len()
        || count != split.children.len()
        || split
            .children
            .iter()
            .enumerate()
            .any(|(index, child)| child.following_gap.is_some() != (index + 1 < count))
        || split.central_index.is_some_and(|central| central >= count)
    {
        return None;
    }
    let gaps = std::iter::repeat_n(thickness, count - 1).sum::<f64>();
    let available = split.children.iter().map(|child| child.extent).sum::<f64>();
    let extent = rect_extent(bounds, axis);
    // Child allocation and rectangle max-minus-min may differ by a few ULPs.
    // Judge residuals at the bounds' coordinate scale, without relaxing gaps.
    let extent_tolerance =
        tolerance(available + gaps, extent).max(rect_extent_tolerance(bounds, axis));
    if !available.is_finite()
        || available <= 0.0
        || !gaps.is_finite()
        || gaps > extent
        || !split.overflow.is_finite()
        || split.overflow < 0.0
        || split.overflow > extent_tolerance
        || !split.unallocated.is_finite()
        || split.unallocated < 0.0
        || split.unallocated > extent_tolerance
        || (available + gaps - extent).abs() > extent_tolerance
        || split.splitter_rects().any(|gap| {
            (rect_extent(gap, axis) - thickness).abs()
                > rect_extent_tolerance(gap, axis).max(tolerance(thickness, thickness))
        })
    {
        return None;
    }
    let mut fixed = ResizeDeltaInterval {
        minimum: 0.0,
        maximum: 0.0,
    };
    for (index, child) in children.iter().enumerate() {
        let actual = split.children[index].extent;
        let child_bounds = projection.nodes.get(child)?.rect;
        let projected = rect_extent(child_bounds, axis);
        if actual <= 0.0
            || (actual - projected).abs()
                > tolerance(actual, projected).max(rect_extent_tolerance(child_bounds, axis))
        {
            return None;
        }
        intersect_extent_interval(
            &mut fixed,
            actual,
            split.children[index].minimum,
            split.children[index].maximum,
            0.0,
            0.0,
        )?;
    }
    Some(())
}

fn intersect_split_intervals(
    interval: &mut ResizeDeltaInterval,
    split: &SplitProjection<'_>,
    coefficients: &[f64],
) -> Option<()> {
    let total = split.children.iter().map(|child| child.extent).sum::<f64>();
    let total_coefficient = coefficients.iter().sum::<f64>();
    intersect_extent_interval(
        interval,
        total,
        total * f64::from(f32::EPSILON),
        f64::MAX,
        total_coefficient,
        0.0,
    )?;
    for (child, &coefficient) in split.children.iter().zip(coefficients) {
        let extent = child.extent;
        intersect_extent_interval(
            interval,
            extent,
            child.minimum,
            child.maximum,
            coefficient,
            0.0,
        )?;
        // Keep normalized f32 shares positive as the total changes. A smaller
        // existing share remains valid at zero displacement.
        let share_floor = (extent / total).min(f64::from(f32::EPSILON));
        let margin = (extent - share_floor * total).max(0.0);
        let slope = coefficient - share_floor * total_coefficient;
        if share_floor <= 0.0 {
            return None;
        }
        if slope > 0.0 {
            interval.minimum = interval.minimum.max(-margin / slope);
        } else if slope < 0.0 {
            interval.maximum = interval.maximum.min(-margin / slope);
        }
    }
    (interval.minimum <= interval.maximum).then_some(())
}

fn split_update(split: &FrozenResizeSplit, delta: f64) -> Option<SplitResize> {
    let extents = split
        .child_extents
        .iter()
        .zip(&split.child_coefficients)
        .map(|(&extent, &coefficient)| extent + coefficient * delta)
        .collect::<Vec<_>>();
    if extents == split.child_extents {
        return Some(SplitResize::new(
            split.source.clone(),
            split.weights.clone(),
        ));
    }
    let mut fixed = ResizeDeltaInterval {
        minimum: 0.0,
        maximum: 0.0,
    };
    intersect_extent_interval(
        &mut fixed,
        rect_extent(split.bounds, split.axis)
            + split.child_coefficients.iter().sum::<f64>() * delta,
        size_extent(split.constraints.min(), split.axis),
        size_extent(split.constraints.max(), split.axis),
        0.0,
        rect_extent_tolerance(split.bounds, split.axis),
    )?;
    for (index, &extent) in extents.iter().enumerate() {
        intersect_extent_interval(
            &mut fixed,
            extent,
            split.child_minimum_extents[index],
            split.child_maximum_extents[index],
            0.0,
            0.0,
        )?;
    }
    let total = extents.iter().sum::<f64>();
    if !total.is_finite() || total <= 0.0 {
        return None;
    }
    let mut shares = extents
        .iter()
        .map(|extent| extent / total)
        .collect::<Vec<_>>();
    if let Some(central) = split.central_index {
        let mut siblings = 0.0;
        for (index, share) in shares.iter_mut().enumerate() {
            if index != central {
                *share = f64::from(weight(*share)?.get());
                siblings += *share;
            }
        }
        let central_share = shares.get_mut(central)?;
        let residual = 1.0 - siblings;
        // Rounded siblings can exhaust the residual. Central layout ignores its
        // own weight, so retain the raw positive share in that case.
        if residual > 0.0 {
            *central_share = residual;
        }
    }
    let weights = shares.into_iter().map(weight).collect::<Option<Vec<_>>>()?;
    Some(SplitResize::new(split.source.clone(), weights))
}

fn weight(value: f64) -> Option<SplitWeight> {
    #[allow(
        clippy::cast_possible_truncation,
        reason = "normalized logical extents use the workspace's f32 weight representation"
    )]
    SplitWeight::new(value as f32).ok()
}

#[cfg(test)]
mod tests;
