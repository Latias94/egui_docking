//! Controlled elapsed-budget service with independent release, close and bootstrap owners.
//!
//! Release feedback is observed through a public core continuation sharing the
//! service turn's budget. This is neither service-only terminal observation nor
//! an OS/GPU latency or CPU-time qualification.

use std::num::NonZeroUsize;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use dockspace::geometry::{LogicalPoint, LogicalRect, LogicalSize, PhysicalSize};
use dockspace::runtime::{
    DockspaceManipulationDelivery, DockspaceManipulationFeedback, DockspaceManipulationStatus,
    DockspaceReceiverDescriptor, NativeCreateReadiness, NativeEffectAcknowledgement,
    NativeEffectOperation, NativeInputStorageLimit, NativeLocalGeometry,
    NativeLocalPointerLocation, NativeProducerStorageLimit, NativeReceiverPurpose,
    NativeReceiverQuery, NativeReportStorageLimit, NativeSurfaceBinding, NativeWindowBinding,
    NativeWindowHit, PaintedSurfaceOutput, SurfacePaintProgramOperation, SurfacePaintProgramStep,
    SurfacePresentationArtifact, SurfacePresentationResult, UniformSurfaceMetrics,
};
use winit::dpi::{PhysicalPosition, PhysicalSize as WinitPhysicalSize};
use winit::event::{
    DeviceId, ElementState, MouseButton, PointerEventFacts, PointerEventGeometry,
    PointerWindowRoute, WindowEvent,
};

use super::*;
use crate::coordinator::NativeCoordinator;
use crate::event::{CapturedNativePointerLocation, NativeWindowEventRecord};
use crate::host_frame::NativeHostFrame;
use crate::mailbox::{HostRecord, NativeDispatch};
use crate::surface_driver::ROOT_WINDOW_TOKEN;

const SOURCE: SurfaceId = OTHER_SURFACE;
const SOURCE_ROOT: RootId = RootId::new(11);
const WAITING: SurfaceId = SurfaceId::new(3);
const WAITING_ROOT: RootId = RootId::new(12);
const WAITING_ITEM: ItemId = ItemId::new(4);
const CADENCE_RECORDS: usize = 16;
const INPUT_TURNS: usize = 5;
const DRAIN_TURNS: usize = 4;
const TRANSLATION_NANOS: u64 = 3_000_000;

fn source_viewport() -> egui::ViewportId {
    egui::ViewportId::from_hash_of("elapsed-service-source")
}

fn source_window() -> WindowId {
    WindowId::from(92)
}

fn profile_session(candidate: DockspaceLayout) -> DockspaceSession {
    let mut bootstrap = DockspaceDocumentBootstrap::new(DOCUMENT);
    for (key, item) in [
        ("pane:first", FIRST),
        ("pane:second", SECOND),
        ("pane:close", THIRD),
        ("pane:waiting", WAITING_ITEM),
    ] {
        assert_eq!(bootstrap.ensure_item(key).unwrap(), item);
    }
    let mut policy = DockPolicy::default();
    policy.set_allow_native_surfaces(true);
    DockspaceSession::from_persistent_layout(candidate, policy, bootstrap).unwrap()
}

fn saved_three_domain_document() -> Vec<u8> {
    let candidate = DockspaceLayout::new([
        surface(SURFACE, ROOT, &[THIRD], THIRD),
        surface(SOURCE, SOURCE_ROOT, &[FIRST, SECOND], FIRST),
        surface(WAITING, WAITING_ROOT, &[WAITING_ITEM], WAITING_ITEM),
    ])
    .unwrap();
    let mut session = profile_session(candidate);
    session
        .enable_managed_native_host(NativePointerRoster::Exact(Vec::new()))
        .unwrap();
    session
        .configure_managed_native_capabilities(crate::capabilities::capabilities_for_backend(
            NativeWindowingBackend::Windows,
        ))
        .unwrap();
    session
        .register_native_root(SURFACE, HostWindowToken::new(11))
        .unwrap();
    commit_session_prefix(&mut session);
    for (surface, token) in [(SOURCE, 22), (WAITING, 33)] {
        session
            .register_owned_native_child(surface, HostWindowToken::new(token), SURFACE)
            .unwrap();
    }
    let registered = commit_session_prefix(&mut session);
    assert_eq!(registered.native_bindings().len(), 3);
    session
        .report_managed_native_snapshot(
            registered.native_bindings().iter().copied().map(|binding| {
                let x = match binding.surface() {
                    SOURCE => 720.0,
                    WAITING => 1440.0,
                    _ => 0.0,
                };
                (binding, ready_window_facts(x))
            }),
            NativeWorkAreaRoster::Unknown,
        )
        .unwrap();
    commit_session_prefix(&mut session);
    session.save_document_json().unwrap()
}

fn finish(mut frame: NativeHostFrame<'_>) -> HostFrameReport {
    frame
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .unwrap();
    frame.commit().unwrap()
}

fn observe_source(
    state: &mut NativeRuntimeState<Panes>,
    root: NativeSurfaceBinding,
    source: NativeSurfaceBinding,
    facts: NativeWindowFacts,
) {
    state
        .coordinator
        .report_snapshot(
            [(root, ready_window_facts(0.0)), (source, facts)],
            NativeWorkAreaRoster::Unknown,
            crate::capabilities::capabilities_for_backend(NativeWindowingBackend::Windows),
        )
        .unwrap();
    commit_native_prefix(state);
}

fn establish_source(state: &mut NativeRuntimeState<Panes>) -> NativeSurfaceBinding {
    let root = state
        .coordinator
        .viewport_binding(egui::ViewportId::ROOT)
        .unwrap();
    let mut frame = state
        .coordinator
        .begin_host_frame(|_| NativeReceiverAnswer::Unknown)
        .unwrap();
    assert!(frame.frame.advance_next_native_surface_create().unwrap());
    let mut report = finish(frame);
    let [create] = report.take_native_effects().try_into().unwrap();
    let NativeEffectOperation::CreateWindow { binding, .. } = create.operation() else {
        panic!("the first waiting bootstrap must emit a real CreateWindow");
    };
    let source = *binding;
    assert_eq!(source.surface(), SOURCE);
    let Some(NativeEffectAcknowledgement::Create(created)) = create.accepted() else {
        panic!("the source bootstrap must supply its exact create acknowledgement");
    };
    // The model host owns preparation and publishes readiness on its exact route.
    state
        .coordinator
        .bind_viewport(source_viewport(), source_window(), source)
        .unwrap();
    // Creation can settle before renderer readiness. These public model facts
    // carry no eframe output token; the other bootstrap stays queued throughout.
    observe_source(
        state,
        root,
        source,
        ready_window_facts(720.0)
            .with_create_readiness(NativeCreateReadiness::Created, Some(created))
            .with_presentation(NativeWindowPresentationState::Hidden),
    );
    observe_source(
        state,
        root,
        source,
        ready_window_facts(720.0).with_create_readiness(NativeCreateReadiness::Prepared, None),
    );
    source
}

struct Scene {
    tabs: Vec<(ItemId, DockspaceReceiverDescriptor, LogicalRect)>,
    presentation: Option<(NativeWindowBinding, SurfacePresentationArtifact)>,
}

impl Scene {
    fn point(&self, item: ItemId, fraction: f64) -> LogicalPoint {
        let (_, receiver, visible) = self.tabs.iter().find(|tab| tab.0 == item).unwrap();
        let point = LogicalPoint::new(
            visible.x() + visible.width() * fraction,
            visible.y() + visible.height() * 0.5,
        )
        .unwrap();
        assert!(receiver.bounds().contains(point));
        point
    }

    fn resolve(&self, query: NativeReceiverQuery) -> NativeReceiverAnswer {
        let query = match query {
            NativeReceiverQuery::WindowHit(query) => {
                return NativeReceiverAnswer::WindowHit(
                    self.presentation
                        .filter(|(window, _)| *window == query.window())
                        .map_or(NativeWindowHit::Unknown, |(_, artifact)| {
                            NativeWindowHit::Surface(artifact)
                        }),
                );
            }
            NativeReceiverQuery::Surface(query) => query,
        };
        assert_eq!(query.surface(), SOURCE);
        let point = query.point().unwrap();
        let Some((_, receiver, _)) = self.tabs.iter().find(|tab| tab.1.bounds().contains(point))
        else {
            return NativeReceiverAnswer::Unknown;
        };
        if matches!(query.purpose(), NativeReceiverPurpose::HoverHit) {
            query.clear_hover_lane(receiver).map_or(
                NativeReceiverAnswer::Unknown,
                NativeReceiverAnswer::ClearHoverLane,
            )
        } else {
            query
                .bind_receiver(receiver)
                .map_or(NativeReceiverAnswer::Unknown, NativeReceiverAnswer::Dock)
        }
    }
}

fn paint(frame: &mut NativeHostFrame<'_>) -> Scene {
    if frame.frame.paint_plan(SOURCE).unwrap().is_none() {
        frame
            .frame
            .measure_surface(
                SOURCE,
                UniformSurfaceMetrics::new(
                    LogicalRect::new(0.0, 0.0, 640.0, 480.0).unwrap(),
                    LogicalSize::new(32.0, 24.0).unwrap(),
                    96.0,
                )
                .unwrap(),
            )
            .unwrap();
    }
    let plan = frame.frame.paint_plan(SOURCE).unwrap().unwrap();
    let mut tabs = Vec::new();
    let receipt = plan
        .execute_surface_program(|step| {
            if let SurfacePaintProgramStep::Operation {
                operation: SurfacePaintProgramOperation::Tab(tab),
                ..
            } = step
                && let Some(receiver) = plan.receiver_for_tab_body(tab)
            {
                tabs.push((tab.item(), receiver, tab.visible_bounds()));
            }
            Ok::<(), ()>(())
        })
        .unwrap();
    frame.frame.confirm_surface_painted(receipt).unwrap();
    assert_eq!(tabs.len(), 2);
    Scene {
        tabs,
        presentation: None,
    }
}

fn present_source(state: &mut NativeRuntimeState<Panes>, source: NativeSurfaceBinding) -> Scene {
    let mut frame = state
        .coordinator
        .begin_host_frame(|_| NativeReceiverAnswer::Unknown)
        .unwrap();
    frame
        .frame
        .measure_surface(
            SOURCE,
            UniformSurfaceMetrics::new(
                LogicalRect::new(0.0, 0.0, 640.0, 480.0).unwrap(),
                LogicalSize::new(32.0, 24.0).unwrap(),
                96.0,
            )
            .unwrap(),
        )
        .unwrap();
    let mut scene = paint(&mut frame);
    let mut report = finish(frame);
    let [output] = report.take_painted_outputs().try_into().unwrap();
    assert!(output.matches_native_binding(source));
    scene.presentation = Some((source.window(), output.presentation_artifact()));
    state
        .coordinator
        .report_model_presentation_for_test(output, SurfacePresentationResult::Presented)
        .unwrap();
    let report = commit_native_prefix(state);
    assert_eq!(report.native_admissions().len(), 1);
    assert!(
        state
            .coordinator
            .session()
            .presented_surface(SOURCE)
            .is_some()
    );
    scene
}

fn capture(
    state: &NativeRuntimeState<Panes>,
    source: NativeSurfaceBinding,
    ordinal: u64,
    point: LogicalPoint,
    button: Option<ElementState>,
) {
    let coordinates = state
        .coordinator
        .session()
        .native_local_coordinates(source)
        .unwrap();
    let geometry = NativeLocalGeometry::new(
        PhysicalSize::new(640.0, 480.0).unwrap(),
        ScaleFactor::new(1.0).unwrap(),
    );
    let position = PhysicalPosition::new(point.x(), point.y());
    let facts = PointerEventFacts {
        surface_position: Some(position),
        geometry: Some(PointerEventGeometry {
            surface_size: WinitPhysicalSize::new(640, 480),
            scale_factor: 1.0,
        }),
        capture: if button == Some(ElementState::Released)
            || button.is_none() && !state.coordinator.session().has_active_gesture()
        {
            PointerWindowRoute::None
        } else {
            PointerWindowRoute::Window(source_window())
        },
        ..PointerEventFacts::default()
    };
    let event = match button {
        Some(state) => WindowEvent::MouseInput {
            primary_press: None,
            device_id: DeviceId::dummy(),
            state,
            button: MouseButton::Left,
            facts,
        },
        None => WindowEvent::CursorMoved {
            device_id: DeviceId::dummy(),
            position,
            facts,
        },
    };
    let bridge = state.coordinator.dispatch_bridge();
    let record = bridge.with_viewports(|routes| {
        NativeWindowEventRecord::for_test_local_capture(
            ordinal,
            source_window(),
            Some(source_viewport()),
            event,
            routes,
            Some(coordinates),
            Some(geometry),
        )
    });
    assert!(matches!(
        record.pointer_location(),
        Some(CapturedNativePointerLocation::NativeLocal(NativeLocalPointerLocation::Exact(sample)))
            if sample.client_position()
                == dockspace::geometry::PhysicalPoint::new(point.x(), point.y()).unwrap()
    ));
    assert_eq!(record.binding(), Some(source));
    let captured = bridge.captured_through_for_test();
    bridge.push_record(HostRecord::WindowEvent(record));
    assert_eq!(bridge.captured_through_for_test(), captured + 1);
}

fn accepted_release(
    state: &mut NativeRuntimeState<Panes>,
    source: NativeSurfaceBinding,
    scene: &Scene,
    clock: &NativeMonotonicClock,
) -> (DockspaceManipulationFeedback, PaintedSurfaceOutput) {
    let press = scene.point(FIRST, 0.25);
    let target = scene.point(SECOND, 0.625);
    let context = egui::Context::default();
    let version = state.coordinator.session().version();
    for (ordinal, point, button) in [
        (2, press, Some(ElementState::Pressed)),
        (3, target, None),
        (4, target, Some(ElementState::Released)),
    ] {
        capture(state, source, ordinal, point, button);
        assert_eq!(
            state
                .coordinator
                .reduce_callback_prefix(&context, &mut NativeInputBudget::new(clock))
                .unwrap()
                .unwrap()
                .get(),
            1
        );
        let mut frame = state
            .coordinator
            .begin_host_frame(|query| scene.resolve(query))
            .unwrap();
        if button == Some(ElementState::Released) {
            paint(&mut frame);
            let mut report = finish(frame);
            let [terminal] = report.manipulation_feedback() else {
                panic!("one accepted release must publish exactly one terminal: {report:?}");
            };
            let terminal = *terminal;
            assert_eq!(
                terminal.status(),
                DockspaceManipulationStatus::Delivered(DockspaceManipulationDelivery::Dock)
            );
            assert!(report.workspace_changed());
            assert_ne!(state.coordinator.session().version(), version);
            let [output] = report.take_painted_outputs().try_into().unwrap();
            assert!(output.matches_native_binding(source));
            state.coordinator.settle_host_frame_report(&report).unwrap();
            assert!(!state.coordinator.session().has_active_gesture());
            assert!(!state.coordinator.session().has_pending_presentation());
            let view = state.coordinator.session().view();
            assert_eq!(view.item(FIRST).unwrap().tabs().items(), [SECOND, FIRST]);
            assert_eq!(view.item(FIRST).unwrap().surface(), SOURCE);
            return (terminal, output);
        }
        let report = finish(frame);
        assert!(report.manipulation_feedback().is_empty());
        state.coordinator.settle_host_frame_report(&report).unwrap();
    }
    unreachable!("the release must return its affine output")
}

#[test]
fn elapsed_service_refill_preserves_release_close_and_ready_bootstrap_owners() {
    let ticks = Arc::new(AtomicU64::new(0));
    let observed = Arc::clone(&ticks);
    let clock =
        NativeMonotonicClock::new(move || Duration::from_nanos(observed.load(Ordering::SeqCst)));
    let slots = |count| NonZeroUsize::new(count).unwrap();
    let input = NativeInputStorageLimit::new(slots(128), 512 * 1024)
        .with_producer_storage_limit(NativeProducerStorageLimit::new(
            slots(8),
            slots(1),
            128 * 1024,
        ))
        .with_report_storage_limit(NativeReportStorageLimit::new(
            slots(64),
            slots(8),
            256 * 1024,
        ));
    let storage = crate::NativeStorageLimit::new(input, 4 * 1024 * 1024, 8, 4).unwrap();
    let mut coordinator = NativeCoordinator::new_with_combined_storage_limit_and_clock(
        profile_session(layout(ROOT, &[THIRD], THIRD)),
        NativePointerRoster::Exact(Vec::new()),
        storage.combined,
        clock.clone(),
    )
    .unwrap();
    coordinator
        .register_native_root(SURFACE, ROOT_WINDOW_TOKEN)
        .unwrap();
    let mut state = NativeRuntimeState {
        coordinator,
        root_surface: SURFACE,
        instance_id: egui::Id::unique("elapsed-service-three-owners"),
        owned_update: None,
        root_placement_request: None,
        root_placement_cache: None,
        root_input_placements: None,
        root_placements_explicit: false,
        invalid_root_placement_request: false,
        pending_service: false,
        style: DockStyle::default(),
        panes: Panes(Vec::new()),
        application_actions: Default::default(),
        pass_actions: Default::default(),
        close_policy: NativeWindowClosePolicy::CloseContent,
        root_context: None,
        shutdown: None,
    };
    let context = egui::Context::default();
    let registered = commit_native_prefix(&mut state);
    state
        .coordinator
        .bind_viewport(
            egui::ViewportId::ROOT,
            WindowId::from(91),
            registered.native_bindings()[0],
        )
        .unwrap();
    state
        .request_document_restore_json(&saved_three_domain_document(), |document, key| {
            document == DOCUMENT
                && matches!(
                    key,
                    "pane:first" | "pane:second" | "pane:close" | "pane:waiting"
                )
        })
        .unwrap();
    let mut restored = commit_queued(&mut state, None);
    assert!(matches!(
        take(&mut state),
        NativeActionStatus::DocumentRestored { .. }
    ));
    assert!(restored.take_native_effects().is_empty());
    assert!(
        state
            .service_pending_prefixes(&context, None, SURFACE, &mut NativeInputBudget::new(&clock),)
            .unwrap()
    );
    record_fresh_root_facts(&mut state);
    commit_native_prefix(&mut state);
    let source = establish_source(&mut state);
    let scene = present_source(&mut state, source);
    let root = state
        .coordinator
        .viewport_binding(egui::ViewportId::ROOT)
        .unwrap();
    let bridge = state.coordinator.dispatch_bridge();
    // The independent root uses the existing tokenless close model: policy may
    // advance, but Allow cannot establish OS destruction or remove its content.
    bridge.push_record(HostRecord::WindowEvent(NativeWindowEventRecord::for_test(
        1,
        WindowId::from(91),
        Some(egui::ViewportId::ROOT),
        Some(root),
        WindowEvent::CloseRequested,
    )));
    assert!(
        state
            .coordinator
            .reduce_callback_prefix(&context, &mut NativeInputBudget::new(&clock),)
            .unwrap()
            .is_some()
    );
    let report = commit_native_prefix(&mut state);
    state
        .coordinator
        .settle_close_control_report(&report)
        .unwrap();
    let close = bridge.with_close_control(|control| {
        let mut pending = control.unsubmitted_requests();
        let close = pending.next().unwrap();
        assert!(pending.next().is_none());
        assert!(control.held_request(close).is_none());
        close
    });
    assert_eq!(close.binding(), root);
    let accepted_at = clock.now();
    let (delivered, output) = accepted_release(&mut state, source, &scene, &clock);
    let release_elapsed = clock.now() - accepted_at;
    assert!(release_elapsed <= Duration::from_millis(100));
    let artifact = output.presentation_artifact();
    let observed = Arc::new(std::sync::Mutex::new((1_usize, 0_usize)));
    let observer = Arc::clone(&observed);
    state.coordinator.observe_reports_for_test(move |report| {
        let mut counts = observer.lock().unwrap();
        for feedback in report
            .manipulation_feedback()
            .iter()
            .filter(|feedback| feedback.manipulation_id() == delivered.manipulation_id())
        {
            assert_eq!(
                feedback.status(),
                DockspaceManipulationStatus::Delivered(DockspaceManipulationDelivery::Dock)
            );
            counts.0 += 1;
        }
        counts.1 += report
            .surface_presentation_commits()
            .iter()
            .filter(|commit| commit.settles(artifact))
            .count();
        assert!(
            counts.0 <= 1 && counts.1 <= 1,
            "a later service report repeated release settlement: {counts:?}"
        );
    });
    let mut output = Some(output);
    {
        let mut probe = state
            .coordinator
            .begin_host_frame(|_| NativeReceiverAnswer::Unknown)
            .unwrap();
        assert!(probe.frame.advance_next_native_surface_create().unwrap());
        // Dropping this candidate proves readiness without claiming the owner.
    }
    assert!(state.coordinator.deferred_viewport_specs().is_empty());
    assert!(bridge.with_close_control(|control| control.has_runnable_policy_work()));
    assert!(state.coordinator.session().has_pending_native_create());
    assert_eq!(bridge.input_backlog_for_test(), (0, None));
    state
        .coordinator
        .set_translation_clock_cost_for_test(Arc::clone(&ticks), TRANSLATION_NANOS);

    let mut ordinal = 4;
    let mut exhausted_turns = 0;
    let mut close_turn = None;
    let mut create_turn = None;
    let mut child = None;
    let mut settlements = 0;
    let captured_before_refill = bridge.captured_through_for_test();
    for turn in 0..INPUT_TURNS + DRAIN_TURNS {
        if turn < INPUT_TURNS {
            for _ in 0..CADENCE_RECORDS {
                ticks.fetch_add(1_000_000, Ordering::SeqCst);
                ordinal += 1;
                capture(
                    &state,
                    source,
                    ordinal,
                    LogicalPoint::new(300.0, 200.0).unwrap(),
                    None,
                );
            }
        }
        let captured = bridge.captured_through_for_test();
        let mut budget = NativeInputBudget::new(&clock);
        if turn == 1 {
            state
                .coordinator
                .report_model_presentation_for_test(
                    output.take().unwrap(),
                    SurfacePresentationResult::Presented,
                )
                .unwrap();
            // Inspect the exact terminal via public core polling and the real
            // driver's report settlement. Both frames share this turn's budget
            // and the retained raw barrier; no renderer token is synthesized.
            for _ in 0..2 {
                let frame = state
                    .coordinator
                    .poll_host_frame(&mut budget, |query| scene.resolve(query))
                    .unwrap()
                    .expect("the retained idle prefix and exact output fit one fresh turn");
                let report = finish(frame);
                settlements += report
                    .surface_presentation_commits()
                    .iter()
                    .filter(|commit| commit.settles(artifact))
                    .count();
                assert!(
                    report.manipulation_feedback().is_empty(),
                    "output settlement cannot repeat delivery"
                );
                assert!(!report.workspace_changed());
                state
                    .settle_published_report(
                        &context,
                        None,
                        SURFACE,
                        None,
                        report,
                        &mut budget,
                        false,
                    )
                    .unwrap();
                state.settle_internal_application_transitions().unwrap();
                if settlements == 1 {
                    break;
                }
            }
            assert_eq!(settlements, 1);
            assert!(!state.coordinator.session().has_pending_presentation());
        }
        let ready = state
            .service_pending_prefixes(&context, None, SURFACE, &mut budget)
            .unwrap();
        assert_eq!(bridge.captured_through_for_test(), captured);
        if turn < INPUT_TURNS {
            assert!(
                budget.is_exhausted(),
                "each live refill must incur the 3ms translation cost"
            );
        }
        if budget.is_exhausted() {
            exhausted_turns += 1;
            assert!(!ready);
            assert!(budget.elapsed() >= Duration::from_millis(3));
            assert!(
                budget.remaining_records() > 0,
                "elapsed time, not the 256-record quota, must yield"
            );
            let before = (
                bridge.input_backlog_for_test(),
                bridge.callback_boundary_pending(),
                state.coordinator.session().native_input_recorded_count(),
                state.coordinator.session().version(),
                state.coordinator.deferred_viewport_specs(),
            );
            assert!(
                !state
                    .service_pending_prefixes(&context, None, SURFACE, &mut budget)
                    .unwrap()
            );
            assert_eq!(
                (
                    bridge.input_backlog_for_test(),
                    bridge.callback_boundary_pending(),
                    state.coordinator.session().native_input_recorded_count(),
                    state.coordinator.session().version(),
                    state.coordinator.deferred_viewport_specs(),
                ),
                before,
                "an exhausted turn cannot claim a successor, append records or publish a new version"
            );
        }
        assert!(!matches!(
            bridge.admission_dispatch(),
            NativeDispatch::Cancel { .. }
        ));
        let resident = state.owned_input_capacity_bytes().unwrap();
        let reserved = state.input_storage_reserved_bytes().unwrap();
        assert!(resident <= reserved && reserved <= storage.byte_limit());
        assert!(bridge.input_backlog_for_test().0 <= 2 * CADENCE_RECORDS);
        let view = state.coordinator.session().view();
        assert!(view.item(THIRD).is_some());
        for item in [FIRST, SECOND] {
            assert_eq!(view.item(item).unwrap().surface(), SOURCE);
        }
        assert_eq!(view.item(FIRST).unwrap().tabs().items(), [SECOND, FIRST]);
        assert_eq!(view.item(WAITING_ITEM).unwrap().surface(), WAITING);
        bridge.with_close_control(|control| {
            assert_eq!(control.request_for_binding(root), Some(close));
            assert!(control.held_request(close).is_none());
            if !control.has_runnable_policy_work() {
                close_turn.get_or_insert(turn);
                assert!(
                    !control
                        .unsubmitted_requests()
                        .any(|request| request == close)
                );
                assert_eq!(control.decision_count(), 0);
            }
        });
        let specs = state.coordinator.deferred_viewport_specs();
        assert!(specs.len() <= 1);
        if let Some(spec) = specs.first() {
            assert_eq!(spec.binding().surface(), WAITING);
            assert_eq!(spec.role(), NativeSurfaceRole::Child);
            assert_eq!(*child.get_or_insert(spec.binding()), spec.binding());
            create_turn.get_or_insert(turn);
        }
        assert!(state.shutdown.is_none());
        if turn >= INPUT_TURNS && ready {
            break;
        }
    }
    assert!(exhausted_turns >= 3);
    assert!(close_turn.is_some_and(|turn| turn < INPUT_TURNS - 1));
    assert!(create_turn.is_some_and(|turn| turn < INPUT_TURNS - 1));
    assert_eq!(settlements, 1);
    assert_eq!(
        bridge.captured_through_for_test(),
        captured_before_refill + (INPUT_TURNS * CADENCE_RECORDS) as u64,
    );
    let view = state.coordinator.session().view();
    assert_eq!(view.item(FIRST).unwrap().tabs().items(), [SECOND, FIRST]);
    assert_eq!(bridge.input_backlog_for_test(), (0, None));
    assert!(!bridge.callback_boundary_pending());
    assert!(!state.coordinator.session().has_pending_native_input());
    assert!(!state.coordinator.session().has_pending_presentation());
    assert_eq!(state.close_progress().requests_retained(), 1);
    assert_eq!(state.coordinator.deferred_viewport_specs().len(), 1);
    let version = state.coordinator.session().version();
    let recorded = state.coordinator.session().native_input_recorded_count();
    for _ in 0..2 {
        assert_eq!(
            state.service_native_events(&context).unwrap(),
            NativeHostWake::Wait
        );
        assert_eq!(state.coordinator.session().version(), version);
        assert_eq!(
            state.coordinator.session().native_input_recorded_count(),
            recorded
        );
    }
    assert_eq!(
        *observed.lock().unwrap(),
        (1, 1),
        "all continuation, service, close-policy and drain reports settle the release once"
    );
    let controlled_elapsed = clock.now() - accepted_at;
    assert!(controlled_elapsed <= Duration::from_millis(100));
    eprintln!(
        "elapsed-service: exhausted_turns={exhausted_turns} close_turn={close_turn:?} create_turn={create_turn:?} release={:?} controlled_elapsed={controlled_elapsed:?} translation_cost=3ms hard_realtime=false",
        release_elapsed,
    );
}
