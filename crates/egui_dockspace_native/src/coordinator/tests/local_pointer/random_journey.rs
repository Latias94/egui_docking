//! Reproducible raw-event journeys exercise the native translator and reducer together.

use super::*;

const STEPS: usize = 64;

struct Random(u64);

impl Random {
    fn choose(&mut self, count: u64) -> usize {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        usize::try_from(self.0 % count).unwrap()
    }
}

fn mouse_event(desktop: bool, point: LogicalPoint, state: Option<ElementState>) -> WindowEvent {
    if desktop {
        raw_desktop_event(point, state)
    } else {
        raw_event(point, state, 2.0)
    }
}

fn capture_route(event: &mut WindowEvent, route: PointerWindowRoute) {
    match event {
        WindowEvent::CursorMoved { facts, .. } | WindowEvent::MouseInput { facts, .. } => {
            facts.capture = route;
        }
        _ => unreachable!("only pointer samples carry capture observations"),
    }
}

fn apply(
    native: &mut NativeCoordinator,
    scene: &Scene,
    captured_at: Duration,
    retry: bool,
) -> Result<HostFrameReport, crate::NativeRuntimeError> {
    native.reduce_callback_prefix(&egui::Context::default(), &mut test_budget())?;
    if retry {
        let version = native.session.version();
        let candidate = native.begin_host_frame(|query| scene.answer(query, captured_at))?;
        drop(candidate);
        assert_eq!(native.session.version(), version);
    }
    let mut frame = native.begin_host_frame(|query| scene.answer(query, captured_at))?;
    frame.complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)?;
    frame.commit()
}

fn journey(seed: u64, desktop: bool) {
    eprintln!("native random journey: seed={seed:#018x}, desktop={desktop}");
    let mut random = Random(seed);
    let (mut native, binding, mut scene, now) = fixture_with_desktop(desktop);
    let mut ordinal = 0;
    let mut delivered = 0;
    let mut cancelled = 0;
    let mut changes = 0;
    let mut history = std::collections::VecDeque::new();
    let mut cases = [0_usize; 7];

    for step in 0..STEPS {
        let items = order(&native);
        let source_item = items[random.choose(3)];
        let target_item = items[random.choose(3)];
        let source = scene.point(source_item, 0.25);
        let target = scene.point(target_item, 0.625);
        let case = if step < cases.len() {
            step
        } else {
            random.choose(7)
        };
        cases[case] += 1;
        let captured_at = Duration::from_millis(40 + u64::try_from(step).unwrap() * 20);
        now.store(
            u64::try_from(captured_at.as_millis()).unwrap(),
            Ordering::SeqCst,
        );

        let mut events = vec![mouse_event(desktop, source, Some(ElementState::Pressed))];
        if case != 0 {
            events.push(mouse_event(desktop, target, None));
        }
        match case {
            2 => {
                let mut unknown = mouse_event(desktop, target, None);
                capture_route(&mut unknown, PointerWindowRoute::Unknown);
                events.push(unknown);
            }
            3 | 5 => {
                events.extend([
                    WindowEvent::CursorLeft {
                        device_id: DeviceId::dummy(),
                    },
                    WindowEvent::CursorEntered {
                        device_id: DeviceId::dummy(),
                    },
                ]);
                if case == 3 {
                    // The release happened outside the remote desktop and was not forwarded.
                    events.push(mouse_event(desktop, source, Some(ElementState::Pressed)));
                    events.push(mouse_event(desktop, target, None));
                }
            }
            4 | 6 => {
                let mut lost = mouse_event(desktop, target, None);
                capture_route(
                    &mut lost,
                    if case == 4 {
                        PointerWindowRoute::Foreign
                    } else {
                        PointerWindowRoute::None
                    },
                );
                events.push(lost);
            }
            _ => {}
        }
        events.push(mouse_event(
            desktop,
            if case == 0 { source } else { target },
            Some(ElementState::Released),
        ));

        for event in events {
            ordinal += 1;
            if history.len() == 16 {
                history.pop_front();
            }
            history.push_back((ordinal, event.clone()));
            let record =
                if NativeWindowEventClass::classify(&event) == NativeWindowEventClass::Pointer {
                    capture(&native, binding, ordinal, event)
                } else {
                    NativeWindowEventRecord::for_test(
                        ordinal,
                        WindowId::from(WINDOW),
                        Some(ViewportId::ROOT),
                        Some(binding),
                        event,
                    )
                };
            native.bridge.push_record(HostRecord::WindowEvent(record));
            let retry = random.choose(4) == 0;
            let report = apply(&mut native, &scene, captured_at, retry).unwrap_or_else(|error| {
                panic!(
                    "seed={seed:#018x}, desktop={desktop}, step={step}, case={case}, recent={history:#?}: {error:?}"
                )
            });
            changes += usize::from(report.workspace_changed());
            for feedback in report.manipulation_feedback() {
                match feedback.status() {
                    Status::Delivered(_) => delivered += 1,
                    Status::Cancelled(_) => cancelled += 1,
                    _ => {}
                }
            }
        }

        assert!(
            !native.session.has_active_gesture(),
            "seed={seed:#018x}, desktop={desktop}, step={step}, case={case}: a terminal must permit the next gesture"
        );
        let mut remaining = order(&native);
        remaining.sort_unstable();
        assert_eq!(remaining, [FIRST, SECOND, THIRD], "step={step}");
        assert!(native.session.is_current_native_binding(binding));
        assert!(!native.session.has_pending_native_input());
        assert!(native.next_window_event().unwrap().is_none());
        assert_eq!(native.bridge.input_backlog_for_test(), (0, None));
        assert!(native.pointer_translator.can_coalesce_idle_cursor_moves());
        scene = paint_and_present(&mut native, binding);
    }
    assert!(
        cases.iter().all(|count| *count > 0),
        "seed={seed:#018x}: {cases:?}"
    );
    assert!(
        delivered > 0,
        "the journey must complete actual docking operations"
    );
    assert!(
        changes > 0,
        "the journey must commit actual changes to the layout"
    );
    assert!(
        cancelled > 0,
        "the journey must exercise cancelled native gestures"
    );
}

#[test]
fn seeded_native_mouse_journeys_recover_and_keep_all_tabs() {
    let seeds = std::env::var("DOCKSPACE_TEST_SEED").map_or_else(
        |_| {
            vec![
                1,
                0x5eed,
                0xc0ffee,
                0xd0c5_face,
                0x107,
                0x3ff,
                0xabc,
                0x1234_5678,
            ]
        },
        |seed| {
            let value = seed
                .strip_prefix("0x")
                .map_or_else(|| seed.parse::<u64>(), |hex| u64::from_str_radix(hex, 16));
            let seed = value.expect("DOCKSPACE_TEST_SEED must be a decimal or 0x-prefixed u64");
            assert_ne!(seed, 0, "the xorshift generator requires a nonzero seed");
            vec![seed]
        },
    );
    for seed in seeds {
        for desktop in [false, true] {
            journey(seed, desktop);
        }
    }
}

#[test]
#[ignore = "extended native input stress; each seed is printed for replay"]
fn seeded_native_mouse_stress() {
    for index in 1_u64..=128 {
        let seed = index.wrapping_mul(0x9e37_79b9_7f4a_7c15);
        for desktop in [false, true] {
            journey(seed, desktop);
        }
    }
}
