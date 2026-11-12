use dockspace::geometry::{GeometryError, LogicalPoint, PhysicalPoint, ScaleFactor};
use winit::dpi::PhysicalPosition;

/// Maps client physical positions into one surface's logical coordinates.
///
/// The origin is the surface's top-left corner in window-client physical pixels,
/// not the window's desktop position. The scale must match the renderer's surface
/// coordinates; it can differ from winit's DPI scale when the UI applies zoom.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SurfaceCoordinates {
    client_origin: PhysicalPoint,
    pixels_per_point: ScaleFactor,
}

impl SurfaceCoordinates {
    /// Creates a mapping from a client physical origin and validated surface scale.
    #[must_use]
    pub const fn new(client_origin: PhysicalPoint, pixels_per_point: ScaleFactor) -> Self {
        Self {
            client_origin,
            pixels_per_point,
        }
    }

    /// Converts one client physical position into surface-local logical coordinates.
    ///
    /// # Errors
    ///
    /// Returns [`GeometryError::NonFinite`] for non-finite input or arithmetic overflow.
    pub fn to_surface(
        self,
        position: PhysicalPosition<f64>,
    ) -> Result<LogicalPoint, GeometryError> {
        PhysicalPoint::new(position.x, position.y)?
            .to_target_logical(self.client_origin, self.pixels_per_point)
    }
}
