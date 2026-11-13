use super::*;

fn rect(x: f64, y: f64, width: f64, height: f64) -> LogicalRect {
    LogicalRect::new(x, y, width, height).unwrap()
}

fn assert_close(actual: f64, expected: f64, tolerance: f64) {
    assert!(
        (actual - expected).abs() <= tolerance,
        "expected {expected}, got {actual} (tolerance {tolerance})"
    );
}

fn assert_pose(actual: LogicalRect, expected: LogicalRect) {
    for (actual, expected) in components(actual).into_iter().zip(components(expected)) {
        assert_close(actual, expected, 1.0e-9);
    }
}

fn assert_velocity(actual: RectVelocity, expected: RectVelocity) {
    for (actual, expected) in actual.components().into_iter().zip(expected.components()) {
        assert_close(actual, expected, 1.0e-8);
    }
}

fn cubic(first: f64, second: f64, parameter: f64) -> f64 {
    let remaining = 1.0 - parameter;
    3.0 * remaining * remaining * parameter * first
        + 3.0 * remaining * parameter * parameter * second
        + parameter * parameter * parameter
}

#[test]
fn bezier_solves_input_x_before_evaluating_y() {
    let curve = Curve::Bezier(CubicBezier::new(0.1, 0.7, 0.9, 1.0).unwrap());
    let parameter = 0.25;
    let input_time = cubic(0.1, 0.9, parameter);
    let expected_progress = cubic(0.7, 1.0, parameter);
    assert_close(curve.sample(input_time), expected_progress, 1.0e-12);
    assert!((curve.sample(input_time) - cubic(0.7, 1.0, input_time)).abs() > 0.05);
}

#[test]
fn custom_bezier_controls_can_be_stored_and_edited_without_duplicate_state() {
    let original = CubicBezier::new(0.1, 0.7, 0.9, 1.0).unwrap();
    let [x1, y1, x2, y2] = original.control_points();
    let restored = CubicBezier::new(x1, y1, x2, y2).unwrap();
    assert_eq!(original, restored);

    let mut controls = original.control_points();
    controls[1] = 0.2;
    let [x1, y1, x2, y2] = controls;
    let edited = CubicBezier::new(x1, y1, x2, y2).unwrap();
    assert!(Curve::Bezier(edited).sample(0.4) < Curve::Bezier(original).sample(0.4));

    controls[1] = 1.5;
    let [x1, y1, x2, y2] = controls;
    assert!(matches!(
        CubicBezier::new(x1, y1, x2, y2),
        Err(CurveError::OutOfRange {
            component: "y1",
            ..
        })
    ));
}

#[test]
fn geometry_curves_are_bounded_monotone_and_validated_once() {
    let curves = [
        Curve::Linear,
        Curve::EaseOut,
        Curve::EaseInOut,
        Curve::Bezier(CubicBezier::new(0.9, 0.8, 0.1, 0.2).unwrap()),
        Curve::Bezier(CubicBezier::new(1.0, 1.0, 0.0, 0.0).unwrap()),
    ];
    for curve in curves {
        assert_eq!(curve.sample(0.0), 0.0);
        assert_eq!(curve.sample(1.0), 1.0);
        let mut previous = 0.0;
        for step in 1..=1000 {
            let value = curve.sample(f64::from(step) / 1000.0);
            assert!((previous..=1.0).contains(&value));
            previous = value;
        }
    }
    assert!(CubicBezier::new(f64::NAN, 0.0, 1.0, 1.0).is_err());
    assert!(CubicBezier::new(0.0, 0.0, f64::INFINITY, 1.0).is_err());
    assert!(CubicBezier::new(-0.1, 0.0, 0.5, 1.0).is_err());
    assert!(CubicBezier::new(0.3, 1.5, 0.7, -0.5).is_err());
    assert!(CubicBezier::new(0.0, 1.0, 0.5, 1.0).is_err());
}

#[test]
fn tween_velocity_matches_the_derivative_of_the_time_mapped_curve() {
    let mut motion = RectMotion::new(rect(10.0, 20.0, 80.0, 60.0));
    motion.retarget(
        rect(210.0, 120.0, 160.0, 90.0),
        Transition::Tween {
            duration: Duration::from_millis(400),
            curve: Curve::Bezier(CubicBezier::new(0.2, 0.5, 0.6, 1.0).unwrap()),
        },
        Duration::ZERO,
    );
    let now = Duration::from_millis(123);
    let epsilon = Duration::from_micros(1);
    let left = motion.sample(now - epsilon).rect();
    let right = motion.sample(now + epsilon).rect();
    let expected = (right.x() - left.x()) / (2.0 * epsilon.as_secs_f64());
    assert_close(motion.sample(now).velocity().x, expected, 1.0e-4);
}

#[test]
fn spring_retarget_preserves_pose_and_arbitrary_channel_velocities() {
    let transition = Transition::CriticalSpring {
        response: Duration::from_millis(200),
    };
    let mut motion = RectMotion::new(rect(0.0, 20.0, 80.0, 60.0));
    motion.retarget(rect(180.0, 80.0, 140.0, 100.0), transition, Duration::ZERO);
    for (now, target) in [
        (Duration::from_millis(37), rect(-60.0, 240.0, 100.0, 120.0)),
        (Duration::from_millis(71), rect(220.0, -40.0, 160.0, 70.0)),
        (Duration::from_millis(93), rect(10.0, 160.0, 120.0, 140.0)),
    ] {
        let before = motion.sample(now);
        assert!(before.is_active());
        motion.retarget(target, transition, now);
        let after = motion.sample(now);
        assert_pose(after.rect(), before.rect());
        assert_velocity(after.velocity(), before.velocity());
        assert!(!after.is_constrained());
        let target_direction_x = target.x() - after.rect().x();
        let target_direction_y = target.y() - after.rect().y();
        let cross =
            after.velocity().x * target_direction_y - after.velocity().y * target_direction_x;
        assert!(cross.abs() > 1.0);
    }
}

#[test]
fn restarted_tween_preserves_pose_but_changes_velocity() {
    let mut motion = RectMotion::new(rect(0.0, 0.0, 80.0, 60.0));
    motion.retarget(
        rect(200.0, 100.0, 120.0, 80.0),
        Transition::Tween {
            duration: Duration::from_millis(400),
            curve: Curve::Linear,
        },
        Duration::ZERO,
    );
    let now = Duration::from_millis(100);
    let before = motion.sample(now);
    motion.retarget(
        rect(-100.0, 300.0, 100.0, 70.0),
        Transition::Tween {
            duration: Duration::from_millis(200),
            curve: Curve::Linear,
        },
        now,
    );
    let after = motion.sample(now);
    assert_pose(after.rect(), before.rect());
    assert_close(before.velocity().x, 500.0, 1.0e-9);
    assert_close(after.velocity().x, -750.0, 1.0e-9);
}

#[test]
fn unchanged_target_and_spec_do_not_restart_the_timeline() {
    let start = rect(0.0, 0.0, 80.0, 60.0);
    let target = rect(200.0, 100.0, 120.0, 80.0);
    for transition in [
        Transition::Tween {
            duration: Duration::from_millis(400),
            curve: Curve::EaseInOut,
        },
        Transition::CriticalSpring {
            response: Duration::from_millis(200),
        },
    ] {
        let mut motion = RectMotion::new(start);
        motion.retarget(target, transition, Duration::ZERO);
        let uninterrupted = motion;
        motion.retarget(target, transition, Duration::from_millis(83));
        let actual = motion.sample(Duration::from_millis(195));
        let expected = uninterrupted.sample(Duration::from_millis(195));
        assert_pose(actual.rect(), expected.rect());
        assert_velocity(actual.velocity(), expected.velocity());
    }
}

#[test]
fn equal_timestamps_are_independent_of_frame_rate_and_long_pauses_settle() {
    let mut motion = RectMotion::new(rect(0.0, 20.0, 80.0, 60.0));
    let target = rect(180.0, 80.0, 140.0, 100.0);
    motion.retarget(
        target,
        Transition::CriticalSpring {
            response: Duration::from_millis(200),
        },
        Duration::ZERO,
    );
    let timestamp = Duration::from_millis(325);
    let expected = motion.sample(timestamp);
    for frames_per_second in [30_u32, 60, 120, 137] {
        for frame in 0..=44 {
            let time = Duration::from_secs_f64(f64::from(frame) / f64::from(frames_per_second));
            let _ = motion.sample(time);
        }
        let actual = motion.sample(timestamp);
        assert_pose(actual.rect(), expected.rect());
        assert_velocity(actual.velocity(), expected.velocity());
    }
    for millis in [1, 11, 63, 64, 191, 324] {
        let _ = motion.sample(Duration::from_millis(millis));
    }
    assert_eq!(motion.sample(timestamp), expected);
    let paused = motion.sample(Duration::MAX);
    assert_eq!(paused.rect(), target);
    assert_eq!(paused.velocity(), RectVelocity::ZERO);
    assert!(!paused.is_active());
}

#[test]
fn default_motion_allows_unknown_preferences_but_off_and_reduced_take_priority() {
    let mut settings = MotionSettings::default();
    assert!(settings.enabled(MotionPreference::Unknown));
    assert!(settings.enabled(MotionPreference::Full));
    assert!(!settings.enabled(MotionPreference::Reduced));
    assert_eq!(
        settings.pointer_focus_transition(MotionPreference::Unknown),
        Transition::Instant
    );
    for resolve in [
        MotionSettings::layout_transition,
        MotionSettings::drop_preview_transition,
    ] {
        assert_ne!(
            resolve(&settings, MotionPreference::Unknown),
            Transition::Instant
        );
        assert_eq!(
            resolve(&settings, MotionPreference::Unknown),
            resolve(&settings, MotionPreference::Full)
        );
        assert_eq!(
            resolve(&settings, MotionPreference::Reduced),
            Transition::Instant
        );
    }
    settings.mode = MotionMode::Off;
    for preference in [
        MotionPreference::Unknown,
        MotionPreference::Full,
        MotionPreference::Reduced,
    ] {
        assert!(!settings.enabled(preference));
        assert_eq!(settings.layout_transition(preference), Transition::Instant);
        assert_eq!(
            settings.drop_preview_transition(preference),
            Transition::Instant
        );
    }
}

#[test]
fn default_layout_and_drop_motion_finish_at_their_documented_deadlines() {
    let settings = MotionSettings::default();
    let start = rect(0.0, 0.0, 800.0, 600.0);
    let target = rect(400.0, 100.0, 400.0, 500.0);
    let started = Duration::from_secs(3);
    for (transition, duration) in [
        (
            settings.layout_transition(MotionPreference::Unknown),
            Duration::from_millis(200),
        ),
        (
            settings.drop_preview_transition(MotionPreference::Unknown),
            Duration::from_millis(125),
        ),
    ] {
        let mut motion = RectMotion::new(start);
        motion.retarget(target, transition, started);
        assert_eq!(motion.sample(started).rect(), start);
        let before_deadline = started + duration - Duration::from_millis(1);
        motion.retarget(target, transition, before_deadline);
        let before = motion.sample(before_deadline);
        assert!(before.is_active());
        assert_ne!(before.rect(), target);
        for now in [started + duration, Duration::MAX] {
            let completed = motion.sample(now);
            assert_eq!(completed.rect(), target);
            assert_eq!(completed.velocity(), RectVelocity::ZERO);
            assert!(!completed.is_active());
        }
    }
}

#[test]
fn zero_timing_and_accessibility_policy_take_the_immediate_path() {
    let spring = Transition::CriticalSpring {
        response: Duration::from_millis(200),
    };
    let mut settings = MotionSettings {
        unknown_prefers_reduced_motion: true,
        layout: spring,
        drop_preview: spring,
        pointer_focus: spring,
        ..MotionSettings::default()
    };
    for resolve in [
        MotionSettings::layout_transition,
        MotionSettings::drop_preview_transition,
        MotionSettings::pointer_focus_transition,
    ] {
        assert_eq!(resolve(&settings, MotionPreference::Full), spring);
        assert_eq!(
            resolve(&settings, MotionPreference::Reduced),
            Transition::Instant
        );
        assert_eq!(
            resolve(&settings, MotionPreference::Unknown),
            Transition::Instant
        );
        settings.unknown_prefers_reduced_motion = false;
        assert_eq!(resolve(&settings, MotionPreference::Unknown), spring);
        settings.unknown_prefers_reduced_motion = true;
    }
    assert!(settings.enabled(MotionPreference::Full));
    assert!(!settings.enabled(MotionPreference::Reduced));
    assert!(!settings.enabled(MotionPreference::Unknown));
    settings.mode = MotionMode::Off;
    assert_eq!(
        settings.layout_transition(MotionPreference::Full),
        Transition::Instant
    );
    assert!(!settings.enabled(MotionPreference::Full));
    settings.mode = MotionMode::System;
    settings.layout = Transition::Instant;
    assert_eq!(
        settings.layout_transition(MotionPreference::Full),
        Transition::Instant
    );
    let start = rect(0.0, 0.0, 80.0, 60.0);
    let target = rect(200.0, 100.0, 120.0, 80.0);
    for transition in [
        Transition::Instant,
        Transition::Tween {
            duration: Duration::ZERO,
            curve: Curve::EaseOut,
        },
        Transition::CriticalSpring {
            response: Duration::ZERO,
        },
    ] {
        let mut motion = RectMotion::new(start);
        motion.retarget(target, transition, Duration::ZERO);
        let sampled = motion.sample(Duration::ZERO);
        assert_eq!(sampled.rect(), target);
        assert_eq!(sampled.velocity(), RectVelocity::ZERO);
        assert!(!sampled.is_active());
    }
}

#[test]
fn inherited_velocity_can_cross_the_target_with_critical_damping() {
    let mut motion = RectMotion::new(rect(0.0, 0.0, 80.0, 60.0));
    motion.retarget(
        rect(1000.0, 0.0, 80.0, 60.0),
        Transition::Tween {
            duration: Duration::from_secs(1),
            curve: Curve::Linear,
        },
        Duration::ZERO,
    );
    let now = Duration::from_millis(100);
    let inherited = motion.sample(now);
    motion.retarget(
        rect(101.0, 0.0, 80.0, 60.0),
        Transition::CriticalSpring {
            response: Duration::from_millis(200),
        },
        now,
    );
    assert_velocity(motion.sample(now).velocity(), inherited.velocity());
    let crossed = motion.sample(now + Duration::from_millis(10));
    assert!(crossed.rect().x() > 101.0);
    assert!(!crossed.is_constrained());
    assert!(crossed.is_active());
}

#[test]
fn a_slow_near_target_crossing_does_not_mistakenly_stop_the_clock() {
    let mut motion = RectMotion::new(rect(1.002, 0.0, 80.0, 60.0));
    motion.retarget(
        rect(0.998, 0.0, 80.0, 60.0),
        Transition::Tween {
            duration: Duration::from_secs(2),
            curve: Curve::Linear,
        },
        Duration::ZERO,
    );
    let now = Duration::from_secs(1);
    motion.retarget(
        rect(0.0, 0.0, 80.0, 60.0),
        Transition::CriticalSpring {
            response: Duration::from_secs_f64(std::f64::consts::TAU / 0.001),
        },
        now,
    );
    let crossing = motion.sample(now + Duration::from_secs(1000));
    assert!(crossing.rect().x().abs() < POSITION_TOLERANCE);
    assert!(crossing.velocity().x.abs() < VELOCITY_TOLERANCE);
    assert!(crossing.is_active());
    assert!(motion.sample(now + Duration::from_secs(2000)).rect().x() < -0.1);
}

#[test]
fn negative_spring_size_projects_explicitly_and_resets_its_velocity() {
    let mut motion = RectMotion::new(rect(0.0, 0.0, 100.0, 60.0));
    motion.retarget(
        rect(100.0, 0.0, 0.0, 60.0),
        Transition::Tween {
            duration: Duration::from_millis(100),
            curve: Curve::Linear,
        },
        Duration::ZERO,
    );
    let now = Duration::from_millis(90);
    motion.retarget(
        rect(120.0, 0.0, 1.0, 60.0),
        Transition::CriticalSpring {
            response: Duration::from_millis(200),
        },
        now,
    );
    let constrained_at = now + Duration::from_millis(30);
    let constrained = motion.sample(constrained_at);
    assert_eq!(constrained.rect().width(), 0.0);
    assert_eq!(constrained.velocity().width, 0.0);
    assert!(constrained.is_constrained());
    assert!(constrained.is_active());
    assert!(constrained.velocity().x > 0.0);
    let mut redirected = motion;
    redirected.retarget(
        rect(140.0, 0.0, 20.0, 60.0),
        Transition::CriticalSpring {
            response: Duration::from_millis(200),
        },
        constrained_at,
    );
    let after = redirected.sample(constrained_at);
    assert_pose(after.rect(), constrained.rect());
    assert_velocity(after.velocity(), constrained.velocity());
    assert!(!after.is_constrained());
    let settled = motion.sample(Duration::from_secs(2));
    assert_eq!(settled.rect(), rect(120.0, 0.0, 1.0, 60.0));
    assert_eq!(settled.velocity(), RectVelocity::ZERO);
    assert!(!settled.is_active());
}

#[test]
fn arithmetic_overflow_is_an_explicit_constrained_settlement() {
    let start = rect(f64::MAX, 0.0, 0.0, 0.0);
    let target = rect(-f64::MAX, 0.0, 0.0, 0.0);
    let mut motion = RectMotion::new(start);
    motion.retarget(
        target,
        Transition::CriticalSpring {
            response: Duration::from_nanos(1),
        },
        Duration::ZERO,
    );
    let sampled = motion.sample(Duration::from_nanos(1));
    assert_eq!(sampled.rect(), target);
    assert_eq!(sampled.velocity(), RectVelocity::ZERO);
    assert!(sampled.is_constrained());
    assert!(!sampled.is_active());
}

#[test]
fn an_unrepresentable_intermediate_sample_does_not_report_final_settlement() {
    let mut motion = RectMotion::new(rect(0.0, 0.0, 0.0, 0.0));
    motion.retarget(
        rect(1.0e308, 0.0, 0.0, 0.0),
        Transition::Tween {
            duration: Duration::from_secs(1),
            curve: Curve::Linear,
        },
        Duration::ZERO,
    );
    let now = Duration::from_millis(100);
    let target = rect(1.1e307, 0.0, 0.0, 0.0);
    motion.retarget(
        target,
        Transition::CriticalSpring {
            response: Duration::from_secs(100),
        },
        now,
    );
    let constrained = motion.sample(now + Duration::from_secs(10));
    assert_eq!(constrained.rect(), target);
    assert_eq!(constrained.velocity(), RectVelocity::ZERO);
    assert!(constrained.is_constrained());
    assert!(constrained.is_active());
    assert!(!motion.sample(Duration::MAX).is_active());
}

#[test]
fn explicit_settlement_discards_motion_and_needs_no_wakeup() {
    let mut motion = RectMotion::new(rect(0.0, 0.0, 80.0, 60.0));
    let target = rect(100.0, 60.0, 120.0, 90.0);
    motion.retarget(
        target,
        Transition::CriticalSpring {
            response: Duration::from_millis(200),
        },
        Duration::ZERO,
    );
    motion.settle(target);
    let sampled = motion.sample(Duration::from_millis(20));
    assert_eq!(sampled.rect(), target);
    assert_eq!(sampled.velocity(), RectVelocity::ZERO);
    assert!(!sampled.is_active());
    assert!(!sampled.is_constrained());
}
