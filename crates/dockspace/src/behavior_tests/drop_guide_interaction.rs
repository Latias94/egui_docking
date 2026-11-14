use super::support;

use dockspace::engine::transition::EngineTransition;
use dockspace::engine::{CoreHostFrame, DockEngine};
use dockspace::geometry::{LogicalPoint, LogicalRect};
use dockspace::graph::command::Edge;
use dockspace::graph::{Axis, Node, RootRecord, SurfacePresentation, Workspace};
use dockspace::ids::{ItemId, NodeId, RootId, SurfaceId};
use dockspace::input::intent::{Authority, PointerButton, PointerId};
use dockspace::input::journal::{
    PointerCaptureOwner, PointerEdge, PointerEdgeJournal, PointerEdgeKind, PointerEdgeLocation,
    PointerEdgeSequence, SurfaceLocalPointerEndpoint, SurfaceLocalPointerProvider,
    SurfaceLocalPointerScope,
};
use dockspace::input::receiver::{
    PointerReceiverCandidate, PointerReceiverDelivery, PointerReceiverDeliveryDisposition,
    PointerReceiverHoverHit, PointerReceiverObservation, PointerReceiverProbe,
    PointerReceiverProbeReceipt, PointerReceiverProbeRequest, PointerReceiverReceipt,
    PointerReceiverReceiptBatch, PresentedPointerReceiverObservation,
};
use dockspace::interaction::targeting::guide::{DropGuideScope, DropGuideSlot};
use dockspace::interaction::targeting::resolver::DropGuideEligibility;
use dockspace::interaction::targeting::targets::DropTargetId;
use dockspace::interaction::{
    InteractionCancelReason, InteractionOutcome, InteractionStatus, PreviewResolutionStatus,
    PreviewVisual,
};
use dockspace::policy::DockPolicy;
use dockspace::presentation::hit::{PresentationHitRegionId, PresentationHitRegionKind};
use dockspace::presentation::scene::PresentationPlan;

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
    provider: SurfaceLocalPointerProvider,
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
            .create_surface_local_pointer_provider(
                SurfaceLocalPointerScope::new(
                    host.lease(),
                    SurfaceLocalPointerEndpoint::Logical(SURFACE),
                ),
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

    fn split() -> (Self, NodeId) {
        let mut builder = Workspace::builder();
        let source_tabs = builder.insert_node(Node::tabs([MOVED_ITEM, SOURCE_REMAINDER]));
        let target_tabs = builder.insert_node(Node::tabs([TARGET_ITEM]));
        let split = builder.insert_node(
            Node::equal_split(Axis::Horizontal, [source_tabs, target_tabs])
                .expect("split guide fixture must be valid"),
        );
        builder.set_root(ROOT, RootRecord::new(split).with_central(target_tabs));
        builder.set_surface(SURFACE, SurfacePresentation::with_main(ROOT));
        let workspace = builder.build().expect("split guide fixture must be valid");
        let policy = DockPolicy::default();
        let mut engine = DockEngine::new(workspace, policy).expect("guide engine must be valid");
        let mut host = support::TestPresentationHost::new(&mut engine);
        support::publish_surface(
            &mut engine,
            &mut host,
            SURFACE,
            rect(0.0, 0.0, 400.0, 300.0),
        );
        let provider = engine
            .create_surface_local_pointer_provider(
                SurfaceLocalPointerScope::new(
                    host.lease(),
                    SurfaceLocalPointerEndpoint::Logical(SURFACE),
                ),
                PointerEdgeSequence::new(0),
            )
            .expect("surface-local pointer provider must be admitted");
        (
            Self {
                engine,
                host,
                tabs: source_tabs,
                provider,
                watermark: 0,
            },
            target_tabs,
        )
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
        hit_point: midpoint(guide.hit().rect()),
        preview: guide.preview(),
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
                .any(|(_, guide)| guide.hit().contains(candidate));
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

fn pane_content(fixture: &Fixture) -> LogicalRect {
    target_plan(fixture)
        .pane_records()
        .iter()
        .find(|pane| pane.id().root == ROOT && pane.id().tabs == fixture.tabs)
        .expect("the target tabs expose one pane")
        .content_bounds()
}

fn pane_content_for(fixture: &Fixture, tabs: NodeId) -> LogicalRect {
    target_plan(fixture)
        .pane_records()
        .iter()
        .find(|pane| pane.id().root == ROOT && pane.id().tabs == tabs)
        .expect("the requested tabs expose one pane")
        .content_bounds()
}

fn pane_center_basin(fixture: &Fixture) -> LogicalRect {
    let content = pane_content(fixture);
    let fraction = fixture
        .engine
        .presentation_config()
        .magnetic_center_fraction();
    let inset_x = content.width() * (1.0 - fraction) * 0.5;
    let inset_y = content.height() * (1.0 - fraction) * 0.5;
    rect(
        content.x() + inset_x,
        content.y() + inset_y,
        content.width() - 2.0 * inset_x,
        content.height() - 2.0 * inset_y,
    )
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
        .submit_surface_pointer_journal(
            &fixture.provider,
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
                    PointerReceiverDeliveryDisposition::EmptyLane,
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
    let transition = fixture.host.finish(frame, &mut fixture.engine);
    assert_eq!(
        fixture.provider.committed_through(),
        PointerEdgeSequence::new(fixture.watermark),
        "committed pointer watermark must advance"
    );
    transition
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
        .submit_surface_pointer_journal(
            &fixture.provider,
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
fn activation_without_button_hit_acquires_magnetic_target_and_keeps_complete_affordance() {
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
    let active = affordance
        .active_target()
        .expect("the magnetic winner highlights its explanatory guide");
    let preview = fixture
        .engine
        .interaction()
        .preview()
        .expect("the magnetic winner publishes a docking preview");
    assert!(matches!(
        preview.visual(),
        dockspace::interaction::PreviewVisual::Dock { target, .. }
            if *target == active.target_id()
    ));
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
            .map(dockspace::interaction::targeting::resolver::DropAffordanceTarget::slot)
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
            .map(dockspace::interaction::targeting::resolver::DropAffordanceTarget::slot)
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
fn magnetic_edge_latch_survives_small_center_overshoot_and_release() {
    let mut fixture = Fixture::new(DockPolicy::default());
    let center = pane_center_basin(&fixture);
    let y = center.y() + 10.0;
    let acquired_at = point(center.x() - 0.25, y);
    let acquired = begin_and_observe(&mut fixture, acquired_at);
    assert!(matches!(
        acquired.reduced_pointer_edges()[0].interaction_outcomes(),
        [
            InteractionOutcome::DragBegan { .. },
            InteractionOutcome::PreviewUpdated {
                preview: Some(_),
                status: PreviewResolutionStatus::Resolved,
                ..
            }
        ]
    ));
    let latched_target = match fixture
        .engine
        .interaction()
        .preview()
        .expect("the pane edge acquires one preview")
        .visual()
    {
        PreviewVisual::Dock { target, .. } => *target,
        visual => panic!("the magnetic edge must publish a dock preview, got {visual:?}"),
    };
    assert!(matches!(
        latched_target,
        DropTargetId::OuterEdge {
            edge: Edge::Left,
            ..
        }
    ));
    paint_active_drag(&mut fixture);

    let overshoot = point(center.x() + 7.5, y);
    let jittered = observe(&mut fixture, overshoot);
    assert!(matches!(
        jittered.reduced_pointer_edges()[0].interaction_outcomes(),
        [InteractionOutcome::PreviewUpdated {
            preview: Some(_),
            status: PreviewResolutionStatus::Resolved,
            ..
        }]
    ));
    assert!(matches!(
        fixture
            .engine
            .interaction()
            .preview()
            .expect("small overshoot keeps the painted target latched")
            .visual(),
        PreviewVisual::Dock { target, .. } if *target == latched_target
    ));

    let before_items = fixture.engine.workspace().item_multiset();
    let delivered = submit_edge(
        &mut fixture,
        PointerEdgeKind::ButtonReleased(PointerButton::Primary),
        overshoot,
        PointerCaptureOwner::None,
        None,
    );
    assert!(matches!(
        delivered.reduced_pointer_edges()[0].interaction_outcomes(),
        [InteractionOutcome::DragDelivered { .. }]
    ));
    assert_eq!(fixture.engine.workspace().item_multiset(), before_items);

    let root = fixture
        .engine
        .workspace()
        .root(ROOT)
        .expect("the source root survives the magnetic split");
    let children = match fixture.engine.workspace().node(root.node) {
        Some(Node::Split {
            axis: Axis::Horizontal,
            children,
            ..
        }) => children,
        node => panic!("the latched left edge must create a horizontal split, got {node:?}"),
    };
    let moved = children
        .iter()
        .position(|child| {
            matches!(
                fixture.engine.workspace().node(*child),
                Some(Node::Tabs { items, .. }) if items.contains(&MOVED_ITEM)
            )
        })
        .expect("the moved item remains present");
    let target = children
        .iter()
        .position(|child| {
            matches!(
                fixture.engine.workspace().node(*child),
                Some(Node::Tabs { items, .. }) if items.contains(&TARGET_ITEM)
            )
        })
        .expect("the target item remains present");
    assert!(moved < target);
}

#[test]
fn magnetic_edge_latch_releases_after_the_configured_hysteresis_envelope() {
    let mut fixture = Fixture::new(DockPolicy::default());
    let center = pane_center_basin(&fixture);
    let y = center.y() + 10.0;
    let acquired_at = point(center.x() - 0.25, y);
    let acquired = begin_and_observe(&mut fixture, acquired_at);
    assert!(matches!(
        acquired.reduced_pointer_edges()[0].interaction_outcomes(),
        [
            InteractionOutcome::DragBegan { .. },
            InteractionOutcome::PreviewUpdated {
                preview: Some(_),
                status: PreviewResolutionStatus::Resolved,
                ..
            }
        ]
    ));
    assert!(matches!(
        fixture
            .engine
            .interaction()
            .preview()
            .expect("the pane edge acquires one preview")
            .visual(),
        PreviewVisual::Dock {
            target: DropTargetId::OuterEdge {
                edge: Edge::Left,
                ..
            },
            ..
        }
    ));
    paint_active_drag(&mut fixture);

    let outside_envelope = point(center.x() + 8.5, y);
    let switched = observe(&mut fixture, outside_envelope);
    assert!(matches!(
        switched.reduced_pointer_edges()[0].interaction_outcomes(),
        [InteractionOutcome::PreviewUpdated {
            preview: Some(_),
            status: PreviewResolutionStatus::Resolved,
            ..
        }]
    ));
    assert!(matches!(
        fixture
            .engine
            .interaction()
            .preview()
            .expect("leaving the release envelope selects the normal fallback")
            .visual(),
        PreviewVisual::Contained { .. }
    ));
    paint_active_drag(&mut fixture);

    let delivered = submit_edge(
        &mut fixture,
        PointerEdgeKind::ButtonReleased(PointerButton::Primary),
        outside_envelope,
        PointerCaptureOwner::None,
        None,
    );
    assert!(matches!(
        delivered.reduced_pointer_edges()[0].interaction_outcomes(),
        [InteractionOutcome::DragDelivered { .. }]
    ));
    assert_eq!(fixture.engine.workspace().contained_floatings().count(), 1);
    let moved = fixture
        .engine
        .workspace()
        .locate_item_by_id(MOVED_ITEM)
        .expect("the moved item remains present");
    assert_eq!(moved.0, RootId::new(2));
}

#[test]
fn adjacent_pane_jitter_keeps_preview_guide_and_release_on_one_latched_target() {
    let (mut fixture, target_tabs) = Fixture::split();
    let source = pane_content_for(&fixture, fixture.tabs);
    let target = pane_content_for(&fixture, target_tabs);
    let y = source.y() + source.height() * 0.6;
    let acquired_at = point(source.max().x() - 1.0, y);
    let acquired = begin_and_observe(&mut fixture, acquired_at);
    assert!(matches!(
        acquired.reduced_pointer_edges()[0].interaction_outcomes(),
        [
            InteractionOutcome::DragBegan { .. },
            InteractionOutcome::PreviewUpdated {
                preview: Some(_),
                status: PreviewResolutionStatus::Resolved,
                ..
            }
        ]
    ));
    let latched_target = match fixture
        .engine
        .interaction()
        .preview()
        .expect("the source pane edge acquires one preview")
        .visual()
    {
        PreviewVisual::Dock { target, .. } => *target,
        visual => panic!("the source pane edge must publish a dock preview, got {visual:?}"),
    };
    paint_active_drag(&mut fixture);

    let jittered_at = point(target.x() + 4.0, y);
    let jittered = observe(&mut fixture, jittered_at);
    assert!(matches!(
        jittered.reduced_pointer_edges()[0].interaction_outcomes(),
        [InteractionOutcome::PreviewUpdated {
            preview: Some(_),
            status: PreviewResolutionStatus::Resolved,
            ..
        }]
    ));
    assert!(matches!(
        fixture
            .engine
            .interaction()
            .preview()
            .expect("small adjacent-pane jitter keeps the old preview")
            .visual(),
        PreviewVisual::Dock { target, .. } if *target == latched_target
    ));
    assert_eq!(
        fixture
            .engine
            .interaction()
            .drop_affordance()
            .and_then(dockspace::interaction::targeting::resolver::DropAffordance::active_target)
            .map(dockspace::interaction::targeting::resolver::DropAffordanceTarget::target_id),
        Some(latched_target),
        "the visible active guide must describe the command retained by the latch"
    );

    let delivered = submit_edge(
        &mut fixture,
        PointerEdgeKind::ButtonReleased(PointerButton::Primary),
        jittered_at,
        PointerCaptureOwner::None,
        None,
    );
    assert!(matches!(
        delivered.reduced_pointer_edges()[0].interaction_outcomes(),
        [InteractionOutcome::DragDelivered { .. }]
    ));
}

#[test]
fn top_and_bottom_buttons_publish_source_aware_future_layout_previews() {
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
            .and_then(dockspace::interaction::targeting::resolver::DropAffordance::active_target)
            .expect("edge button must be active");
        assert_eq!(active.slot(), DropGuideSlot::Edge(edge));
        assert!(active.eligibility().is_eligible());
        assert_eq!(active.target_id(), expected.target);
        let preview = match edge {
            Edge::Top => rect(0.0, 0.0, 400.0, 149.5),
            Edge::Bottom => rect(0.0, 150.5, 400.0, 149.5),
            Edge::Left | Edge::Right => unreachable!("test uses only vertical edges"),
        };
        assert_ne!(preview, expected.preview);
        assert!(matches!(
            fixture
                .engine
                .interaction()
                .preview()
                .expect("edge preview must exist")
                .visual(),
            PreviewVisual::Dock { target, rect, .. }
                if *target == expected.target && *rect == preview
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
        .and_then(dockspace::interaction::targeting::resolver::DropAffordance::active_target)
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
            dockspace::input::journal::PointerStreamCancelReason::ExplicitPlatformCancellation,
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
            [InteractionOutcome::DragDelivered { .. }]
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
