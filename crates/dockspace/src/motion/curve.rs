//! Bounded time-to-progress curves shared by rectangle transitions.

use std::{error::Error, fmt};

/// Semantic presets and custom, validated cubic Bezier curves.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Curve {
    /// Constant progress per unit time.
    Linear,
    /// Feedback or entrance: `cubic-bezier(0.23, 1, 0.32, 1)`.
    EaseOut,
    /// Movement of visible content: `cubic-bezier(0.77, 0, 0.175, 1)`.
    EaseInOut,
    /// A custom bounded, monotone curve with finite velocity.
    Bezier(CubicBezier),
}

impl Curve {
    /// Maps normalized input time to progress by solving Bezier x before y.
    ///
    /// Inputs outside `[0, 1]` use the corresponding endpoint. NaN uses zero.
    #[must_use]
    pub fn sample(self, progress: f64) -> f64 {
        self.value_and_slope(progress).0
    }

    pub(super) fn value_and_slope(self, progress: f64) -> (f64, f64) {
        let progress = if progress.is_nan() {
            0.0
        } else {
            progress.clamp(0.0, 1.0)
        };
        let bezier = match self {
            Self::Linear => return (progress, 1.0),
            Self::EaseOut => CubicBezier {
                x1: 0.23,
                y1: 1.0,
                x2: 0.32,
                y2: 1.0,
                start_slope: 1.0 / 0.23,
                end_slope: 0.0,
            },
            Self::EaseInOut => CubicBezier {
                x1: 0.77,
                y1: 0.0,
                x2: 0.175,
                y2: 1.0,
                start_slope: 0.0,
                end_slope: 0.0,
            },
            Self::Bezier(bezier) => bezier,
        };
        bezier.value_and_slope(progress)
    }
}

/// Invalid custom geometry easing parameters.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum CurveError {
    /// A control coordinate was NaN or infinite.
    NonFinite {
        /// Invalid coordinate name.
        component: &'static str,
        /// Invalid value.
        value: f64,
    },
    /// A control coordinate was outside the bounded geometry subset.
    OutOfRange {
        /// Invalid coordinate name.
        component: &'static str,
        /// Invalid value.
        value: f64,
    },
    /// Time-to-progress mapping would require infinite velocity.
    NonFiniteSlope,
}

impl fmt::Display for CurveError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NonFinite { component, value } => {
                write!(
                    formatter,
                    "Bezier coordinate `{component}` must be finite, got {value}"
                )
            }
            Self::OutOfRange { component, value } => {
                write!(
                    formatter,
                    "Bezier coordinate `{component}` must lie in [0, 1], got {value}"
                )
            }
            Self::NonFiniteSlope => {
                formatter.write_str("Bezier time-to-progress slope must remain finite")
            }
        }
    }
}

impl Error for CurveError {}

/// Cubic Bezier controls validated once at construction.
///
/// Both x and y controls must lie in `[0, 1]`. This bounded geometry subset has
/// monotone output, including when the controls are reversed. Vertical time-to-
/// progress tangents are rejected because geometry velocity must remain finite.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CubicBezier {
    x1: f64,
    y1: f64,
    x2: f64,
    y2: f64,
    start_slope: f64,
    end_slope: f64,
}

impl CubicBezier {
    /// Constructs a bounded monotone geometry curve with finite slopes.
    ///
    /// # Errors
    ///
    /// Returns [`CurveError`] for non-finite or out-of-range controls, or a
    /// vertical time-to-progress tangent at an endpoint or interior singularity.
    pub fn new(x1: f64, y1: f64, x2: f64, y2: f64) -> Result<Self, CurveError> {
        for (component, value) in [("x1", x1), ("y1", y1), ("x2", x2), ("y2", y2)] {
            if !value.is_finite() {
                return Err(CurveError::NonFinite { component, value });
            }
            if !(0.0..=1.0).contains(&value) {
                return Err(CurveError::OutOfRange { component, value });
            }
        }
        let start_slope = endpoint_slope(x1, y1, x2, y2)?;
        let end_slope = endpoint_slope(1.0 - x2, 1.0 - y2, 1.0 - x1, 1.0 - y1)?;
        if x1 == 1.0 && x2 == 0.0 && (y1 != 1.0 || y2 != 0.0) {
            return Err(CurveError::NonFiniteSlope);
        }
        Ok(Self {
            x1,
            y1,
            x2,
            y2,
            start_slope,
            end_slope,
        })
    }

    fn value_and_slope(self, progress: f64) -> (f64, f64) {
        if self.x1 == self.y1 && self.x2 == self.y2 {
            return (progress, 1.0);
        }
        if progress == 0.0 {
            return (0.0, self.start_slope);
        }
        if progress == 1.0 {
            return (1.0, self.end_slope);
        }
        let mut lower = 0.0;
        let mut upper = 1.0;
        let mut parameter = progress;
        for _ in 0..48 {
            parameter = (lower + upper) * 0.5;
            let x = cubic(self.x1, self.x2, parameter);
            if x < progress {
                lower = parameter;
            } else if x > progress {
                upper = parameter;
            } else {
                break;
            }
        }
        (
            cubic(self.y1, self.y2, parameter),
            cubic_derivative(self.y1, self.y2, parameter)
                / cubic_derivative(self.x1, self.x2, parameter),
        )
    }
}

fn endpoint_slope(x1: f64, y1: f64, x2: f64, y2: f64) -> Result<f64, CurveError> {
    let slope = if x1 != 0.0 {
        y1 / x1
    } else if y1 != 0.0 {
        return Err(CurveError::NonFiniteSlope);
    } else if x2 != 0.0 {
        y2 / x2
    } else if y2 != 0.0 {
        return Err(CurveError::NonFiniteSlope);
    } else {
        1.0
    };
    if slope.is_finite() {
        Ok(slope)
    } else {
        Err(CurveError::NonFiniteSlope)
    }
}

fn cubic(first: f64, second: f64, parameter: f64) -> f64 {
    let remaining = 1.0 - parameter;
    3.0 * remaining * remaining * parameter * first
        + 3.0 * remaining * parameter * parameter * second
        + parameter * parameter * parameter
}

fn cubic_derivative(first: f64, second: f64, parameter: f64) -> f64 {
    let remaining = 1.0 - parameter;
    3.0 * (remaining * remaining * first
        + 2.0 * remaining * parameter * (second - first)
        + parameter * parameter * (1.0 - second))
}
