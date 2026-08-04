mod support;

use dockspace::engine::{CoreHostFrame, DockEngine};
use dockspace::geometry::{LogicalPoint, LogicalRect};
use dockspace::graph::{Node, RootRecord, SurfacePresentation, Workspace};
use dockspace::ids::{ItemId, RootId, SurfaceId};
use dockspace::intent::{Authority, AuthorityUnavailableReason, PointerButton, PointerId};
use dockspace::interaction::{
    InteractionEventKind, InteractionOutcome, ScrollReductionOutcome, ScrollTerminationReason,
};
use dockspace::pointer_journal::{
    FiniteScrollVector, PointerCaptureOwner, PointerEdge, PointerEdgeJournal, PointerEdgeKind,
    PointerEdgeLocation, PointerEdgeSequence, PointerProviderScope, ScrollCancelReason,
    ScrollDeliveryEndpoint, ScrollDelta, ScrollDeviceId, ScrollEdge, ScrollModifiers,
    ScrollMomentum, ScrollPhase, ScrollSequenceToken, SurfaceLocalPointerEndpoint,
    SurfaceLocalPointerScope,
};
use dockspace::pointer_receiver::{
    PointerReceiverDelivery, PointerReceiverDeliveryDisposition, PointerReceiverObservation,
    PointerReceiverProbeReceipt, PointerReceiverReceipt, PointerReceiverReceiptBatch,
    PointerReceiverUnknownReason, PresentedPointerReceiverObservation, ScrollReceiverChallenge,
};
use dockspace::policy::DockPolicy;
use dockspace::presentation_hit::{PresentationHitRegionId, PresentationHitRegionKind};
use dockspace::scene::SurfaceScene;
use dockspace::scene_manifest::{
    TabListMenuMetrics, TabStripControlMetric, TabStripControlMetrics, TabStripControlPlacement,
};
use support::{
    MeasurementProfile, TestPresentationHost, complete_host_frame_with_current_outputs,
    complete_host_frame_with_retained_or_unavailable, measurements, publish_surface_with,
};

const SURFACE: SurfaceId = SurfaceId::new(1);
const ROOT: RootId = RootId::new(10);
const POINTER: PointerId = PointerId::new(7);

fn workspace() -> Workspace {
    let mut builder = Workspace::builder();
    let items = [
        ItemId::new(1),
        ItemId::new(2),
        ItemId::new(3),
        ItemId::new(4),
    ];
    let tabs = builder.insert_node(Node::tabs_with_selection(items, Some(items[0])));
    builder.set_root(ROOT, RootRecord::new(tabs).with_central(tabs));
    builder.set_surface(SURFACE, SurfacePresentation::with_main(ROOT));
    builder.build().expect("scroll workspace is valid")
}

fn bounds(width: f64) -> LogicalRect {
    LogicalRect::new(0.0, 0.0, width, 180.0).expect("scroll bounds are valid")
}

fn measurement_profile() -> MeasurementProfile {
    MeasurementProfile {
        tab_content_width: 88.0,
        tab_strip_controls: Some(
            TabStripControlMetrics::new(4.0)
                .expect("control metrics are valid")
                .with_scroll_backward(
                    TabStripControlMetric::new(18.0, TabStripControlPlacement::OverlayLeading)
                        .expect("backward control metric is valid"),
                )
                .with_scroll_forward(
                    TabStripControlMetric::new(18.0, TabStripControlPlacement::OverlayTrailing)
                        .expect("forward control metric is valid"),
                )
                .with_tab_list_menu(
                    TabStripControlMetric::new(20.0, TabStripControlPlacement::ReservedTrailing)
                        .expect("menu control metric is valid"),
                ),
        ),
        tab_list_menu: Some(
            TabListMenuMetrics::new(24.0, 8.0, 8.0, 2.0, 92.0, 10.0)
                .expect("menu metrics are valid"),
        ),
        ..MeasurementProfile::default()
    }
}

struct ScrollFixture {
    engine: DockEngine,
    host: TestPresentationHost,
    provider: dockspace::pointer_journal::PointerInputLease,
    region: PresentationHitRegionId,
    point: LogicalPoint,
    endpoint: ScrollDeliveryEndpoint,
    watermark: u64,
}

impl ScrollFixture {
    fn new() -> Self {
        let mut engine =
            DockEngine::new(workspace(), DockPolicy::default()).expect("scroll engine is valid");
        let mut host = TestPresentationHost::new(&mut engine);
        publish_surface_with(
            &mut engine,
            &mut host,
            SURFACE,
            bounds(260.0),
            measurement_profile(),
        );
        let (region, point, endpoint) = {
            let projection = engine
                .interaction_projection(SURFACE)
                .expect("overflowing tab strip is interactive");
            let region = projection
                .hit_manifest()
                .regions()
                .iter()
                .find(|region| {
                    matches!(
                        region.id().kind(),
                        PresentationHitRegionKind::TabStripScroll(_)
                    )
                })
                .expect("overflowing tab strip publishes a scroll receiver");
            let rect = region.hit().rect();
            let point = LogicalPoint::new(
                rect.x() + rect.width() * 0.5,
                rect.y() + rect.height() * 0.5,
            )
            .expect("scroll receiver midpoint is finite");
            let endpoint = ScrollDeliveryEndpoint::new(
                host.lease(),
                SURFACE,
                None,
                projection.authority().coordinate_generation(),
            )
            .expect("headless scroll endpoint matches the presentation host");
            (region.id(), point, endpoint)
        };
        let provider = engine
            .create_pointer_provider(
                PointerProviderScope::SurfaceLocal(SurfaceLocalPointerScope::new(
                    host.lease(),
                    SurfaceLocalPointerEndpoint::Logical(SURFACE),
                )),
                PointerEdgeSequence::new(0),
            )
            .expect("surface-local scroll provider is admitted");
        Self {
            engine,
            host,
            provider,
            region,
            point,
            endpoint,
            watermark: 0,
        }
    }

    fn scroll_edge(
        &self,
        sequence: u64,
        device: ScrollDeviceId,
        phase: ScrollPhase,
        token: Option<ScrollSequenceToken>,
        delta: Option<ScrollDelta>,
        endpoint: ScrollDeliveryEndpoint,
        modifiers: ScrollModifiers,
    ) -> PointerEdge {
        let scroll = ScrollEdge::new(
            device,
            token,
            phase,
            delta,
            Authority::Known(ScrollMomentum::Direct),
            Authority::Known(modifiers),
            Authority::Known(endpoint),
        )
        .expect("scroll edge has a legal phase shape");
        PointerEdge::new(
            PointerEdgeSequence::new(sequence),
            POINTER,
            PointerEdgeKind::Scrolled(scroll),
            PointerEdgeLocation::SurfaceLocal {
                position: Authority::Known(self.point),
            },
            Authority::Known(PointerCaptureOwner::None),
        )
    }

    fn submit_known(
        &mut self,
        phase: ScrollPhase,
        token: Option<ScrollSequenceToken>,
        delta: Option<ScrollDelta>,
    ) -> dockspace::transition::EngineTransition {
        self.submit_known_for_device(ScrollDeviceId::new(1), phase, token, delta)
    }

    fn submit_known_for_device(
        &mut self,
        device: ScrollDeviceId,
        phase: ScrollPhase,
        token: Option<ScrollSequenceToken>,
        delta: Option<ScrollDelta>,
    ) -> dockspace::transition::EngineTransition {
        self.submit_with_disposition(
            device,
            phase,
            token,
            delta,
            PointerReceiverDeliveryDisposition::Dock(self.region),
        )
    }

    fn submit_with_disposition(
        &mut self,
        device: ScrollDeviceId,
        phase: ScrollPhase,
        token: Option<ScrollSequenceToken>,
        delta: Option<ScrollDelta>,
        disposition: PointerReceiverDeliveryDisposition,
    ) -> dockspace::transition::EngineTransition {
        self.submit_with_receipt(
            device,
            phase,
            token,
            delta,
            self.endpoint,
            Some(disposition),
        )
    }

    fn submit_unknown_at_endpoint(
        &mut self,
        phase: ScrollPhase,
        token: ScrollSequenceToken,
        delta: ScrollDelta,
        endpoint: ScrollDeliveryEndpoint,
    ) -> dockspace::transition::EngineTransition {
        self.submit_with_receipt(
            ScrollDeviceId::new(1),
            phase,
            Some(token),
            Some(delta),
            endpoint,
            None,
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn submit_with_receipt(
        &mut self,
        device: ScrollDeviceId,
        phase: ScrollPhase,
        token: Option<ScrollSequenceToken>,
        delta: Option<ScrollDelta>,
        endpoint: ScrollDeliveryEndpoint,
        disposition: Option<PointerReceiverDeliveryDisposition>,
    ) -> dockspace::transition::EngineTransition {
        self.submit_with_receipt_and_modifiers(
            device,
            phase,
            token,
            delta,
            endpoint,
            disposition,
            ScrollModifiers::default(),
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn submit_with_receipt_and_modifiers(
        &mut self,
        device: ScrollDeviceId,
        phase: ScrollPhase,
        token: Option<ScrollSequenceToken>,
        delta: Option<ScrollDelta>,
        endpoint: ScrollDeliveryEndpoint,
        disposition: Option<PointerReceiverDeliveryDisposition>,
        modifiers: ScrollModifiers,
    ) -> dockspace::transition::EngineTransition {
        self.submit_with_receipt_and_modifiers_and_inspect(
            device,
            phase,
            token,
            delta,
            endpoint,
            disposition,
            modifiers,
            |_| {},
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn submit_with_receipt_and_modifiers_and_inspect(
        &mut self,
        device: ScrollDeviceId,
        phase: ScrollPhase,
        token: Option<ScrollSequenceToken>,
        delta: Option<ScrollDelta>,
        endpoint: ScrollDeliveryEndpoint,
        disposition: Option<PointerReceiverDeliveryDisposition>,
        modifiers: ScrollModifiers,
        inspect: impl FnOnce(&dockspace::pointer_receiver::PointerReceiverCandidate),
    ) -> dockspace::transition::EngineTransition {
        let sequence = self.watermark + 1;
        let edge = self.scroll_edge(sequence, device, phase, token, delta, endpoint, modifiers);
        let journal = PointerEdgeJournal::new(
            PointerEdgeSequence::new(self.watermark),
            PointerEdgeSequence::new(sequence),
            vec![edge],
        )
        .expect("single-edge scroll journal is contiguous");
        let mut frame = self.host.begin(&self.engine);
        frame
            .submit_pointer_journal(self.provider, journal)
            .expect("scroll journal stages");
        let candidate = frame
            .pointer_receiver_candidates()
            .expect("scroll edge requests receiver evidence")
            .candidates()[0]
            .clone();
        inspect(&candidate);
        let observation = if !candidate.receiver_is_applicable() {
            PointerReceiverObservation::NotApplicable
        } else {
            disposition.map_or(
                PointerReceiverObservation::Unknown(PointerReceiverUnknownReason::NotReported),
                |disposition| {
                    let projection = frame
                        .view()
                        .interaction_projection(SURFACE)
                        .expect("scroll output remains interactive");
                    let delivery = PointerReceiverDelivery::new(projection, disposition)
                        .expect("scroll delivery is bound to the exact output");
                    PointerReceiverObservation::Presented(
                        PresentedPointerReceiverObservation::new([
                            PointerReceiverProbeReceipt::Delivery(delivery),
                        ])
                        .expect("scroll observation answers the delivery probe"),
                    )
                },
            )
        };
        frame
            .submit_pointer_receiver_receipts(
                PointerReceiverReceiptBatch::new([candidate.receipt(observation)])
                    .expect("scroll receipt roster is exact"),
            )
            .expect("scroll receipt reduces");
        complete_host_frame_with_retained_or_unavailable(&self.engine, &mut frame);
        let transition = self.host.finish(frame, &mut self.engine);
        self.watermark = sequence;
        transition
    }

    fn end_pointer_stream(&mut self) -> dockspace::transition::EngineTransition {
        let sequence = self.watermark + 1;
        let edge = PointerEdge::new(
            PointerEdgeSequence::new(sequence),
            POINTER,
            PointerEdgeKind::ButtonReleased(PointerButton::Secondary),
            PointerEdgeLocation::SurfaceLocal {
                position: Authority::Known(self.point),
            },
            Authority::Known(PointerCaptureOwner::None),
        )
        .ending_stream();
        let journal = PointerEdgeJournal::new(
            PointerEdgeSequence::new(self.watermark),
            PointerEdgeSequence::new(sequence),
            vec![edge],
        )
        .expect("terminal pointer journal is contiguous");
        let mut frame = self.host.begin(&self.engine);
        frame
            .submit_pointer_journal(self.provider, journal)
            .expect("terminal pointer journal stages");
        let candidate = frame
            .pointer_receiver_candidates()
            .expect("terminal pointer edge has an exact candidate")
            .candidates()[0]
            .clone();
        frame
            .submit_pointer_receiver_receipts(
                PointerReceiverReceiptBatch::new([
                    candidate.receipt(PointerReceiverObservation::NotApplicable)
                ])
                .expect("terminal pointer receipt roster is exact"),
            )
            .expect("terminal pointer receipt reduces");
        complete_host_frame_with_retained_or_unavailable(&self.engine, &mut frame);
        let transition = self.host.finish(frame, &mut self.engine);
        self.watermark = sequence;
        transition
    }

    fn stage_empty_pointer(&self, frame: &mut CoreHostFrame) {
        let watermark = PointerEdgeSequence::new(self.watermark);
        frame
            .submit_pointer_journal(
                self.provider,
                PointerEdgeJournal::new(watermark, watermark, Vec::new())
                    .expect("empty pointer journal preserves its watermark"),
            )
            .expect("empty pointer journal stages");
        frame
            .submit_pointer_receiver_receipts(
                PointerReceiverReceiptBatch::new(Vec::<PointerReceiverReceipt>::new())
                    .expect("empty journal has an empty receipt roster"),
            )
            .expect("empty receipt roster reduces");
    }

    fn compile_surface(&mut self, width: f64) -> dockspace::transition::EngineTransition {
        let contribution =
            measurements(&self.engine, SURFACE, bounds(width), measurement_profile());
        let mut frame = self.host.begin(&self.engine);
        self.stage_empty_pointer(&mut frame);
        let token = frame
            .view()
            .begin_surface_contribution(SURFACE)
            .expect("surface contribution begins in the frozen roster");
        let contribution = frame
            .view()
            .prepare_surface_contribution(token, contribution)
            .expect("replacement surface measurements are complete");
        frame
            .push_surface_contribution(contribution)
            .expect("replacement surface contribution is unique");
        self.host.finish(frame, &mut self.engine)
    }

    fn emit_current_surface(&mut self) -> dockspace::transition::EngineTransition {
        let mut frame = self.host.begin(&self.engine);
        self.stage_empty_pointer(&mut frame);
        let frame = complete_host_frame_with_current_outputs(&self.engine, frame);
        self.host.finish_presentation(frame, &mut self.engine)
    }

    fn observe_emission(&mut self) -> dockspace::transition::EngineTransition {
        let mut frame = self.host.begin(&self.engine);
        self.stage_empty_pointer(&mut frame);
        complete_host_frame_with_retained_or_unavailable(&self.engine, &mut frame);
        self.host.finish(frame, &mut self.engine)
    }
}

fn line_delta(x: f64, y: f64) -> ScrollDelta {
    ScrollDelta::Lines(FiniteScrollVector::new(x, y).expect("scroll delta is finite"))
}

fn scroll_terminal_reasons(
    transition: &dockspace::transition::EngineTransition,
) -> Vec<ScrollTerminationReason> {
    transition
        .interaction_events()
        .iter()
        .filter_map(|event| match event.kind() {
            InteractionEventKind::ScrollTerminated { reason, .. } => Some(*reason),
            _ => None,
        })
        .collect()
}

#[test]
fn smooth_continuation_freezes_its_receiver_without_a_pointer_position() {
    let mut fixture = ScrollFixture::new();
    let token = ScrollSequenceToken::new(41);
    fixture.submit_known(ScrollPhase::Begin, Some(token), Some(line_delta(1.0, 0.0)));

    let sequence = fixture.watermark + 1;
    let scroll = ScrollEdge::new(
        ScrollDeviceId::new(1),
        Some(token),
        ScrollPhase::Update,
        Some(line_delta(-1.0, 0.0)),
        Authority::Known(ScrollMomentum::Direct),
        Authority::Known(ScrollModifiers::default()),
        Authority::Known(fixture.endpoint),
    )
    .expect("smooth continuation is valid");
    let edge = PointerEdge::new(
        PointerEdgeSequence::new(sequence),
        POINTER,
        PointerEdgeKind::Scrolled(scroll),
        PointerEdgeLocation::SurfaceLocal {
            position: Authority::Unknown(AuthorityUnavailableReason::CoordinateUnavailable),
        },
        Authority::Known(PointerCaptureOwner::None),
    );
    let journal = PointerEdgeJournal::new(
        PointerEdgeSequence::new(fixture.watermark),
        PointerEdgeSequence::new(sequence),
        vec![edge],
    )
    .expect("continuation journal is contiguous");
    let mut frame = fixture.host.begin(&fixture.engine);
    frame
        .submit_pointer_journal(fixture.provider, journal)
        .expect("continuation stages");

    let candidate = &frame
        .pointer_receiver_candidates()
        .expect("continuation requests current presentation proof")
        .candidates()[0];
    assert_eq!(candidate.route_point(), None);
    let Some(ScrollReceiverChallenge::Locked {
        receiver,
        probe_point,
        projected_delta,
    }) = candidate.scroll_challenge()
    else {
        panic!("the active sequence must request its frozen receiver");
    };
    assert_eq!(receiver, fixture.region);
    assert!(
        fixture
            .engine
            .interaction_projection(SURFACE)
            .expect("the scroll owner has a current projection")
            .hit_manifest()
            .region(receiver)
            .expect("the locked receiver remains in the current manifest")
            .hit()
            .contains(probe_point)
    );
    assert_eq!(
        projected_delta,
        Some(FiniteScrollVector::new(-1.0, 0.0).expect("probe vector is finite"))
    );
}

#[test]
fn exact_ready_output_without_the_locked_receiver_terminates_smooth_scroll() {
    let mut fixture = ScrollFixture::new();
    let token = ScrollSequenceToken::new(41);
    let began = fixture.submit_known(ScrollPhase::Begin, Some(token), Some(line_delta(1.0, 0.0)));
    assert!(matches!(
        began.reduced_pointer_edges()[0].interaction_outcomes(),
        [
            InteractionOutcome::Scroll(ScrollReductionOutcome::Began { .. }),
            InteractionOutcome::Scroll(ScrollReductionOutcome::Applied(_)),
        ]
    ));

    let compiled = fixture.compile_surface(900.0);
    assert!(scroll_terminal_reasons(&compiled).is_empty());
    let ready = fixture
        .engine
        .scene()
        .surface(SURFACE)
        .and_then(SurfaceScene::ready)
        .expect("wide surface compiled an exact Ready output");
    assert_eq!(
        ready.plan().tab_bar_records()[0].maximum_scroll_offset(),
        0.0
    );

    let emitted = fixture.emit_current_surface();
    assert!(scroll_terminal_reasons(&emitted).is_empty());
    let observed = fixture.observe_emission();
    assert_eq!(
        scroll_terminal_reasons(&observed),
        [ScrollTerminationReason::ReceiverLost]
    );
    assert!(
        fixture
            .engine
            .interaction_projection(SURFACE)
            .expect("wide surface has current interaction authority")
            .hit_manifest()
            .regions()
            .iter()
            .all(|region| !matches!(
                region.id().kind(),
                PresentationHitRegionKind::TabStripScroll(_)
            ))
    );
}

#[test]
fn semantically_identical_emission_refresh_preserves_smooth_scroll_owner() {
    let mut fixture = ScrollFixture::new();
    let token = ScrollSequenceToken::new(41);
    fixture.submit_known(ScrollPhase::Begin, Some(token), Some(line_delta(1.0, 0.0)));

    let emitted = fixture.emit_current_surface();
    assert!(scroll_terminal_reasons(&emitted).is_empty());
    let observed = fixture.observe_emission();
    assert!(scroll_terminal_reasons(&observed).is_empty());

    let update = fixture.submit_known(
        ScrollPhase::Update,
        Some(token),
        Some(line_delta(-1.0, 0.0)),
    );
    assert!(matches!(
        update.reduced_pointer_edges()[0].interaction_outcomes(),
        [InteractionOutcome::Scroll(ScrollReductionOutcome::Applied(application))]
            if application.requested_delta() == 40.0
                && application.applied_delta() == 40.0
                && application.offset() == 40.0
    ));

    let compiled = fixture.compile_surface(260.0);
    assert!(scroll_terminal_reasons(&compiled).is_empty());
    let emitted = fixture.emit_current_surface();
    assert!(scroll_terminal_reasons(&emitted).is_empty());
    let observed = fixture.observe_emission();
    assert!(scroll_terminal_reasons(&observed).is_empty());

    let ended = fixture.submit_known(ScrollPhase::End, Some(token), None);
    assert!(matches!(
        ended.reduced_pointer_edges()[0].interaction_outcomes(),
        [InteractionOutcome::Scroll(
            ScrollReductionOutcome::Terminated {
                reason: ScrollTerminationReason::Completed,
                ..
            }
        )]
    ));
}

#[test]
fn ordinary_pointer_stream_end_terminates_smooth_scroll_exactly_once() {
    let mut fixture = ScrollFixture::new();
    let token = ScrollSequenceToken::new(41);
    let began = fixture.submit_known(ScrollPhase::Begin, Some(token), None);
    let first_session = match began.reduced_pointer_edges()[0].interaction_outcomes() {
        [InteractionOutcome::Scroll(ScrollReductionOutcome::Began { session, .. })] => *session,
        outcomes => panic!("smooth scroll must freeze its begin receiver, got {outcomes:?}"),
    };
    let second = fixture.submit_known_for_device(
        ScrollDeviceId::new(2),
        ScrollPhase::Begin,
        Some(token),
        None,
    );
    let second_session = match second.reduced_pointer_edges()[0].interaction_outcomes() {
        [InteractionOutcome::Scroll(ScrollReductionOutcome::Began { session, .. })] => *session,
        outcomes => panic!("second smooth scroll must freeze its begin receiver, got {outcomes:?}"),
    };

    let terminal = fixture.end_pointer_stream();
    let terminal_outcomes = terminal.reduced_pointer_edges()[0].interaction_outcomes();
    assert_eq!(terminal_outcomes.len(), 2);
    for session in [first_session, second_session] {
        assert!(terminal_outcomes.iter().any(|outcome| matches!(
            outcome,
            InteractionOutcome::Scroll(ScrollReductionOutcome::Terminated {
                session: actual,
                reason: ScrollTerminationReason::StreamCancelled,
                ..
            }) if *actual == session
        )));
    }

    let successor = fixture.submit_known(ScrollPhase::Begin, Some(token), None);
    assert!(matches!(
        successor.reduced_pointer_edges()[0].interaction_outcomes(),
        [InteractionOutcome::Scroll(
            ScrollReductionOutcome::Began { session: actual, .. }
        )] if *actual != first_session && *actual != second_session
    ));

    let retired = fixture
        .engine
        .retire_pointer_provider(fixture.provider)
        .expect("provider retirement remains valid after the stream terminal");
    assert_eq!(
        scroll_terminal_reasons(&retired),
        [ScrollTerminationReason::ProviderRetired]
    );
}

#[test]
fn smooth_scroll_end_without_delta_terminates_its_frozen_owner() {
    let mut fixture = ScrollFixture::new();
    let token = ScrollSequenceToken::new(41);
    let began = fixture.submit_known(ScrollPhase::Begin, Some(token), None);
    let session = match began.reduced_pointer_edges()[0].interaction_outcomes() {
        [InteractionOutcome::Scroll(ScrollReductionOutcome::Began { session, .. })] => *session,
        outcomes => panic!("smooth scroll must freeze its begin receiver, got {outcomes:?}"),
    };

    let ended = fixture.submit_known(ScrollPhase::End, Some(token), None);
    assert!(matches!(
        ended.reduced_pointer_edges()[0].interaction_outcomes(),
        [InteractionOutcome::Scroll(
            ScrollReductionOutcome::Terminated {
                session: actual,
                receiver: Some(receiver),
                reason: ScrollTerminationReason::Completed,
            }
        )] if *actual == session && *receiver == fixture.region
    ));
}

#[test]
fn zero_deltas_keep_the_smooth_scroll_receiver_locked() {
    let mut fixture = ScrollFixture::new();
    let token = ScrollSequenceToken::new(41);
    let began = fixture.submit_known(ScrollPhase::Begin, Some(token), Some(line_delta(0.0, 0.0)));
    let session = match began.reduced_pointer_edges()[0].interaction_outcomes() {
        [
            InteractionOutcome::Scroll(ScrollReductionOutcome::Began { session, .. }),
            InteractionOutcome::Scroll(ScrollReductionOutcome::Applied(_)),
        ] => *session,
        outcomes => panic!("zero-delta begin must freeze its receiver, got {outcomes:?}"),
    };

    let update = fixture.submit_known(ScrollPhase::Update, Some(token), Some(line_delta(0.0, 0.0)));
    assert!(matches!(
        update.reduced_pointer_edges()[0].interaction_outcomes(),
        [InteractionOutcome::Scroll(ScrollReductionOutcome::Applied(application))]
            if application.session() == Some(session)
                && application.requested_delta() == 0.0
                && application.applied_delta() == 0.0
    ));

    let directional = fixture.submit_known(
        ScrollPhase::Update,
        Some(token),
        Some(line_delta(-1.0, 0.0)),
    );
    assert!(matches!(
        directional.reduced_pointer_edges()[0].interaction_outcomes(),
        [InteractionOutcome::Scroll(ScrollReductionOutcome::Applied(application))]
            if application.session() == Some(session)
    ));
}

#[test]
fn semantic_termination_waits_for_the_provider_terminal_without_reopening() {
    let mut fixture = ScrollFixture::new();
    let token = ScrollSequenceToken::new(41);
    fixture.submit_known(ScrollPhase::Begin, Some(token), Some(line_delta(1.0, 0.0)));

    let terminated = fixture.submit_with_disposition(
        ScrollDeviceId::new(1),
        ScrollPhase::Update,
        Some(token),
        Some(line_delta(1.0, 0.0)),
        PointerReceiverDeliveryDisposition::NoReceiver,
    );
    assert!(matches!(
        terminated.reduced_pointer_edges()[0].interaction_outcomes(),
        [InteractionOutcome::Scroll(
            ScrollReductionOutcome::Terminated {
                reason: ScrollTerminationReason::ReceiverLost,
                ..
            }
        )]
    ));

    let late_update = fixture.submit_with_receipt_and_modifiers_and_inspect(
        ScrollDeviceId::new(1),
        ScrollPhase::Update,
        Some(token),
        Some(line_delta(1.0, 0.0)),
        fixture.endpoint,
        None,
        ScrollModifiers::default(),
        |candidate| {
            assert_eq!(
                candidate.scroll_challenge(),
                Some(ScrollReceiverChallenge::OwnedTerminal)
            );
            assert!(!candidate.receiver_is_applicable());
        },
    );
    assert!(
        late_update.reduced_pointer_edges()[0]
            .interaction_outcomes()
            .is_empty(),
        "an already terminated semantic session must not resume"
    );

    let provider_terminal = fixture.submit_known(ScrollPhase::End, Some(token), None);
    assert!(
        provider_terminal.reduced_pointer_edges()[0]
            .interaction_outcomes()
            .is_empty(),
        "the provider terminal consumes the tombstone without a second semantic terminal"
    );

    let successor =
        fixture.submit_known(ScrollPhase::Begin, Some(ScrollSequenceToken::new(42)), None);
    assert!(matches!(
        successor.reduced_pointer_edges()[0].interaction_outcomes(),
        [InteractionOutcome::Scroll(
            ScrollReductionOutcome::Began { .. }
        )]
    ));
}

#[test]
fn binding_retirement_waits_for_the_physical_scroll_terminal() {
    let mut fixture = ScrollFixture::new();
    let token = ScrollSequenceToken::new(41);
    let began = fixture.submit_known(ScrollPhase::Begin, Some(token), Some(line_delta(1.0, 0.0)));
    let session = match began.reduced_pointer_edges()[0].interaction_outcomes() {
        [
            InteractionOutcome::Scroll(ScrollReductionOutcome::Began { session, .. }),
            InteractionOutcome::Scroll(ScrollReductionOutcome::Applied(_)),
        ] => *session,
        outcomes => panic!("smooth scroll must freeze its begin receiver, got {outcomes:?}"),
    };

    let retired = fixture.submit_known(
        ScrollPhase::Cancel(ScrollCancelReason::BindingRetired),
        Some(token),
        None,
    );
    assert!(matches!(
        retired.reduced_pointer_edges()[0].interaction_outcomes(),
        [InteractionOutcome::Scroll(
            ScrollReductionOutcome::Terminated {
                session: actual,
                reason: ScrollTerminationReason::Cancelled(
                    ScrollCancelReason::BindingRetired
                ),
                ..
            }
        )] if *actual == session
    ));

    let late_update =
        fixture.submit_known(ScrollPhase::Update, Some(token), Some(line_delta(1.0, 0.0)));
    assert!(
        late_update.reduced_pointer_edges()[0]
            .interaction_outcomes()
            .is_empty(),
        "a late provider tail cannot reopen the retired binding owner"
    );

    let provider_terminal = fixture.submit_known(ScrollPhase::End, Some(token), None);
    assert!(
        provider_terminal.reduced_pointer_edges()[0]
            .interaction_outcomes()
            .is_empty(),
        "the physical terminal only consumes the retired-tail tombstone"
    );

    let successor =
        fixture.submit_known(ScrollPhase::Begin, Some(ScrollSequenceToken::new(42)), None);
    assert!(matches!(
        successor.reduced_pointer_edges()[0].interaction_outcomes(),
        [InteractionOutcome::Scroll(
            ScrollReductionOutcome::Began { .. }
        )]
    ));
}

#[test]
fn framework_reserved_update_releases_the_owned_derivative_until_provider_terminal() {
    let mut fixture = ScrollFixture::new();
    let token = ScrollSequenceToken::new(41);
    fixture.submit_known(ScrollPhase::Begin, Some(token), Some(line_delta(1.0, 0.0)));

    let modified = fixture.submit_with_receipt_and_modifiers_and_inspect(
        ScrollDeviceId::new(1),
        ScrollPhase::Update,
        Some(token),
        Some(line_delta(1.0, 0.0)),
        fixture.endpoint,
        None,
        ScrollModifiers::new(false, true, false, false),
        |candidate| {
            assert_eq!(
                candidate.scroll_challenge(),
                Some(ScrollReceiverChallenge::FrameworkReserved)
            );
            assert!(!candidate.receiver_is_applicable());
        },
    );
    assert!(matches!(
        modified.reduced_pointer_edges()[0].interaction_outcomes(),
        [InteractionOutcome::Scroll(
            ScrollReductionOutcome::Terminated {
                reason: ScrollTerminationReason::ReceiverLost,
                ..
            }
        )]
    ));

    let late_update = fixture.submit_with_receipt_and_modifiers_and_inspect(
        ScrollDeviceId::new(1),
        ScrollPhase::Update,
        Some(token),
        Some(line_delta(1.0, 0.0)),
        fixture.endpoint,
        None,
        ScrollModifiers::default(),
        |candidate| {
            assert_eq!(
                candidate.scroll_challenge(),
                Some(ScrollReceiverChallenge::Unavailable)
            );
            assert!(!candidate.receiver_is_applicable());
        },
    );
    assert!(
        late_update.reduced_pointer_edges()[0]
            .interaction_outcomes()
            .is_empty()
    );

    let terminal = fixture.submit_with_receipt(
        ScrollDeviceId::new(1),
        ScrollPhase::End,
        Some(token),
        None,
        fixture.endpoint,
        None,
    );
    assert!(
        terminal.reduced_pointer_edges()[0]
            .interaction_outcomes()
            .is_empty()
    );
}

#[test]
fn zero_delta_endpoint_change_terminates_and_cannot_be_hidden_by_a_return_to_origin() {
    let mut fixture = ScrollFixture::new();
    let token = ScrollSequenceToken::new(41);
    fixture.submit_known(ScrollPhase::Begin, Some(token), Some(line_delta(0.0, 0.0)));
    let changed_endpoint = ScrollDeliveryEndpoint::new(
        fixture.host.lease(),
        SURFACE,
        None,
        fixture
            .endpoint
            .coordinate_generation()
            .checked_next()
            .expect("the test coordinate generation can advance"),
    )
    .expect("the changed endpoint stays in the same authority domain");

    let changed = fixture.submit_unknown_at_endpoint(
        ScrollPhase::Update,
        token,
        line_delta(0.0, 0.0),
        changed_endpoint,
    );
    assert!(matches!(
        changed.reduced_pointer_edges()[0].interaction_outcomes(),
        [InteractionOutcome::Scroll(
            ScrollReductionOutcome::Terminated {
                reason: ScrollTerminationReason::DeliveryEndpointChanged,
                ..
            }
        )]
    ));

    let returned = fixture.submit_known(
        ScrollPhase::Update,
        Some(token),
        Some(line_delta(-1.0, 0.0)),
    );
    assert!(
        returned.reduced_pointer_edges()[0]
            .interaction_outcomes()
            .is_empty(),
        "returning to the original endpoint cannot resurrect a terminated session"
    );
    let terminal = fixture.submit_known(ScrollPhase::End, Some(token), None);
    assert!(
        terminal.reduced_pointer_edges()[0]
            .interaction_outcomes()
            .is_empty()
    );
}
