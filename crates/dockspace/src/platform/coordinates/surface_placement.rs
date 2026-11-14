use crate::geometry::{LogicalSize, PhysicalPoint, PhysicalRect, ScaleFactor};
use crate::ids::SurfaceId;

/// Host-chosen layout and projection for a surface in one real native window.
///
/// The size uses surface logical units. Origin and clip use window-client
/// physical pixels; scale is physical pixels per surface logical unit. The
/// clip does not change layout size or constrain captured source motion.
/// This value describes geometry only: exact placement authority is minted
/// by core when the complete candidate roster is committed.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct NativeSurfacePlacement {
    surface: SurfaceId,
    size: LogicalSize,
    origin: PhysicalPoint,
    scale_factor: ScaleFactor,
    clip: PhysicalRect,
}

impl NativeSurfacePlacement {
    /// Describes a placement. Candidate submission validates transformed extents.
    #[must_use]
    pub const fn new(
        surface: SurfaceId,
        size: LogicalSize,
        origin: PhysicalPoint,
        scale_factor: ScaleFactor,
        clip: PhysicalRect,
    ) -> Self {
        Self {
            surface,
            size,
            origin,
            scale_factor,
            clip,
        }
    }

    /// Returns the independent logical docking surface.
    #[must_use]
    pub const fn surface(self) -> SurfaceId {
        self.surface
    }

    /// Returns the layout size, independent of clipping.
    #[must_use]
    pub const fn size(self) -> LogicalSize {
        self.size
    }

    /// Returns the surface origin in window-client physical pixels.
    #[must_use]
    pub const fn origin(self) -> PhysicalPoint {
        self.origin
    }

    /// Returns physical pixels per surface logical unit.
    #[must_use]
    pub const fn scale_factor(self) -> ScaleFactor {
        self.scale_factor
    }

    /// Returns the effective host clip in window-client physical pixels.
    #[must_use]
    pub const fn clip(self) -> PhysicalRect {
        self.clip
    }
}
