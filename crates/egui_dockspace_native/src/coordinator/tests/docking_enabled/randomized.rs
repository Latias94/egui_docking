//! Seeded raw-event journeys move real panes in both directions between native routes.

use std::collections::VecDeque;

use dockspace::runtime::{NativeGlobalFocus, NativePointerRoster};
use egui_dockspace::{DockInputConfig, DockingActivation};

use super::*;

const MOVING: ItemId = ItemId::new(2);
const THIRD_SURFACE: SurfaceId = SurfaceId::new(3);
const SURFACES: [SurfaceId; 3] = [FIRST_SURFACE, SECOND_SURFACE, THIRD_SURFACE];
const ROUNDS: usize = 12;

mod capacity;
mod last_pane;

struct Random(u64);

impl Random {
    fn choose(&mut self, count: u64) -> usize {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        usize::try_from(self.0 % count).unwrap()
    }
}

struct Trace {
    seed: u64,
    round: usize,
    ordinal: u64,
    history: VecDeque<String>,
}

impl Trace {
    fn event(&mut self, event: impl std::fmt::Debug) -> u64 {
        self.ordinal += 1;
        if self.history.len() == 20 {
            self.history.pop_front();
        }
        self.history
            .push_back(format!("{}: {event:?}", self.ordinal));
        self.ordinal
    }

    fn replay(&self) -> String {
        format!(
            "DOCKSPACE_TEST_SEED={:#x}, round={}, recent={:#?}",
            self.seed, self.round, self.history
        )
    }
}

fn routes(owned_child: bool) -> (NativeCoordinator, Vec<NativeSurfaceBinding>) {
    use dockspace::model::{DockspaceContainedLayout, FloatingPresentationId};

    let mut surfaces = vec![
        DockspaceSurfaceLayout::new(
            FIRST_SURFACE,
            DockspaceRootLayout::new(
                RootId::new(1),
                DockspaceNode::central_tabs(if owned_child {
                    vec![
                        ItemId::new(1),
                        ItemId::new(3),
                        ItemId::new(4),
                        ItemId::new(5),
                    ]
                } else {
                    vec![ItemId::new(1), ItemId::new(3)]
                }),
            ),
        ),
        if owned_child {
            // A contained-only child keeps source input preserved. This isolates
            // last-pane retirement from an independent pass-through command.
            DockspaceSurfaceLayout::rootless(SECOND_SURFACE).with_contained(
                DockspaceContainedLayout::new(
                    FloatingPresentationId::new(1),
                    DockspaceRootLayout::new(RootId::new(2), DockspaceNode::tabs([MOVING])),
                    LogicalRect::new(40.0, 60.0, 360.0, 300.0).unwrap(),
                ),
            )
        } else {
            DockspaceSurfaceLayout::new(
                SECOND_SURFACE,
                DockspaceRootLayout::new(
                    RootId::new(2),
                    DockspaceNode::tabs([MOVING, ItemId::new(4)]),
                ),
            )
        },
    ];
    if !owned_child {
        surfaces.push(DockspaceSurfaceLayout::new(
            THIRD_SURFACE,
            DockspaceRootLayout::new(RootId::new(3), DockspaceNode::tabs([ItemId::new(5)])),
        ));
    }
    let layout = DockspaceLayout::new(surfaces).unwrap();
    let mut policy = DockPolicy::default();
    policy.set_allow_native_surfaces(true);
    let mut native = NativeCoordinator::new_with_storage_allowance(
        DockspaceSession::from_layout(layout, policy).unwrap(),
        NativePointerRoster::Exact(Vec::new()),
        None,
        None,
        None,
        // Keep callback ages independent of scheduling delays during replay.
        Some(dockspace::runtime::NativeMonotonicClock::new(|| {
            Duration::ZERO
        })),
    )
    .unwrap();
    native
        .register_native_root(FIRST_SURFACE, HostWindowToken::new(11))
        .unwrap();
    commit_boundary(&mut native);
    if owned_child {
        native
            .session
            .register_owned_native_child(SECOND_SURFACE, HostWindowToken::new(22), FIRST_SURFACE)
            .unwrap();
    } else {
        native
            .register_native_root(SECOND_SURFACE, HostWindowToken::new(22))
            .unwrap();
        native
            .register_native_root(THIRD_SURFACE, HostWindowToken::new(33))
            .unwrap();
    }
    let bindings = commit_boundary(&mut native).native_bindings().to_vec();
    assert_eq!(bindings.len(), if owned_child { 2 } else { 3 });
    for &binding in &bindings {
        let viewport = match binding.surface() {
            FIRST_SURFACE => ViewportId::ROOT,
            SECOND_SURFACE => ViewportId::from_hash_of("randomized-peer"),
            THIRD_SURFACE => ViewportId::from_hash_of("randomized-third"),
            _ => unreachable!(),
        };
        native
            .bind_viewport(viewport, window(binding.surface()), binding)
            .unwrap();
    }
    // Choose the backend once, before publishing any capability snapshot.
    native
        .report_snapshot(
            bindings
                .iter()
                .copied()
                .map(|binding| (binding, ready_window_facts(x(binding.surface())))),
            NativeWorkAreaRoster::Unknown,
            crate::capabilities::capabilities_for_backend(eframe::NativeWindowingBackend::X11),
        )
        .unwrap();
    commit_boundary(&mut native);
    (native, bindings)
}

fn x(surface: SurfaceId) -> f64 {
    match surface {
        FIRST_SURFACE => 0.0,
        SECOND_SURFACE => 720.0,
        THIRD_SURFACE => 1440.0,
        _ => unreachable!(),
    }
}

fn window(surface: SurfaceId) -> WindowId {
    WindowId::from(match surface {
        FIRST_SURFACE => 11,
        SECOND_SURFACE => 22,
        THIRD_SURFACE => 33,
        _ => unreachable!(),
    })
}

fn scene(
    native: &mut NativeCoordinator,
    target_surface: SurfaceId,
) -> (RecordedSource, PhysicalPosition<f64>, PhysicalPosition<f64>) {
    let (_, artifacts) = paint_and_present_all_surfaces_with_artifacts(native);
    let source_surface = native.session.view().item(MOVING).unwrap().surface();
    let mut frame = native
        .begin_host_frame(|_| NativeReceiverAnswer::Unknown)
        .unwrap();
    let source_plan = frame.frame.paint_plan(source_surface).unwrap().unwrap();
    let mut descriptor = None;
    source_plan
        .visit_surface_program(|step| {
            if let SurfacePaintProgramStep::Operation {
                operation: SurfacePaintProgramOperation::Tab(tab),
                ..
            } = step
                && tab.item() == MOVING
            {
                descriptor = source_plan.receiver_for_tab_body(tab);
            }
            Ok::<(), ()>(())
        })
        .unwrap();
    let descriptor = descriptor.unwrap();
    let source = PhysicalPosition::new(
        x(source_surface) + descriptor.center().x(),
        descriptor.center().y(),
    );
    let target_plan = frame.frame.paint_plan(target_surface).unwrap().unwrap();
    let mut target = None;
    target_plan
        .visit_surface_program(|step| {
            if let SurfacePaintProgramStep::Operation {
                operation: SurfacePaintProgramOperation::Pane(pane),
                ..
            } = step
            {
                let bounds = pane.content_bounds();
                target = Some(PhysicalPosition::new(
                    x(target_surface) + bounds.x() + bounds.width() * 0.75,
                    bounds.y() + bounds.height() * 0.75,
                ));
            }
            Ok::<(), ()>(())
        })
        .unwrap();
    finish(frame);
    let artifacts = artifacts
        .into_iter()
        .map(|(binding, artifact)| {
            (
                binding.window(),
                native.session.native_surface_placement(binding).unwrap(),
                artifact,
            )
        })
        .collect();
    (
        RecordedSource {
            descriptor,
            artifacts,
        },
        source,
        target.unwrap(),
    )
}

fn mouse(
    delivery: NativeSurfaceBinding,
    point: PhysicalPosition<f64>,
    state: Option<ElementState>,
    hover: SurfaceId,
    capture: PointerWindowRoute,
    modifiers: Option<ModifiersState>,
) -> WindowEvent {
    let position = PhysicalPosition::new(point.x - x(delivery.surface()), point.y);
    let facts = PointerEventFacts {
        surface_position: Some(position),
        desktop_position: Some(point),
        modifiers,
        hover: PointerWindowRoute::Window(window(hover)),
        capture,
        ..Default::default()
    };
    match state {
        Some(state) => WindowEvent::MouseInput {
            device_id: DeviceId::dummy(),
            primary_press: None,
            state,
            button: MouseButton::Left,
            facts,
        },
        None => WindowEvent::CursorMoved {
            device_id: DeviceId::dummy(),
            position,
            facts,
        },
    }
}

fn transient_preview(frame: &mut NativeHostFrame<'_>) -> Option<DockspacePreviewVisual> {
    let mut result = None;
    for surface in frame.frame.surfaces() {
        if frame.frame.paint_plan(surface).unwrap().is_none() {
            frame
                .frame
                .measure_surface(
                    surface,
                    UniformSurfaceMetrics::new(
                        LogicalRect::new(0.0, 0.0, 640.0, 480.0).unwrap(),
                        LogicalSize::new(32.0, 24.0).unwrap(),
                        80.0,
                    )
                    .unwrap(),
                )
                .unwrap();
        }
        frame
            .frame
            .paint_plan(surface)
            .unwrap()
            .unwrap()
            .visit_surface_program(|step| {
                if let SurfacePaintProgramStep::DragPreview(preview) = step {
                    assert!(result.replace(preview.visual()).is_none());
                }
                Ok::<(), ()>(())
            })
            .unwrap();
    }
    result
}

fn apply(
    native: &mut NativeCoordinator,
    binding: NativeSurfaceBinding,
    event: WindowEvent,
    receivers: &RecordedSource,
    retry: bool,
    trace: &mut Trace,
) -> (Option<DockspacePreviewVisual>, HostFrameReport) {
    let ordinal = trace.event(&event);
    let record = captured_record(native, binding, ordinal, event);
    native.bridge.push_record(HostRecord::WindowEvent(record));
    let reduced = native
        .reduce_callback_prefix(&egui::Context::default(), &mut test_budget())
        .unwrap_or_else(|error| panic!("{}: {error:?}", trace.replay()));
    assert_eq!(reduced.unwrap().get(), 1, "{}", trace.replay());
    let version = native.session.version();
    let mut frame = native
        .begin_host_frame(|query| resolve(query, receivers))
        .unwrap_or_else(|error| panic!("{}: {error:?}", trace.replay()));
    let candidate_preview = transient_preview(&mut frame);
    if retry {
        drop(frame);
        assert_eq!(native.session.version(), version, "{}", trace.replay());
        assert!(
            native.session.has_pending_native_input(),
            "{}",
            trace.replay()
        );
        frame = native
            .begin_host_frame(|query| resolve(query, receivers))
            .unwrap_or_else(|error| panic!("{}: {error:?}", trace.replay()));
        assert_eq!(
            transient_preview(&mut frame),
            candidate_preview,
            "{}",
            trace.replay()
        );
    }
    frame
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .unwrap();
    let mut report = frame
        .commit()
        .unwrap_or_else(|error| panic!("{}: {error:?}", trace.replay()));
    // The model publishes exact focus observations before releases, so no OS
    // focus request is silently dropped or treated as successful dispatch.
    assert!(
        report.take_native_effects().is_empty(),
        "{}",
        trace.replay()
    );
    native.settle_host_frame_report(&report).unwrap();
    for id in 1..=5 {
        assert!(
            native.session.view().item(ItemId::new(id)).is_some(),
            "{}: lost pane {id}",
            trace.replay()
        );
    }
    (candidate_preview, report)
}

fn global_focus(native: &mut NativeCoordinator, binding: NativeSurfaceBinding, trace: &mut Trace) {
    let ordinal = trace.event(("global focus", binding));
    native.bridge.push_record(HostRecord::GlobalFocus(
        crate::focus_control::NativeGlobalFocusRecord::for_test(
            ordinal,
            NativeGlobalFocus::Dock(binding.window()),
        ),
    ));
    native
        .reduce_callback_prefix(&egui::Context::default(), &mut test_budget())
        .unwrap();
    let mut report = commit_boundary(native);
    assert!(
        report.take_native_effects().is_empty(),
        "{}",
        trace.replay()
    );
}

fn journey(seed: u64) {
    eprintln!("native cross-window journey: DOCKSPACE_TEST_SEED={seed:#x}");
    let mut random = Random(seed);
    let (mut native, bindings) = routes(false);
    let mut trace = Trace {
        seed,
        round: 0,
        ordinal: 0,
        history: VecDeque::new(),
    };
    let mut cancellations = 0;
    let mut delivered = 0;
    let mut directions = [[0_usize; 3]; 3];
    for round in 0..ROUNDS {
        trace.round = round;
        let source_surface = native.session.view().item(MOVING).unwrap().surface();
        // Cover every directed pair before random routing. Each seed must
        // prove actual A<->B, B<->C and A<->C delivery, not just visit three IDs.
        let target_surface = if round < 6 {
            [
                FIRST_SURFACE,
                THIRD_SURFACE,
                SECOND_SURFACE,
                THIRD_SURFACE,
                FIRST_SURFACE,
                SECOND_SURFACE,
            ][round]
        } else {
            let others = SURFACES
                .into_iter()
                .filter(|surface| *surface != source_surface)
                .collect::<Vec<_>>();
            others[random.choose(2)]
        };
        let source_binding = bindings
            .iter()
            .copied()
            .find(|binding| binding.surface() == source_surface)
            .unwrap();
        let target_binding = bindings
            .iter()
            .copied()
            .find(|binding| binding.surface() == target_surface)
            .unwrap();
        let (receivers, source, target) = scene(&mut native, target_surface);
        global_focus(&mut native, source_binding, &mut trace);
        let enable_with_alt = random.choose(2) == 0;
        native.set_input(DockInputConfig {
            docking: if enable_with_alt {
                DockingActivation::HoldToEnable(egui::Modifiers::ALT)
            } else {
                DockingActivation::HoldToDisable(egui::Modifiers::SHIFT)
            },
            ..Default::default()
        });
        let enabled = if enable_with_alt {
            ModifiersState::ALT
        } else {
            ModifiersState::empty()
        };
        let disabled = if enable_with_alt {
            ModifiersState::empty()
        } else {
            ModifiersState::SHIFT
        };
        let held = PointerWindowRoute::Window(window(source_surface));
        let mut cancelled_in_round = false;
        for (point, state, hover) in [
            (source, Some(ElementState::Pressed), source_surface),
            (target, None, target_surface),
        ] {
            apply(
                &mut native,
                source_binding,
                mouse(source_binding, point, state, hover, held, Some(enabled)),
                &receivers,
                random.choose(4) == 0,
                &mut trace,
            );
        }
        let version = native.session.version();
        let case = if round < 4 { round } else { random.choose(4) };
        let event = match case {
            0 => mouse(
                source_binding,
                target,
                None,
                target_surface,
                PointerWindowRoute::Foreign,
                None,
            ),
            1 => mouse(
                source_binding,
                target,
                None,
                target_surface,
                PointerWindowRoute::None,
                None,
            ),
            2 | 3 => {
                apply(
                    &mut native,
                    source_binding,
                    WindowEvent::CursorLeft {
                        device_id: DeviceId::dummy(),
                    },
                    &receivers,
                    false,
                    &mut trace,
                );
                apply(
                    &mut native,
                    source_binding,
                    WindowEvent::CursorEntered {
                        device_id: DeviceId::dummy(),
                    },
                    &receivers,
                    false,
                    &mut trace,
                );
                if case == 3 {
                    apply(
                        &mut native,
                        source_binding,
                        mouse(
                            source_binding,
                            target,
                            None,
                            target_surface,
                            PointerWindowRoute::Unknown,
                            None,
                        ),
                        &receivers,
                        false,
                        &mut trace,
                    );
                }
                mouse(
                    source_binding,
                    source,
                    Some(ElementState::Pressed),
                    source_surface,
                    held,
                    Some(enabled),
                )
            }
            _ => unreachable!(),
        };
        let (_, terminal) = apply(
            &mut native,
            source_binding,
            event,
            &receivers,
            true,
            &mut trace,
        );
        for feedback in terminal.manipulation_feedback() {
            if matches!(feedback.status(), Status::Cancelled(_)) {
                cancellations += 1;
                cancelled_in_round = true;
            }
        }
        assert!(
            cancelled_in_round,
            "{}: cancellation case {case} did not terminate its old drag",
            trace.replay()
        );
        assert_eq!(native.session.version(), version, "{}", trace.replay());
        assert!(!terminal.workspace_changed(), "{}", trace.replay());
        if !native.session.has_active_gesture() {
            apply(
                &mut native,
                source_binding,
                mouse(
                    source_binding,
                    source,
                    Some(ElementState::Pressed),
                    source_surface,
                    held,
                    Some(enabled),
                ),
                &receivers,
                false,
                &mut trace,
            );
        }
        apply(
            &mut native,
            source_binding,
            WindowEvent::ModifiersChanged(disabled.into()),
            &receivers,
            false,
            &mut trace,
        );
        let (disabled_preview, _) = apply(
            &mut native,
            source_binding,
            mouse(source_binding, target, None, target_surface, held, None),
            &receivers,
            false,
            &mut trace,
        );
        assert!(disabled_preview.is_none(), "{}", trace.replay());
        apply(
            &mut native,
            source_binding,
            WindowEvent::ModifiersChanged(enabled.into()),
            &receivers,
            false,
            &mut trace,
        );
        let (enabled_preview, _) = apply(
            &mut native,
            source_binding,
            mouse(source_binding, target, None, target_surface, held, None),
            &receivers,
            random.choose(2) == 0,
            &mut trace,
        );
        assert!(
            matches!(enabled_preview, Some(DockspacePreviewVisual::Dock { surface, .. }) if surface == target_surface),
            "{}: enabled modifiers must restore the real cross-window preview",
            trace.replay()
        );
        if random.choose(2) == 0 {
            apply(
                &mut native,
                source_binding,
                WindowEvent::Focused(false),
                &receivers,
                false,
                &mut trace,
            );
        }
        global_focus(&mut native, target_binding, &mut trace);
        let release_capture = if random.choose(2) == 0 {
            apply(
                &mut native,
                source_binding,
                mouse(
                    source_binding,
                    target,
                    None,
                    target_surface,
                    PointerWindowRoute::Unknown,
                    None,
                ),
                &receivers,
                true,
                &mut trace,
            );
            PointerWindowRoute::None
        } else {
            PointerWindowRoute::Unknown
        };
        let (_, report) = apply(
            &mut native,
            source_binding,
            mouse(
                source_binding,
                target,
                Some(ElementState::Released),
                target_surface,
                release_capture,
                Some(enabled),
            ),
            &receivers,
            true,
            &mut trace,
        );
        assert!(
            report.workspace_changed(),
            "{}: release failed: {:?}",
            trace.replay(),
            report.manipulation_feedback()
        );
        assert!(
            report
                .manipulation_feedback()
                .iter()
                .any(|feedback| feedback.status() == Status::Delivered(Delivery::Dock)),
            "{}",
            trace.replay()
        );
        delivered += 1;
        let from = SURFACES
            .iter()
            .position(|surface| *surface == source_surface)
            .unwrap();
        let to = SURFACES
            .iter()
            .position(|surface| *surface == target_surface)
            .unwrap();
        directions[from][to] += 1;
        assert_eq!(
            native.session.view().item(MOVING).unwrap().surface(),
            target_surface,
            "{}",
            trace.replay()
        );
        assert!(!native.session.has_active_gesture(), "{}", trace.replay());
        apply(
            &mut native,
            target_binding,
            mouse(
                target_binding,
                target,
                None,
                target_surface,
                PointerWindowRoute::None,
                Some(enabled),
            ),
            &receivers,
            false,
            &mut trace,
        );
        assert!(
            bindings
                .iter()
                .all(|binding| native.session.is_current_native_binding(*binding)),
            "{}",
            trace.replay()
        );
        assert!(
            !native.session.has_pending_native_input(),
            "{}",
            trace.replay()
        );
        assert_eq!(
            native.bridge.input_backlog_for_test(),
            (0, None),
            "{}",
            trace.replay()
        );
        assert!(
            native.pointer_translator.can_coalesce_idle_cursor_moves(),
            "{}",
            trace.replay()
        );
        assert_eq!(
            native
                .session
                .view()
                .item(ItemId::new(1))
                .unwrap()
                .surface(),
            FIRST_SURFACE
        );
        assert_eq!(
            native
                .session
                .view()
                .item(ItemId::new(4))
                .unwrap()
                .surface(),
            SECOND_SURFACE
        );
        assert_eq!(
            native
                .session
                .view()
                .item(ItemId::new(5))
                .unwrap()
                .surface(),
            THIRD_SURFACE,
            "{}: the third window keeps its anchor while other panes migrate",
            trace.replay(),
        );
    }
    assert_eq!(
        delivered, ROUNDS,
        "seed={seed:#x}: every round must commit a physical cross-window drop"
    );
    assert!(cancellations >= ROUNDS, "seed={seed:#x}");
    for (from, destinations) in directions.iter().enumerate() {
        for (to, count) in destinations.iter().enumerate() {
            assert!(
                from == to || *count > 0,
                "seed={seed:#x}: window {from} never docked into {to}"
            );
        }
    }
}

#[test]
fn seeded_native_cross_window_drags_recover_and_dock_between_all_three_windows() {
    let seeds = std::env::var("DOCKSPACE_TEST_SEED").map_or_else(
        |_| {
            (1_u64..=32)
                .map(|seed| seed.wrapping_mul(0x9e37_79b9_7f4a_7c15))
                .collect::<Vec<_>>()
        },
        |seed| {
            let value = seed
                .strip_prefix("0x")
                .map_or_else(|| seed.parse::<u64>(), |hex| u64::from_str_radix(hex, 16))
                .unwrap();
            assert_ne!(value, 0, "the xorshift generator requires a nonzero seed");
            vec![value]
        },
    );
    for seed in seeds {
        journey(seed);
    }
}

#[test]
fn a_replacement_mouse_press_in_another_window_cancels_the_old_source_and_docks_into_a_third() {
    let (mut native, bindings) = routes(false);
    let [first, second, third]: [NativeSurfaceBinding; 3] = bindings.try_into().unwrap();
    let (receivers, new_source, target) = scene(&mut native, THIRD_SURFACE);
    let mut frame = native
        .begin_host_frame(|_| NativeReceiverAnswer::Unknown)
        .unwrap();
    let plan = frame.frame.paint_plan(FIRST_SURFACE).unwrap().unwrap();
    let mut old_descriptor = None;
    plan.visit_surface_program(|step| {
        if let SurfacePaintProgramStep::Operation {
            operation: SurfacePaintProgramOperation::Tab(tab),
            ..
        } = step
            && tab.item() == ItemId::new(1)
        {
            old_descriptor = plan.receiver_for_tab_body(tab);
        }
        Ok::<(), ()>(())
    })
    .unwrap();
    let old_descriptor = old_descriptor.unwrap();
    finish(frame);
    let old_source =
        PhysicalPosition::new(old_descriptor.center().x(), old_descriptor.center().y());
    let old_receivers = RecordedSource {
        descriptor: old_descriptor,
        artifacts: receivers.artifacts.clone(),
    };
    let mut trace = Trace {
        seed: 0xa_b_c,
        round: 0,
        ordinal: 0,
        history: VecDeque::new(),
    };
    global_focus(&mut native, first, &mut trace);
    for (point, state, hover) in [
        (old_source, Some(ElementState::Pressed), FIRST_SURFACE),
        (new_source, None, SECOND_SURFACE),
    ] {
        apply(
            &mut native,
            first,
            mouse(
                first,
                point,
                state,
                hover,
                PointerWindowRoute::Window(window(FIRST_SURFACE)),
                None,
            ),
            &old_receivers,
            true,
            &mut trace,
        );
    }
    assert!(native.session.has_active_gesture());
    let version = native.session.version();
    global_focus(&mut native, second, &mut trace);
    let held = PointerWindowRoute::Window(window(SECOND_SURFACE));
    let (_, restarted) = apply(
        &mut native,
        second,
        mouse(
            second,
            new_source,
            Some(ElementState::Pressed),
            SECOND_SURFACE,
            held,
            None,
        ),
        &receivers,
        true,
        &mut trace,
    );
    assert!(
        restarted
            .manipulation_feedback()
            .iter()
            .any(|feedback| matches!(feedback.status(), Status::Cancelled(_))),
        "{}",
        trace.replay()
    );
    assert!(!restarted.workspace_changed());
    assert_eq!(native.session.version(), version);
    assert!(native.session.has_active_gesture());
    apply(
        &mut native,
        second,
        mouse(second, target, None, THIRD_SURFACE, held, None),
        &receivers,
        true,
        &mut trace,
    );
    global_focus(&mut native, third, &mut trace);
    let (_, delivered) = apply(
        &mut native,
        second,
        mouse(
            second,
            target,
            Some(ElementState::Released),
            THIRD_SURFACE,
            PointerWindowRoute::None,
            None,
        ),
        &receivers,
        true,
        &mut trace,
    );
    assert!(
        delivered.workspace_changed(),
        "{}: {:?}",
        trace.replay(),
        delivered.manipulation_feedback()
    );
    assert!(
        delivered
            .manipulation_feedback()
            .iter()
            .any(|feedback| feedback.status() == Status::Delivered(Delivery::Dock)),
        "{}",
        trace.replay()
    );
    assert_eq!(
        native.session.view().item(MOVING).unwrap().surface(),
        THIRD_SURFACE
    );
    assert_eq!(
        native
            .session
            .view()
            .item(ItemId::new(1))
            .unwrap()
            .surface(),
        FIRST_SURFACE
    );
    apply(
        &mut native,
        third,
        mouse(
            third,
            target,
            None,
            THIRD_SURFACE,
            PointerWindowRoute::None,
            None,
        ),
        &receivers,
        false,
        &mut trace,
    );
    assert!(!native.session.has_active_gesture());
    assert!(native.pointer_translator.can_coalesce_idle_cursor_moves());
    assert!(!native.session.has_pending_native_input());
}
