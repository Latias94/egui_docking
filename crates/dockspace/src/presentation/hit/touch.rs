//! Private device-specific hit fragments retained with one exact output.

use crate::geometry::{LogicalPoint, LogicalRect};
use crate::graph::Axis;
use crate::interaction::targeting::targets::{DropOcclusionRecord, SceneLayerKey};
use crate::presentation::scene::{
    ContainedRecord, ContainedResizeDirection, PresentationLayoutFacts,
    PresentationMenuAnchorRecord, PresentationPlan, SplitterJunctionId, SplitterRecord,
    SplitterSceneId, TabBarRecord,
};

use super::geometry_reuse::rect_bits;
use super::{PresentationHitRegion, PresentationHitRegionKind};

#[cfg(test)]
mod tests;

#[cfg(test)]
thread_local! {
    static FRAGMENT_BUFFER_CUTS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    static REFERENCE_SUBTRACTION: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    static REFERENCE_LINEAR_SPLITTER_LOOKUP: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    static NEIGHBOR_BOUNDS_VISITS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

pub(super) const fn replaces(kind: PresentationHitRegionKind) -> bool {
    matches!(
        kind,
        PresentationHitRegionKind::SplitterHandle(_)
            | PresentationHitRegionKind::SplitterJunction(_)
            | PresentationHitRegionKind::ContainedResize { .. }
    )
}

// Both compilation and reuse consume this exact input view. New touch geometry
// dependencies belong here and in same_inputs; output tickets never belong here.
#[derive(Clone, Copy)]
struct TouchContext<'a> {
    bounds: LogicalRect,
    facts: Option<&'a PresentationLayoutFacts>,
    splitters: &'a [SplitterRecord],
    occlusions: &'a [DropOcclusionRecord],
    bars: &'a [TabBarRecord],
    contained: &'a [ContainedRecord],
    anchors: &'a [PresentationMenuAnchorRecord],
}

impl<'a> TouchContext<'a> {
    fn new(plan: &'a PresentationPlan) -> Self {
        Self {
            bounds: plan.bounds(),
            facts: plan.layout_facts(),
            splitters: plan.splitter_records(),
            occlusions: plan.drop_occlusions(),
            bars: plan.tab_bar_records(),
            contained: plan.contained_records(),
            anchors: plan.presentation_menu_anchor_records(),
        }
    }

    fn protected_chrome(self) -> impl Iterator<Item = (SceneLayerKey, LogicalRect)> + 'a {
        self.bars
            .iter()
            .map(|bar| (bar.layer(), bar.bounds()))
            .chain(
                self.contained
                    .iter()
                    .map(|contained| (contained.layer(), contained.title_bounds())),
            )
            .chain(
                self.anchors.iter().filter_map(|anchor| {
                    anchor.ready_bounds().map(|bounds| (anchor.layer(), bounds))
                }),
            )
    }

    fn same_inputs(self, other: Self) -> bool {
        let same_facts = match (self.facts, other.facts) {
            (Some(left), Some(right)) => {
                let extents = |facts: &PresentationLayoutFacts| {
                    let config = facts.config();
                    [
                        config.touch_resize_hit_extent(),
                        config.splitter_hit_extent(),
                        config.floating_resize_extent(),
                    ]
                    .map(f64::to_bits)
                };
                extents(left) == extents(right)
                    && left
                        .roots()
                        .map(|root| (root.root(), rect_bits(root.bounds())))
                        .eq(right
                            .roots()
                            .map(|root| (root.root(), rect_bits(root.bounds()))))
            }
            (None, None) => true,
            _ => false,
        };
        same_facts
            && rect_bits(self.bounds) == rect_bits(other.bounds)
            && self
                .splitters
                .iter()
                .map(|splitter| {
                    (*splitter.id(), splitter.axis(), rect_bits(splitter.draw_bounds()))
                })
                .eq(other.splitters.iter().map(|splitter| {
                    (*splitter.id(), splitter.axis(), rect_bits(splitter.draw_bounds()))
                }))
            // Order and complete identities preserve first-match unclipped bounds
            // and the exact sequence of occlusion subtraction.
            && self
                .occlusions
                .iter()
                .map(|record| (record.floating(), record.layer(), rect_bits(record.region().rect())))
                .eq(other.occlusions.iter().map(|record| {
                    (record.floating(), record.layer(), rect_bits(record.region().rect()))
                }))
            && self
                .protected_chrome()
                .map(|(layer, bounds)| (layer, rect_bits(bounds)))
                .eq(other
                    .protected_chrome()
                    .map(|(layer, bounds)| (layer, rect_bits(bounds))))
    }
}

pub(super) fn same_context(left: &PresentationPlan, right: &PresentationPlan) -> bool {
    TouchContext::new(left).same_inputs(TouchContext::new(right))
}

pub(super) struct TouchGeometry {
    pub(super) fragments: Vec<(usize, LogicalRect)>,
    pub(super) order: Box<[usize]>,
}

#[cfg(test)]
fn compile(
    plan: &PresentationPlan,
    regions: &[PresentationHitRegion],
) -> Vec<(usize, LogicalRect)> {
    compile_reusing_order(plan, regions, None).fragments
}

pub(super) fn compile_reusing_order(
    plan: &PresentationPlan,
    regions: &[PresentationHitRegion],
    previous_order: Option<&[usize]>,
) -> TouchGeometry {
    let context = TouchContext::new(plan);
    let Some(facts) = context.facts else {
        // Synthetic plans have no exact configuration authority. Fail closed.
        return TouchGeometry {
            fragments: Vec::new(),
            order: Box::new([]),
        };
    };
    let config = facts.config();
    let splitter_extent = config
        .touch_resize_hit_extent()
        .max(config.splitter_hit_extent());
    let contained_extent = config
        .touch_resize_hit_extent()
        .max(config.floating_resize_extent());
    let splitters = SplitterLookup::new(context.splitters);
    let protected = protected_regions(context, regions);
    let mut allocations = Vec::new();
    for (region_index, region) in regions.iter().enumerate() {
        let (rect, anchor) = match region.id().kind() {
            PresentationHitRegionKind::SplitterHandle(id) => {
                let Some(splitter) = splitters.find(id) else {
                    continue;
                };
                let Some(root) = facts.root(id.root) else {
                    continue;
                };
                let Some(clip) = intersection(root.bounds(), context.bounds) else {
                    continue;
                };
                (
                    splitter_band(
                        splitter.draw_bounds(),
                        splitter.axis(),
                        splitter_extent,
                        clip,
                    ),
                    splitter.draw_bounds(),
                )
            }
            PresentationHitRegionKind::SplitterJunction(id) => {
                let Some(root) = facts.root(id.root()) else {
                    continue;
                };
                let Some(clip) = intersection(root.bounds(), context.bounds) else {
                    continue;
                };
                let extent = splitter_extent
                    .max(region.hit().rect().width())
                    .max(region.hit().rect().height());
                (
                    junction_square(&splitters, id, extent, clip),
                    region.hit().rect(),
                )
            }
            PresentationHitRegionKind::ContainedResize {
                floating,
                direction,
            } => {
                let Some(outer) = context
                    .occlusions
                    .iter()
                    .find(|record| record.floating() == floating)
                    .map(|record| record.region().rect())
                else {
                    continue;
                };
                (
                    contained_partition(
                        outer,
                        config.floating_resize_extent(),
                        contained_extent,
                        direction,
                        context.bounds,
                    ),
                    region.hit().rect(),
                )
            }
            _ => continue,
        };
        let Some(rect) = rect.or_else(|| intersection(region.hit().rect(), context.bounds)) else {
            continue;
        };
        allocations.push(TouchAllocation {
            region_index,
            region,
            anchor,
            expanded: rect,
            fragments: vec![rect],
        });
    }
    let (neighbors, order) =
        TouchNeighbors::build_reusing_order(&allocations, &protected, previous_order);
    partition_allocations(&mut allocations, &neighbors);
    let mut compiled = Vec::new();
    let mut base = Vec::new();
    let allocation_count = allocations.len();
    for (index, allocation) in allocations.into_iter().enumerate() {
        let region = allocation.region;
        let mut fragments = allocation.fragments;
        for occlusion in context.occlusions {
            if occlusion.layer() > region.stack().layer() {
                subtract_from(&mut fragments, occlusion.region().rect());
            }
        }
        // Chrome protects expanded halos. The already partitioned, visible base
        // keeps the same receiver precedence as the mouse footprint.
        base.clear();
        base.extend(
            fragments
                .iter()
                .filter_map(|rect| intersection(*rect, region.hit().rect())),
        );
        let adjacent = neighbors.for_region(index);
        let protected_start = adjacent.partition_point(|&neighbor| neighbor < allocation_count);
        for &neighbor in &adjacent[protected_start..] {
            let protected_index = neighbor - allocation_count;
            let (layer, protected_rect) = protected[protected_index];
            if layer <= region.stack().layer() {
                subtract_from(&mut fragments, protected_rect);
            }
        }
        for fragment in &fragments {
            subtract_from(&mut base, *fragment);
        }
        compiled.extend(
            fragments
                .into_iter()
                .chain(base.iter().copied())
                .map(|rect| (allocation.region_index, rect)),
        );
    }
    debug_assert!(compiled.windows(2).all(|pair| pair[0].0 <= pair[1].0));
    TouchGeometry {
        fragments: compiled,
        order,
    }
}

#[cfg_attr(test, derive(Clone))]
struct TouchAllocation<'a> {
    region_index: usize,
    region: &'a PresentationHitRegion,
    anchor: LogicalRect,
    expanded: LogicalRect,
    fragments: Vec<LogicalRect>,
}

struct SplitterLookup<'a> {
    records: &'a [SplitterRecord],
    strictly_sorted: bool,
}

impl<'a> SplitterLookup<'a> {
    fn new(records: &'a [SplitterRecord]) -> Self {
        Self {
            records,
            strictly_sorted: records.windows(2).all(|pair| pair[0].id() < pair[1].id()),
        }
    }

    fn find(&self, id: SplitterSceneId) -> Option<&'a SplitterRecord> {
        let use_binary_search = self.strictly_sorted;
        #[cfg(test)]
        let use_binary_search = use_binary_search && !REFERENCE_LINEAR_SPLITTER_LOOKUP.get();
        if use_binary_search {
            self.records
                .binary_search_by_key(&id, |record| *record.id())
                .ok()
                .map(|index| &self.records[index])
        } else {
            // Synthetic and duplicate inventories retain the original first match.
            self.records.iter().find(|record| *record.id() == id)
        }
    }
}

// This broad phase only removes disjoint pairs; exact geometry and stable pair
// order remain in the partitioner. Dense layouts retain bounded scratch space.
enum TouchNeighbors {
    Sparse(Vec<Vec<usize>>),
    Dense(Vec<usize>),
}

#[derive(Clone, Copy)]
struct TouchBounds {
    min_x: f64,
    min_y: f64,
    max_x: f64,
    max_y: f64,
}

impl TouchBounds {
    const EMPTY: Self = Self {
        min_x: f64::INFINITY,
        min_y: f64::INFINITY,
        max_x: f64::NEG_INFINITY,
        max_y: f64::NEG_INFINITY,
    };

    fn covering(self, other: Self) -> Self {
        Self {
            min_x: self.min_x.min(other.min_x),
            min_y: self.min_y.min(other.min_y),
            max_x: self.max_x.max(other.max_x),
            max_y: self.max_y.max(other.max_y),
        }
    }

    fn union(first: LogicalRect, second: LogicalRect) -> Self {
        // Keep endpoints directly: rebuilding a rectangle via max - min can
        // round its far edge inward at extreme finite coordinate magnitudes.
        Self {
            min_x: first.x().min(second.x()),
            min_y: first.y().min(second.y()),
            max_x: first.max().x().max(second.max().x()),
            max_y: first.max().y().max(second.max().y()),
        }
    }

    fn overlaps(&self, other: &Self) -> bool {
        self.min_x < other.max_x
            && other.min_x < self.max_x
            && self.min_y < other.max_y
            && other.min_y < self.max_y
    }
}

impl TouchNeighbors {
    #[cfg(test)]
    fn build(
        allocations: &[TouchAllocation<'_>],
        protected: &[(SceneLayerKey, LogicalRect)],
    ) -> Self {
        Self::build_reusing_order(allocations, protected, None).0
    }

    fn build_reusing_order(
        allocations: &[TouchAllocation<'_>],
        protected: &[(SceneLayerKey, LogicalRect)],
        previous_order: Option<&[usize]>,
    ) -> (Self, Box<[usize]>) {
        let allocation_count = allocations.len();
        let dense = || Self::Dense((0..allocation_count + protected.len()).collect());
        let mut entries = Vec::with_capacity(allocation_count + protected.len());
        for (index, allocation) in allocations.iter().enumerate() {
            let bounds = TouchBounds::union(allocation.expanded, allocation.region.hit().rect());
            entries.push((bounds, allocation.region.stack().layer(), index));
        }
        entries.extend(
            protected
                .iter()
                .enumerate()
                .map(|(index, &(layer, bounds))| {
                    (
                        TouchBounds::union(bounds, bounds),
                        layer,
                        allocation_count + index,
                    )
                }),
        );
        // A static bounds hierarchy prunes both axes and never compares two
        // protected-chrome records. Leaves retain original row identities;
        // the final per-row sort preserves all ordered subtraction semantics.
        let leaves = entries.len().next_power_of_two();
        let mut bounds = vec![TouchBounds::EMPTY; leaves * 2];
        if let Some(order) = previous_order.filter(|order| order.len() == entries.len()) {
            // This private hint is always a complete permutation of row indices.
            // Reuse only the tree topology: every current leaf and parent bounds
            // is recomputed, so any changed geometry still receives exact queries.
            entries = order.iter().map(|&row| entries[row]).collect();
            for (index, entry) in entries.iter().enumerate() {
                bounds[leaves + index] = entry.0;
            }
            for index in (1..leaves).rev() {
                bounds[index] = bounds[index * 2].covering(bounds[index * 2 + 1]);
            }
        } else {
            partition_neighbor_bounds(&mut entries, &mut bounds, 1, leaves);
        }
        let order = entries.iter().map(|entry| entry.2).collect::<Box<[_]>>();
        let mut neighbors = vec![Vec::new(); allocation_count];
        let mut links = 0_usize;
        let limit = allocation_count.saturating_mul(64);
        let mut pending = Vec::new();
        for (first_id, allocation) in allocations.iter().enumerate() {
            let first = TouchBounds::union(allocation.expanded, allocation.region.hit().rect());
            let first_layer = allocation.region.stack().layer();
            pending.push(1);
            while let Some(node) = pending.pop() {
                #[cfg(test)]
                NEIGHBOR_BOUNDS_VISITS.set(NEIGHBOR_BOUNDS_VISITS.get() + 1);
                if !first.overlaps(&bounds[node]) {
                    continue;
                }
                if node < leaves {
                    pending.extend([node * 2, node * 2 + 1]);
                    continue;
                }
                let Some((_, second_layer, second_id)) = entries.get(node - leaves) else {
                    continue;
                };
                let (second_layer, second_id) = (*second_layer, *second_id);
                let relevant = if second_id < allocation_count {
                    first_id < second_id && first_layer == second_layer
                } else {
                    second_layer <= first_layer
                };
                if !relevant {
                    continue;
                }
                neighbors[first_id].push(second_id);
                links += 1;
                if second_id < allocation_count {
                    neighbors[second_id].push(first_id);
                    links += 1;
                }
                if links > limit {
                    return (dense(), order);
                }
            }
        }
        for adjacent in &mut neighbors {
            adjacent.sort_unstable();
        }
        (Self::Sparse(neighbors), order)
    }

    fn for_region(&self, index: usize) -> &[usize] {
        match self {
            Self::Sparse(neighbors) => &neighbors[index],
            Self::Dense(all) => all,
        }
    }
}

fn partition_neighbor_bounds(
    entries: &mut [(TouchBounds, SceneLayerKey, usize)],
    bounds: &mut [TouchBounds],
    node: usize,
    leaves: usize,
) {
    if entries.is_empty() {
        return;
    }
    let covering = entries
        .iter()
        .fold(TouchBounds::EMPTY, |all, entry| all.covering(entry.0));
    bounds[node] = covering;
    if leaves == 1 {
        return;
    }
    let half = leaves / 2;
    let split = half.min(entries.len());
    if split < entries.len() {
        // Split by the spread of centers, rather than the boxes' own extent:
        // long parallel bands need separation on their short axis. Halving
        // endpoints before addition keeps finite centers at extreme magnitudes.
        let center = |bounds: TouchBounds| {
            (
                bounds.min_x * 0.5 + bounds.max_x * 0.5,
                bounds.min_y * 0.5 + bounds.max_y * 0.5,
            )
        };
        let centers = entries.iter().fold(TouchBounds::EMPTY, |all, entry| {
            let (x, y) = center(entry.0);
            all.covering(TouchBounds {
                min_x: x,
                min_y: y,
                max_x: x,
                max_y: y,
            })
        });
        let horizontal = centers.max_x - centers.min_x >= centers.max_y - centers.min_y;
        entries.select_nth_unstable_by(split, |first, second| {
            let first_center = center(first.0);
            let second_center = center(second.0);
            let (first_axis, second_axis) = if horizontal {
                (first_center.0, second_center.0)
            } else {
                (first_center.1, second_center.1)
            };
            first_axis
                .total_cmp(&second_axis)
                .then(first.2.cmp(&second.2))
        });
    }
    let (left, right) = entries.split_at_mut(split);
    partition_neighbor_bounds(left, bounds, node * 2, half);
    partition_neighbor_bounds(right, bounds, node * 2 + 1, half);
}

fn partition_allocations(allocations: &mut [TouchAllocation<'_>], neighbors: &TouchNeighbors) {
    let allocation_count = allocations.len();
    let mut original = Vec::new();
    for index in 0..allocation_count {
        let (before, after) = allocations.split_at_mut(index + 1);
        let first = &mut before[index];
        for &neighbor in neighbors.for_region(index) {
            if neighbor >= allocation_count {
                break;
            }
            let Some(second_index) = neighbor.checked_sub(index + 1) else {
                continue;
            };
            let Some(second) = after.get_mut(second_index) else {
                continue;
            };
            if first.region.stack().layer() != second.region.stack().layer() {
                continue;
            }
            let Some(overlap) = intersection(first.expanded, second.expanded) else {
                continue;
            };
            if first.region.stack() == second.region.stack()
                && is_splitter_handle(first.region)
                && is_splitter_handle(second.region)
            {
                // Layout projection separates painted bases with each split's reserved band.
                // Descendant bands meet ancestors only at frontiers, where perpendicular
                // internal contacts become junctions. This overlap is between expanded
                // touch targets, not overlapping painted splitter bases.
                subtract_from(&mut first.fragments, second.anchor);
                subtract_from(&mut second.fragments, first.anchor);
            }
            if let Some((axis, boundary, first_low)) =
                separating_boundary(first.anchor, second.anchor)
            {
                partition_overlap(first, second, overlap, axis, boundary, first_low);
            } else {
                match first.region.stack().cmp(&second.region.stack()) {
                    std::cmp::Ordering::Less => subtract_from(&mut first.fragments, overlap),
                    std::cmp::Ordering::Greater => subtract_from(&mut second.fragments, overlap),
                    std::cmp::Ordering::Equal => match splitter_tie_break(first, second) {
                        std::cmp::Ordering::Greater => {
                            subtract_from(&mut second.fragments, overlap);
                        }
                        std::cmp::Ordering::Less => {
                            subtract_from(&mut first.fragments, overlap);
                        }
                        std::cmp::Ordering::Equal => {
                            subtract_from(&mut first.fragments, overlap);
                            subtract_from(&mut second.fragments, overlap);
                        }
                    },
                }
            }
        }
    }

    // Restore base coverage without undoing a same-priority splitter partition.
    for index in 0..allocation_count {
        let (before, current_and_after) = allocations.split_at_mut(index);
        let Some((current, after)) = current_and_after.split_first_mut() else {
            continue;
        };
        let base = current.region.hit().rect();
        original.clear();
        original.push(base);
        for &neighbor in neighbors.for_region(index) {
            if neighbor >= allocation_count {
                break;
            }
            let other = if neighbor < index {
                &before[neighbor]
            } else if neighbor > index {
                let Some(other) = after.get(neighbor - index - 1) else {
                    continue;
                };
                other
            } else {
                continue;
            };
            if other.region.stack().layer() != current.region.stack().layer() {
                continue;
            }
            if is_splitter_region(current.region)
                && is_splitter_region(other.region)
                && current.region.stack() == other.region.stack()
            {
                if let Some(overlap) = intersection(base, other.region.hit().rect()) {
                    if let Some((axis, boundary, current_low)) =
                        separating_boundary(current.anchor, other.anchor)
                    {
                        if let Some(cut) = overlap_half(overlap, axis, boundary, !current_low) {
                            subtract_from(&mut original, cut);
                        }
                    } else if current.region.id() > other.region.id() {
                        subtract_from(&mut original, overlap);
                    }
                }
                continue;
            }
            subtract_from(&mut current.fragments, other.region.hit().rect());
            if other.region.stack() >= current.region.stack() {
                subtract_from(&mut original, other.region.hit().rect());
            }
        }
        // Restore only missing base fragments; unchanged bands stay compact.
        for fragment in &current.fragments {
            subtract_from(&mut original, *fragment);
        }
        current.fragments.extend(
            original
                .iter()
                .copied()
                .filter(|rect| rect.width() > 0.0 && rect.height() > 0.0),
        );
    }
}

fn is_splitter_region(region: &PresentationHitRegion) -> bool {
    matches!(
        region.id().kind(),
        PresentationHitRegionKind::SplitterHandle(_)
            | PresentationHitRegionKind::SplitterJunction(_)
    )
}

fn is_splitter_handle(region: &PresentationHitRegion) -> bool {
    matches!(
        region.id().kind(),
        PresentationHitRegionKind::SplitterHandle(_)
    )
}

fn splitter_tie_break(
    first: &TouchAllocation<'_>,
    second: &TouchAllocation<'_>,
) -> std::cmp::Ordering {
    if !is_splitter_region(first.region) || !is_splitter_region(second.region) {
        return std::cmp::Ordering::Equal;
    }
    second.region.id().cmp(&first.region.id())
}

fn separating_boundary(first: LogicalRect, second: LogicalRect) -> Option<(Axis, f64, bool)> {
    let gap = |first_min, first_max, second_min, second_max| {
        if first_max <= second_min {
            Some((
                second_min - first_max,
                midpoint(first_max, second_min),
                true,
            ))
        } else if second_max <= first_min {
            Some((
                first_min - second_max,
                midpoint(second_max, first_min),
                false,
            ))
        } else {
            None
        }
    };
    let x = gap(first.x(), first.max().x(), second.x(), second.max().x());
    let y = gap(first.y(), first.max().y(), second.y(), second.max().y());
    match (x, y) {
        (Some(x), Some(y)) if y.0 > x.0 => Some((Axis::Vertical, y.1, y.2)),
        (Some(x), _) => Some((Axis::Horizontal, x.1, x.2)),
        (None, Some(y)) => Some((Axis::Vertical, y.1, y.2)),
        (None, None) => {
            let first_x = midpoint(first.x(), first.max().x());
            let second_x = midpoint(second.x(), second.max().x());
            let first_y = midpoint(first.y(), first.max().y());
            let second_y = midpoint(second.y(), second.max().y());
            let delta_x = (first_x - second_x).abs();
            let delta_y = (first_y - second_y).abs();
            if delta_x > delta_y {
                Some((
                    Axis::Horizontal,
                    midpoint(first_x, second_x),
                    first_x < second_x,
                ))
            } else if delta_y > 0.0 {
                Some((
                    Axis::Vertical,
                    midpoint(first_y, second_y),
                    first_y < second_y,
                ))
            } else {
                None
            }
        }
    }
}

fn partition_overlap(
    first: &mut TouchAllocation<'_>,
    second: &mut TouchAllocation<'_>,
    overlap: LogicalRect,
    axis: Axis,
    boundary: f64,
    first_low: bool,
) {
    if let Some(cut) = overlap_half(overlap, axis, boundary, !first_low) {
        subtract_from(&mut first.fragments, cut);
    }
    if let Some(cut) = overlap_half(overlap, axis, boundary, first_low) {
        subtract_from(&mut second.fragments, cut);
    }
}

fn overlap_half(overlap: LogicalRect, axis: Axis, boundary: f64, low: bool) -> Option<LogicalRect> {
    match (axis, low) {
        (Axis::Horizontal, true) => clipped_edges(
            overlap.x(),
            overlap.y(),
            boundary,
            overlap.max().y(),
            overlap,
        ),
        (Axis::Horizontal, false) => clipped_edges(
            boundary,
            overlap.y(),
            overlap.max().x(),
            overlap.max().y(),
            overlap,
        ),
        (Axis::Vertical, true) => clipped_edges(
            overlap.x(),
            overlap.y(),
            overlap.max().x(),
            boundary,
            overlap,
        ),
        (Axis::Vertical, false) => clipped_edges(
            overlap.x(),
            boundary,
            overlap.max().x(),
            overlap.max().y(),
            overlap,
        ),
    }
}

fn protected_regions(
    context: TouchContext<'_>,
    regions: &[PresentationHitRegion],
) -> Vec<(SceneLayerKey, LogicalRect)> {
    let mut protected = Vec::new();
    let mut visible = Vec::new();
    let mut protect = |rect, layer| {
        if let Some(rect) = intersection(rect, context.bounds) {
            // Hidden background chrome must not punch holes in a front window's band.
            visible.clear();
            visible.push(rect);
            for occlusion in context.occlusions {
                if occlusion.layer() > layer {
                    subtract_from(&mut visible, occlusion.region().rect());
                }
            }
            protected.extend(visible.iter().copied().map(|rect| (layer, rect)));
        }
    };
    for region in regions {
        if matches!(
            region.id().kind(),
            PresentationHitRegionKind::TabBody(_)
                | PresentationHitRegionKind::TabClose(_)
                | PresentationHitRegionKind::TabGroupDrag { .. }
                | PresentationHitRegionKind::TabStripControl(_)
                | PresentationHitRegionKind::ContainedTitle(_)
                | PresentationHitRegionKind::ContainedClose(_)
        ) {
            protect(region.hit().rect(), region.stack().layer());
        }
    }
    // Complete strip/title bounds also protect disabled and non-receiver chrome.
    for (layer, bounds) in context.protected_chrome() {
        protect(bounds, layer);
    }
    protected
}

fn splitter_band(
    draw: LogicalRect,
    axis: Axis,
    extent: f64,
    clip: LogicalRect,
) -> Option<LogicalRect> {
    let half = extent * 0.5;
    match axis {
        Axis::Horizontal => {
            let center = midpoint(draw.x(), draw.max().x());
            clipped_edges(center - half, draw.y(), center + half, draw.max().y(), clip)
        }
        Axis::Vertical => {
            let center = midpoint(draw.y(), draw.max().y());
            clipped_edges(draw.x(), center - half, draw.max().x(), center + half, clip)
        }
    }
}

fn junction_square(
    splitters: &SplitterLookup<'_>,
    id: SplitterJunctionId,
    extent: f64,
    clip: LogicalRect,
) -> Option<LogicalRect> {
    let mut x = None;
    let mut y = None;
    for splitter in id.splitters() {
        let record = splitters.find(splitter)?;
        let draw = record.draw_bounds();
        match record.axis() {
            Axis::Horizontal => x = Some(midpoint(draw.x(), draw.max().x())),
            Axis::Vertical => y = Some(midpoint(draw.y(), draw.max().y())),
        }
    }
    let (x, y) = (x?, y?);
    let half = extent * 0.5;
    clipped_edges(x - half, y - half, x + half, y + half, clip)
}

fn contained_partition(
    outer: LogicalRect,
    mouse_extent: f64,
    extent: f64,
    direction: ContainedResizeDirection,
    clip: LogicalRect,
) -> Option<LogicalRect> {
    // Expand around the original mouse band's center, not the clipped frame.
    let expansion = (extent - mouse_extent) * 0.5;
    let x0 = outer.x() - expansion;
    let y0 = outer.y() - expansion;
    let x1 = outer.max().x() + expansion;
    let y1 = outer.max().y() + expansion;
    // Midpoint limits preserve the existing disjoint eight-way small-window partition.
    let inset = midpoint(extent, mouse_extent);
    let center_x = midpoint(outer.x(), outer.max().x());
    let center_y = midpoint(outer.y(), outer.max().y());
    let left = (outer.x() + inset).min(center_x);
    let right = (outer.max().x() - inset).max(center_x);
    let top = (outer.y() + inset).min(center_y);
    let bottom = (outer.max().y() - inset).max(center_y);
    let (left, top, right, bottom) = match direction {
        ContainedResizeDirection::NorthWest => (x0, y0, left, top),
        ContainedResizeDirection::North => (left, y0, right, top),
        ContainedResizeDirection::NorthEast => (right, y0, x1, top),
        ContainedResizeDirection::East => (right, top, x1, bottom),
        ContainedResizeDirection::SouthEast => (right, bottom, x1, y1),
        ContainedResizeDirection::South => (left, bottom, right, y1),
        ContainedResizeDirection::SouthWest => (x0, bottom, left, y1),
        ContainedResizeDirection::West => (x0, top, left, bottom),
    };
    clipped_edges(left, top, right, bottom, clip)
}

fn midpoint(minimum: f64, maximum: f64) -> f64 {
    minimum * 0.5 + maximum * 0.5
}

fn clipped_edges(
    left: f64,
    top: f64,
    right: f64,
    bottom: f64,
    clip: LogicalRect,
) -> Option<LogicalRect> {
    let left = left.max(clip.x());
    let top = top.max(clip.y());
    let right = right.min(clip.max().x());
    let bottom = bottom.min(clip.max().y());
    if left >= right || top >= bottom {
        return None;
    }
    // Clip endpoints before constructing geometry, including very large finite extents.
    LogicalRect::from_min_max(
        LogicalPoint::new(left, top).ok()?,
        LogicalPoint::new(right, bottom).ok()?,
    )
    .ok()
}

fn intersection(rect: LogicalRect, clip: LogicalRect) -> Option<LogicalRect> {
    clipped_edges(rect.x(), rect.y(), rect.max().x(), rect.max().y(), clip)
}

fn subtract_from(fragments: &mut Vec<LogicalRect>, cut: LogicalRect) {
    #[cfg(test)]
    if REFERENCE_SUBTRACTION.get() {
        tests::subtract_reference(fragments, cut);
        return;
    }
    let Some(first) = fragments
        .iter()
        .position(|rect| intersection(*rect, cut).is_some())
    else {
        return;
    };
    #[cfg(test)]
    FRAGMENT_BUFFER_CUTS.set(FRAGMENT_BUFFER_CUTS.get() + 1);
    // Append the transformed suffix beyond the original inputs before moving
    // it into place. Cuts keep the allocation and the exact fragment order.
    let original_len = fragments.len();
    for index in first..original_len {
        let rect = fragments[index];
        let Some(overlap) = intersection(rect, cut) else {
            fragments.push(rect);
            continue;
        };
        for (left, top, right, bottom) in [
            (rect.x(), rect.y(), rect.max().x(), overlap.y()),
            (rect.x(), overlap.max().y(), rect.max().x(), rect.max().y()),
            (rect.x(), overlap.y(), overlap.x(), overlap.max().y()),
            (
                overlap.max().x(),
                overlap.y(),
                rect.max().x(),
                overlap.max().y(),
            ),
        ] {
            if let Some(fragment) = clipped_edges(left, top, right, bottom, rect) {
                fragments.push(fragment);
            }
        }
    }
    let remaining = fragments.len() - original_len;
    fragments.copy_within(original_len.., first);
    fragments.truncate(first + remaining);
}
