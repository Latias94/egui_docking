//! Exact renderer-independent hit regions.

use crate::geometry::{LogicalPoint, LogicalRect};

/// A semantic rectangle participating in protocol hit testing.
///
/// Membership is half-open on the maximum edges. Empty rectangles never hit,
/// and no tolerance or pointer-distance heuristic is applied.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct HitRegion {
    rect: LogicalRect,
}

impl HitRegion {
    /// Creates a hit region from validated logical geometry.
    #[must_use]
    pub const fn new(rect: LogicalRect) -> Self {
        Self { rect }
    }

    /// Returns the exact semantic rectangle.
    #[must_use]
    pub const fn rect(self) -> LogicalRect {
        self.rect
    }

    /// Returns whether the point belongs to this non-empty half-open region.
    #[must_use]
    pub fn contains(self, point: LogicalPoint) -> bool {
        self.rect.width() > 0.0 && self.rect.height() > 0.0 && self.rect.contains(point)
    }
}

impl From<LogicalRect> for HitRegion {
    fn from(rect: LogicalRect) -> Self {
        Self::new(rect)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn point(x: f64, y: f64) -> LogicalPoint {
        LogicalPoint::new(x, y).expect("test point is finite")
    }

    #[test]
    fn membership_is_half_open_without_tolerance() {
        let region = HitRegion::new(LogicalRect::new(10.0, 20.0, 30.0, 40.0).expect("valid rect"));

        assert!(region.contains(point(10.0, 20.0)));
        assert!(region.contains(point(39.999_999, 59.999_999)));
        assert!(!region.contains(point(40.0, 30.0)));
        assert!(!region.contains(point(20.0, 60.0)));
        assert!(!region.contains(point(9.999_999, 20.0)));
    }

    #[test]
    fn zero_area_never_hits() {
        let vertical = HitRegion::new(LogicalRect::new(10.0, 20.0, 0.0, 40.0).expect("valid rect"));
        let horizontal =
            HitRegion::new(LogicalRect::new(10.0, 20.0, 30.0, 0.0).expect("valid rect"));

        assert!(!vertical.contains(point(10.0, 30.0)));
        assert!(!horizontal.contains(point(20.0, 20.0)));
    }
}
