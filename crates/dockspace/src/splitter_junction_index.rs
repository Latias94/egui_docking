//! Exact structural splitter-junction reporting without pairwise scans.

use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet};

use crate::drop_target::SceneLayerKey;
use crate::geometry::{GeometryError, LogicalRect};
use crate::graph::Axis;
use crate::ids::RootId;
use crate::scene::{SplitterJunctionId, SplitterRecord, SplitterSceneId};

/// One exact three- or four-arm junction derived from authoritative splitter geometry.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct SplitterJunctionCandidate {
    pub(crate) id: SplitterJunctionId,
    pub(crate) hit: LogicalRect,
    pub(crate) layer: SceneLayerKey,
}

impl SplitterJunctionCandidate {
    pub(crate) const fn id(self) -> SplitterJunctionId {
        self.id
    }
}

#[derive(Default)]
struct SplitterGroup {
    north_south: Vec<IndexedSplitter>,
    west_east: Vec<IndexedSplitter>,
}

/// Reports every unambiguous three- or four-arm structural splitter junction.
///
/// Splitters are partitioned by root and presentation layer, then processed by
/// an x-axis sweep over their painted bounds. Active rectangles are indexed by
/// a deterministic AVL interval tree on y. Both sweep axes use closed-interval
/// contact so splitters that meet exactly at a frontier remain discoverable.
/// Expanded hit geometry is consulted only after structural contact is proven.
/// Contacts are aggregated by their exact centerline intersection, and any
/// directional conflict fails closed rather than selecting a winner.
pub(crate) fn derive_splitter_junction_candidates<'a>(
    splitters: impl IntoIterator<Item = &'a SplitterRecord>,
) -> Result<Vec<SplitterJunctionCandidate>, GeometryError> {
    let splitters = splitters
        .into_iter()
        .filter(|splitter| splitter.operable())
        .map(IndexedSplitter::from_record);
    derive_indexed_splitter_junction_candidates(splitters).map(|(candidates, _)| candidates)
}

fn derive_indexed_splitter_junction_candidates(
    splitters: impl IntoIterator<Item = IndexedSplitter>,
) -> Result<(Vec<SplitterJunctionCandidate>, SweepMetrics), GeometryError> {
    let mut groups = BTreeMap::<(RootId, SceneLayerKey), SplitterGroup>::new();
    for splitter in splitters {
        let group = groups
            .entry((splitter.id.root, splitter.layer))
            .or_default();
        match splitter.axis {
            Axis::Horizontal => group.north_south.push(splitter),
            Axis::Vertical => group.west_east.push(splitter),
        }
    }

    let mut metrics = SweepMetrics::default();
    let mut candidates = Vec::new();
    for group in groups.values() {
        sweep_group(group, &mut candidates, &mut metrics)?;
    }
    candidates.sort_unstable_by_key(|candidate| candidate.id);
    candidates = discard_duplicate_identities(candidates);
    Ok((candidates, metrics))
}

#[derive(Debug, Clone, Copy)]
struct IndexedSplitter {
    id: SplitterSceneId,
    draw: LogicalRect,
    hit: LogicalRect,
    axis: Axis,
    layer: SceneLayerKey,
}

impl IndexedSplitter {
    fn from_record(record: &SplitterRecord) -> Self {
        Self {
            id: *record.id(),
            draw: record.draw_bounds(),
            hit: record.hit().rect(),
            axis: record.axis(),
            layer: record.layer(),
        }
    }
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
struct SweepMetrics {
    interval_nodes_visited: usize,
    broad_phase_pairs: usize,
    structural_contacts: usize,
}

#[derive(Debug, Clone, Copy)]
struct JunctionCenter {
    x: f64,
    y: f64,
}

impl JunctionCenter {
    fn from_perpendicular(north_south: IndexedSplitter, west_east: IndexedSplitter) -> Self {
        Self {
            x: midpoint(north_south.draw.x(), north_south.draw.max().x()),
            y: midpoint(west_east.draw.y(), west_east.draw.max().y()),
        }
    }

    fn compare(self, other: Self) -> Ordering {
        self.x
            .total_cmp(&other.x)
            .then_with(|| self.y.total_cmp(&other.y))
    }
}

impl PartialEq for JunctionCenter {
    fn eq(&self, other: &Self) -> bool {
        self.compare(*other) == Ordering::Equal
    }
}

impl Eq for JunctionCenter {}

impl PartialOrd for JunctionCenter {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for JunctionCenter {
    fn cmp(&self, other: &Self) -> Ordering {
        self.compare(*other)
    }
}

#[derive(Debug, Default)]
struct JunctionAccumulator {
    north: ArmClaim,
    east: ArmClaim,
    south: ArmClaim,
    west: ArmClaim,
    splitters: BTreeSet<SplitterSceneId>,
    x_hit_band: HitBand,
    y_hit_band: HitBand,
}

#[derive(Debug, Default)]
struct HitBand {
    interval: Option<(f64, f64)>,
    empty: bool,
}

impl HitBand {
    fn include(&mut self, minimum: f64, maximum: f64) {
        if self.empty {
            return;
        }
        let (minimum, maximum) = self.interval.map_or((minimum, maximum), |current| {
            (current.0.max(minimum), current.1.min(maximum))
        });
        if maximum <= minimum {
            self.interval = None;
            self.empty = true;
        } else {
            self.interval = Some((minimum, maximum));
        }
    }

    fn interval(self) -> Option<(f64, f64)> {
        (!self.empty).then_some(self.interval).flatten()
    }
}

#[derive(Debug, Default)]
struct ArmClaim {
    splitter: Option<SplitterSceneId>,
    conflicting: bool,
}

impl ArmClaim {
    fn claim(&mut self, splitter: SplitterSceneId) {
        match self.splitter {
            None => self.splitter = Some(splitter),
            Some(existing) if existing == splitter => {}
            Some(_) => self.conflicting = true,
        }
    }
}

impl JunctionAccumulator {
    fn add_contact(
        &mut self,
        center: JunctionCenter,
        north_south: IndexedSplitter,
        west_east: IndexedSplitter,
    ) -> Result<(), GeometryError> {
        self.claim_north_south(center, north_south);
        self.claim_west_east(center, west_east);
        self.include_splitter(north_south)?;
        self.include_splitter(west_east)?;
        Ok(())
    }

    fn claim_north_south(&mut self, center: JunctionCenter, splitter: IndexedSplitter) {
        if splitter.draw.y() < center.y {
            self.north.claim(splitter.id);
        }
        if splitter.draw.max().y() > center.y {
            self.south.claim(splitter.id);
        }
    }

    fn claim_west_east(&mut self, center: JunctionCenter, splitter: IndexedSplitter) {
        if splitter.draw.x() < center.x {
            self.west.claim(splitter.id);
        }
        if splitter.draw.max().x() > center.x {
            self.east.claim(splitter.id);
        }
    }

    fn include_splitter(&mut self, splitter: IndexedSplitter) -> Result<(), GeometryError> {
        if !self.splitters.insert(splitter.id) {
            return Ok(());
        }
        match splitter.axis {
            Axis::Horizontal => self
                .x_hit_band
                .include(splitter.hit.x(), splitter.hit.max().x()),
            Axis::Vertical => self
                .y_hit_band
                .include(splitter.hit.y(), splitter.hit.max().y()),
        }
        Ok(())
    }

    fn finish(
        self,
        layer: SceneLayerKey,
    ) -> Result<Option<SplitterJunctionCandidate>, GeometryError> {
        let ambiguous = self.north.conflicting
            || self.east.conflicting
            || self.south.conflicting
            || self.west.conflicting;
        if ambiguous {
            return Ok(None);
        }
        let id = SplitterJunctionId::new(
            self.north.splitter,
            self.east.splitter,
            self.south.splitter,
            self.west.splitter,
        );
        if id.arm_count() < 3 {
            return Ok(None);
        }
        let Some((minimum_x, maximum_x)) = self.x_hit_band.interval() else {
            return Ok(None);
        };
        let Some((minimum_y, maximum_y)) = self.y_hit_band.interval() else {
            return Ok(None);
        };
        let hit = LogicalRect::new(
            minimum_x,
            minimum_y,
            maximum_x - minimum_x,
            maximum_y - minimum_y,
        )?;
        Ok(Some(SplitterJunctionCandidate { id, hit, layer }))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum SweepSet {
    NorthSouth,
    WestEast,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum SweepPhase {
    StartNorthSouth,
    StartWestEast,
    EndNorthSouth,
    EndWestEast,
}

#[derive(Debug, Clone, Copy)]
struct SweepEvent {
    x: f64,
    phase: SweepPhase,
    set: SweepSet,
    index: usize,
    id: SplitterSceneId,
}

impl SweepEvent {
    fn compare(left: &Self, right: &Self) -> Ordering {
        left.x
            .total_cmp(&right.x)
            .then_with(|| left.phase.cmp(&right.phase))
            .then_with(|| left.id.cmp(&right.id))
    }
}

fn sweep_group(
    group: &SplitterGroup,
    candidates: &mut Vec<SplitterJunctionCandidate>,
    metrics: &mut SweepMetrics,
) -> Result<(), GeometryError> {
    if group.north_south.is_empty() || group.west_east.is_empty() {
        return Ok(());
    }

    let mut events = Vec::with_capacity(
        group
            .north_south
            .len()
            .saturating_add(group.west_east.len())
            .saturating_mul(2),
    );
    push_events(&mut events, SweepSet::NorthSouth, &group.north_south);
    push_events(&mut events, SweepSet::WestEast, &group.west_east);
    events.sort_unstable_by(SweepEvent::compare);

    let mut active_north_south = IntervalTree::default();
    let mut active_west_east = IntervalTree::default();
    let mut junctions = BTreeMap::<JunctionCenter, JunctionAccumulator>::new();
    let mut matches = Vec::new();
    for event in events {
        let records = match event.set {
            SweepSet::NorthSouth => &group.north_south,
            SweepSet::WestEast => &group.west_east,
        };
        let Some(record) = records.get(event.index).copied() else {
            continue;
        };
        let interval = IntervalEntry::from_record(record, event.index);
        match event.phase {
            SweepPhase::EndNorthSouth => active_north_south.remove(interval.key()),
            SweepPhase::EndWestEast => active_west_east.remove(interval.key()),
            SweepPhase::StartNorthSouth => {
                matches.clear();
                metrics.interval_nodes_visited =
                    metrics
                        .interval_nodes_visited
                        .saturating_add(active_west_east.query(
                            interval.start,
                            interval.end,
                            &mut matches,
                        ));
                for west_east in matches.iter().copied() {
                    if let Some(west_east) = group.west_east.get(west_east).copied() {
                        append_contact(record, west_east, &mut junctions, metrics)?;
                    }
                }
                active_north_south.insert(interval);
            }
            SweepPhase::StartWestEast => {
                matches.clear();
                metrics.interval_nodes_visited =
                    metrics
                        .interval_nodes_visited
                        .saturating_add(active_north_south.query(
                            interval.start,
                            interval.end,
                            &mut matches,
                        ));
                for north_south in matches.iter().copied() {
                    if let Some(north_south) = group.north_south.get(north_south).copied() {
                        append_contact(north_south, record, &mut junctions, metrics)?;
                    }
                }
                active_west_east.insert(interval);
            }
        }
    }
    for junction in junctions.into_values() {
        if let Some(candidate) = junction.finish(group.north_south[0].layer)? {
            candidates.push(candidate);
        }
    }
    Ok(())
}

fn push_events(events: &mut Vec<SweepEvent>, set: SweepSet, records: &[IndexedSplitter]) {
    let (end_phase, start_phase) = match set {
        SweepSet::NorthSouth => (SweepPhase::EndNorthSouth, SweepPhase::StartNorthSouth),
        SweepSet::WestEast => (SweepPhase::EndWestEast, SweepPhase::StartWestEast),
    };
    for (index, record) in records.iter().enumerate() {
        let draw = record.draw;
        events.push(SweepEvent {
            x: draw.max().x(),
            phase: end_phase,
            set,
            index,
            id: record.id,
        });
        events.push(SweepEvent {
            x: draw.x(),
            phase: start_phase,
            set,
            index,
            id: record.id,
        });
    }
}

fn append_contact(
    north_south: IndexedSplitter,
    west_east: IndexedSplitter,
    junctions: &mut BTreeMap<JunctionCenter, JunctionAccumulator>,
    metrics: &mut SweepMetrics,
) -> Result<(), GeometryError> {
    metrics.broad_phase_pairs = metrics.broad_phase_pairs.saturating_add(1);
    if !closed_rectangles_touch(north_south.draw, west_east.draw) {
        return Ok(());
    }
    metrics.structural_contacts = metrics.structural_contacts.saturating_add(1);
    let center = JunctionCenter::from_perpendicular(north_south, west_east);
    junctions
        .entry(center)
        .or_default()
        .add_contact(center, north_south, west_east)?;
    Ok(())
}

fn discard_duplicate_identities(
    candidates: Vec<SplitterJunctionCandidate>,
) -> Vec<SplitterJunctionCandidate> {
    let mut unique = BTreeMap::<SplitterJunctionId, Option<SplitterJunctionCandidate>>::new();
    for candidate in candidates {
        unique
            .entry(candidate.id)
            .and_modify(|existing| *existing = None)
            .or_insert(Some(candidate));
    }
    unique.into_values().flatten().collect()
}

fn midpoint(minimum: f64, maximum: f64) -> f64 {
    minimum.mul_add(0.5, maximum * 0.5)
}

fn closed_rectangles_touch(first: LogicalRect, second: LogicalRect) -> bool {
    first.x() <= second.max().x()
        && second.x() <= first.max().x()
        && first.y() <= second.max().y()
        && second.y() <= first.max().y()
}

#[derive(Debug, Clone, Copy)]
struct IntervalKey {
    start: f64,
    id: SplitterSceneId,
}

impl IntervalKey {
    fn compare(self, other: Self) -> Ordering {
        self.start
            .total_cmp(&other.start)
            .then_with(|| self.id.cmp(&other.id))
    }
}

#[derive(Debug, Clone, Copy)]
struct IntervalEntry {
    start: f64,
    end: f64,
    id: SplitterSceneId,
    index: usize,
}

impl IntervalEntry {
    fn from_record(record: IndexedSplitter, index: usize) -> Self {
        let draw = record.draw;
        Self {
            start: draw.y(),
            end: draw.max().y(),
            id: record.id,
            index,
        }
    }

    const fn key(self) -> IntervalKey {
        IntervalKey {
            start: self.start,
            id: self.id,
        }
    }
}

#[derive(Debug, Default)]
struct IntervalTree {
    root: Option<Box<IntervalNode>>,
}

impl IntervalTree {
    fn insert(&mut self, entry: IntervalEntry) {
        self.root = Some(IntervalNode::insert(self.root.take(), entry));
    }

    fn remove(&mut self, key: IntervalKey) {
        self.root = IntervalNode::remove(self.root.take(), key);
    }

    fn query(&self, start: f64, end: f64, output: &mut Vec<usize>) -> usize {
        self.root.as_ref().map_or(0, |root| {
            let mut visited = 0;
            root.query(start, end, output, &mut visited);
            visited
        })
    }
}

#[derive(Debug)]
struct IntervalNode {
    entry: IntervalEntry,
    maximum_end: f64,
    height: i16,
    left: Option<Box<Self>>,
    right: Option<Box<Self>>,
}

impl IntervalNode {
    fn new(entry: IntervalEntry) -> Box<Self> {
        Box::new(Self {
            entry,
            maximum_end: entry.end,
            height: 1,
            left: None,
            right: None,
        })
    }

    fn height(node: &Option<Box<Self>>) -> i16 {
        node.as_ref().map_or(0, |node| node.height)
    }

    fn maximum_end(node: &Option<Box<Self>>) -> f64 {
        node.as_ref()
            .map_or(f64::NEG_INFINITY, |node| node.maximum_end)
    }

    fn refresh(&mut self) {
        self.height = 1 + Self::height(&self.left).max(Self::height(&self.right));
        self.maximum_end = self
            .entry
            .end
            .max(Self::maximum_end(&self.left))
            .max(Self::maximum_end(&self.right));
    }

    fn balance(&self) -> i16 {
        Self::height(&self.left) - Self::height(&self.right)
    }

    fn insert(node: Option<Box<Self>>, entry: IntervalEntry) -> Box<Self> {
        let Some(mut node) = node else {
            return Self::new(entry);
        };
        match entry.key().compare(node.entry.key()) {
            Ordering::Less => node.left = Some(Self::insert(node.left.take(), entry)),
            Ordering::Greater => node.right = Some(Self::insert(node.right.take(), entry)),
            Ordering::Equal => node.entry = entry,
        }
        Self::rebalance(node)
    }

    fn remove(node: Option<Box<Self>>, key: IntervalKey) -> Option<Box<Self>> {
        let mut node = node?;
        match key.compare(node.entry.key()) {
            Ordering::Less => node.left = Self::remove(node.left.take(), key),
            Ordering::Greater => node.right = Self::remove(node.right.take(), key),
            Ordering::Equal => match (node.left.take(), node.right.take()) {
                (None, right) => return right,
                (left, None) => return left,
                (left, Some(right)) => {
                    let (remaining, successor) = Self::take_min(right);
                    node.entry = successor;
                    node.left = left;
                    node.right = remaining;
                }
            },
        }
        Some(Self::rebalance(node))
    }

    fn take_min(mut node: Box<Self>) -> (Option<Box<Self>>, IntervalEntry) {
        let Some(left) = node.left.take() else {
            return (node.right.take(), node.entry);
        };
        let (remaining, minimum) = Self::take_min(left);
        node.left = remaining;
        (Some(Self::rebalance(node)), minimum)
    }

    fn rebalance(mut node: Box<Self>) -> Box<Self> {
        node.refresh();
        if node.balance() > 1 {
            if node.left.as_ref().is_some_and(|left| left.balance() < 0)
                && let Some(left) = node.left.take()
            {
                node.left = Some(Self::rotate_left(left));
            }
            return Self::rotate_right(node);
        }
        if node.balance() < -1 {
            if node.right.as_ref().is_some_and(|right| right.balance() > 0)
                && let Some(right) = node.right.take()
            {
                node.right = Some(Self::rotate_right(right));
            }
            return Self::rotate_left(node);
        }
        node
    }

    fn rotate_left(mut root: Box<Self>) -> Box<Self> {
        let Some(mut pivot) = root.right.take() else {
            return root;
        };
        root.right = pivot.left.take();
        root.refresh();
        pivot.left = Some(root);
        pivot.refresh();
        pivot
    }

    fn rotate_right(mut root: Box<Self>) -> Box<Self> {
        let Some(mut pivot) = root.left.take() else {
            return root;
        };
        root.left = pivot.right.take();
        root.refresh();
        pivot.right = Some(root);
        pivot.refresh();
        pivot
    }

    fn query(&self, start: f64, end: f64, output: &mut Vec<usize>, visited: &mut usize) {
        *visited = visited.saturating_add(1);
        if self
            .left
            .as_ref()
            .is_some_and(|left| left.maximum_end >= start)
            && let Some(left) = &self.left
        {
            left.query(start, end, output, visited);
        }
        if self.entry.start <= end && self.entry.end >= start {
            output.push(self.entry.index);
        }
        if self.entry.start <= end
            && let Some(right) = &self.right
        {
            right.query(start, end, output, visited);
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::{
        IndexedSplitter, IntervalEntry, IntervalTree, JunctionAccumulator, JunctionCenter,
        SplitterGroup, SplitterJunctionCandidate, SplitterSceneId, SweepMetrics, append_contact,
        derive_indexed_splitter_junction_candidates, discard_duplicate_identities,
    };
    use crate::drop_target::SceneLayerKey;
    use crate::geometry::LogicalRect;
    use crate::graph::Axis;
    use crate::ids::{NodeId, RootId};
    use crate::scene::SplitterJunctionDirection;

    const LAYER: SceneLayerKey = SceneLayerKey::new(1);

    fn rect(x: f64, y: f64, width: f64, height: f64) -> LogicalRect {
        LogicalRect::new(x, y, width, height).expect("test rectangle is valid")
    }

    fn splitter(index: usize, axis: Axis, draw: LogicalRect, hit: LogicalRect) -> IndexedSplitter {
        IndexedSplitter {
            id: splitter_id(index),
            draw,
            hit,
            axis,
            layer: LAYER,
        }
    }

    fn splitter_id(index: usize) -> SplitterSceneId {
        SplitterSceneId {
            root: RootId::new(1),
            split: NodeId::default(),
            index,
        }
    }

    fn derive(
        splitters: impl IntoIterator<Item = IndexedSplitter>,
    ) -> (Vec<SplitterJunctionCandidate>, SweepMetrics) {
        derive_indexed_splitter_junction_candidates(splitters).expect("test geometry is valid")
    }

    fn assert_arm(
        candidate: SplitterJunctionCandidate,
        direction: SplitterJunctionDirection,
        expected: Option<usize>,
    ) {
        assert_eq!(candidate.id.arm(direction), expected.map(splitter_id));
    }

    fn entry(index: usize, start: f64, end: f64) -> IntervalEntry {
        IntervalEntry {
            start,
            end,
            id: splitter_id(index),
            index,
        }
    }

    #[test]
    fn t_junction_preserves_three_directional_arms() {
        let north_south = splitter(
            0,
            Axis::Horizontal,
            rect(9.0, 0.0, 2.0, 20.0),
            rect(7.0, 0.0, 6.0, 20.0),
        );
        let west = splitter(
            1,
            Axis::Vertical,
            rect(0.0, 9.0, 9.0, 2.0),
            rect(0.0, 7.0, 13.0, 6.0),
        );

        let (candidates, _) = derive([north_south, west]);

        assert_eq!(candidates.len(), 1);
        let candidate = candidates[0];
        assert_eq!(candidate.hit, rect(7.0, 7.0, 6.0, 6.0));
        assert_arm(candidate, SplitterJunctionDirection::North, Some(0));
        assert_arm(candidate, SplitterJunctionDirection::East, None);
        assert_arm(candidate, SplitterJunctionDirection::South, Some(0));
        assert_arm(candidate, SplitterJunctionDirection::West, Some(1));
    }

    #[test]
    fn cross_junction_aggregates_four_distinct_touching_handles() {
        let north = splitter(
            0,
            Axis::Horizontal,
            rect(9.0, 0.0, 2.0, 9.0),
            rect(7.0, 0.0, 6.0, 13.0),
        );
        let south = splitter(
            1,
            Axis::Horizontal,
            rect(9.0, 11.0, 2.0, 9.0),
            rect(7.0, 7.0, 6.0, 13.0),
        );
        let west = splitter(
            2,
            Axis::Vertical,
            rect(0.0, 9.0, 9.0, 2.0),
            rect(0.0, 7.0, 13.0, 6.0),
        );
        let east = splitter(
            3,
            Axis::Vertical,
            rect(11.0, 9.0, 9.0, 2.0),
            rect(7.0, 7.0, 13.0, 6.0),
        );

        let (candidates, _) = derive([north, south, west, east]);

        assert_eq!(candidates.len(), 1);
        let candidate = candidates[0];
        assert_eq!(candidate.hit, rect(7.0, 7.0, 6.0, 6.0));
        assert_arm(candidate, SplitterJunctionDirection::North, Some(0));
        assert_arm(candidate, SplitterJunctionDirection::East, Some(3));
        assert_arm(candidate, SplitterJunctionDirection::South, Some(1));
        assert_arm(candidate, SplitterJunctionDirection::West, Some(2));
    }

    #[test]
    fn ambiguous_direction_fails_closed_without_selecting_a_winner() {
        let north_south = splitter(
            0,
            Axis::Horizontal,
            rect(9.0, 0.0, 2.0, 20.0),
            rect(7.0, 0.0, 6.0, 20.0),
        );
        let west_a = splitter(
            1,
            Axis::Vertical,
            rect(0.0, 9.0, 9.0, 2.0),
            rect(0.0, 7.0, 13.0, 6.0),
        );
        let west_b = splitter(
            2,
            Axis::Vertical,
            rect(1.0, 9.0, 8.0, 2.0),
            rect(1.0, 7.0, 12.0, 6.0),
        );

        let (candidates, _) = derive([north_south, west_a, west_b]);

        assert!(candidates.is_empty());
    }

    #[test]
    fn two_arm_contact_is_not_promoted_to_a_junction() {
        let north = splitter(
            0,
            Axis::Horizontal,
            rect(9.0, 0.0, 2.0, 9.0),
            rect(7.0, 0.0, 6.0, 13.0),
        );
        let west = splitter(
            1,
            Axis::Vertical,
            rect(0.0, 9.0, 9.0, 2.0),
            rect(0.0, 7.0, 13.0, 6.0),
        );

        let (candidates, _) = derive([north, west]);

        assert!(candidates.is_empty());
    }

    #[test]
    fn expanded_hit_overlap_cannot_invent_structural_touching() {
        let north_south = splitter(
            0,
            Axis::Horizontal,
            rect(9.0, 0.0, 2.0, 8.0),
            rect(7.0, 0.0, 6.0, 13.0),
        );
        let west_east = splitter(
            1,
            Axis::Vertical,
            rect(0.0, 9.0, 8.0, 2.0),
            rect(0.0, 7.0, 13.0, 6.0),
        );

        let (candidates, metrics) = derive([north_south, west_east]);

        assert!(candidates.is_empty());
        assert_eq!(metrics.broad_phase_pairs, 0);
        assert_eq!(metrics.structural_contacts, 0);
    }

    #[test]
    fn terminal_arms_form_a_junction_hit_from_perpendicular_bands() {
        let north_south = splitter(
            0,
            Axis::Horizontal,
            rect(4.0, 0.0, 2.0, 20.0),
            rect(3.0, 0.0, 4.0, 20.0),
        );
        let west = splitter(
            1,
            Axis::Vertical,
            rect(0.0, 9.0, 4.0, 2.0),
            rect(0.0, 7.0, 5.0, 6.0),
        );
        let east = splitter(
            2,
            Axis::Vertical,
            rect(6.0, 9.0, 4.0, 2.0),
            rect(5.0, 7.0, 5.0, 6.0),
        );

        let (candidates, metrics) = derive([north_south, west, east]);

        assert_eq!(candidates.len(), 1);
        assert_eq!(candidates[0].hit, rect(3.0, 7.0, 4.0, 6.0));
        assert_eq!(metrics.structural_contacts, 2);
    }

    #[test]
    fn deterministic_generated_scenes_match_brute_force_oracle() {
        for seed in 1..=64 {
            let splitters = generated_splitters(seed, 48);
            let (indexed, _) = derive(splitters.iter().copied());
            let brute_force = brute_force_candidates(&splitters);
            assert_eq!(indexed, brute_force, "sweep diverged for seed {seed}");
        }
    }

    #[test]
    fn one_thousand_twenty_four_splitters_remain_output_sensitive() {
        const PER_AXIS: usize = 512;
        let mut splitters = Vec::with_capacity(PER_AXIS * 2);
        for index in 0..PER_AXIS {
            let y = index as f64 * 4.0;
            splitters.push(splitter(
                index,
                Axis::Vertical,
                rect(0.0, y, 5_000.0, 1.0),
                rect(0.0, y, 5_000.0, 1.0),
            ));
            let x = index as f64 * 8.0 + 10.0;
            splitters.push(splitter(
                PER_AXIS + index,
                Axis::Horizontal,
                rect(x, y, 1.0, 1.0),
                rect(x, y, 1.0, 1.0),
            ));
        }

        let (candidates, metrics) = derive(splitters);

        assert_eq!(candidates.len(), PER_AXIS);
        assert_eq!(metrics.broad_phase_pairs, PER_AXIS);
        assert_eq!(metrics.structural_contacts, PER_AXIS);
        assert!(
            metrics.interval_nodes_visited < PER_AXIS * 32,
            "sweep visited {} interval nodes for {PER_AXIS} output-sensitive junctions",
            metrics.interval_nodes_visited
        );
    }

    #[test]
    fn huge_hit_extents_cannot_turn_sparse_draw_contacts_into_a_quadratic_scan() {
        const PER_AXIS: usize = 512;
        const SPACING: f64 = 40.0;
        let shared_hit = rect(-100_000.0, -100_000.0, 200_000.0, 200_000.0);
        let mut splitters = Vec::with_capacity(PER_AXIS * 2);
        for index in 0..PER_AXIS {
            let origin = index as f64 * SPACING;
            splitters.push(splitter(
                index,
                Axis::Horizontal,
                rect(origin + 9.0, origin, 2.0, 20.0),
                shared_hit,
            ));
            splitters.push(splitter(
                PER_AXIS + index,
                Axis::Vertical,
                rect(origin, origin + 9.0, 20.0, 2.0),
                shared_hit,
            ));
        }

        let (candidates, metrics) = derive(splitters);

        assert_eq!(candidates.len(), PER_AXIS);
        assert_eq!(metrics.broad_phase_pairs, PER_AXIS);
        assert_eq!(metrics.structural_contacts, PER_AXIS);
        assert!(
            metrics.interval_nodes_visited < PER_AXIS * 32,
            "draw-bound sweep visited {} interval nodes for {PER_AXIS} sparse contacts",
            metrics.interval_nodes_visited
        );
    }

    #[test]
    fn interval_queries_scale_with_tree_height_and_report_exact_overlaps() {
        const COUNT: usize = 1_024;
        let mut tree = IntervalTree::default();
        for index in 0..COUNT {
            let start = index as f64 * 2.0;
            tree.insert(entry(index, start, start + 1.0));
        }

        let mut total_visits = 0;
        for index in 0..COUNT {
            let start = index as f64 * 2.0;
            let mut matches = Vec::new();
            total_visits += tree.query(start + 0.25, start + 0.75, &mut matches);
            assert_eq!(matches, [index]);
        }

        assert!(
            total_visits < COUNT * 32,
            "balanced interval queries visited {total_visits} nodes for {COUNT} exact hits"
        );
    }

    #[test]
    fn interval_removal_preserves_balance_and_closed_boundaries() {
        let mut tree = IntervalTree::default();
        let entries = [entry(0, 0.0, 4.0), entry(1, 4.0, 8.0), entry(2, 2.0, 6.0)];
        for entry in entries {
            tree.insert(entry);
        }
        tree.remove(entries[2].key());

        let mut left = Vec::new();
        tree.query(3.0, 4.0, &mut left);
        assert_eq!(left, [0, 1]);
        let mut right = Vec::new();
        tree.query(4.0, 5.0, &mut right);
        assert_eq!(right, [0, 1]);
    }

    fn brute_force_candidates(splitters: &[IndexedSplitter]) -> Vec<SplitterJunctionCandidate> {
        let mut groups = BTreeMap::<(RootId, SceneLayerKey), SplitterGroup>::new();
        for splitter in splitters.iter().copied() {
            let group = groups
                .entry((splitter.id.root, splitter.layer))
                .or_default();
            match splitter.axis {
                Axis::Horizontal => group.north_south.push(splitter),
                Axis::Vertical => group.west_east.push(splitter),
            }
        }

        let mut candidates = Vec::new();
        for group in groups.values() {
            let mut junctions = BTreeMap::<JunctionCenter, JunctionAccumulator>::new();
            let mut metrics = SweepMetrics::default();
            for north_south in group.north_south.iter().copied() {
                for west_east in group.west_east.iter().copied() {
                    if super::closed_rectangles_touch(north_south.draw, west_east.draw) {
                        append_contact(north_south, west_east, &mut junctions, &mut metrics)
                            .expect("generated geometry is valid");
                    }
                }
            }
            for junction in junctions.into_values() {
                if let Some(candidate) = junction
                    .finish(group.north_south[0].layer)
                    .expect("generated geometry is valid")
                {
                    candidates.push(candidate);
                }
            }
        }
        candidates.sort_unstable_by_key(|candidate| candidate.id);
        discard_duplicate_identities(candidates)
    }

    fn generated_splitters(seed: u64, count: usize) -> Vec<IndexedSplitter> {
        let mut state = seed;
        (0..count)
            .map(|index| {
                let axis = if next(&mut state).is_multiple_of(2) {
                    Axis::Horizontal
                } else {
                    Axis::Vertical
                };
                let x = (next(&mut state) % 32) as f64;
                let y = (next(&mut state) % 32) as f64;
                let width = (next(&mut state) % 12 + 1) as f64;
                let height = (next(&mut state) % 12 + 1) as f64;
                let draw = rect(x, y, width, height);
                let expansion = (next(&mut state) % 4) as f64;
                let hit = rect(
                    x - expansion,
                    y - expansion,
                    width + expansion * 2.0,
                    height + expansion * 2.0,
                );
                splitter(index, axis, draw, hit)
            })
            .collect()
    }

    fn next(state: &mut u64) -> u64 {
        *state = state
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        *state
    }
}
