//! Unproven-press observation models using real core gestures and presentation receipts.
//!
//! Winit ingress exercises the production translator. Observation facts enter
//! the receipt reserved by production preparation, without enqueueing an OS
//! command. Model button events have no OS press identity, so even supplied
//! routes cannot authorize a continuation. These tests qualify receipt, budget
//! and retry behavior, not positive driver sampling or renderer authority.

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use dockspace::model::DockspaceActionOutcome;
use dockspace::runtime::{
    DockspaceReceiverDescriptor, NativeMonotonicClock, NativePointerContinuation,
    NativeReceiverPurpose, NativeReceiverQuery, NativeWindowBinding, NativeWindowHit,
    SurfacePresentationArtifact,
};
use eframe::NativeViewportPointerObservation;

use super::*;
use crate::pointer_observation::{NativePointerObservationKey, NativePointerObservationRecord};

const TARGET_ITEM: ItemId = ItemId::new(62);

type PreparedObservation =
    eframe::PreparedNativeViewportCommand<eframe::NativeViewportPointerObservationCommandToken>;

#[derive(Clone, Copy)]
struct Receivers {
    window: NativeWindowBinding,
    artifact: SurfacePresentationArtifact,
    source: DockspaceReceiverDescriptor,
    target: DockspaceReceiverDescriptor,
}

impl Receivers {
    fn answer(self, query: NativeReceiverQuery) -> NativeReceiverAnswer {
        let query = match query {
            NativeReceiverQuery::WindowHit(query) => {
                return NativeReceiverAnswer::WindowHit(if query.window() == self.window {
                    NativeWindowHit::Surface(self.artifact)
                } else {
                    NativeWindowHit::Unknown
                });
            }
            NativeReceiverQuery::Surface(query) => query,
        };
        if matches!(query.purpose(), NativeReceiverPurpose::HoverHit) {
            query.clear_hover_lane(&self.target).map_or(
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

struct Fixture {
    native: NativeCoordinator,
    binding: NativeSurfaceBinding,
    receivers: Receivers,
    context: egui::Context,
    clock: NativeMonotonicClock,
    milliseconds: Arc<AtomicU64>,
    target_desktop: PhysicalPoint,
}

impl Fixture {
    fn new() -> Self {
        let milliseconds = Arc::new(AtomicU64::new(1_000));
        let source = Arc::clone(&milliseconds);
        let clock =
            NativeMonotonicClock::new(move || Duration::from_millis(source.load(Ordering::SeqCst)));
        let layout = DockspaceLayout::new([DockspaceSurfaceLayout::new(
            SOURCE_SURFACE,
            DockspaceRootLayout::new(
                SOURCE_ROOT,
                DockspaceNode::central_tabs([SOURCE_ITEM, TARGET_ITEM]),
            ),
        )])
        .expect("the stationary two-tab layout validates");
        let session = DockspaceSession::from_layout(layout, DockPolicy::default())
            .expect("the stationary session initializes");
        let mut native = NativeCoordinator::new_with_storage_allowance(
            session,
            NativePointerRoster::Exact(Vec::new()),
            None,
            None,
            None,
            Some(clock.clone()),
        )
        .expect("the model shares its clock with native ingress");
        let binding = register_source(&mut native);
        publish_desktop_authority(&mut native, binding);
        let (source, artifact) = measure_and_present_source_with_artifact(
            &mut native,
            LogicalRect::new(0.0, 0.0, 640.0, 360.0).unwrap(),
        );
        let target = {
            let mut frame = native
                .begin_host_frame(|_| NativeReceiverAnswer::Unknown)
                .expect("the presented tab geometry is readable");
            let plan = frame
                .frame
                .paint_plan(SOURCE_SURFACE)
                .unwrap()
                .expect("the measured 640-pixel surface is paintable");
            let mut tabs = Vec::new();
            plan.visit_surface_program(|step| {
                if let SurfacePaintProgramStep::Operation {
                    operation: SurfacePaintProgramOperation::Tab(tab),
                    ..
                } = step
                {
                    assert!(tab.operable());
                    assert_eq!(tab.full_bounds(), tab.visible_bounds());
                    tabs.push(tab.item());
                }
                Ok::<(), ()>(())
            })
            .unwrap();
            assert_eq!(tabs, [SOURCE_ITEM, TARGET_ITEM]);
            assert_eq!(plan.tab_receiver(SOURCE_ITEM), Some(source));
            plan.tab_receiver(TARGET_ITEM)
                .expect("the second fully visible tab has a receiver")
        };
        // Bind both descriptors to the output actually presented by the helper.
        assert!(native.session.bind_presented_receiver(&source).is_some());
        assert!(native.session.bind_presented_receiver(&target).is_some());
        let target_desktop = PhysicalPoint::new(target.center().x(), target.center().y()).unwrap();
        let mut fixture = Self {
            native,
            binding,
            receivers: Receivers {
                source,
                target,
                window: binding.window(),
                artifact,
            },
            context: egui::Context::default(),
            clock,
            milliseconds,
            target_desktop,
        };
        let recorded = fixture.native.session.native_input_recorded_count();
        let press = PhysicalPoint::new(source.center().x(), source.center().y()).unwrap();
        push_window_event(
            &mut fixture.native,
            1,
            WindowEvent::MouseInput {
                primary_press: None,
                device_id: DeviceId::dummy(),
                state: ElementState::Pressed,
                button: MouseButton::Left,
                facts: pointer_facts(
                    press,
                    PointerWindowRoute::Window(source_window()),
                    PointerWindowRoute::Window(source_window()),
                ),
            },
        );
        fixture.commit(&mut fixture.budget());
        fixture.at(Duration::from_millis(1_010));
        let motion = fixture.motion();
        push_window_event(&mut fixture.native, 2, motion);
        let report = fixture.commit(&mut fixture.budget());
        assert!(selected_items(&report).is_empty());
        assert_eq!(
            fixture.native.session.native_input_recorded_count(),
            recorded + 2
        );
        assert!(fixture.native.session.has_active_gesture());
        assert!(
            fixture
                .native
                .session
                .view()
                .item(SOURCE_ITEM)
                .unwrap()
                .is_selected()
        );
        assert!(
            !fixture
                .native
                .session
                .view()
                .item(TARGET_ITEM)
                .unwrap()
                .is_selected()
        );
        let request = fixture.request();
        assert_eq!(request.source_binding(), binding);
        assert_eq!(request.surface(), SOURCE_SURFACE);
        assert_eq!(request.position(), target.center());
        assert!(request.local_coordinates().is_none());
        assert!(request.deadline() > fixture.clock.now());
        fixture
            .native
            .bridge
            .enable_observation_attachment_for_test();
        fixture
    }

    fn at(&self, time: Duration) {
        assert!(
            time >= self.clock.now(),
            "the model clock never moves backwards"
        );
        self.milliseconds.store(
            time.as_millis()
                .try_into()
                .expect("model milliseconds fit u64"),
            Ordering::SeqCst,
        );
        assert_eq!(self.native.bridge.observation_time(), time);
    }

    fn budget(&self) -> NativeInputBudget {
        NativeInputBudget::new(&self.clock)
    }

    fn request(&self) -> NativePointerContinuation {
        self.native
            .session
            .native_pointer_continuation()
            .expect("the committed Winit gesture owns a stationary request")
    }

    fn motion(&self) -> WindowEvent {
        WindowEvent::CursorMoved {
            device_id: DeviceId::dummy(),
            position: PhysicalPosition::new(self.target_desktop.x(), self.target_desktop.y()),
            facts: pointer_facts(
                self.target_desktop,
                PointerWindowRoute::Window(source_window()),
                PointerWindowRoute::Window(source_window()),
            ),
        }
    }

    fn prepare(&mut self, budget: &mut NativeInputBudget) -> Option<PreparedObservation> {
        let prepared = self
            .native
            .prepare_stationary_observation(&self.context, budget)?;
        self.native.admit_stationary_observation(prepared)
    }

    fn key(&self, prepared: &PreparedObservation) -> NativePointerObservationKey {
        let key = self.native.pending_stationary_key().unwrap();
        assert_eq!(key.token, prepared.token());
        assert_eq!(key.viewport, ViewportId::ROOT);
        assert_eq!(key.window, source_window());
        assert_eq!(key.device, DeviceId::dummy());
        assert!(self.native.bridge.pointer_observation_pending(key));
        key
    }

    fn prepare_due(&mut self) -> (PreparedObservation, NativePointerObservationKey) {
        self.at(self.request().deadline().max(self.clock.now()));
        let prepared = self
            .prepare(&mut self.budget())
            .expect("the request is due");
        let key = self.key(&prepared);
        (prepared, key)
    }

    fn observed(
        &self,
        capture: PointerWindowRoute,
        delivery: PointerWindowRoute,
    ) -> NativeViewportPointerObservation {
        NativeViewportPointerObservation::Observed {
            facts: pointer_facts(
                self.target_desktop,
                PointerWindowRoute::Window(source_window()),
                capture,
            ),
            delivery,
        }
    }

    fn receive(
        &self,
        key: NativePointerObservationKey,
        observation: NativeViewportPointerObservation,
    ) -> bool {
        let record = self.native.bridge.with_viewports(|routes| {
            NativePointerObservationRecord::for_test(key, self.binding, observation, routes)
        });
        self.native
            .bridge
            .record_pointer_observation_for_test(record)
    }

    fn reduce(&mut self, budget: &mut NativeInputBudget) -> Option<usize> {
        self.native
            .reduce_callback_prefix(&self.context, budget)
            .expect("the exact model callback reduces")
            .map(|count| count.get())
    }

    fn commit(&mut self, budget: &mut NativeInputBudget) -> HostFrameReport {
        let receivers = self.receivers;
        let mut frame = self
            .native
            .poll_host_frame(budget, |query| receivers.answer(query))
            .expect("the ordered model boundary polls")
            .expect("the model boundary fits a fresh shared-clock budget");
        frame
            .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
            .unwrap();
        let mut report = frame.commit().expect("the exact model boundary commits");
        let effects = report.take_native_effects();
        assert!(effects.iter().all(|effect| !matches!(
            effect.operation(),
            NativeEffectOperation::CreateWindow { .. }
        )));
        self.native
            .accept_native_effects(&self.context, effects)
            .unwrap();
        report
    }
}

fn selected_items(report: &HostFrameReport) -> Vec<(ItemId, bool)> {
    report
        .inputs()
        .iter()
        .filter_map(|outcome| match outcome {
            HostInputOutcome::ProductActionApplied(DockspaceActionOutcome::Selected {
                item,
                changed,
            }) => Some((*item, *changed)),
            _ => None,
        })
        .collect()
}

#[test]
fn model_stationary_unproven_press_rejects_sample_and_retries_its_exact_terminal() {
    let mut fixture = Fixture::new();
    let request = fixture.request();
    let version = fixture.native.session.version();
    let recorded = fixture.native.session.native_input_recorded_count();
    let captured = fixture.native.bridge.captured_through_for_test();
    fixture.at(request.deadline() - Duration::from_millis(1));
    let mut budget = fixture.budget();
    assert!(fixture.prepare(&mut budget).is_none());
    assert_eq!(budget.remaining_records(), 256);
    assert!(fixture.native.pending_stationary_key().is_none());

    fixture.at(request.deadline());
    let mut budget = fixture.budget();
    let prepared = fixture
        .prepare(&mut budget)
        .expect("the deadline is inclusive");
    let key = fixture.key(&prepared);
    assert_eq!(budget.remaining_records(), 255);
    assert!(fixture.prepare(&mut budget).is_none());
    assert_eq!(budget.remaining_records(), 255);
    assert_eq!(fixture.native.session.version(), version);
    assert_eq!(
        fixture.native.session.native_input_recorded_count(),
        recorded
    );
    assert_eq!(fixture.native.bridge.captured_through_for_test(), captured);

    let captured_at = fixture.clock.now();
    assert!(fixture.receive(
        key,
        fixture.observed(
            PointerWindowRoute::Window(source_window()),
            PointerWindowRoute::Window(source_window()),
        ),
    ));
    let (sample, time) = fixture.native.bridge.front_pointer_observation().unwrap();
    assert_eq!(sample.key(), key);
    assert_eq!(time, captured_at);
    let sequence = fixture.native.bridge.captured_through_for_test();
    assert_eq!(sequence, captured + 1);
    assert_eq!(fixture.reduce(&mut budget), Some(1));
    assert_eq!(
        fixture.native.session.native_input_recorded_count(),
        recorded
    );
    assert!(fixture.native.bridge.pointer_observation_pending(key));

    let receivers = fixture.receivers;
    let mut first_queries = Vec::new();
    let mut frame = fixture
        .native
        .poll_host_frame(&mut budget, |query| {
            first_queries.push(query);
            receivers.answer(query)
        })
        .unwrap()
        .unwrap();
    frame
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .unwrap();
    drop(frame);
    assert!(
        first_queries.is_empty(),
        "absence cannot become a receiver query"
    );
    fixture.at(captured_at + Duration::from_millis(5));
    assert_eq!(fixture.native.session.version(), version);
    assert_eq!(
        fixture.native.session.native_input_recorded_count(),
        recorded
    );
    assert_eq!(fixture.native.bridge.captured_through_for_test(), sequence);
    assert_eq!(
        fixture.native.bridge.input_backlog_for_test(),
        (1, Some(Duration::from_millis(5)))
    );
    assert!(fixture.native.bridge.callback_boundary_pending());
    let mut retry_budget = fixture.budget();
    assert!(fixture.prepare(&mut retry_budget).is_none());
    assert_eq!(fixture.reduce(&mut retry_budget), None);
    assert!(!fixture.receive(
        key,
        fixture.observed(
            PointerWindowRoute::Window(source_window()),
            PointerWindowRoute::Window(source_window()),
        ),
    ));

    let mut retry_queries = Vec::new();
    let mut retry = fixture
        .native
        .poll_host_frame(&mut retry_budget, |query| {
            retry_queries.push(query);
            receivers.answer(query)
        })
        .unwrap()
        .unwrap();
    retry
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .unwrap();
    let mut report = retry.commit().unwrap();
    fixture
        .native
        .accept_native_effects(&fixture.context, report.take_native_effects())
        .unwrap();
    assert_eq!(retry_queries, first_queries);
    assert!(selected_items(&report).is_empty());
    assert!(!report.workspace_changed());
    assert!(
        fixture
            .native
            .session
            .view()
            .item(SOURCE_ITEM)
            .unwrap()
            .is_selected()
    );
    assert_eq!(
        fixture.native.session.native_input_recorded_count(),
        recorded
    );
    assert_eq!(fixture.native.bridge.captured_through_for_test(), sequence);
    assert!(!fixture.native.bridge.pointer_observation_pending(key));
    assert_eq!(fixture.native.bridge.input_backlog_for_test(), (0, None));
    assert!(fixture.prepare(&mut fixture.budget()).is_none());
    assert!(fixture.native.pending_stationary_key().is_none());
    let committed_version = fixture.native.session.version();
    let idle = fixture.commit(&mut fixture.budget());
    assert!(selected_items(&idle).is_empty());
    assert_eq!(fixture.native.session.version(), committed_version);
    assert_eq!(
        fixture.native.session.native_input_recorded_count(),
        recorded
    );
}

#[test]
fn model_stationary_unproven_press_queries_settle_without_selecting_or_rescheduling() {
    for (name, routes) in [
        ("unavailable", None),
        (
            "unknown capture",
            Some((
                PointerWindowRoute::Unknown,
                PointerWindowRoute::Window(source_window()),
            )),
        ),
        (
            "unknown delivery",
            Some((
                PointerWindowRoute::Window(source_window()),
                PointerWindowRoute::Unknown,
            )),
        ),
        (
            "supplied capture and delivery cannot replace a press identity",
            Some((
                PointerWindowRoute::Window(source_window()),
                PointerWindowRoute::Window(source_window()),
            )),
        ),
    ] {
        let mut fixture = Fixture::new();
        let original = fixture.request();
        let version = fixture.native.session.version();
        let (_prepared, key) = fixture.prepare_due();
        let observation = routes.map_or(
            NativeViewportPointerObservation::Unavailable,
            |(capture, delivery)| fixture.observed(capture, delivery),
        );
        assert!(fixture.receive(key, observation), "{name}");
        let mut budget = fixture.budget();
        assert_eq!(fixture.reduce(&mut budget), Some(1), "{name}");
        let report = fixture.commit(&mut budget);
        assert!(selected_items(&report).is_empty(), "{name}");
        assert!(!report.workspace_changed(), "{name}");
        assert!(
            !fixture.native.bridge.pointer_observation_pending(key),
            "{name}"
        );
        assert_eq!(
            fixture.native.bridge.input_backlog_for_test(),
            (0, None),
            "{name}"
        );
        let captured = fixture.native.bridge.captured_through_for_test();
        let recorded = fixture.native.session.native_input_recorded_count();
        for _ in 0..3 {
            fixture.at(fixture.clock.now() + Duration::from_millis(1));
            let mut budget = fixture.budget();
            assert!(fixture.prepare(&mut budget).is_none(), "{name}");
            assert!(fixture.native.pending_stationary_key().is_none(), "{name}");
            assert_eq!(fixture.reduce(&mut budget), None, "{name}");
            assert!(
                selected_items(&fixture.commit(&mut budget)).is_empty(),
                "{name}"
            );
            assert_eq!(fixture.native.session.version(), version, "{name}");
            assert_eq!(
                fixture.native.session.native_input_recorded_count(),
                recorded,
                "{name}"
            );
            assert_eq!(
                fixture.native.bridge.captured_through_for_test(),
                captured,
                "{name}"
            );
        }
        assert!(
            fixture
                .native
                .session
                .view()
                .item(SOURCE_ITEM)
                .unwrap()
                .is_selected(),
            "{name}"
        );
        assert!(fixture.native.session.has_active_gesture(), "{name}");
        {
            // The same target retains its dwell origin but receives a new core token.
            let motion = fixture.motion();
            push_window_event(&mut fixture.native, 3, motion);
            let motion_report = fixture.commit(&mut fixture.budget());
            assert!(selected_items(&motion_report).is_empty());
            let current = fixture.request();
            assert_ne!(current.token(), original.token());
            assert_eq!(current.position(), original.position());
            assert_eq!(current.deadline(), original.deadline());
            let (prepared, next_key) = fixture.prepare_due();
            assert_eq!(fixture.key(&prepared), next_key);
            assert_ne!(next_key.token, key.token);
            assert!(fixture.receive(
                next_key,
                fixture.observed(
                    PointerWindowRoute::Window(source_window()),
                    PointerWindowRoute::Window(source_window()),
                ),
            ));
            let mut budget = fixture.budget();
            assert_eq!(fixture.reduce(&mut budget), Some(1));
            let report = fixture.commit(&mut budget);
            assert!(selected_items(&report).is_empty());
            assert!(!report.workspace_changed());
            assert!(!fixture.native.bridge.pointer_observation_pending(next_key));
            assert!(fixture.prepare(&mut fixture.budget()).is_none());
        }
    }
}

#[test]
fn model_stationary_release_revokes_token_and_late_sample_settles_once() {
    let mut fixture = Fixture::new();
    let request = fixture.request();
    let version = fixture.native.session.version();
    let (_prepared, key) = fixture.prepare_due();
    let origin = fixture.receivers.source.center();
    let origin = PhysicalPoint::new(origin.x(), origin.y()).unwrap();
    push_window_event(
        &mut fixture.native,
        3,
        WindowEvent::MouseInput {
            primary_press: None,
            device_id: DeviceId::dummy(),
            state: ElementState::Released,
            button: MouseButton::Left,
            facts: pointer_facts(origin, PointerWindowRoute::None, PointerWindowRoute::None),
        },
    );
    let release = fixture.commit(&mut fixture.budget());
    assert!(selected_items(&release).is_empty());
    assert!(
        fixture
            .native
            .session
            .native_pointer_continuation()
            .is_none()
    );
    assert_eq!(
        fixture
            .native
            .pointer_translator
            .mouse_device_for_pointer(request.pointer()),
        None
    );
    assert!(fixture.native.bridge.pointer_observation_pending(key));
    assert!(fixture.prepare(&mut fixture.budget()).is_none());

    fixture.at(fixture.clock.now() + Duration::from_millis(1));
    let recorded = fixture.native.session.native_input_recorded_count();
    let captured = fixture.native.bridge.captured_through_for_test();
    assert!(fixture.receive(
        key,
        fixture.observed(
            PointerWindowRoute::Window(source_window()),
            PointerWindowRoute::Window(source_window()),
        ),
    ));
    assert_eq!(
        fixture.native.bridge.captured_through_for_test(),
        captured + 1
    );
    let mut budget = fixture.budget();
    assert_eq!(fixture.reduce(&mut budget), Some(1));
    assert_eq!(budget.remaining_records(), 255);
    assert_eq!(
        fixture.native.session.native_input_recorded_count(),
        recorded
    );
    assert!(fixture.native.bridge.pointer_observation_pending(key));
    assert!(!fixture.receive(key, NativeViewportPointerObservation::Unavailable));
    let report = fixture.commit(&mut budget);
    assert!(selected_items(&report).is_empty());
    assert!(!report.workspace_changed());
    assert_eq!(fixture.native.session.version(), version);
    assert!(!fixture.native.bridge.pointer_observation_pending(key));
    assert_eq!(fixture.native.bridge.input_backlog_for_test(), (0, None));
    assert!(fixture.prepare(&mut fixture.budget()).is_none());
    assert!(fixture.native.pending_stationary_key().is_none());
    assert!(!fixture.receive(key, NativeViewportPointerObservation::Unavailable));
    assert_eq!(fixture.reduce(&mut fixture.budget()), None);
    assert!(selected_items(&fixture.commit(&mut fixture.budget())).is_empty());
    assert_eq!(
        fixture.native.session.native_input_recorded_count(),
        recorded
    );
    assert_eq!(
        fixture.native.bridge.captured_through_for_test(),
        captured + 1
    );
}

#[test]
fn model_stationary_budget_and_raw_input_precede_dispatch_until_receipt_commits() {
    let mut fixture = Fixture::new();
    let original = fixture.request();
    let captured = fixture.native.bridge.captured_through_for_test();
    let recorded = fixture.native.session.native_input_recorded_count();
    fixture.at(original.deadline() - Duration::from_millis(1));
    let mut elapsed_budget = fixture.budget();
    fixture.at(original.deadline() + Duration::from_millis(1));
    assert_eq!(elapsed_budget.elapsed(), Duration::from_millis(2));
    assert!(elapsed_budget.is_exhausted());
    assert!(fixture.prepare(&mut elapsed_budget).is_none());
    assert_eq!(elapsed_budget.remaining_records(), 256);
    let mut spent_budget = fixture.budget();
    spent_budget.complete_unsequenced_records(spent_budget.remaining_records());
    assert!(fixture.prepare(&mut spent_budget).is_none());
    assert_eq!(spent_budget.remaining_records(), 0);
    assert!(fixture.native.pending_stationary_key().is_none());
    assert_eq!(fixture.native.bridge.captured_through_for_test(), captured);
    assert_eq!(
        fixture.native.session.native_input_recorded_count(),
        recorded
    );

    let motion = fixture.motion();
    let raw = fixture.native.bridge.with_viewports_mut(|routes| {
        NativeWindowEventRecord::for_test_ingress(
            3,
            source_window(),
            Some(ViewportId::ROOT),
            motion,
            routes,
        )
    });
    fixture
        .native
        .bridge
        .push_record(HostRecord::WindowEvent(raw));
    assert_eq!(
        fixture.native.bridge.captured_through_for_test(),
        captured + 1
    );
    assert_eq!(fixture.request(), original);
    let mut budget = fixture.budget();
    assert!(fixture.prepare(&mut budget).is_none());
    assert_eq!(budget.remaining_records(), 256);
    assert!(fixture.native.pending_stationary_key().is_none());
    assert_eq!(
        fixture.native.session.native_input_recorded_count(),
        recorded
    );
    assert_eq!(fixture.reduce(&mut budget), Some(1));
    assert!(fixture.prepare(&mut budget).is_none());
    fixture.commit(&mut budget);
    let sampled_request = fixture.request();
    assert_ne!(sampled_request.token(), original.token());
    let (prepared, key) = fixture.prepare_due();
    assert_eq!(fixture.key(&prepared), key);

    // A newer real move revokes the old challenge while its receipt remains owned.
    let motion = fixture.motion();
    push_window_event(&mut fixture.native, 4, motion);
    fixture.commit(&mut fixture.budget());
    let current = fixture.request();
    assert_ne!(current.token(), sampled_request.token());
    assert!(current.deadline() <= fixture.clock.now());
    assert!(fixture.prepare(&mut fixture.budget()).is_none());
    let recorded = fixture.native.session.native_input_recorded_count();
    let captured = fixture.native.bridge.captured_through_for_test();
    assert!(fixture.receive(
        key,
        fixture.observed(
            PointerWindowRoute::Window(source_window()),
            PointerWindowRoute::Window(source_window()),
        ),
    ));
    let mut budget = fixture.budget();
    assert_eq!(fixture.reduce(&mut budget), Some(1));
    assert_eq!(
        fixture.native.session.native_input_recorded_count(),
        recorded
    );
    let receivers = fixture.receivers;
    let mut frame = fixture
        .native
        .poll_host_frame(&mut budget, |query| receivers.answer(query))
        .unwrap()
        .unwrap();
    frame
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .unwrap();
    drop(frame);
    assert!(fixture.native.bridge.pointer_observation_pending(key));
    assert_eq!(fixture.native.pending_stationary_key(), Some(key));
    assert!(fixture.prepare(&mut fixture.budget()).is_none());
    assert_eq!(
        fixture.native.bridge.captured_through_for_test(),
        captured + 1
    );
    assert_eq!(
        fixture.native.session.native_input_recorded_count(),
        recorded
    );

    let report = fixture.commit(&mut fixture.budget());
    assert!(selected_items(&report).is_empty());
    assert!(!fixture.native.bridge.pointer_observation_pending(key));
    assert_eq!(fixture.native.bridge.input_backlog_for_test(), (0, None));
    assert_eq!(fixture.request(), current);
    let mut budget = fixture.budget();
    let next = fixture
        .prepare(&mut budget)
        .expect("only receipt commit frees preparation for the newer real challenge");
    let next_key = fixture.key(&next);
    assert_ne!(next.token(), prepared.token());
    assert_ne!(next_key, key);
    assert_eq!(budget.remaining_records(), 255);
    assert_eq!(
        fixture.native.bridge.captured_through_for_test(),
        captured + 1
    );
    assert_eq!(
        fixture.native.session.native_input_recorded_count(),
        recorded
    );
    assert!(fixture.receive(next_key, NativeViewportPointerObservation::Unavailable));
    assert_eq!(fixture.reduce(&mut budget), Some(1));
    assert!(selected_items(&fixture.commit(&mut budget)).is_empty());
    assert!(fixture.prepare(&mut fixture.budget()).is_none());
}

#[test]
fn model_stationary_unproven_press_terminal_retries_after_artifact_replacement() {
    let mut fixture = Fixture::new();
    let request = fixture.request();
    let old_presented = fixture
        .native
        .session
        .presented_surface(SOURCE_SURFACE)
        .unwrap();
    let (_prepared, key) = fixture.prepare_due();
    let source = measure_and_present_source(&mut fixture.native);
    let current_presented = fixture
        .native
        .session
        .presented_surface(SOURCE_SURFACE)
        .unwrap();
    assert_ne!(current_presented, old_presented);
    assert_eq!(fixture.request(), request);
    assert!(
        fixture
            .native
            .session
            .bind_presented_receiver(&source.descriptor)
            .is_some()
    );
    assert!(fixture.receive(
        key,
        fixture.observed(
            PointerWindowRoute::Window(source_window()),
            PointerWindowRoute::Window(source_window()),
        ),
    ));
    let mut budget = fixture.budget();
    assert_eq!(fixture.reduce(&mut budget), Some(1));
    let version = fixture.native.session.version();
    let captured = fixture.native.bridge.captured_through_for_test();
    let mut first_queries = Vec::new();
    let mut first = fixture
        .native
        .poll_host_frame(&mut budget, |query| {
            first_queries.push(query);
            NativeReceiverAnswer::Unknown
        })
        .unwrap()
        .unwrap();
    first
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .unwrap();
    drop(first);
    assert!(first_queries.is_empty());
    assert!(fixture.native.bridge.pointer_observation_pending(key));
    assert_eq!(fixture.native.session.version(), version);
    assert!(fixture.prepare(&mut fixture.budget()).is_none());

    let mut retry_queries = Vec::new();
    let mut retry = fixture
        .native
        .poll_host_frame(&mut fixture.budget(), |query| {
            retry_queries.push(query);
            NativeReceiverAnswer::Unknown
        })
        .unwrap()
        .unwrap();
    retry
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .unwrap();
    let report = retry.commit().unwrap();
    assert_eq!(retry_queries, first_queries);
    assert!(selected_items(&report).is_empty());
    assert!(!report.workspace_changed());
    assert_eq!(fixture.native.session.version(), version);
    assert_eq!(fixture.native.bridge.captured_through_for_test(), captured);
    assert!(!fixture.native.bridge.pointer_observation_pending(key));
    assert_eq!(
        fixture.native.session.native_pointer_continuation(),
        Some(request)
    );
    assert!(fixture.prepare(&mut fixture.budget()).is_none());
    assert!(
        fixture
            .native
            .session
            .view()
            .item(SOURCE_ITEM)
            .unwrap()
            .is_selected()
    );
}

#[test]
fn model_stationary_preparation_prepays_before_a_slow_application_suffix() {
    let mut fixture = Fixture::new();
    fixture.at(fixture.request().deadline());
    let mut budget = fixture.budget();
    let prepared = fixture
        .native
        .prepare_stationary_observation(&fixture.context, &mut budget)
        .unwrap();
    assert_eq!(budget.remaining_records(), 255);
    assert!(fixture.native.pending_stationary_key().is_none());
    assert!(!fixture.native.bridge.has_pending_callback_records());
    fixture.at(fixture.clock.now() + Duration::from_millis(5));
    assert!(budget.is_exhausted());
    let command = fixture
        .native
        .admit_stationary_observation(prepared)
        .expect("the final pass consumes its existing prepaid preparation");
    let key = fixture.key(&command);
    assert_eq!(budget.remaining_records(), 255);
    assert!(fixture.receive(key, NativeViewportPointerObservation::Unavailable));
    let mut next_budget = fixture.budget();
    assert_eq!(fixture.reduce(&mut next_budget), Some(1));
    assert!(selected_items(&fixture.commit(&mut next_budget)).is_empty());
    assert!(fixture.prepare(&mut fixture.budget()).is_none());
}

#[test]
fn model_stationary_unqueued_preparation_can_drop_or_reject_new_raw_input() {
    let mut fixture = Fixture::new();
    fixture.at(fixture.request().deadline());
    let captured = fixture.native.bridge.captured_through_for_test();
    let mut budget = fixture.budget();
    {
        let _unqueued = fixture
            .native
            .prepare_stationary_observation(&fixture.context, &mut budget)
            .unwrap();
    }
    assert!(fixture.native.pending_stationary_key().is_none());
    assert_eq!(fixture.native.bridge.captured_through_for_test(), captured);
    let prepared = fixture
        .native
        .prepare_stationary_observation(&fixture.context, &mut budget)
        .unwrap();
    let motion = fixture.motion();
    push_window_event(&mut fixture.native, 3, motion);
    assert!(
        fixture
            .native
            .admit_stationary_observation(prepared)
            .is_none()
    );
    assert!(fixture.native.pending_stationary_key().is_none());
    assert_eq!(
        fixture.native.bridge.captured_through_for_test(),
        captured + 1
    );
    let report = fixture.commit(&mut fixture.budget());
    assert!(selected_items(&report).is_empty());
    let (_command, key) = fixture.prepare_due();
    assert!(fixture.receive(key, NativeViewportPointerObservation::Unavailable));
    assert_eq!(fixture.reduce(&mut fixture.budget()), Some(1));
    fixture.commit(&mut fixture.budget());
    assert!(fixture.prepare(&mut fixture.budget()).is_none());
}
