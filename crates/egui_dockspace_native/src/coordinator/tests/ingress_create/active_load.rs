//! Controlled 1 kHz native ingress, with real core program/presentation receipts.
//!
//! This model does not certify OS event timing, GPU presentation or real-time latency.
//! Native property effects enter their original adapter queue; platform dispatch
//! is deliberately outside this callback-to-core throughput qualification.

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use dockspace::model::{DockspaceContainedLayout, FloatingPresentationId};
use dockspace::runtime::{
    DockspaceManipulationDelivery, DockspaceManipulationFeedback,
    DockspaceManipulationPendingReason, DockspaceManipulationStatus, DockspaceReceiverDescriptor,
    NativeMonotonicClock, NativeReceiverPurpose, NativeReceiverQuery, SurfacePaintProgramOperation,
    SurfacePresentationArtifact,
};

use super::*;

const TARGET_ROOT: RootId = RootId::new(52);
const TARGET_ITEM: ItemId = ItemId::new(62);
const SIBLING_ITEM: ItemId = ItemId::new(63);
const MOVES: u64 = 10_000;
const CADENCE: u64 = 16;

#[derive(Clone, Copy)]
struct Receivers {
    source: DockspaceReceiverDescriptor,
    target: DockspaceReceiverDescriptor,
}

impl Receivers {
    fn resolve(self, query: NativeReceiverQuery) -> NativeReceiverAnswer {
        if matches!(query.purpose(), NativeReceiverPurpose::HoverHit) {
            let descriptor = if query
                .point()
                .is_some_and(|point| self.source.bounds().contains(point))
            {
                &self.source
            } else {
                &self.target
            };
            query.clear_hover_lane(descriptor).map_or(
                NativeReceiverAnswer::Unknown,
                NativeReceiverAnswer::ClearHoverLane,
            )
        } else {
            query
                .bind_receiver(&self.source)
                .map_or(NativeReceiverAnswer::Unknown, NativeReceiverAnswer::Dock)
        }
    }
}

fn fixture(clock: NativeMonotonicClock) -> (NativeCoordinator, Receivers) {
    let layout = DockspaceLayout::new([DockspaceSurfaceLayout::new(
        SOURCE_SURFACE,
        DockspaceRootLayout::new(TARGET_ROOT, DockspaceNode::central_tabs([TARGET_ITEM])),
    )
    .with_contained(DockspaceContainedLayout::new(
        FloatingPresentationId::new(1),
        DockspaceRootLayout::new(
            SOURCE_ROOT,
            DockspaceNode::tabs([SOURCE_ITEM, SIBLING_ITEM]),
        ),
        LogicalRect::new(20.0, 45.0, 180.0, 150.0).unwrap(),
    ))])
    .unwrap();
    let mut native = NativeCoordinator::new_with_combined_storage_limit_and_clock(
        DockspaceSession::from_layout(layout, DockPolicy::default()).unwrap(),
        NativePointerRoster::Exact(Vec::new()),
        super::super::owner_storage::combined_profile(),
        clock,
    )
    .unwrap();
    let binding = register_source(&mut native);
    publish_desktop_authority(&mut native, binding);
    let mut measured = native
        .begin_host_frame(|_| NativeReceiverAnswer::Unknown)
        .unwrap();
    measured
        .frame
        .measure_surface(
            SOURCE_SURFACE,
            UniformSurfaceMetrics::new(
                LogicalRect::new(0.0, 0.0, 640.0, 360.0).unwrap(),
                LogicalSize::new(0.0, 0.0).unwrap(),
                80.0,
            )
            .unwrap(),
        )
        .unwrap();
    measured
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .unwrap();
    measured.commit().unwrap();
    let mut painted = native
        .begin_host_frame(|_| NativeReceiverAnswer::Unknown)
        .unwrap();
    let plan = painted.frame.paint_plan(SOURCE_SURFACE).unwrap().unwrap();
    let source = plan.tab_receiver(SOURCE_ITEM).unwrap();
    let mut target = None;
    let receipt = plan
        .execute_surface_program(|step| {
            if let SurfacePaintProgramStep::Operation {
                operation: SurfacePaintProgramOperation::Pane(pane),
                ..
            } = step
                && pane.root() == TARGET_ROOT
            {
                target = plan.receiver_for_pane(pane);
            }
            Ok::<(), ()>(())
        })
        .unwrap();
    painted.frame.confirm_surface_painted(receipt).unwrap();
    painted
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .unwrap();
    let mut report = painted.commit().unwrap();
    assert!(report.take_native_effects().is_empty());
    let [output] = report.take_painted_outputs().try_into().unwrap();
    native
        .session
        .report_surface_presentation(output, SurfacePresentationResult::Presented)
        .unwrap();
    let mut accepted = native
        .begin_host_frame(|_| NativeReceiverAnswer::Unknown)
        .unwrap();
    accepted
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .unwrap();
    accepted.commit().unwrap();
    (
        native,
        Receivers {
            source,
            target: target.unwrap(),
        },
    )
}

fn capture(
    native: &NativeCoordinator,
    ordinal: u64,
    point: PhysicalPoint,
    button: Option<ElementState>,
) {
    let facts = pointer_facts(
        point,
        PointerWindowRoute::Window(source_window()),
        PointerWindowRoute::Window(source_window()),
    );
    let event = button.map_or_else(
        || WindowEvent::CursorMoved {
            device_id: DeviceId::dummy(),
            position: PhysicalPosition::new(point.x(), point.y()),
            facts,
        },
        |state| WindowEvent::MouseInput {
            device_id: DeviceId::dummy(),
            state,
            button: MouseButton::Left,
            facts,
        },
    );
    let record = native.bridge.with_viewports_mut(|routes| {
        NativeWindowEventRecord::for_test_ingress(
            ordinal,
            source_window(),
            Some(ViewportId::ROOT),
            event,
            routes,
        )
    });
    let captured = native.bridge.captured_through_for_test();
    native.bridge.push_record(HostRecord::WindowEvent(record));
    assert_eq!(
        native.bridge.captured_through_for_test(),
        captured + 1,
        "the model edge must be admitted"
    );
}

fn dispatch(
    native: &mut NativeCoordinator,
    receivers: Receivers,
    clock: &NativeMonotonicClock,
    expected_raw_records: usize,
    paint_release: bool,
    expected_settlement: Option<SurfacePresentationArtifact>,
) -> (
    Vec<DockspaceManipulationFeedback>,
    bool,
    Option<SurfacePresentationArtifact>,
) {
    let context = egui::Context::default();
    let mut budget = NativeInputBudget::new(clock);
    let mut feedback = Vec::new();
    let mut preview = false;
    let mut painted = None;
    let mut settled_outputs = 0;
    loop {
        let _ = native
            .reduce_callback_prefix(&context, &mut budget)
            .unwrap();
        let mut frame = native
            .poll_host_frame(&mut budget, |query| receivers.resolve(query))
            .unwrap()
            .expect("a 16-edge controlled slice fits the shared dispatch budget");
        if let Some(plan) = frame.frame.paint_plan(SOURCE_SURFACE).unwrap() {
            preview |= program_has_drag_preview(plan);
            if paint_release {
                let receipt = plan.execute_surface_program(|_| Ok::<(), ()>(())).unwrap();
                frame.frame.confirm_surface_painted(receipt).unwrap();
            }
        }
        frame
            .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
            .unwrap();
        let mut report = frame.commit().unwrap();
        assert!(
            !report
                .inputs()
                .contains(&HostInputOutcome::NativeInputCancelledForOverload)
        );
        assert!(!report.native_input_pending());
        if let Some(artifact) = expected_settlement {
            // Dock delivery replaces the preview scene, so the accepted release
            // artifact is settled without remaining current receiver authority.
            settled_outputs += report
                .surface_presentation_commits()
                .iter()
                .filter(|commit| commit.settles(artifact))
                .count();
        }
        feedback.extend_from_slice(report.manipulation_feedback());
        let effects = report.take_native_effects();
        assert!(
            effects.iter().all(|effect| matches!(
                effect.operation(),
                NativeEffectOperation::SetPointerPassthrough { .. }
            )),
            "a local model must not request native creation: {effects:?}"
        );
        native.accept_native_effects(&context, effects).unwrap();
        for output in report.take_painted_outputs() {
            assert!(paint_release);
            assert_eq!(output.surface(), SOURCE_SURFACE);
            assert!(painted.replace(output.presentation_artifact()).is_none());
            native
                .session
                .report_surface_presentation(output, SurfacePresentationResult::Presented)
                .unwrap();
        }
        native.settle_host_frame_report(&report).unwrap();
        if !native.bridge.has_pending_callback_records() {
            break;
        }
        assert!(
            !budget.is_exhausted(),
            "one cadence may not restart its budget to hide a yield"
        );
    }
    assert_eq!(painted.is_some(), paint_release);
    assert_eq!(settled_outputs, usize::from(expected_settlement.is_some()));
    let completed = 256 - budget.remaining_records();
    assert!(completed >= expected_raw_records + settled_outputs);
    assert!(completed <= CADENCE as usize + 4);
    (feedback, preview, painted)
}

fn sampled_owned_input_bytes(native: &NativeCoordinator) -> usize {
    let owned = native.owned_input_capacity_bytes().unwrap();
    let reserved = native.combined_storage_reserved_bytes().unwrap();
    assert!(owned <= reserved && reserved <= 4 * 1024 * 1024);
    owned
}

#[test]
fn controlled_one_kilohertz_active_drag_preserves_all_edges_and_delivers_release_within_budget() {
    let ticks = Arc::new(AtomicU64::new(0));
    let observed = Arc::clone(&ticks);
    let clock =
        NativeMonotonicClock::new(move || Duration::from_millis(observed.load(Ordering::SeqCst)));
    let (mut native, receivers) = fixture(clock.clone());
    let press = receivers.source.center();
    let press = PhysicalPoint::new(press.x(), press.y()).unwrap();
    let target = PhysicalPoint::new(400.0, 220.0).unwrap();
    let wall_started = Instant::now();
    let baseline = native.session.native_input_recorded_count();
    capture(&native, 1, press, Some(ElementState::Pressed));
    let (feedback, _, _) = dispatch(&mut native, receivers, &clock, 1, false, None);
    assert!(feedback.is_empty());
    assert!(!native.pointer_translator.can_coalesce_idle_cursor_moves());
    let mut peak_owned_input_bytes = sampled_owned_input_bytes(&native);
    let mut peak_records = 0;
    let mut peak_age = Duration::ZERO;
    for index in 1..=MOVES {
        ticks.store(index, Ordering::SeqCst);
        // Keep each edge distinct inside one non-guide target basin.
        let point =
            PhysicalPoint::new(target.x() + (index % 17) as f64 * 0.125, target.y()).unwrap();
        capture(&native, index + 1, point, None);
        let (records, age) = native.bridge.input_backlog_for_test();
        let expected_age = Duration::from_millis((index - 1) % CADENCE);
        assert_eq!(records, ((index - 1) % CADENCE + 1) as usize);
        assert_eq!(age, Some(expected_age));
        peak_records = peak_records.max(records);
        peak_age = peak_age.max(expected_age);
        peak_owned_input_bytes = peak_owned_input_bytes.max(sampled_owned_input_bytes(&native));
        if index % CADENCE == 0 || index == MOVES {
            let (feedback, preview, _) =
                dispatch(&mut native, receivers, &clock, records, false, None);
            assert!(
                feedback.is_empty(),
                "active input terminated early: {feedback:?}"
            );
            assert!(
                preview,
                "the accepted active prefix must retain a docking preview"
            );
            assert!(!native.pointer_translator.can_coalesce_idle_cursor_moves());
            assert!(native.session.has_active_gesture());
            assert_eq!(
                native.session.native_input_recorded_count(),
                baseline + index + 1,
                "active edges must never enter idle-motion coalescing"
            );
            peak_owned_input_bytes = peak_owned_input_bytes.max(sampled_owned_input_bytes(&native));
        }
    }
    ticks.store(MOVES + 1, Ordering::SeqCst);
    let accepted_release = clock.now();
    capture(&native, MOVES + 2, target, Some(ElementState::Released));
    peak_owned_input_bytes = peak_owned_input_bytes.max(sampled_owned_input_bytes(&native));
    ticks.store(MOVES + CADENCE, Ordering::SeqCst);
    let (feedback, _, output) = dispatch(&mut native, receivers, &clock, 1, true, None);
    peak_owned_input_bytes = peak_owned_input_bytes.max(sampled_owned_input_bytes(&native));
    let [pending] = feedback.as_slice() else {
        panic!("one release must wait for its exact output terminal: {feedback:?}");
    };
    assert_eq!(
        pending.status(),
        DockspaceManipulationStatus::Pending(
            DockspaceManipulationPendingReason::AwaitingPresentation
        )
    );
    assert_eq!(
        native.session.view().item(SOURCE_ITEM).unwrap().root(),
        SOURCE_ROOT,
        "the release frame may not mutate layout before exact output settlement"
    );
    ticks.store(MOVES + 2 * CADENCE, Ordering::SeqCst);
    let (settled, _, _) = dispatch(&mut native, receivers, &clock, 0, false, output);
    peak_owned_input_bytes = peak_owned_input_bytes.max(sampled_owned_input_bytes(&native));
    let [terminal] = settled.as_slice() else {
        panic!("the exact presentation must deliver one terminal result: {settled:?}");
    };
    assert_eq!(
        terminal.status(),
        DockspaceManipulationStatus::Delivered(DockspaceManipulationDelivery::Dock)
    );
    assert_eq!(pending.manipulation_id(), terminal.manipulation_id());
    assert!(clock.now() - accepted_release <= Duration::from_millis(100));
    assert_eq!(
        native.session.view().item(SOURCE_ITEM).unwrap().root(),
        TARGET_ROOT
    );
    assert_eq!(
        native.session.view().item(SIBLING_ITEM).unwrap().root(),
        SOURCE_ROOT
    );
    assert!(!native.session.has_pending_native_create());
    eprintln!(
        "controlled-1khz: moves={MOVES}, cadence={CADENCE}ms, release={}ms, peak_records={peak_records}, oldest={}ms, sampled_owned_input_bytes={peak_owned_input_bytes}, model_wall={:?}",
        (clock.now() - accepted_release).as_millis(),
        peak_age.as_millis(),
        wall_started.elapsed()
    );
}

#[test]
fn combined_pressure_preserves_an_accepted_release_and_reserved_terminals_through_retry() {
    use crate::mailbox::NativeDispatch;
    use eframe::{NativeHostHandler, NativeHostWake, NativeViewportPropertyCommandRetirement};

    // Constant model time isolates capacity rejection from the independent age/deadline caps.
    // Core painted outputs are real affine values; this fixture has no eframe output tokens.
    let clock = NativeMonotonicClock::new(|| Duration::ZERO);
    let (mut native, receivers) = fixture(clock.clone());
    let context = egui::Context::default();
    let binding = native.viewport_binding(ViewportId::ROOT).unwrap();
    let press = receivers.source.center();
    let press = PhysicalPoint::new(press.x(), press.y()).unwrap();
    let target = PhysicalPoint::new(400.0, 220.0).unwrap();
    capture(&native, 1, press, Some(ElementState::Pressed));
    assert!(
        dispatch(&mut native, receivers, &clock, 1, false, None)
            .0
            .is_empty()
    );
    capture(&native, 2, target, None);
    let (feedback, preview, _) = dispatch(&mut native, receivers, &clock, 1, false, None);
    assert!(feedback.is_empty() && preview && native.session.has_active_gesture());
    assert!(!native.pointer_translator.can_coalesce_idle_cursor_moves());

    // Reserve actual fork-issued command identities, without dispatching an OS operation.
    let focus = eframe::prepare_native_viewport_focus(ViewportId::ROOT);
    let visibility = eframe::prepare_native_viewport_visibility(ViewportId::ROOT, true);
    assert!(native.bridge.reserve_focus_callback(
        ViewportId::ROOT,
        binding,
        source_window(),
        focus.token(),
    ));
    assert!(native.bridge.reserve_visibility_callback(
        ViewportId::ROOT,
        binding,
        source_window(),
        visibility.token(),
        true,
    ));
    let successor_focus = eframe::prepare_native_viewport_focus(ViewportId::ROOT);
    let successor_visibility = eframe::prepare_native_viewport_visibility(ViewportId::ROOT, false);
    assert!(!native.bridge.reserve_focus_callback(
        ViewportId::ROOT,
        binding,
        source_window(),
        successor_focus.token(),
    ));
    assert!(!native.bridge.reserve_visibility_callback(
        ViewportId::ROOT,
        binding,
        source_window(),
        successor_visibility.token(),
        false,
    ));
    drop((successor_focus, successor_visibility));
    assert!(native.bridge.has_pending_coordinator_work());
    assert_eq!(
        native
            .bridge
            .with_viewports(|routes| routes.bindings().count()),
        1
    );

    let bounds = PhysicalRect::new(0.0, 0.0, 640.0, 360.0).unwrap();
    let display = PhysicalRect::new(0.0, 0.0, 1920.0, 1080.0).unwrap();
    let work = PhysicalRect::new(0.0, 0.0, 1920.0, 1040.0).unwrap();
    let scale = ScaleFactor::new(1.0).unwrap();
    native.bridge.invalidate_viewport_roster();
    let before_roster = native.bridge.captured_through_for_test();
    let roster = NativeViewportRosterRecord::for_test_with_backend_and_work_areas(
        [ready_window_observation_with_input(
            binding,
            bounds,
            scale,
            NativeWindowInputState::ReceivesInput,
            None,
        )],
        FrozenWorkAreaRoster::for_test_exact([(1, display, work, scale)]),
        eframe::NativeWindowingBackend::X11,
    );
    native.bridge.push_viewport_roster(roster.clone());
    assert_eq!(native.bridge.captured_through_for_test(), before_roster + 1);
    assert_eq!(
        native
            .bridge
            .front_viewport_roster()
            .unwrap()
            .roster()
            .observation_count(),
        1
    );
    for ordinal in 3..=6 {
        capture(
            &native,
            ordinal,
            PhysicalPoint::new(target.x() + ordinal as f64 * 0.125, target.y()).unwrap(),
            None,
        );
    }
    capture(&native, 7, target, Some(ElementState::Released));
    let release_through = native.bridge.captured_through_for_test();
    let recorded = native.session.native_input_recorded_count();
    let version = native.session.version();
    let mut peak_owned = sampled_owned_input_bytes(&native);
    let mut rejected = None;
    for ordinal in 8..=4_104 {
        let point =
            PhysicalPoint::new(target.x() + (ordinal % 17) as f64 * 0.125, target.y()).unwrap();
        let facts = pointer_facts(
            point,
            PointerWindowRoute::Window(source_window()),
            PointerWindowRoute::Window(source_window()),
        );
        let record = native.bridge.with_viewports_mut(|routes| {
            NativeWindowEventRecord::for_test_ingress(
                ordinal,
                source_window(),
                Some(ViewportId::ROOT),
                WindowEvent::CursorMoved {
                    device_id: DeviceId::dummy(),
                    position: PhysicalPosition::new(point.x(), point.y()),
                    facts,
                },
                routes,
            )
        });
        let before = native.bridge.captured_through_for_test();
        native
            .bridge
            .push_record(HostRecord::WindowEvent(record.clone()));
        peak_owned = peak_owned.max(sampled_owned_input_bytes(&native));
        if native.bridge.captured_through_for_test() == before {
            rejected = Some(record);
            break;
        }
    }
    let rejected = rejected.expect("the unchanged combined allowance must reject ordinary ingress");
    let boundary = native.bridge.captured_through_for_test();
    assert!(
        boundary > release_through,
        "pressure must include accepted post-release records"
    );
    let (retained, age) = native.bridge.input_backlog_for_test();
    assert!(retained > 6 && retained <= 4_096);
    assert_eq!(age, Some(Duration::ZERO));
    assert_eq!(native.session.native_input_recorded_count(), recorded);
    assert_eq!(native.session.version(), version);
    assert!(native.session.has_active_gesture());
    assert_eq!(
        native.bridge.admission_dispatch(),
        NativeDispatch::ContinueBoundary
    );
    native.bridge.push_record(HostRecord::WindowEvent(rejected));
    assert_eq!(native.bridge.captured_through_for_test(), boundary);

    // Protected receipts still accept their exact terminal after ordinary capacity freezes.
    let retirements = [
        NativeViewportPropertyCommandRetirement::Focus {
            viewport_id: ViewportId::ROOT,
            token: focus.token(),
        },
        NativeViewportPropertyCommandRetirement::Visibility {
            viewport_id: ViewportId::ROOT,
            token: visibility.token(),
            visible: true,
        },
    ];
    for retirement in retirements {
        assert_eq!(
            NativeHostHandler::on_viewport_property_command_retired(
                native.bridge.as_ref(),
                retirement,
            ),
            NativeHostWake::RepaintRootAfterCurrent
        );
    }
    assert_eq!(native.bridge.captured_through_for_test(), boundary + 2);

    let mut feedback = Vec::new();
    let mut dropped_release = false;
    let mut cancellation_count = 0;
    let mut completed = 0;
    let mut pointer_baseline = None;
    let mut artifact = None;
    let mut settled_outputs = 0;
    let mut terminal_roster_queued = false;
    // One bounded drain for this test: each pass shares its budget from prelude through core poll.
    for _ in 0..128 {
        let mut budget = NativeInputBudget::new(&clock);
        native.try_report_pending_effect_results().unwrap();
        let release = native.bridge.front_event().is_some_and(|event| {
            matches!(
                event.event(),
                WindowEvent::MouseInput {
                    state: ElementState::Released,
                    ..
                }
            )
        });
        native
            .reduce_callback_prefix(&context, &mut budget)
            .unwrap();
        if native.queued_snapshot.is_some() {
            pointer_baseline = Some(native.session.native_input_recorded_count());
        }
        let retained_input = native.session.native_input_recorded_count();
        let version = native.session.version();
        let Some(mut frame) = native
            .poll_host_frame(&mut budget, |query| receivers.resolve(query))
            .unwrap()
        else {
            assert!(budget.is_exhausted());
            completed += 256 - budget.remaining_records();
            continue;
        };
        if release {
            assert!(!dropped_release);
            frame
                .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
                .unwrap();
            drop(frame);
            assert_eq!(native.session.version(), version);
            assert_eq!(native.session.native_input_recorded_count(), retained_input);
            assert_eq!(
                retained_input - pointer_baseline.unwrap(),
                5,
                "all four active moves and the accepted release must reach the recorder"
            );
            assert!(native.bridge.callback_boundary_pending());
            frame = native
                .poll_host_frame(&mut budget, |query| receivers.resolve(query))
                .unwrap()
                .unwrap();
            let plan = frame.frame.paint_plan(SOURCE_SURFACE).unwrap().unwrap();
            let receipt = plan.execute_surface_program(|_| Ok::<(), ()>(())).unwrap();
            frame.frame.confirm_surface_painted(receipt).unwrap();
            dropped_release = true;
        }
        frame
            .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
            .unwrap();
        let mut report = frame.commit().unwrap();
        cancellation_count += report
            .inputs()
            .iter()
            .filter(|input| matches!(input, HostInputOutcome::NativeInputCancelledForOverload))
            .count();
        if let Some(artifact) = artifact {
            settled_outputs += report
                .surface_presentation_commits()
                .iter()
                .filter(|commit| commit.settles(artifact))
                .count();
        }
        for outcome in report.manipulation_feedback() {
            if matches!(
                outcome.status(),
                DockspaceManipulationStatus::Delivered(DockspaceManipulationDelivery::Dock)
            ) {
                assert!(artifact.is_some());
                assert_eq!(
                    settled_outputs, 1,
                    "delivery must follow the exact output settlement"
                );
            }
        }
        if settled_outputs == 0 {
            assert_eq!(
                native.session.view().item(SOURCE_ITEM).unwrap().root(),
                SOURCE_ROOT,
                "accepted release cannot publish ownership before exact presentation"
            );
        }
        feedback.extend_from_slice(report.manipulation_feedback());
        native
            .accept_native_effects(&context, report.take_native_effects())
            .unwrap();
        for output in report.take_painted_outputs() {
            assert!(release && artifact.is_none());
            artifact = Some(output.presentation_artifact());
            native
                .session
                .report_surface_presentation(output, SurfacePresentationResult::Presented)
                .unwrap();
        }
        native.settle_host_frame_report(&report).unwrap();
        // No platform dispatch is claimed: return queued input effects through their failure path.
        if native.bridge.permits_unscheduled_input()
            && let Some(command) = native.pointer_passthrough_command()
        {
            assert!(native.fail_pointer_passthrough_dispatch(command));
        }
        completed += 256 - budget.remaining_records();
        peak_owned = peak_owned.max(sampled_owned_input_bytes(&native));
        if cancellation_count == 1
            && !terminal_roster_queued
            && !native.bridge.has_pending_callback_records()
            && !native.session.has_pending_native_input()
            && native.pending_effect_results.is_empty()
            && !native.input_control.has_pending_work()
        {
            native.bridge.push_terminal_viewport_roster(roster.clone());
            terminal_roster_queued = true;
        }
        if terminal_roster_queued
            && native.is_quiescent()
            && !native.session.has_pending_native_input()
        {
            break;
        }
    }
    assert!(dropped_release && artifact.is_some());
    assert!(terminal_roster_queued);
    assert_eq!(settled_outputs, 1);
    let [pending, terminal] = feedback.as_slice() else {
        panic!("one accepted release must publish one pending and one terminal: {feedback:?}");
    };
    assert_eq!(
        pending.status(),
        DockspaceManipulationStatus::Pending(
            DockspaceManipulationPendingReason::AwaitingPresentation
        )
    );
    assert_eq!(
        terminal.status(),
        DockspaceManipulationStatus::Delivered(DockspaceManipulationDelivery::Dock)
    );
    assert_eq!(pending.manipulation_id(), terminal.manipulation_id());
    assert_eq!(
        native.session.view().item(SOURCE_ITEM).unwrap().root(),
        TARGET_ROOT
    );
    assert_eq!(
        native.session.view().item(SIBLING_ITEM).unwrap().root(),
        SOURCE_ROOT
    );
    assert_eq!(cancellation_count, 1);
    assert!(completed >= retained + retirements.len());
    assert_eq!(native.bridge.captured_through_for_test(), boundary + 3);
    assert_eq!(native.bridge.input_backlog_for_test(), (0, None));
    assert_eq!(native.bridge.admission_dispatch(), NativeDispatch::Idle);
    for retirement in retirements {
        assert_eq!(
            NativeHostHandler::on_viewport_property_command_retired(
                native.bridge.as_ref(),
                retirement,
            ),
            NativeHostWake::Wait
        );
    }
    assert!(!native.session.has_active_gesture());
    assert!(!native.session.has_pending_native_input());
    assert!(native.queued_snapshot.is_none() && native.pending_effect_results.is_empty());
    assert!(
        native.is_quiescent(),
        "the bounded drain must release every pending owner"
    );
    eprintln!(
        "combined-pressure: retained={retained}, command_receipts=2, roster_windows=1, completed={completed}, sampled_owned_input_bytes={peak_owned}"
    );
}
