//! Feasible-delta solving and weight reconstruction for splitter resizing.

use std::collections::BTreeMap;

use crate::command::{NodeSource, SplitResize};
use crate::geometry::LogicalPoint;
use crate::graph::Axis;
use crate::interaction::{
    ActiveResize, FrozenResizeAxisGroup, FrozenResizeHandle, ResizeDeltaInterval,
};
use crate::scene::SplitterRecord;

pub(super) fn split_resize_updates(
    resize: &ActiveResize,
    current_pointer: LogicalPoint,
) -> Option<Vec<SplitResize>> {
    let mut updates = Vec::new();
    for group in &resize.axis_groups {
        let initial = match group.axis {
            Axis::Horizontal => resize.initial_pointer.x(),
            Axis::Vertical => resize.initial_pointer.y(),
        };
        let current = match group.axis {
            Axis::Horizontal => current_pointer.x(),
            Axis::Vertical => current_pointer.y(),
        };
        let requested_delta = current - initial;
        if !requested_delta.is_finite() {
            return None;
        }
        let shared_delta =
            requested_delta.clamp(group.common_delta.minimum, group.common_delta.maximum);
        for handle in &group.handles {
            updates.push(split_resize_update(handle, shared_delta)?);
        }
    }
    Some(updates)
}

pub(super) fn resize_delta_interval(record: &SplitterRecord) -> Option<ResizeDeltaInterval> {
    let index = record.id().index;
    let extents = record.child_extents();
    let before = *extents.get(index)?;
    let after = *extents.get(index + 1)?;
    let pair_extent = before + after;
    let full_extent = extents.iter().sum::<f64>();
    if !full_extent.is_finite() || full_extent <= 0.0 {
        return None;
    }
    let representable_minimum = full_extent * f64::from(f32::EPSILON);
    let before_minimum = record.before_minimum_extent().max(representable_minimum);
    let after_minimum = record.after_minimum_extent().max(representable_minimum);
    let before_maximum = record.before_maximum_extent();
    let after_maximum = record.after_maximum_extent();
    let minimum_sum = before_minimum + after_minimum;
    let tolerance = f64::EPSILON
        * pair_extent
            .abs()
            .max(minimum_sum.abs())
            .max(before_maximum.abs())
            .max(after_maximum.abs())
            .max(1.0)
        * 16.0;
    if !pair_extent.is_finite()
        || pair_extent <= 0.0
        || !minimum_sum.is_finite()
        || !before_maximum.is_finite()
        || !after_maximum.is_finite()
        || before_maximum < before_minimum
        || after_maximum < after_minimum
        || (minimum_sum > pair_extent && minimum_sum - pair_extent > tolerance)
    {
        return None;
    }
    let minimum = (before_minimum - before).max(after - after_maximum);
    let mut maximum = (before_maximum - before).min(after - after_minimum);
    if minimum > maximum && minimum - maximum > tolerance {
        return None;
    }
    if minimum > maximum {
        maximum = minimum;
    }
    Some(ResizeDeltaInterval { minimum, maximum })
}

pub(super) fn prepare_resize_axis_groups(
    handles: Vec<(NodeSource, SplitterRecord)>,
) -> Option<Vec<FrozenResizeAxisGroup>> {
    let mut grouped = BTreeMap::<Axis, Vec<FrozenResizeHandle>>::new();
    for (source, record) in handles {
        let allowed_delta = resize_delta_interval(&record)?;
        grouped
            .entry(record.axis())
            .or_default()
            .push(FrozenResizeHandle {
                source,
                record,
                allowed_delta,
            });
    }

    grouped
        .into_iter()
        .map(|(axis, mut handles)| {
            handles.sort_unstable_by_key(|handle| *handle.record.id());
            let minimum = handles
                .iter()
                .map(|handle| handle.allowed_delta.minimum)
                .max_by(f64::total_cmp)?;
            let maximum = handles
                .iter()
                .map(|handle| handle.allowed_delta.maximum)
                .min_by(f64::total_cmp)?;
            (minimum <= maximum).then_some(FrozenResizeAxisGroup {
                axis,
                handles,
                common_delta: ResizeDeltaInterval { minimum, maximum },
            })
        })
        .collect()
}

pub(super) fn split_resize_update(handle: &FrozenResizeHandle, delta: f64) -> Option<SplitResize> {
    if !delta.is_finite() {
        return None;
    }
    if delta < handle.allowed_delta.minimum || delta > handle.allowed_delta.maximum {
        return None;
    }
    let record = &handle.record;
    let index = record.id().index;
    let mut extents = record.child_extents().to_vec();
    let before = *extents.get(index)?;
    let after = *extents.get(index + 1)?;
    extents[index] = before + delta;
    extents[index + 1] = after - delta;
    let available = extents.iter().sum::<f64>();
    if !available.is_finite() || available <= 0.0 {
        return None;
    }

    let mut values = vec![0.0_f64; extents.len()];
    if let Some(central) = record.central_index() {
        if central >= values.len() {
            return None;
        }
        let mut noncentral_sum = 0.0;
        for (child, extent) in extents.iter().copied().enumerate() {
            if child == central {
                continue;
            }
            let share = extent / available;
            if !share.is_finite() || share <= 0.0 {
                return None;
            }
            values[child] = share;
            noncentral_sum += share;
        }
        let residual = 1.0 - noncentral_sum;
        if !residual.is_finite() || residual <= 0.0 {
            return None;
        }
        values[central] = residual;
    } else {
        for (share, extent) in values.iter_mut().zip(extents) {
            *share = extent / available;
            if !share.is_finite() || *share <= 0.0 {
                return None;
            }
        }
    }

    #[allow(
        clippy::cast_possible_truncation,
        reason = "split weights intentionally use the workspace's f32 storage model"
    )]
    let values = values.into_iter().map(|value| value as f32);
    let weights = crate::graph::SplitWeight::normalize(values).ok()?;
    Some(SplitResize::new(handle.source.clone(), weights))
}
