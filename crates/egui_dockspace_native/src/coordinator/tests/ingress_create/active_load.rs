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
