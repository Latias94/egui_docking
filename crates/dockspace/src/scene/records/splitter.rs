//! Splitter, structural gap, and junction presentation records.

use thiserror::Error;

use crate::drop_target::SceneLayerKey;
use crate::geometry::LogicalRect;
use crate::graph::{Axis, SplitWeight};
use crate::hit_region::HitRegion;
use crate::ids::{NodeId, RootId, SurfaceId};

/// Structural identity of one rendered splitter rectangle.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SplitterSceneId {
    /// Owning root.
    pub root: RootId,
    /// Split node owning the separator.
    pub split: NodeId,
    /// Gap before child `index + 1`.
    pub index: usize,
}

/// Core-compiled draw, hit, and resize semantics for one splitter.
#[derive(Debug, Clone, PartialEq)]
pub struct SplitterRecord {
    id: SplitterSceneId,
    draw_bounds: LogicalRect,
    hit: HitRegion,
    axis: Axis,
    before_bounds: LogicalRect,
    after_bounds: LogicalRect,
    before_minimum_extent: f64,
    after_minimum_extent: f64,
    before_maximum_extent: f64,
    after_maximum_extent: f64,
    child_extents: Vec<f64>,
    central_index: Option<usize>,
    weights: Vec<SplitWeight>,
    layer: SceneLayerKey,
    operable: bool,
}

/// Presentation availability for one structural splitter gap.
///
/// Every gap in a compiled root is inventoried exactly once. A collapsed gap
/// has no positive-area draw or hit rectangle and therefore cannot be painted
/// or interacted with, but remains explicit rather than silently disappearing
/// from the semantic plan.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum SplitterGapPresentation {
    /// The gap has a positive-area draw record.
    Rendered,
    /// Compression left no positive-area splitter rectangle.
    Collapsed,
}

/// Complete structural inventory entry for one splitter gap.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SplitterGapRecord {
    id: SplitterSceneId,
    presentation: SplitterGapPresentation,
}

impl SplitterGapRecord {
    pub(crate) const fn new(id: SplitterSceneId, presentation: SplitterGapPresentation) -> Self {
        Self { id, presentation }
    }

    /// Returns the stable structural splitter-gap identity.
    #[must_use]
    pub const fn id(&self) -> SplitterSceneId {
        self.id
    }

    /// Returns whether this gap produced a positive-area splitter record.
    #[must_use]
    pub const fn presentation(&self) -> SplitterGapPresentation {
        self.presentation
    }
}

/// Directional arm incident to one structural splitter junction.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum SplitterJunctionDirection {
    /// Splitter geometry extending above the junction center.
    North,
    /// Splitter geometry extending right of the junction center.
    East,
    /// Splitter geometry extending below the junction center.
    South,
    /// Splitter geometry extending left of the junction center.
    West,
}

/// Stable identity of one three- or four-arm splitter junction.
///
/// A splitter which passes through the junction occupies both opposite arms.
/// Directional identity preserves the complete touching frontier; it does not
/// collapse the junction to an arbitrary perpendicular pair.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SplitterJunctionId {
    north: Option<SplitterSceneId>,
    east: Option<SplitterSceneId>,
    south: Option<SplitterSceneId>,
    west: Option<SplitterSceneId>,
}

impl SplitterJunctionId {
    pub(crate) const fn new(
        north: Option<SplitterSceneId>,
        east: Option<SplitterSceneId>,
        south: Option<SplitterSceneId>,
        west: Option<SplitterSceneId>,
    ) -> Self {
        Self {
            north,
            east,
            south,
            west,
        }
    }

    /// Returns the splitter occupying one directional arm, when present.
    #[must_use]
    pub const fn arm(self, direction: SplitterJunctionDirection) -> Option<SplitterSceneId> {
        match direction {
            SplitterJunctionDirection::North => self.north,
            SplitterJunctionDirection::East => self.east,
            SplitterJunctionDirection::South => self.south,
            SplitterJunctionDirection::West => self.west,
        }
    }

    /// Returns the number of occupied directional arms.
    #[must_use]
    pub fn arm_count(self) -> usize {
        [self.north, self.east, self.south, self.west]
            .into_iter()
            .flatten()
            .count()
    }

    /// Returns every distinct incident splitter in canonical identity order.
    #[must_use]
    pub fn splitters(self) -> Vec<SplitterSceneId> {
        let mut splitters = [self.north, self.east, self.south, self.west]
            .into_iter()
            .flatten()
            .collect::<Vec<_>>();
        splitters.sort_unstable();
        splitters.dedup();
        splitters
    }
}

/// Exact shared hit region for one structural splitter junction.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SplitterJunctionRecord {
    id: SplitterJunctionId,
    hit: HitRegion,
    layer: SceneLayerKey,
}

/// Structural splitter target resolved from one authoritative presentation plan.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SplitterResizeTarget {
    /// One ordinary splitter handle owns the point.
    Handle(SplitterSceneId),
    /// One three- or four-arm splitter junction owns all incident handles.
    Junction(SplitterJunctionId),
}

/// A presentation plan could not resolve a unique splitter target at one point.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum SplitterResizeHitError {
    /// More than one junction target claimed the point on the authoritative layer.
    #[error(
        "{count} splitter junctions overlap at one point on layer {layer:?} of surface {surface}"
    )]
    AmbiguousJunctions {
        /// Surface containing the ambiguous hit.
        surface: SurfaceId,
        /// Authoritative presentation layer.
        layer: SceneLayerKey,
        /// Number of matching junction targets.
        count: usize,
    },
    /// More than one ordinary handle claimed the point on the authoritative layer.
    #[error(
        "{count} splitter handles overlap at one point on layer {layer:?} of surface {surface}"
    )]
    AmbiguousHandles {
        /// Surface containing the ambiguous hit.
        surface: SurfaceId,
        /// Authoritative presentation layer.
        layer: SceneLayerKey,
        /// Number of matching handle targets.
        count: usize,
    },
}

impl SplitterJunctionRecord {
    pub(crate) fn new(id: SplitterJunctionId, hit: HitRegion, layer: SceneLayerKey) -> Self {
        Self { id, hit, layer }
    }

    /// Returns the complete directional junction identity.
    #[must_use]
    pub const fn id(&self) -> SplitterJunctionId {
        self.id
    }

    /// Returns the exact region which owns atomic multi-handle resize input.
    #[must_use]
    pub const fn hit(&self) -> HitRegion {
        self.hit
    }

    /// Returns the roster-derived semantic layer.
    #[must_use]
    pub const fn layer(&self) -> SceneLayerKey {
        self.layer
    }
}

impl SplitterRecord {
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn new(
        id: SplitterSceneId,
        draw_bounds: LogicalRect,
        hit: HitRegion,
        axis: Axis,
        before_bounds: LogicalRect,
        after_bounds: LogicalRect,
        before_minimum_extent: f64,
        after_minimum_extent: f64,
        before_maximum_extent: f64,
        after_maximum_extent: f64,
        child_extents: Vec<f64>,
        central_index: Option<usize>,
        weights: Vec<SplitWeight>,
        layer: SceneLayerKey,
        operable: bool,
    ) -> Self {
        Self {
            id,
            draw_bounds,
            hit,
            axis,
            before_bounds,
            after_bounds,
            before_minimum_extent,
            after_minimum_extent,
            before_maximum_extent,
            after_maximum_extent,
            child_extents,
            central_index,
            weights,
            layer,
            operable,
        }
    }

    /// Returns the stable split-gap identity.
    #[must_use]
    pub const fn id(&self) -> &SplitterSceneId {
        &self.id
    }

    /// Returns the thin painted splitter rectangle.
    #[must_use]
    pub const fn draw_bounds(&self) -> LogicalRect {
        self.draw_bounds
    }

    /// Returns the exact expanded resize hit region.
    #[must_use]
    pub const fn hit(&self) -> HitRegion {
        self.hit
    }

    /// Returns the split axis controlled by this record.
    #[must_use]
    pub const fn axis(&self) -> Axis {
        self.axis
    }

    /// Returns the adjacent child bounds before the splitter.
    #[must_use]
    pub const fn before_bounds(&self) -> LogicalRect {
        self.before_bounds
    }

    /// Returns the adjacent child bounds after the splitter.
    #[must_use]
    pub const fn after_bounds(&self) -> LogicalRect {
        self.after_bounds
    }

    /// Returns the measured minimum extent of the child before this splitter.
    #[must_use]
    pub const fn before_minimum_extent(&self) -> f64 {
        self.before_minimum_extent
    }

    /// Returns the measured minimum extent of the child after this splitter.
    #[must_use]
    pub const fn after_minimum_extent(&self) -> f64 {
        self.after_minimum_extent
    }

    /// Returns the measured maximum extent of the child before this splitter.
    #[must_use]
    pub const fn before_maximum_extent(&self) -> f64 {
        self.before_maximum_extent
    }

    /// Returns the measured maximum extent of the child after this splitter.
    #[must_use]
    pub const fn after_maximum_extent(&self) -> f64 {
        self.after_maximum_extent
    }

    /// Returns actual projected extents for every direct split child.
    #[must_use]
    pub fn child_extents(&self) -> &[f64] {
        &self.child_extents
    }

    /// Returns the child containing the root's central leaf, when one exists.
    #[must_use]
    pub const fn central_index(&self) -> Option<usize> {
        self.central_index
    }

    /// Returns the effective split weights represented by this plan.
    #[must_use]
    pub fn weights(&self) -> &[SplitWeight] {
        &self.weights
    }

    /// Returns the roster-derived semantic layer.
    #[must_use]
    pub const fn layer(&self) -> SceneLayerKey {
        self.layer
    }

    /// Returns whether any authoritative hit area remains on this record's layer.
    ///
    /// A fully obscured splitter remains in the draw plan so adapters can paint
    /// the complete topology, but it must not participate in pointer, keyboard,
    /// or accessibility interaction.
    #[must_use]
    pub const fn operable(&self) -> bool {
        self.operable
    }

    pub(in crate::scene) fn retain_operability(&mut self, condition: bool) {
        self.operable &= condition;
    }
}
