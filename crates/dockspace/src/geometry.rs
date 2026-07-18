use thiserror::Error;

/// A failure to construct or convert validated geometry.
#[derive(Clone, Debug, Error, PartialEq)]
pub enum GeometryError {
    /// A component was NaN or infinite.
    #[error("geometry component `{component}` must be finite, got {value}")]
    NonFinite {
        /// Name of the invalid component.
        component: &'static str,
        /// Invalid value.
        value: f64,
    },
    /// A component which represents a magnitude was negative.
    #[error("geometry component `{component}` must be non-negative, got {value}")]
    Negative {
        /// Name of the invalid component.
        component: &'static str,
        /// Invalid value.
        value: f64,
    },
    /// A lower bound exceeded its upper bound.
    #[error("geometry bounds for `{component}` are inverted: minimum {minimum}, maximum {maximum}")]
    InvertedBounds {
        /// Axis or constrained component.
        component: &'static str,
        /// Invalid lower bound.
        minimum: f64,
        /// Invalid upper bound.
        maximum: f64,
    },
    /// A scale factor was zero.
    #[error("scale factor must be greater than zero")]
    ZeroScaleFactor,
}

fn finite(component: &'static str, value: f64) -> Result<f64, GeometryError> {
    if value.is_finite() {
        Ok(value)
    } else {
        Err(GeometryError::NonFinite { component, value })
    }
}

fn non_negative(component: &'static str, value: f64) -> Result<f64, GeometryError> {
    let value = finite(component, value)?;
    if value >= 0.0 {
        Ok(value)
    } else {
        Err(GeometryError::Negative { component, value })
    }
}

fn ordered(component: &'static str, minimum: f64, maximum: f64) -> Result<(), GeometryError> {
    if minimum <= maximum {
        Ok(())
    } else {
        Err(GeometryError::InvertedBounds {
            component,
            minimum,
            maximum,
        })
    }
}

/// A point in renderer-independent logical coordinates.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LogicalPoint {
    x: f64,
    y: f64,
}

impl LogicalPoint {
    /// Constructs a finite logical point.
    ///
    /// # Errors
    ///
    /// Returns [`GeometryError::NonFinite`] when either coordinate is NaN or infinite.
    pub fn new(x: f64, y: f64) -> Result<Self, GeometryError> {
        Ok(Self {
            x: finite("x", x)?,
            y: finite("y", y)?,
        })
    }

    /// Returns the horizontal coordinate.
    #[must_use]
    pub const fn x(self) -> f64 {
        self.x
    }

    /// Returns the vertical coordinate.
    #[must_use]
    pub const fn y(self) -> f64 {
        self.y
    }

    /// Converts a target-surface logical point into desktop physical pixels.
    ///
    /// # Errors
    ///
    /// Returns [`GeometryError::NonFinite`] when scaling or translating either coordinate
    /// overflows.
    pub fn to_desktop_physical(
        self,
        target_origin: PhysicalPoint,
        target_scale: ScaleFactor,
    ) -> Result<PhysicalPoint, GeometryError> {
        PhysicalPoint::new(
            target_origin.x + self.x * target_scale.0,
            target_origin.y + self.y * target_scale.0,
        )
    }
}

/// A non-negative size in renderer-independent logical coordinates.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LogicalSize {
    width: f64,
    height: f64,
}

impl LogicalSize {
    /// Constructs a finite, non-negative logical size.
    ///
    /// # Errors
    ///
    /// Returns [`GeometryError::NonFinite`] or [`GeometryError::Negative`] for an invalid
    /// component.
    pub fn new(width: f64, height: f64) -> Result<Self, GeometryError> {
        Ok(Self {
            width: non_negative("width", width)?,
            height: non_negative("height", height)?,
        })
    }

    /// Returns the width.
    #[must_use]
    pub const fn width(self) -> f64 {
        self.width
    }

    /// Returns the height.
    #[must_use]
    pub const fn height(self) -> f64 {
        self.height
    }
}

/// An axis-aligned rectangle in renderer-independent logical coordinates.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LogicalRect {
    min: LogicalPoint,
    max: LogicalPoint,
}

impl LogicalRect {
    /// Constructs a rectangle from finite, ordered corners.
    ///
    /// # Errors
    ///
    /// Returns [`GeometryError::InvertedBounds`] when either minimum exceeds its maximum.
    pub fn from_min_max(min: LogicalPoint, max: LogicalPoint) -> Result<Self, GeometryError> {
        ordered("x", min.x, max.x)?;
        ordered("y", min.y, max.y)?;
        Ok(Self { min, max })
    }

    /// Constructs a rectangle from its minimum corner and non-negative size.
    ///
    /// # Errors
    ///
    /// Returns [`GeometryError::NonFinite`] when calculating the maximum corner overflows.
    pub fn from_min_size(min: LogicalPoint, size: LogicalSize) -> Result<Self, GeometryError> {
        let max = LogicalPoint::new(min.x + size.width, min.y + size.height)?;
        Self::from_min_max(min, max)
    }

    /// Constructs a rectangle from scalar components.
    ///
    /// # Errors
    ///
    /// Returns [`GeometryError`] when a component is non-finite, a size is negative, or the
    /// maximum-corner calculation overflows.
    pub fn new(x: f64, y: f64, width: f64, height: f64) -> Result<Self, GeometryError> {
        Self::from_min_size(LogicalPoint::new(x, y)?, LogicalSize::new(width, height)?)
    }

    /// Returns the minimum corner.
    #[must_use]
    pub const fn min(self) -> LogicalPoint {
        self.min
    }

    /// Returns the maximum corner.
    #[must_use]
    pub const fn max(self) -> LogicalPoint {
        self.max
    }

    /// Returns the minimum horizontal coordinate.
    #[must_use]
    pub const fn x(self) -> f64 {
        self.min.x
    }

    /// Returns the minimum vertical coordinate.
    #[must_use]
    pub const fn y(self) -> f64 {
        self.min.y
    }

    /// Returns the rectangle width.
    #[must_use]
    pub fn width(self) -> f64 {
        self.max.x - self.min.x
    }

    /// Returns the rectangle height.
    #[must_use]
    pub fn height(self) -> f64 {
        self.max.y - self.min.y
    }

    /// Returns the rectangle size.
    #[must_use]
    pub fn size(self) -> LogicalSize {
        LogicalSize {
            width: self.width(),
            height: self.height(),
        }
    }

    /// Tests membership using half-open maximum edges.
    #[must_use]
    pub fn contains(self, point: LogicalPoint) -> bool {
        point.x >= self.min.x
            && point.x < self.max.x
            && point.y >= self.min.y
            && point.y < self.max.y
    }

    /// Converts a target-surface logical rectangle into desktop physical pixels.
    ///
    /// # Errors
    ///
    /// Returns [`GeometryError::NonFinite`] when scaling or translating a corner overflows.
    pub fn to_desktop_physical(
        self,
        target_origin: PhysicalPoint,
        target_scale: ScaleFactor,
    ) -> Result<PhysicalRect, GeometryError> {
        PhysicalRect::from_min_max(
            self.min.to_desktop_physical(target_origin, target_scale)?,
            self.max.to_desktop_physical(target_origin, target_scale)?,
        )
    }
}

/// A point in desktop physical-pixel coordinates.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PhysicalPoint {
    x: f64,
    y: f64,
}

impl PhysicalPoint {
    /// Constructs a finite desktop physical point.
    ///
    /// # Errors
    ///
    /// Returns [`GeometryError::NonFinite`] when either coordinate is NaN or infinite.
    pub fn new(x: f64, y: f64) -> Result<Self, GeometryError> {
        Ok(Self {
            x: finite("physical_x", x)?,
            y: finite("physical_y", y)?,
        })
    }

    /// Returns the horizontal coordinate.
    #[must_use]
    pub const fn x(self) -> f64 {
        self.x
    }

    /// Returns the vertical coordinate.
    #[must_use]
    pub const fn y(self) -> f64 {
        self.y
    }

    /// Converts this desktop point into one target surface's logical space.
    ///
    /// The target's acknowledged desktop-physical origin is subtracted before
    /// applying that target's scale. Desktop coordinates from different monitors
    /// are never divided into a synthetic global logical coordinate system.
    ///
    /// # Errors
    ///
    /// Returns [`GeometryError::NonFinite`] when subtracting the target origin overflows.
    pub fn to_target_logical(
        self,
        target_origin: Self,
        target_scale: ScaleFactor,
    ) -> Result<LogicalPoint, GeometryError> {
        LogicalPoint::new(
            (self.x - target_origin.x) / target_scale.0,
            (self.y - target_origin.y) / target_scale.0,
        )
    }
}

/// A non-negative size in desktop physical pixels.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PhysicalSize {
    width: f64,
    height: f64,
}

impl PhysicalSize {
    /// Constructs a finite, non-negative desktop physical size.
    ///
    /// # Errors
    ///
    /// Returns [`GeometryError::NonFinite`] or [`GeometryError::Negative`] for an invalid
    /// component.
    pub fn new(width: f64, height: f64) -> Result<Self, GeometryError> {
        Ok(Self {
            width: non_negative("physical_width", width)?,
            height: non_negative("physical_height", height)?,
        })
    }

    /// Returns the width.
    #[must_use]
    pub const fn width(self) -> f64 {
        self.width
    }

    /// Returns the height.
    #[must_use]
    pub const fn height(self) -> f64 {
        self.height
    }
}

/// An axis-aligned rectangle in desktop physical pixels.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PhysicalRect {
    min: PhysicalPoint,
    max: PhysicalPoint,
}

impl PhysicalRect {
    /// Constructs a physical rectangle from finite, ordered corners.
    ///
    /// # Errors
    ///
    /// Returns [`GeometryError::InvertedBounds`] when either minimum exceeds its maximum.
    pub fn from_min_max(min: PhysicalPoint, max: PhysicalPoint) -> Result<Self, GeometryError> {
        ordered("physical_x", min.x, max.x)?;
        ordered("physical_y", min.y, max.y)?;
        Ok(Self { min, max })
    }

    /// Constructs a physical rectangle from its minimum corner and size.
    ///
    /// # Errors
    ///
    /// Returns [`GeometryError::NonFinite`] when calculating the maximum corner overflows.
    pub fn from_min_size(min: PhysicalPoint, size: PhysicalSize) -> Result<Self, GeometryError> {
        let max = PhysicalPoint::new(min.x + size.width, min.y + size.height)?;
        Self::from_min_max(min, max)
    }

    /// Constructs a rectangle from scalar components.
    ///
    /// # Errors
    ///
    /// Returns [`GeometryError`] when a component is non-finite, a size is negative, or the
    /// maximum-corner calculation overflows.
    pub fn new(x: f64, y: f64, width: f64, height: f64) -> Result<Self, GeometryError> {
        Self::from_min_size(PhysicalPoint::new(x, y)?, PhysicalSize::new(width, height)?)
    }

    /// Returns the minimum corner.
    #[must_use]
    pub const fn min(self) -> PhysicalPoint {
        self.min
    }

    /// Returns the maximum corner.
    #[must_use]
    pub const fn max(self) -> PhysicalPoint {
        self.max
    }

    /// Returns the minimum horizontal coordinate.
    #[must_use]
    pub const fn x(self) -> f64 {
        self.min.x
    }

    /// Returns the minimum vertical coordinate.
    #[must_use]
    pub const fn y(self) -> f64 {
        self.min.y
    }

    /// Returns the rectangle width.
    #[must_use]
    pub fn width(self) -> f64 {
        self.max.x - self.min.x
    }

    /// Returns the rectangle height.
    #[must_use]
    pub fn height(self) -> f64 {
        self.max.y - self.min.y
    }

    /// Returns the rectangle size.
    #[must_use]
    pub fn size(self) -> PhysicalSize {
        PhysicalSize {
            width: self.width(),
            height: self.height(),
        }
    }

    /// Tests membership using half-open maximum edges.
    #[must_use]
    pub fn contains(self, point: PhysicalPoint) -> bool {
        point.x >= self.min.x
            && point.x < self.max.x
            && point.y >= self.min.y
            && point.y < self.max.y
    }

    /// Converts this desktop rectangle into one target surface's logical space.
    ///
    /// # Errors
    ///
    /// Returns [`GeometryError::NonFinite`] when translating a corner overflows.
    pub fn to_target_logical(
        self,
        target_origin: PhysicalPoint,
        target_scale: ScaleFactor,
    ) -> Result<LogicalRect, GeometryError> {
        LogicalRect::from_min_max(
            self.min.to_target_logical(target_origin, target_scale)?,
            self.max.to_target_logical(target_origin, target_scale)?,
        )
    }
}

/// A validated logical-to-physical scale for a specific target surface.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ScaleFactor(f64);

impl ScaleFactor {
    /// Constructs a finite, strictly positive target scale factor.
    ///
    /// # Errors
    ///
    /// Returns [`GeometryError::NonFinite`] for NaN or infinity,
    /// [`GeometryError::Negative`] for a negative value, or
    /// [`GeometryError::ZeroScaleFactor`] for zero.
    pub fn new(value: f64) -> Result<Self, GeometryError> {
        let value = non_negative("scale_factor", value)?;
        if value == 0.0 {
            Err(GeometryError::ZeroScaleFactor)
        } else {
            Ok(Self(value))
        }
    }

    /// Returns the scale value.
    #[must_use]
    pub const fn get(self) -> f64 {
        self.0
    }

    /// Converts a logical size once using this target surface's scale.
    ///
    /// # Errors
    ///
    /// Returns [`GeometryError::NonFinite`] when multiplication overflows.
    pub fn logical_size_to_physical(
        self,
        size: LogicalSize,
    ) -> Result<PhysicalSize, GeometryError> {
        PhysicalSize::new(size.width * self.0, size.height * self.0)
    }

    /// Converts a physical size once using this target surface's scale.
    ///
    /// # Errors
    ///
    /// Returns [`GeometryError::NonFinite`] if conversion produces a non-finite component.
    pub fn physical_size_to_logical(
        self,
        size: PhysicalSize,
    ) -> Result<LogicalSize, GeometryError> {
        LogicalSize::new(size.width / self.0, size.height / self.0)
    }
}

/// Validated minimum and maximum logical size constraints.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Constraints {
    min: LogicalSize,
    max: LogicalSize,
}

impl Constraints {
    /// Constructs constraints whose minimum does not exceed the maximum.
    ///
    /// # Errors
    ///
    /// Returns [`GeometryError::InvertedBounds`] when a minimum exceeds its maximum.
    pub fn new(min: LogicalSize, max: LogicalSize) -> Result<Self, GeometryError> {
        ordered("width", min.width, max.width)?;
        ordered("height", min.height, max.height)?;
        Ok(Self { min, max })
    }

    /// Returns the minimum logical size.
    #[must_use]
    pub const fn min(self) -> LogicalSize {
        self.min
    }

    /// Returns the maximum logical size.
    #[must_use]
    pub const fn max(self) -> LogicalSize {
        self.max
    }

    /// Clamps a validated logical size to these constraints.
    #[must_use]
    pub fn clamp(self, size: LogicalSize) -> LogicalSize {
        LogicalSize {
            width: size.width.clamp(self.min.width, self.max.width),
            height: size.height.clamp(self.min.height, self.max.height),
        }
    }
}
