//! Host-clock-driven rectangle motion without rendering or receiver authority.
//!
//! Tweens follow a bounded, fixed start-to-target path and preserve C0 when
//! restarted. Critical springs preserve each channel's position and velocity on
//! retarget. Projection to valid geometry and explicit settlement are exceptions
//! to that C1 guarantee. Hosts supply monotonic time and stop requesting animation
//! frames when [`RectMotionSample::is_active`] is false.

use std::time::Duration;

mod curve;
pub use curve::{CubicBezier, Curve, CurveError};

use crate::geometry::LogicalRect;

/// Motion inputs frozen for one host update and all of its rendering passes.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MotionFrame {
    /// Monotonic host time; the core does not read a platform clock.
    pub now: Duration,
    /// Application settings, independent of layout measurements.
    pub settings: MotionSettings,
    /// System preference observed by the host, or an explicit unknown fact.
    pub preference: MotionPreference,
}

/// Maximum spring position residual in logical points before settlement.
pub const POSITION_TOLERANCE: f64 = 0.0001;
/// Maximum spring speed in logical points per second before settlement.
pub const VELOCITY_TOLERANCE: f64 = 0.001;

/// The system preference reported by the host, without inferring OS support.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MotionPreference {
    /// Geometry motion is permitted.
    Full,
    /// Geometry travel, scaling, and bounce must be removed.
    Reduced,
    /// The host has no system preference fact.
    Unknown,
}

/// Global runtime policy, independent of the compile-time feature.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MotionMode {
    /// Always present target geometry immediately.
    Off,
    /// Resolve the host preference and then the per-effect setting.
    System,
}

/// Runtime settings frozen by the host for one outer update.
///
/// Defaults use a 200 ms response for layout, a 125 ms response for drop preview,
/// and immediate pointer focus. These responses are tuning seeds, not exact
/// settling deadlines. Unknown system preferences default to reduced motion;
/// applications can explicitly opt into full motion for that case.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MotionSettings {
    /// Global motion policy.
    pub mode: MotionMode,
    /// Accessibility fallback when the host reports [`MotionPreference::Unknown`].
    pub unknown_prefers_reduced_motion: bool,
    /// Structural layout policy.
    pub layout: Transition,
    /// Drop-decoration policy.
    pub drop_preview: Transition,
    /// Pointer-focus decoration policy.
    pub pointer_focus: Transition,
}

impl Default for MotionSettings {
    fn default() -> Self {
        Self {
            mode: MotionMode::System,
            unknown_prefers_reduced_motion: true,
            layout: Transition::CriticalSpring {
                response: Duration::from_millis(200),
            },
            drop_preview: Transition::CriticalSpring {
                response: Duration::from_millis(125),
            },
            pointer_focus: Transition::Instant,
        }
    }
}

impl MotionSettings {
    /// Resolves global and accessibility policies for structural layout motion.
    #[must_use]
    pub fn layout_transition(&self, preference: MotionPreference) -> Transition {
        self.resolve(self.layout, preference)
    }

    /// Resolves global and accessibility policies for drop decoration motion.
    #[must_use]
    pub fn drop_preview_transition(&self, preference: MotionPreference) -> Transition {
        self.resolve(self.drop_preview, preference)
    }

    /// Resolves global and accessibility policies for pointer-focus decoration.
    ///
    /// This decoration never delays keyboard focus or changes receiver authority.
    #[must_use]
    pub fn pointer_focus_transition(&self, preference: MotionPreference) -> Transition {
        self.resolve(self.pointer_focus, preference)
    }

    /// Whether any geometry effect is enabled after resolving the host preference.
    #[must_use]
    pub fn enabled(&self, preference: MotionPreference) -> bool {
        self.layout_transition(preference) != Transition::Instant
            || self.drop_preview_transition(preference) != Transition::Instant
            || self.pointer_focus_transition(preference) != Transition::Instant
    }

    fn resolve(&self, transition: Transition, preference: MotionPreference) -> Transition {
        if self.mode == MotionMode::Off
            || preference == MotionPreference::Reduced
            || (preference == MotionPreference::Unknown && self.unknown_prefers_reduced_motion)
        {
            return Transition::Instant;
        }
        transition.normalized()
    }
}

/// A validated curve or duration; a zero timing normalizes to immediate motion.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Transition {
    /// No geometry travel and no animation wakeup.
    Instant,
    /// A finite timeline along a fixed path, with C0 retargeting.
    Tween {
        /// Exact timeline length.
        duration: Duration,
        /// Bounded, monotone time-to-progress mapping.
        curve: Curve,
    },
    /// An analytic critically damped spring with independent channel velocities.
    CriticalSpring {
        /// Natural period: `omega = 2 * pi / response`, not a settling deadline.
        response: Duration,
    },
}

impl Transition {
    /// Converts zero duration or response to the division-free immediate path.
    #[must_use]
    pub const fn normalized(self) -> Self {
        match self {
            Self::Tween { duration, .. } if duration.is_zero() => Self::Instant,
            Self::CriticalSpring { response } if response.is_zero() => Self::Instant,
            transition => transition,
        }
    }
}

/// Independent rectangle-channel velocities in logical points per second.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct RectVelocity {
    /// Horizontal minimum-corner velocity.
    pub x: f64,
    /// Vertical minimum-corner velocity.
    pub y: f64,
    /// Width velocity.
    pub width: f64,
    /// Height velocity.
    pub height: f64,
}

impl RectVelocity {
    /// A stationary rectangle.
    pub const ZERO: Self = Self {
        x: 0.0,
        y: 0.0,
        width: 0.0,
        height: 0.0,
    };

    const fn components(self) -> [f64; 4] {
        [self.x, self.y, self.width, self.height]
    }

    const fn from_components(components: [f64; 4]) -> Self {
        let [x, y, width, height] = components;
        Self {
            x,
            y,
            width,
            height,
        }
    }
}

/// A valid sampled rectangle and its explicitly qualified motion state.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RectMotionSample {
    rect: LogicalRect,
    velocity: RectVelocity,
    active: bool,
    constrained: bool,
}

impl RectMotionSample {
    /// Returns finite geometry with nonnegative dimensions.
    #[must_use]
    pub const fn rect(self) -> LogicalRect {
        self.rect
    }

    /// Returns channel velocities after any explicit constraint projection.
    #[must_use]
    pub const fn velocity(self) -> RectVelocity {
        self.velocity
    }

    /// Whether another animation frame is needed.
    #[must_use]
    pub const fn is_active(self) -> bool {
        self.active
    }

    /// Whether geometry constraints interrupted the unconstrained trajectory.
    ///
    /// Negative sizes project to zero with zero velocity in those channels. A
    /// non-finite channel or an unrepresentable rectangle projects the whole
    /// sample to its valid target with zero velocity. Sampling remains active
    /// until the underlying trajectory settles. Neither case claims C1.
    #[must_use]
    pub const fn is_constrained(self) -> bool {
        self.constrained
    }

    const fn settled(rect: LogicalRect, constrained: bool) -> Self {
        Self {
            rect,
            velocity: RectVelocity::ZERO,
            active: false,
            constrained,
        }
    }
}

/// A prepared, allocation-free trajectory sampled using a host clock.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RectMotion {
    target: LogicalRect,
    state: MotionState,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum MotionState {
    Settled {
        constrained: bool,
    },
    Tween {
        start: [f64; 4],
        per_second: [f64; 4],
        started: Duration,
        duration: Duration,
        curve: Curve,
    },
    Spring {
        start: [f64; 4],
        velocity: [f64; 4],
        displacement: [f64; 4],
        coefficient: [f64; 4],
        omega: f64,
        started: Duration,
        response: Duration,
    },
}

impl MotionState {
    const fn transition(self) -> Transition {
        match self {
            Self::Settled { .. } => Transition::Instant,
            Self::Tween {
                duration, curve, ..
            } => Transition::Tween { duration, curve },
            Self::Spring { response, .. } => Transition::CriticalSpring { response },
        }
    }
}

impl RectMotion {
    /// Establishes a stationary pose, usually from admitted visible geometry.
    #[must_use]
    pub const fn new(rect: LogicalRect) -> Self {
        Self {
            target: rect,
            state: MotionState::Settled { constrained: false },
        }
    }

    /// Samples the running trajectory and starts toward a new target at `now`.
    ///
    /// Springs inherit the sampled position and all four velocities; tweens
    /// inherit position and restart their fixed timeline. Repeating an unchanged
    /// target and transition leaves that timeline intact. Changing settings uses
    /// this same retarget rule. An immediate transition discards velocity.
    /// Numeric overflow explicitly settles to target and reports a constraint.
    pub fn retarget(&mut self, target: LogicalRect, transition: Transition, now: Duration) {
        let transition = transition.normalized();
        if transition == Transition::Instant {
            self.settle(target);
            return;
        }
        if self.target == target && self.state.transition() == transition {
            return;
        }
        let current = self.sample(now);
        if current.rect == target && current.velocity == RectVelocity::ZERO {
            self.settle(target);
            return;
        }
        let start = components(current.rect);
        let end = components(target);
        self.target = target;
        self.state = match transition {
            Transition::Instant => MotionState::Settled { constrained: false },
            Transition::Tween { duration, curve } => {
                let seconds = duration.as_secs_f64();
                let per_second = std::array::from_fn(|index| {
                    let delta = end[index] - start[index];
                    if delta.is_finite() {
                        delta / seconds
                    } else {
                        end[index] / seconds - start[index] / seconds
                    }
                });
                if !all_finite(per_second) {
                    MotionState::Settled { constrained: true }
                } else {
                    MotionState::Tween {
                        start,
                        per_second,
                        started: now,
                        duration,
                        curve,
                    }
                }
            }
            Transition::CriticalSpring { response } => {
                let omega = std::f64::consts::TAU / response.as_secs_f64();
                let velocity = current.velocity.components();
                let displacement = std::array::from_fn(|index| start[index] - end[index]);
                let coefficient =
                    std::array::from_fn(|index| velocity[index] + omega * displacement[index]);
                if !all_finite(displacement) || !all_finite(coefficient) {
                    MotionState::Settled { constrained: true }
                } else {
                    MotionState::Spring {
                        start,
                        velocity,
                        displacement,
                        coefficient,
                        omega,
                        started: now,
                        response,
                    }
                }
            }
        };
    }

    /// Analytically samples at `now`, without advancing or accumulating time.
    ///
    /// Equal timestamps produce equal results regardless of prior frame rate.
    /// Hosts must provide a monotonic clock; times before the trajectory start
    /// use its initial state. A long pause uses one analytic evaluation.
    #[must_use]
    pub fn sample(&self, now: Duration) -> RectMotionSample {
        let end = components(self.target);
        let (position, velocity) = match self.state {
            MotionState::Settled { constrained } => {
                return RectMotionSample::settled(self.target, constrained);
            }
            MotionState::Tween {
                start,
                per_second,
                started,
                duration,
                curve,
            } => {
                let elapsed = now.saturating_sub(started);
                if elapsed >= duration {
                    return RectMotionSample::settled(self.target, false);
                }
                let progress = elapsed.as_secs_f64() / duration.as_secs_f64();
                let (value, slope) = curve.value_and_slope(progress);
                (
                    std::array::from_fn(|index| start[index] * (1.0 - value) + end[index] * value),
                    per_second.map(|value| value * slope),
                )
            }
            MotionState::Spring {
                start,
                velocity,
                displacement,
                coefficient,
                omega,
                started,
                ..
            } => {
                let elapsed = now.saturating_sub(started).as_secs_f64();
                if elapsed == 0.0 {
                    return project_sample(start, velocity, self.target);
                }
                let scaled_time = omega * elapsed;
                let decay = (-scaled_time).exp();
                if decay == 0.0 {
                    return RectMotionSample::settled(self.target, false);
                }
                let elapsed_decay = elapsed * decay;
                let scaled_decay = scaled_time * decay;
                // These envelopes decrease after scaled_time >= 1. Unlike a
                // momentary near-zero crossing, they cannot settle then reawaken.
                let settled = scaled_time >= 1.0
                    && (0..4).all(|index| {
                        displacement[index].abs() * decay + coefficient[index].abs() * elapsed_decay
                            <= POSITION_TOLERANCE
                            && velocity[index].abs() * decay
                                + coefficient[index].abs() * scaled_decay
                                <= VELOCITY_TOLERANCE
                    });
                if settled {
                    return RectMotionSample::settled(self.target, false);
                }
                let target_weight = -(-scaled_time).exp_m1();
                (
                    std::array::from_fn(|index| {
                        start[index] * decay
                            + end[index] * target_weight
                            + coefficient[index] * elapsed_decay
                    }),
                    std::array::from_fn(|index| {
                        velocity[index] * decay - coefficient[index] * scaled_decay
                    }),
                )
            }
        };
        project_sample(position, velocity, self.target)
    }

    /// Explicitly discards the trajectory, including inherited velocity.
    ///
    /// Use this for direct manipulation, hidden/resumed surfaces, or an explicit
    /// rebase. No C1 continuity is claimed across the reset.
    pub fn settle(&mut self, target: LogicalRect) {
        self.target = target;
        self.state = MotionState::Settled { constrained: false };
    }
}

fn components(rect: LogicalRect) -> [f64; 4] {
    [rect.x(), rect.y(), rect.width(), rect.height()]
}

fn all_finite(values: [f64; 4]) -> bool {
    values.into_iter().all(f64::is_finite)
}

fn project_sample(
    mut position: [f64; 4],
    mut velocity: [f64; 4],
    target: LogicalRect,
) -> RectMotionSample {
    if !all_finite(position) || !all_finite(velocity) {
        return RectMotionSample {
            active: true,
            ..RectMotionSample::settled(target, true)
        };
    }
    let mut constrained = false;
    for index in [2, 3] {
        if position[index] < 0.0 {
            position[index] = 0.0;
            velocity[index] = 0.0;
            constrained = true;
        }
    }
    let [x, y, width, height] = position;
    match LogicalRect::new(x, y, width, height) {
        Ok(rect) => RectMotionSample {
            rect,
            velocity: RectVelocity::from_components(velocity),
            active: true,
            constrained,
        },
        Err(_) => RectMotionSample {
            active: true,
            ..RectMotionSample::settled(target, true)
        },
    }
}

#[cfg(test)]
mod tests;
