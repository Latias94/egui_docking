mod support;

use dockspace::command::Edge;
use dockspace::drop_guide::{DropGuideScope, DropGuideSlot};
use dockspace::drop_resolver::DropGuideEligibility;
use dockspace::drop_target::DropTargetId;
use dockspace::engine::{CoreHostFrame, DockEngine};
use dockspace::geometry::{LogicalPoint, LogicalRect};
use dockspace::graph::{Axis, Node, RootRecord, SurfacePresentation, Workspace};
use dockspace::ids::{ItemId, NodeId, RootId, SurfaceId};
use dockspace::intent::{Authority, PointerButton, PointerId};
use dockspace::interaction::{
    InteractionCancelReason, InteractionOutcome, InteractionStatus, PreviewResolutionStatus,
    PreviewVisual,
};
use dockspace::pointer_journal::{
    PointerCaptureOwner, PointerEdge, PointerEdgeJournal, PointerEdgeKind, PointerEdgeLocation,
    PointerEdgeSequence, PointerInputLease, PointerProviderScope, SurfaceLocalPointerEndpoint,
    SurfaceLocalPointerScope,
};
use dockspace::pointer_receiver::{
    PointerReceiverCandidate, PointerReceiverDelivery, PointerReceiverDeliveryDisposition,
    PointerReceiverHoverHit, PointerReceiverObservation, PointerReceiverProbe,
    PointerReceiverProbeReceipt, PointerReceiverProbeRequest, PointerReceiverReceipt,
    PointerReceiverReceiptBatch, PresentedPointerReceiverObservation,
};
use dockspace::policy::DockPolicy;
use dockspace::presentation_hit::{PresentationHitRegionId, PresentationHitRegionKind};
use dockspace::scene::PresentationPlan;
use dockspace::transition::EngineTransition;

const ROOT: RootId = RootId::new(1);
const SURFACE: SurfaceId = SurfaceId::new(1);
const POINTER: PointerId = PointerId::new(1);
const MOVED_ITEM: ItemId = ItemId::new(1);
const SOURCE_REMAINDER: ItemId = ItemId::new(2);
const TARGET_ITEM: ItemId = ItemId::new(10);

struct Fixture {
    engine: DockEngine,
    host: support::TestPresentationHost,
    tabs: NodeId,
    provider: PointerInputLease,
    watermark: u64,
}

impl Fixture {
    fn new(policy: DockPolicy) -> Self {
        let mut builder = Workspace::builder();
        let tabs = builder.insert_node(Node::tabs([MOVED_ITEM, SOURCE_REMAINDER, TARGET_ITEM]));
        builder.set_root(ROOT, RootRecord::new(tabs).with_central(tabs));
        builder.set_surface(SURFACE, SurfacePresentation::with_main(ROOT));
        let workspace = builder.build().expect("guide fixture must be valid");
        let mut engine = DockEngine::new(workspace, policy).expect("guide engine must be valid");
        let mut host = support::TestPresentationHost::new(&mut engine);
        support::publish_surface(
            &mut engine,
            &mut host,
            SURFACE,
            rect(0.0, 0.0, 400.0, 300.0),
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
        Self {
            engine,
            host,
            tabs,
            provider,
            watermark: 0,
        }
    }
}

fn rect(x: f64, y: f64, width: f64, height: f64) -> LogicalRect {
    LogicalRect::new(x, y, width, height).expect("guide rectangle must be valid")
}

fn point(x: f64, y: f64) -> LogicalPoint {
    LogicalPoint::new(x, y).expect("guide point must be valid")
}

fn midpoint(bounds: LogicalRect) -> LogicalPoint {
    point(
        bounds.x() + bounds.width() * 0.5,
        bounds.y() + bounds.height() * 0.5,
    )
}

fn target_plan(fixture: &Fixture) -> &PresentationPlan {
    support::painted_plan(&fixture.engine, SURFACE)
}

#[derive(Debug, Clone, Copy)]
struct GuideFact {
    target: DropTargetId,
    hit_point: LogicalPoint,
    preview: LogicalRect,
}

fn outer_edge_guide(fixture: &Fixture, edge: Edge) -> GuideFact {
    let guide = target_plan(fixture)
        .drop_guide_clusters()
        .iter()
        .find(|cluster| cluster.id().root == ROOT && cluster.id().scope == DropGuideScope::Outer)
        .and_then(|cluster| cluster.target(DropGuideSlot::Edge(edge)))
        .expect("compiled target plan must expose every outer edge guide");
    GuideFact {
        target: guide.id(),
        hit_point: midpoint(guide.target().region().rect()),
        preview: guide.target().visual().rect(),
    }
}

fn activation_without_button_hit(fixture: &Fixture) -> LogicalPoint {
    let plan = target_plan(fixture);
    let activation = plan
        .drop_guide_clusters()
        .iter()
        .find(|cluster| matches!(cluster.id().scope, DropGuideScope::Inner(_)))
        .expect("compiled target plan must expose an inner activation")
        .activation();
    for row in 0..16 {
        for column in 0..16 {
            let candidate = point(
                activation.rect().x()
                    + activation.rect().width() * (f64::from(column) + 0.5) / 16.0,
                activation.rect().y() + activation.rect().height() * (f64::from(row) + 0.5) / 16.0,
            );
            let hits_guide = plan
                .drop_guide_clusters()
                .iter()
                .flat_map(|cluster| cluster.targets())
                .any(|(_, guide)| guide.target().region().contains(candidate));
            let hits_structural = plan
                .drop_targets()
                .iter()
                .any(|target| target.region().contains(candidate));
            if !hits_guide && !hits_structural {
                return candidate;
            }
        }
    }
    panic!("compiled activation must contain a passive guide location")
}

fn outside_surface(fixture: &Fixture) -> LogicalPoint {
    let bounds = target_plan(fixture).bounds();
    point(bounds.max().x() + 1.0, bounds.max().y() + 1.0)
}

fn source_tab(fixture: &Fixture) -> (PresentationHitRegionId, LogicalPoint) {
    let region = fixture
        .engine
        .interaction_projection(SURFACE)
        .expect("surface is interactive")
        .hit_manifest()
        .regions()
        .iter()
        .find(|region| {
            matches!(
                region.id().kind(),
                PresentationHitRegionKind::TabBody(tab) if tab.item == MOVED_ITEM
            )
        })
        .copied()
        .expect("moved item has a tab receiver");
    (region.id(), midpoint(region.hit().rect()))
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
    .expect("single pointer edge journal must be contiguous")
}

fn exact_observation(
    candidate: &PointerReceiverCandidate,
    delivery: Option<PointerReceiverDelivery>,
    hover: Option<PointerReceiverHoverHit>,
) -> PointerReceiverObservation {
    let probes = match candidate.probes() {
        PointerReceiverProbeRequest::NotApplicable => {
            return PointerReceiverObservation::NotApplicable;
        }
        PointerReceiverProbeRequest::Delivery => vec![PointerReceiverProbeReceipt::Delivery(
            delivery.expect("candidate requires delivery"),
        )],
        PointerReceiverProbeRequest::HoverHit => vec![PointerReceiverProbeReceipt::HoverHit(
            hover.expect("candidate requires hover"),
        )],
        PointerReceiverProbeRequest::DeliveryAndHoverHit => vec![
            PointerReceiverProbeReceipt::Delivery(delivery.expect("candidate requires delivery")),
            PointerReceiverProbeReceipt::HoverHit(hover.expect("candidate requires hover")),
        ],
    };
    PointerReceiverObservation::Presented(
        PresentedPointerReceiverObservation::new(probes)
            .expect("candidate observation answers its exact probe roster"),
    )
}

fn complete(fixture: &Fixture, frame: &mut CoreHostFrame) {
    support::complete_host_frame_with_retained_or_unavailable(&fixture.engine, frame);
}

fn submit_edge(
    fixture: &mut Fixture,
    kind: PointerEdgeKind,
    position: LogicalPoint,
    capture: PointerCaptureOwner,
    delivery_region: Option<PresentationHitRegionId>,
) -> EngineTransition {
    let mut frame = fixture.host.begin(&fixture.engine);
    frame
        .submit_pointer_journal(
            fixture.provider,
            edge_journal(fixture.watermark, kind, position, capture),
        )
        .expect("pointer edge must stage");
    let candidate = frame
        .pointer_receiver_candidates()
        .expect("pointer edge freezes a candidate roster")
        .candidates()[0]
        .clone();
    let projection = frame
        .view()
        .interaction_projection(SURFACE)
        .expect("sealed frame retains surface authority");
    let delivery = candidate
        .probes()
        .requires(PointerReceiverProbe::Delivery)
        .then(|| {
            PointerReceiverDelivery::new(
                projection,
                delivery_region.map_or(
                    PointerReceiverDeliveryDisposition::NoReceiver,
                    PointerReceiverDeliveryDisposition::Dock,
                ),
            )
            .expect("delivery observation is output-bound")
        });
    let hover = candidate
        .probes()
        .requires(PointerReceiverProbe::HoverHit)
        .then(|| {
            frame
                .view()
                .resolve_hover_drop_receiver(SURFACE, position)
                .expect("core resolves the exact hover receiver")
        });
    frame
        .submit_pointer_receiver_receipts(
            PointerReceiverReceiptBatch::new([
                candidate.receipt(exact_observation(&candidate, delivery, hover))
            ])
            .expect("pointer receipt batch is exact"),
        )
        .expect("pointer receipt must stage");
    complete(fixture, &mut frame);
    fixture.watermark += 1;
    fixture.host.finish(frame, &mut fixture.engine)
}

fn begin_and_observe(fixture: &mut Fixture, at: LogicalPoint) -> EngineTransition {
    let (source, press) = source_tab(fixture);
    let armed = submit_edge(
        fixture,
        PointerEdgeKind::ButtonPressed(PointerButton::Primary),
        press,
        PointerCaptureOwner::ProviderEndpoint,
        Some(source),
    );
    assert!(matches!(
        armed.reduced_pointer_edges()[0].interaction_outcomes(),
        [InteractionOutcome::DragArmed { .. }]
    ));
    submit_edge(
        fixture,
        PointerEdgeKind::Moved,
        at,
        PointerCaptureOwner::ProviderEndpoint,
        None,
    )
}

fn observe(fixture: &mut Fixture, at: LogicalPoint) -> EngineTransition {
    submit_edge(
        fixture,
        PointerEdgeKind::Moved,
        at,
        PointerCaptureOwner::ProviderEndpoint,
        None,
    )
}

fn paint_active_drag(fixture: &mut Fixture) {
    let watermark = PointerEdgeSequence::new(fixture.watermark);
    let mut frame = fixture.host.begin(&fixture.engine);
    frame
        .submit_pointer_journal(
            fixture.provider,
            PointerEdgeJournal::new(watermark, watermark, Vec::new())
                .expect("empty journal preserves the watermark"),
        )
        .expect("paint frame retains the active provider watermark");
    frame
        .submit_pointer_receiver_receipts(
            PointerReceiverReceiptBatch::new(Vec::<PointerReceiverReceipt>::new())
                .expect("empty journal has an exact empty receipt set"),
        )
        .expect("empty receipt set stages");
    let frame = support::complete_host_frame_with_current_outputs(&fixture.engine, frame);
    fixture.host.finish_presentation(frame, &mut fixture.engine);
}

#[test]
fn activation_without_button_hit_publishes_center_and_outer_four_affordance() {
    let mut fixture = Fixture::new(DockPolicy::default());
    let scene = fixture
        .engine
        .scene()
        .ready_surface(SURFACE)
        .expect("surface has acknowledged paint authority")
        .stamp();
    let passive = activation_without_button_hit(&fixture);
    let transition = begin_and_observe(&mut fixture, passive);

    let outcomes = transition.reduced_pointer_edges()[0].interaction_outcomes();
    assert!(
        matches!(
            outcomes,
            [
                InteractionOutcome::DragBegan { .. },
                InteractionOutcome::PreviewUpdated { .. }
            ]
        ),
        "unexpected passive activation outcomes: {outcomes:#?}"
    );
    let affordance = fixture
        .engine
        .interaction()
        .drop_affordance()
        .expect("activation must publish guide affordance");
    assert_eq!(affordance.scene(), scene);
    assert!(affordance.active_target().is_none());
    assert_eq!(affordance.clusters().len(), 2);
    let inner = affordance
        .clusters()
        .iter()
        .find(|cluster| matches!(cluster.id().scope, DropGuideScope::Inner(_)))
        .expect("central target must publish its inner center guide");
    assert!(matches!(
        inner.id().scope,
        DropGuideScope::Inner(node) if node == fixture.tabs
    ));
    assert_eq!(
        inner
            .targets()
            .iter()
            .map(dockspace::drop_resolver::DropAffordanceTarget::slot)
            .collect::<Vec<_>>(),
        vec![DropGuideSlot::Center]
    );
    let outer = affordance
        .clusters()
        .iter()
        .find(|cluster| cluster.id().scope == DropGuideScope::Outer)
        .expect("central target must publish its separate outer guide");
    assert_eq!(
        outer
            .targets()
            .iter()
            .map(dockspace::drop_resolver::DropAffordanceTarget::slot)
            .collect::<Vec<_>>(),
        vec![
            DropGuideSlot::Edge(Edge::Left),
            DropGuideSlot::Edge(Edge::Right),
            DropGuideSlot::Edge(Edge::Top),
            DropGuideSlot::Edge(Edge::Bottom),
        ]
    );
}

#[test]
fn top_and_bottom_buttons_publish_matching_active_slots_and_previews() {
    for edge in [Edge::Top, Edge::Bottom] {
        let mut fixture = Fixture::new(DockPolicy::default());
        let expected = outer_edge_guide(&fixture, edge);
        let transition = begin_and_observe(&mut fixture, expected.hit_point);
        assert!(matches!(
            transition.reduced_pointer_edges()[0].interaction_outcomes(),
            [
                InteractionOutcome::DragBegan { .. },
                InteractionOutcome::PreviewUpdated {
                    preview: Some(_),
                    status: PreviewResolutionStatus::Resolved,
                    ..
                }
            ]
        ));
        let active = fixture
            .engine
            .interaction()
            .drop_affordance()
            .and_then(dockspace::drop_resolver::DropAffordance::active_target)
            .expect("edge button must be active");
        assert_eq!(active.slot(), DropGuideSlot::Edge(edge));
        assert!(active.eligibility().is_eligible());
        assert_eq!(active.target_id(), expected.target);
        assert!(matches!(
            fixture
                .engine
                .interaction()
                .preview()
                .expect("edge preview must exist")
                .visual(),
            PreviewVisual::Dock { target, rect, .. }
                if *target == expected.target && *rect == expected.preview
        ));
    }
}

#[test]
fn rejected_active_guide_remains_visible_without_a_preview() {
    let mut policy = DockPolicy::default();
    policy.set_allow_edge_split(false);
    let mut fixture = Fixture::new(policy);
    let top = outer_edge_guide(&fixture, Edge::Top);
    let transition = begin_and_observe(&mut fixture, top.hit_point);

    assert!(matches!(
        transition.reduced_pointer_edges()[0].interaction_outcomes(),
        [
            InteractionOutcome::DragBegan { .. },
            InteractionOutcome::PreviewUpdated {
                preview: None,
                status: PreviewResolutionStatus::Rejected,
                ..
            }
        ]
    ));
    assert!(fixture.engine.interaction().preview().is_none());
    let active = fixture
        .engine
        .interaction()
        .drop_affordance()
        .and_then(dockspace::drop_resolver::DropAffordance::active_target)
        .expect("rejected guide must remain active");
    assert_eq!(active.slot(), DropGuideSlot::Edge(Edge::Top));
    assert!(matches!(
        active.eligibility(),
        DropGuideEligibility::Rejected(_)
    ));
}

#[test]
fn leaving_the_activation_and_stream_cancellation_clear_the_affordance() {
    let mut fixture = Fixture::new(DockPolicy::default());
    let passive = activation_without_button_hit(&fixture);
    let outside = outside_surface(&fixture);
    begin_and_observe(&mut fixture, passive);
    assert!(fixture.engine.interaction().drop_affordance().is_some());

    let left = observe(&mut fixture, outside);
    let outcomes = left.reduced_pointer_edges()[0].interaction_outcomes();
    assert!(
        matches!(outcomes, [InteractionOutcome::PreviewUpdated { .. }]),
        "unexpected leave outcomes: {outcomes:#?}"
    );
    assert!(fixture.engine.interaction().drop_affordance().is_none());

    observe(&mut fixture, passive);
    assert!(fixture.engine.interaction().drop_affordance().is_some());
    let cancelled = submit_edge(
        &mut fixture,
        PointerEdgeKind::StreamCancelled(
            dockspace::pointer_journal::PointerStreamCancelReason::ExplicitPlatformCancellation,
        ),
        passive,
        PointerCaptureOwner::None,
        None,
    );
    assert!(matches!(
        cancelled.reduced_pointer_edges()[0].interaction_outcomes(),
        [InteractionOutcome::Cancelled {
            reason: InteractionCancelReason::PointerStreamCancelled,
            ..
        }]
    ));
    assert_eq!(
        fixture.engine.interaction().status(),
        InteractionStatus::Idle
    );
    assert!(fixture.engine.interaction().drop_affordance().is_none());
    assert!(fixture.engine.interaction().preview().is_none());
}

#[test]
fn top_and_bottom_presented_releases_commit_the_exact_target_and_conserve_items() {
    for edge in [Edge::Top, Edge::Bottom] {
        let mut fixture = Fixture::new(DockPolicy::default());
        let before_items = fixture.engine.workspace().item_multiset();
        let guide = outer_edge_guide(&fixture, edge);
        begin_and_observe(&mut fixture, guide.hit_point);
        assert!(matches!(
            fixture
                .engine
                .interaction()
                .preview()
                .expect("delivery preview must exist")
                .visual(),
            PreviewVisual::Dock { target, .. } if *target == guide.target
        ));
        paint_active_drag(&mut fixture);

        let delivered = submit_edge(
            &mut fixture,
            PointerEdgeKind::ButtonReleased(PointerButton::Primary),
            guide.hit_point,
            PointerCaptureOwner::None,
            None,
        );
        assert!(matches!(
            delivered.reduced_pointer_edges()[0].interaction_outcomes(),
            [
                InteractionOutcome::PreviewUpdated { .. },
                InteractionOutcome::DragDelivered { .. }
            ]
        ));
        assert_eq!(fixture.engine.workspace().item_multiset(), before_items);
        assert_eq!(
            fixture.engine.interaction().status(),
            InteractionStatus::Idle
        );
        assert!(fixture.engine.interaction().drop_affordance().is_none());

        let root = fixture
            .engine
            .workspace()
            .root(ROOT)
            .expect("root must remain present");
        let (axis, children) = match fixture.engine.workspace().node(root.node) {
            Some(Node::Split { axis, children, .. }) => (*axis, children),
            node => panic!("root must become a split, got {node:?}"),
        };
        assert_eq!(axis, Axis::Vertical);
        let moved_index = children
            .iter()
            .position(|child| {
                matches!(
                    fixture.engine.workspace().node(*child),
                    Some(Node::Tabs { items, .. }) if items.contains(&MOVED_ITEM)
                )
            })
            .expect("moved item must occupy one target child");
        let original_index = children
            .iter()
            .position(|child| {
                matches!(
                    fixture.engine.workspace().node(*child),
                    Some(Node::Tabs { items, .. }) if items.contains(&TARGET_ITEM)
                )
            })
            .expect("original target item must occupy one target child");
        match edge {
            Edge::Top => assert!(moved_index < original_index),
            Edge::Bottom => assert!(moved_index > original_index),
            Edge::Left | Edge::Right => unreachable!("test uses only vertical edges"),
        }
    }
}
