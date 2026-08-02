mod support;

use dockspace::engine::{CoreHostFrame, DockEngine};
use dockspace::geometry::{LogicalPoint, LogicalRect};
use dockspace::graph::{ContainedFloating, Node, RootRecord, SurfacePresentation, Workspace};
use dockspace::ids::{FloatingPresentationId, ItemId, RootId, SurfaceId};
use dockspace::intent::{Authority, PointerButton, PointerId};
use dockspace::interaction::{InteractionOutcome, InteractionStatus};
use dockspace::pointer_journal::{
    PointerCaptureOwner, PointerEdge, PointerEdgeJournal, PointerEdgeKind, PointerEdgeLocation,
    PointerEdgeSequence, PointerInputLease, PointerProviderScope, SurfaceLocalPointerEndpoint,
    SurfaceLocalPointerScope,
};
use dockspace::pointer_receiver::{
    PointerReceiverDelivery, PointerReceiverDeliveryDisposition, PointerReceiverObservation,
    PointerReceiverProbe, PointerReceiverProbeReceipt, PointerReceiverReceiptBatch,
    PresentedPointerReceiverObservation,
};
use dockspace::policy::DockPolicy;
use dockspace::presentation_config::DockPresentationConfig;
use dockspace::presentation_hit::{PresentationHitRegionId, PresentationHitRegionKind};
use dockspace::scene::ContainedResizeDirection;
use dockspace::transition::EngineTransition;
use support::{
    TestPresentationHost, complete_host_frame_with_current_outputs,
    complete_host_frame_with_retained_or_unavailable, publish_surface,
};

const SURFACE: SurfaceId = SurfaceId::new(1);
const MAIN_ROOT: RootId = RootId::new(1);
const FLOATING_ROOT: RootId = RootId::new(2);
const FRONT_ROOT: RootId = RootId::new(3);
const FLOATING: FloatingPresentationId = FloatingPresentationId::new(1);
const FRONT: FloatingPresentationId = FloatingPresentationId::new(2);
const POINTER: PointerId = PointerId::new(7);

struct Fixture {
    engine: DockEngine,
    host: TestPresentationHost,
    provider: PointerInputLease,
    watermark: u64,
}

fn point(x: f64, y: f64) -> LogicalPoint {
    LogicalPoint::new(x, y).expect("test point must be finite")
}

fn rect(x: f64, y: f64, width: f64, height: f64) -> LogicalRect {
    LogicalRect::new(x, y, width, height).expect("test rectangle must be valid")
}

fn initial_rect() -> LogicalRect {
    rect(120.0, 70.0, 120.0, 90.0)
}

fn presentation_config() -> DockPresentationConfig {
    DockPresentationConfig::builder()
        .tab_bar_height(24.0)
        .tab_group_grip_extent(8.0)
        .tab_min_width(16.0)
        .tab_close_extent(8.0)
        .guide_extent(16.0)
        .guide_gap(2.0)
        .guide_hit_padding(0.0)
        .guide_outer_inset(8.0)
        .floating_title_height(24.0)
        .floating_border_width(4.0)
        .minimum_pane_size(40.0, 30.0)
        .minimum_floating_size(40.0, 30.0)
        .build()
        .expect("test presentation config must be valid")
}

fn workspace(contained: LogicalRect, with_front_peer: bool) -> Workspace {
    let mut builder = Workspace::builder();
    let main = builder.insert_node(Node::tabs([ItemId::new(1)]));
    let floating = builder.insert_node(Node::tabs([ItemId::new(2)]));
    builder.set_root(MAIN_ROOT, RootRecord::new(main));
    builder.set_root(FLOATING_ROOT, RootRecord::new(floating));
    builder.set_surface(SURFACE, SurfacePresentation::with_main(MAIN_ROOT));
    builder.set_contained_floating(FLOATING, ContainedFloating::new(FLOATING_ROOT, contained));
    builder
        .attach_contained(SURFACE, FLOATING)
        .expect("contained root must attach");
    if with_front_peer {
        let front = builder.insert_node(Node::tabs([ItemId::new(3)]));
        builder.set_root(FRONT_ROOT, RootRecord::new(front));
        builder.set_contained_floating(
            FRONT,
            ContainedFloating::new(FRONT_ROOT, rect(260.0, 130.0, 110.0, 90.0)),
        );
        builder
            .attach_contained(SURFACE, FRONT)
            .expect("front peer must attach");
    }
    builder.build().expect("test workspace must be valid")
}

fn fixture(policy: DockPolicy, with_front_peer: bool) -> Fixture {
    let mut engine = DockEngine::new_with_presentation_config(
        workspace(initial_rect(), with_front_peer),
        policy,
        presentation_config(),
    )
    .expect("test engine must be valid");
    let mut host = TestPresentationHost::new(&mut engine);
    publish_surface(
        &mut engine,
        &mut host,
        SURFACE,
        rect(100.0, 50.0, 300.0, 200.0),
    );
    let provider = engine
        .create_pointer_provider(
            PointerProviderScope::SurfaceLocal(SurfaceLocalPointerScope::new(
                host.lease(),
                SurfaceLocalPointerEndpoint::Logical(SURFACE),
            )),
            PointerEdgeSequence::new(0),
        )
        .expect("surface-local pointer provider must be admitted");
    Fixture {
        engine,
        host,
        provider,
        watermark: 0,
    }
}

fn chrome_point(
    fixture: &Fixture,
    kind: impl Fn(PresentationHitRegionKind) -> bool,
) -> (PresentationHitRegionId, LogicalPoint) {
    let projection = fixture
        .engine
        .interaction_projection(SURFACE)
        .expect("fixture surface must be receiver-authoritative");
    let region = projection
        .hit_manifest()
        .regions()
        .iter()
        .find(|region| kind(region.id().kind()))
        .expect("requested contained chrome must be published");
    let bounds = region.hit().rect();
    (
        region.id(),
        point(
            bounds.x() + bounds.width() * 0.5,
            bounds.y() + bounds.height() * 0.5,
        ),
    )
}

fn contained_title_point(fixture: &Fixture) -> (PresentationHitRegionId, LogicalPoint) {
    chrome_point(
        fixture,
        |kind| matches!(kind, PresentationHitRegionKind::ContainedTitle(actual) if actual == FLOATING),
    )
}

fn contained_resize_point(
    fixture: &Fixture,
    direction: ContainedResizeDirection,
) -> (PresentationHitRegionId, LogicalPoint) {
    chrome_point(fixture, |kind| {
        matches!(
            kind,
            PresentationHitRegionKind::ContainedResize {
                floating: FLOATING,
                direction: actual,
            } if actual == direction
        )
    })
}

fn contained_minimum(fixture: &Fixture) -> dockspace::geometry::LogicalSize {
    fixture
        .engine
        .interaction_projection(SURFACE)
        .expect("fixture surface must be receiver-authoritative")
        .plan()
        .contained_records()
        .iter()
        .find(|record| record.floating() == FLOATING)
        .expect("contained presentation must be projected")
        .minimum_size()
}

fn edge_journal(
    previous: u64,
    kind: PointerEdgeKind,
    position: LogicalPoint,
    capture: PointerCaptureOwner,
) -> PointerEdgeJournal {
    let sequence = PointerEdgeSequence::new(previous + 1);
    PointerEdgeJournal::new(
        PointerEdgeSequence::new(previous),
        sequence,
        vec![PointerEdge::new(
            sequence,
            POINTER,
            kind,
            PointerEdgeLocation::SurfaceLocal {
                position: Authority::Known(position),
            },
            Authority::Known(capture),
        )],
    )
    .expect("single-edge journal must be contiguous")
}

fn submit_edge(
    fixture: &mut Fixture,
    kind: PointerEdgeKind,
    position: LogicalPoint,
    capture: PointerCaptureOwner,
    delivery: Option<PresentationHitRegionId>,
) -> EngineTransition {
    let mut frame = fixture.host.begin(&fixture.engine);
    frame
        .submit_pointer_journal(
            fixture.provider,
            edge_journal(fixture.watermark, kind, position, capture),
        )
        .expect("pointer edge must prepare");
    let candidate = frame
        .pointer_receiver_candidates()
        .expect("pointer edge must freeze a receiver challenge")
        .candidates()[0]
        .clone();
    let observation = receiver_observation(&frame, candidate.probes(), delivery, position);
    frame
        .submit_pointer_receiver_receipts(
            PointerReceiverReceiptBatch::new([candidate.receipt(observation)])
                .expect("pointer receipt roster must be exact"),
        )
        .expect("pointer receipt must stage");
    complete_host_frame_with_retained_or_unavailable(&fixture.engine, &mut frame);
    let transition = fixture.host.finish(frame, &mut fixture.engine);
    fixture.watermark += 1;
    transition
}

fn receiver_observation(
    frame: &CoreHostFrame,
    probes: dockspace::pointer_receiver::PointerReceiverProbeRequest,
    delivery: Option<PresentationHitRegionId>,
    position: LogicalPoint,
) -> PointerReceiverObservation {
    if probes.is_not_applicable() {
        return PointerReceiverObservation::NotApplicable;
    }
    let mut receipts = Vec::new();
    if probes.requires(PointerReceiverProbe::Delivery) {
        let projection = frame
            .view()
            .interaction_projection(SURFACE)
            .expect("delivery requires current interaction authority");
        let region = delivery.expect("delivery probe requires an exact receiver");
        receipts.push(PointerReceiverProbeReceipt::Delivery(
            PointerReceiverDelivery::new(
                projection,
                PointerReceiverDeliveryDisposition::Dock(region),
            )
            .expect("claimed delivery region must be executable"),
        ));
    }
    if probes.requires(PointerReceiverProbe::HoverHit) {
        receipts.push(PointerReceiverProbeReceipt::HoverHit(
            frame
                .view()
                .resolve_hover_drop_receiver(SURFACE, position)
                .expect("core must resolve the exact hover-drop receiver"),
        ));
    }
    PointerReceiverObservation::Presented(
        PresentedPointerReceiverObservation::new(receipts)
            .expect("each requested probe must have exactly one answer"),
    )
}

fn paint_active_preview(fixture: &mut Fixture) {
    let watermark = PointerEdgeSequence::new(fixture.watermark);
    let mut frame = fixture.host.begin(&fixture.engine);
    frame
        .submit_pointer_journal(
            fixture.provider,
            PointerEdgeJournal::new(watermark, watermark, Vec::new())
                .expect("empty journal must retain its watermark"),
        )
        .expect("empty pointer checkpoint must stage");
    frame
        .submit_pointer_receiver_receipts(
            PointerReceiverReceiptBatch::new(Vec::new())
                .expect("empty journal has no receiver challenges"),
        )
        .expect("empty receiver receipt roster must stage");
    let frame = complete_host_frame_with_current_outputs(&fixture.engine, frame);
    fixture.host.finish_presentation(frame, &mut fixture.engine);
}

fn begin_resize(fixture: &mut Fixture, direction: ContainedResizeDirection) -> LogicalPoint {
    let (region, press) = contained_resize_point(fixture, direction);
    let transition = submit_edge(
        fixture,
        PointerEdgeKind::ButtonPressed(PointerButton::Primary),
        press,
        PointerCaptureOwner::ProviderEndpoint,
        Some(region),
    );
    assert!(matches!(
        transition.reduced_pointer_edges()[0].interaction_outcomes(),
        [InteractionOutcome::ContainedTransformBegan { .. }]
    ));
    press
}

fn update_transform(fixture: &mut Fixture, position: LogicalPoint) -> InteractionOutcome {
    let transition = submit_edge(
        fixture,
        PointerEdgeKind::Moved,
        position,
        PointerCaptureOwner::ProviderEndpoint,
        None,
    );
    transition.reduced_pointer_edges()[0]
        .interaction_outcomes()
        .last()
        .expect("active pointer motion must publish an interaction outcome")
        .clone()
}

#[test]
fn journal_resize_edges_hold_opposite_anchors_and_clamp_to_minimum_and_bounds() {
    let cases = [
        (
            ContainedResizeDirection::West,
            (200.0, 0.0),
            rect(200.0, 70.0, 40.0, 90.0),
        ),
        (
            ContainedResizeDirection::East,
            (300.0, 0.0),
            rect(120.0, 70.0, 280.0, 90.0),
        ),
        (
            ContainedResizeDirection::North,
            (0.0, -200.0),
            rect(120.0, 50.0, 120.0, 110.0),
        ),
        (
            ContainedResizeDirection::South,
            (0.0, 300.0),
            rect(120.0, 70.0, 120.0, 180.0),
        ),
        (
            ContainedResizeDirection::NorthWest,
            (-200.0, -200.0),
            rect(100.0, 50.0, 140.0, 110.0),
        ),
        (
            ContainedResizeDirection::NorthEast,
            (300.0, -200.0),
            rect(120.0, 50.0, 280.0, 110.0),
        ),
        (
            ContainedResizeDirection::SouthEast,
            (300.0, 300.0),
            rect(120.0, 70.0, 280.0, 180.0),
        ),
        (
            ContainedResizeDirection::SouthWest,
            (-200.0, 300.0),
            rect(100.0, 70.0, 140.0, 180.0),
        ),
    ];

    for (direction, (dx, dy), expected) in cases {
        let mut fixture = fixture(DockPolicy::default(), false);
        let expected = if direction == ContainedResizeDirection::West {
            let minimum = contained_minimum(&fixture);
            rect(
                initial_rect().x() + initial_rect().width() - minimum.width(),
                initial_rect().y(),
                minimum.width(),
                initial_rect().height(),
            )
        } else {
            expected
        };
        let press = begin_resize(&mut fixture, direction);
        let outcome = update_transform(&mut fixture, point(press.x() + dx, press.y() + dy));
        assert!(
            matches!(
                outcome,
                InteractionOutcome::ContainedTransformPreviewUpdated { preview, .. }
                    if preview.rect() == expected
            ),
            "unexpected {direction:?} resize outcome: {outcome:?}; expected {expected:?}"
        );
        assert_eq!(
            fixture
                .engine
                .workspace()
                .contained_floating(FLOATING)
                .expect("durable contained record must remain")
                .rect,
            initial_rect(),
            "pointer motion publishes a preview without mutating durable state"
        );
    }
}

#[test]
fn rear_contained_chrome_press_atomically_raises_before_starting_each_gesture() {
    let directions = [
        None,
        Some(ContainedResizeDirection::NorthWest),
        Some(ContainedResizeDirection::North),
        Some(ContainedResizeDirection::NorthEast),
        Some(ContainedResizeDirection::East),
        Some(ContainedResizeDirection::SouthEast),
        Some(ContainedResizeDirection::South),
        Some(ContainedResizeDirection::SouthWest),
        Some(ContainedResizeDirection::West),
    ];

    for direction in directions {
        let mut fixture = fixture(DockPolicy::default(), true);
        assert_eq!(
            fixture
                .engine
                .workspace()
                .surface(SURFACE)
                .expect("surface must exist")
                .contained,
            [FLOATING, FRONT]
        );
        let (region, press) = direction.map_or_else(
            || contained_title_point(&fixture),
            |direction| contained_resize_point(&fixture, direction),
        );
        let transition = submit_edge(
            &mut fixture,
            PointerEdgeKind::ButtonPressed(PointerButton::Primary),
            press,
            PointerCaptureOwner::ProviderEndpoint,
            Some(region),
        );

        assert_eq!(
            fixture
                .engine
                .workspace()
                .surface(SURFACE)
                .expect("surface must remain")
                .contained,
            [FRONT, FLOATING],
            "activation and raise must publish atomically"
        );
        if direction.is_some() {
            assert!(matches!(
                transition.reduced_pointer_edges()[0].interaction_outcomes(),
                [InteractionOutcome::ContainedTransformBegan { .. }]
            ));
            assert!(matches!(
                fixture.engine.interaction().status(),
                InteractionStatus::ContainedTransforming { .. }
            ));
        } else {
            assert!(matches!(
                transition.reduced_pointer_edges()[0].interaction_outcomes(),
                [InteractionOutcome::DragArmed { .. }]
            ));
            assert!(matches!(
                fixture.engine.interaction().status(),
                InteractionStatus::Armed { .. }
            ));
        }
    }
}

#[test]
fn existing_contained_remains_editable_when_future_creation_is_disabled() {
    let mut policy = DockPolicy::default();
    policy.set_allow_contained_floating(false);
    let mut fixture = fixture(policy, false);
    begin_resize(&mut fixture, ContainedResizeDirection::East);
    assert!(matches!(
        fixture.engine.interaction().status(),
        InteractionStatus::ContainedTransforming { .. }
    ));
}

#[test]
fn journal_contained_move_commits_the_exact_painted_preview() {
    let mut fixture = fixture(DockPolicy::default(), false);
    let (title, press) = contained_title_point(&fixture);
    submit_edge(
        &mut fixture,
        PointerEdgeKind::ButtonPressed(PointerButton::Primary),
        press,
        PointerCaptureOwner::ProviderEndpoint,
        Some(title),
    );
    let moved = point(press.x() + 30.0, press.y() + 20.0);
    let update = update_transform(&mut fixture, moved);
    let expected = rect(150.0, 90.0, 120.0, 90.0);
    assert!(matches!(
        update,
        InteractionOutcome::PreviewUpdated { preview: Some(ref preview), .. }
            if matches!(
                preview.visual(),
                dockspace::interaction::PreviewVisual::Contained { rect, .. }
                    if *rect == expected
            )
    ));
    paint_active_preview(&mut fixture);
    let release = submit_edge(
        &mut fixture,
        PointerEdgeKind::ButtonReleased(PointerButton::Primary),
        moved,
        PointerCaptureOwner::None,
        None,
    );
    assert!(matches!(
        release.reduced_pointer_edges()[0].interaction_outcomes(),
        [
            InteractionOutcome::PreviewUpdated { .. },
            InteractionOutcome::DragDelivered { .. }
        ]
    ));
    assert_eq!(
        fixture
            .engine
            .workspace()
            .contained_floating(FLOATING)
            .expect("contained presentation must remain")
            .rect,
        expected
    );
}
