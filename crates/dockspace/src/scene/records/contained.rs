//! Contained-floating chrome, resize, and measurement records.

use crate::drop_target::SceneLayerKey;
use crate::geometry::{LogicalRect, LogicalSize};
use crate::hit_region::HitRegion;
use crate::ids::{FloatingPresentationId, RootId};

/// One device-independent contained-floating resize direction.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ContainedResizeDirection {
    /// Top edge.
    North,
    /// Top-right corner.
    NorthEast,
    /// Right edge.
    East,
    /// Bottom-right corner.
    SouthEast,
    /// Bottom edge.
    South,
    /// Bottom-left corner.
    SouthWest,
    /// Left edge.
    West,
    /// Top-left corner.
    NorthWest,
}

/// Exact hit geometry for one contained-floating resize direction.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ContainedResizeRecord {
    direction: ContainedResizeDirection,
    hit: HitRegion,
}

impl ContainedResizeRecord {
    pub(crate) const fn new(direction: ContainedResizeDirection, hit: HitRegion) -> Self {
        Self { direction, hit }
    }

    /// Returns the resize direction.
    #[must_use]
    pub const fn direction(self) -> ContainedResizeDirection {
        self.direction
    }

    /// Returns the exact half-open resize hit region.
    #[must_use]
    pub const fn hit(self) -> HitRegion {
        self.hit
    }
}

/// Core-compiled contained-floating frame and chrome geometry.
#[derive(Debug, Clone, PartialEq)]
pub struct ContainedRecord {
    floating: FloatingPresentationId,
    root: RootId,
    ordinal: usize,
    outer_bounds: LogicalRect,
    inner_bounds: LogicalRect,
    title_bounds: LogicalRect,
    title_drag_hit: HitRegion,
    content_bounds: LogicalRect,
    close_bounds: Option<LogicalRect>,
    transform_operable: bool,
    resize: [ContainedResizeRecord; 8],
    minimum_size: LogicalSize,
    layer: SceneLayerKey,
}

impl ContainedRecord {
    #[allow(clippy::too_many_arguments)]
    pub(crate) const fn new(
        floating: FloatingPresentationId,
        root: RootId,
        ordinal: usize,
        outer_bounds: LogicalRect,
        inner_bounds: LogicalRect,
        title_bounds: LogicalRect,
        title_drag_hit: HitRegion,
        content_bounds: LogicalRect,
        close_bounds: Option<LogicalRect>,
        transform_operable: bool,
        resize: [ContainedResizeRecord; 8],
        minimum_size: LogicalSize,
        layer: SceneLayerKey,
    ) -> Self {
        Self {
            floating,
            root,
            ordinal,
            outer_bounds,
            inner_bounds,
            title_bounds,
            title_drag_hit,
            content_bounds,
            close_bounds,
            transform_operable,
            resize,
            minimum_size,
            layer,
        }
    }

    /// Returns the stable contained presentation identity.
    #[must_use]
    pub const fn floating(&self) -> FloatingPresentationId {
        self.floating
    }

    /// Returns the presented docking root.
    #[must_use]
    pub const fn root(&self) -> RootId {
        self.root
    }

    /// Returns the back-to-front structural roster ordinal.
    #[must_use]
    pub const fn ordinal(&self) -> usize {
        self.ordinal
    }

    /// Returns the surface-clipped outer frame bounds.
    #[must_use]
    pub const fn outer_bounds(&self) -> LogicalRect {
        self.outer_bounds
    }

    /// Returns the frame interior after border allocation.
    #[must_use]
    pub const fn inner_bounds(&self) -> LogicalRect {
        self.inner_bounds
    }

    /// Returns the title-bar draw bounds.
    #[must_use]
    pub const fn title_bounds(&self) -> LogicalRect {
        self.title_bounds
    }

    /// Returns the title-bar drag region excluding resize and close controls.
    #[must_use]
    pub const fn title_drag_hit(&self) -> HitRegion {
        self.title_drag_hit
    }

    /// Returns the root content bounds.
    #[must_use]
    pub const fn content_bounds(&self) -> LogicalRect {
        self.content_bounds
    }

    /// Returns the close-control bounds when frozen policy permits every payload item to close.
    #[must_use]
    pub const fn close_bounds(&self) -> Option<LogicalRect> {
        self.close_bounds
    }

    /// Returns whether the complete root, every item, and the owning surface
    /// permit an in-place contained transform under the frozen policy.
    #[must_use]
    pub const fn transform_operable(&self) -> bool {
        self.transform_operable
    }

    /// Returns all eight non-overlapping resize regions in canonical order.
    #[must_use]
    pub const fn resize(&self) -> &[ContainedResizeRecord; 8] {
        &self.resize
    }

    /// Returns the measured minimum outer size.
    #[must_use]
    pub const fn minimum_size(&self) -> LogicalSize {
        self.minimum_size
    }

    /// Returns the roster-derived semantic layer.
    #[must_use]
    pub const fn layer(&self) -> SceneLayerKey {
        self.layer
    }
}

/// Adapter-measured minimum size for one exact contained presentation.
///
/// The record carries only the stable contained identity and the renderer fact.
/// Surface, root, durable rectangle, and roster order remain core-owned and are
/// derived from the workspace while sealing.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ContainedMinimumMeasurement {
    floating: FloatingPresentationId,
    minimum_size: LogicalSize,
}

impl ContainedMinimumMeasurement {
    /// Creates one typed contained minimum-size measurement.
    #[must_use]
    pub const fn new(floating: FloatingPresentationId, minimum_size: LogicalSize) -> Self {
        Self {
            floating,
            minimum_size,
        }
    }

    /// Returns the stable contained presentation identity.
    #[must_use]
    pub const fn floating(self) -> FloatingPresentationId {
        self.floating
    }

    /// Returns the renderer-measured minimum outer size.
    #[must_use]
    pub const fn minimum_size(self) -> LogicalSize {
        self.minimum_size
    }
}
