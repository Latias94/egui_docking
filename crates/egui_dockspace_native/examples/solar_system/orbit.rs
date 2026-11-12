//! Application-owned orbital motion in desktop physical coordinates.

use std::time::{Duration, Instant};

use eframe::egui;
use winit::raw_window_handle::RawDisplayHandle;

const FRAME_INTERVAL: Duration = Duration::from_nanos(1_000_000_000 / 60);

/// One application clock; child windows never schedule independent animation ticks.
#[derive(Default)]
pub(super) struct AnimationClock {
    next: Option<Instant>,
}

impl AnimationClock {
    pub fn due(&mut self, now: Instant, active: bool) -> bool {
        if !active {
            self.next = None;
            return false;
        }
        let Some(deadline) = self.next else {
            self.next = Some(now + FRAME_INTERVAL);
            return true;
        };
        if now < deadline {
            return false;
        }
        // Keep the original cadence after a slow frame without replaying missed ticks.
        let remainder = now.duration_since(deadline).as_nanos() % FRAME_INTERVAL.as_nanos();
        let remainder = Duration::from_nanos(
            u64::try_from(remainder).expect("a sub-frame nanosecond remainder fits u64"),
        );
        self.next = Some(now + (FRAME_INTERVAL - remainder));
        true
    }

    pub fn delay(&self, now: Instant) -> Option<Duration> {
        self.next
            .map(|deadline| deadline.saturating_duration_since(now))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum GeometryAvailability {
    Ready,
    Waiting,
    Unsupported,
}

impl GeometryAvailability {
    pub fn observed(display: Option<RawDisplayHandle>, root: Option<WindowGeometry>) -> Self {
        if matches!(display, Some(RawDisplayHandle::Wayland(_))) {
            Self::Unsupported
        } else if root.is_some() {
            Self::Ready
        } else {
            Self::Waiting
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub(super) struct WindowGeometry {
    pub physical: egui::Rect,
    pub pixels_per_point: f32,
}

impl WindowGeometry {
    pub fn observed(outer: Option<egui::Rect>, pixels_per_point: f32) -> Option<Self> {
        let outer = outer?;
        if !outer.is_finite()
            || !outer.is_positive()
            || !pixels_per_point.is_finite()
            || pixels_per_point <= 0.0
        {
            return None;
        }
        Some(Self {
            physical: egui::Rect::from_min_max(
                outer.min * pixels_per_point,
                outer.max * pixels_per_point,
            ),
            pixels_per_point,
        })
    }
}

#[derive(Default)]
pub(super) struct Orbit {
    pub root: Option<WindowGeometry>,
    pub paused: bool,
    seconds: f32,
}

impl Orbit {
    pub fn tick(&mut self, elapsed: Duration, pointer_down: bool) {
        if !self.paused && !pointer_down {
            self.seconds += elapsed.as_secs_f32().min(0.1);
        }
    }

    pub fn physical_position(&self, planet: usize, child_size: egui::Vec2) -> Option<egui::Pos2> {
        let root = self.root?;
        let angle = self.seconds * 0.22 + super::PLANETS[planet].phase;
        let gap = (18.0 + super::PLANETS[planet].lane) * root.pixels_per_point;
        // At least one axis clears both rectangles, even at the ellipse's diagonals.
        let radii = (root.physical.size() * 0.5 + child_size * 0.5 + egui::Vec2::splat(gap))
            * std::f32::consts::SQRT_2;
        Some(
            root.physical.center() + egui::vec2(radii.x * angle.cos(), radii.y * angle.sin())
                - child_size * 0.5,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn orbit() -> Orbit {
        Orbit {
            root: WindowGeometry::observed(
                Some(egui::Rect::from_min_size(
                    egui::pos2(300.0, 200.0),
                    egui::vec2(500.0, 300.0),
                )),
                2.0,
            ),
            ..Orbit::default()
        }
    }

    #[test]
    fn ellipse_clears_the_root_at_every_angle() {
        let mut orbit = orbit();
        let child_size = egui::vec2(420.0, 300.0);
        for angle in 0_u16..360 {
            orbit.seconds = f32::from(angle).to_radians() / 0.22;
            for planet in 0..super::super::PLANETS.len() {
                let child = egui::Rect::from_min_size(
                    orbit.physical_position(planet, child_size).unwrap(),
                    child_size,
                );
                assert!(!child.intersects(orbit.root.unwrap().physical));
            }
        }
    }

    #[test]
    fn pause_does_not_accumulate_elapsed_time() {
        let mut orbit = orbit();
        orbit.paused = true;
        orbit.tick(Duration::from_secs(30), false);
        orbit.paused = false;
        orbit.tick(Duration::from_secs(30), true);
        assert_eq!(orbit.seconds, 0.0);
        orbit.tick(Duration::from_millis(16), false);
        assert!((orbit.seconds - 0.016).abs() < f32::EPSILON);
    }

    #[test]
    fn missing_first_frame_geometry_waits_without_declaring_the_backend_unsupported() {
        assert_eq!(
            GeometryAvailability::observed(None, None),
            GeometryAvailability::Waiting
        );
        assert_eq!(
            GeometryAvailability::observed(None, orbit().root),
            GeometryAvailability::Ready
        );
    }

    #[test]
    fn wayland_cannot_offer_global_window_placement_even_with_a_retained_rectangle() {
        let display = RawDisplayHandle::Wayland(
            winit::raw_window_handle::WaylandDisplayHandle::new(std::ptr::NonNull::dangling()),
        );
        for root in [None, orbit().root] {
            assert_eq!(
                GeometryAvailability::observed(Some(display), root),
                GeometryAvailability::Unsupported
            );
        }
    }

    #[test]
    fn animation_deadlines_keep_their_cadence_and_skip_missed_ticks() {
        let start = Instant::now();
        let mut clock = AnimationClock::default();
        assert!(clock.due(start, true));
        let early = start + Duration::from_millis(8);
        assert!(!clock.due(early, true));
        assert_eq!(
            clock.delay(early),
            Some(FRAME_INTERVAL - Duration::from_millis(8))
        );

        let late = start + FRAME_INTERVAL * 3 + Duration::from_millis(4);
        assert!(clock.due(late, true));
        assert!(
            !clock.due(late, true),
            "a late frame cannot dispatch a catch-up burst"
        );
        assert_eq!(
            clock.delay(late),
            Some(FRAME_INTERVAL - Duration::from_millis(4))
        );
        assert!(clock.due(start + FRAME_INTERVAL * 4, true));

        assert!(!clock.due(start + Duration::from_secs(5), false));
        assert_eq!(clock.delay(start + Duration::from_secs(5)), None);
        assert!(clock.due(start + Duration::from_secs(30), true));
        assert_eq!(
            clock.delay(start + Duration::from_secs(30)),
            Some(FRAME_INTERVAL)
        );
    }
}
