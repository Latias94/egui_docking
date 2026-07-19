use dockspace::command::{DockFraction, DockTarget, Edge, MovePayload};
use dockspace::drop_guide::{
    DropGuideClusterRecord, DropGuideEdgeSet, DropGuideScope, DropGuideSlot, DropGuideTargetRecord,
};
use dockspace::drop_resolver::DropGuideEligibility;
use dockspace::drop_target::{
    DropTargetAvailability, DropTargetId, DropTargetRecord, DropVisual, SceneLayerKey,
};
use dockspace::engine::DockEngine;
use dockspace::geometry::{LogicalPoint, LogicalRect};
use dockspace::graph::{Axis, Node, RootRecord, SurfacePresentation, Workspace};
use dockspace::hit_region::HitRegion;
use dockspace::ids::{ItemId, NodeId, RootId, SurfaceId};
use dockspace::intent::{
    Authority, PointerButton, PointerButtonState, PointerId, RendererIntent, SurfacePointer,
    TargetAuthority,
};
use dockspace::interaction::{
    DragSessionId, InteractionCancelReason, InteractionOutcome, InteractionStatus,
    PreviewResolutionStatus, PreviewVisual,
};
use dockspace::policy::DockPolicy;
use dockspace::scene::{BuildingScene, ReadySurfaceScene, SceneStamp};
use dockspace::transition::{EngineTransition, InputOutcome};

const SOURCE_ROOT: RootId = RootId::new(1);
const TARGET_ROOT: RootId = RootId::new(2);
const SOURCE_SURFACE: SurfaceId = SurfaceId::new(1);
const TARGET_SURFACE: SurfaceId = SurfaceId::new(2);
const POINTER: PointerId = PointerId::new(1);
const MOVED_ITEM: ItemId = ItemId::new(1);
const SOURCE_REMAINDER: ItemId = ItemId::new(2);
const TARGET_ITEM: ItemId = ItemId::new(10);

struct Fixture {
    engine: DockEngine,
    source_tabs: NodeId,
    target_tabs: NodeId,
}

impl Fixture {
    fn new(policy: DockPolicy) -> Self {
        let mut builder = Workspace::builder();
        let source_tabs = builder.insert_node(Node::tabs([MOVED_ITEM, SOURCE_REMAINDER]));
        let target_tabs = builder.insert_node(Node::tabs([TARGET_ITEM]));
        builder.set_root(SOURCE_ROOT, RootRecord::new(source_tabs));
        builder.set_root(TARGET_ROOT, RootRecord::new(target_tabs));
        builder.set_surface(SOURCE_SURFACE, SurfacePresentation::new(SOURCE_ROOT));
        builder.set_surface(TARGET_SURFACE, SurfacePresentation::new(TARGET_ROOT));
        let workspace = builder.build().expect("guide fixture must be valid");
        let engine = DockEngine::new(workspace, policy).expect("guide engine must be valid");
        Self {
            engine,
            source_tabs,
            target_tabs,
        }
    }

    fn payload(&self) -> MovePayload {
        MovePayload::Item(
            self.engine
                .workspace()
                .capture_item_source(SOURCE_ROOT, self.source_tabs, MOVED_ITEM)
                .expect("source item must be current"),
        )
    }
}

fn rect(x: f64, y: f64, width: f64, height: f64) -> LogicalRect {
    LogicalRect::new(x, y, width, height).expect("guide rectangle must be valid")
}

fn point(x: f64, y: f64) -> LogicalPoint {
    LogicalPoint::new(x, y).expect("guide point must be valid")
}

fn activation_rect() -> LogicalRect {
    rect(20.0, 20.0, 60.0, 60.0)
}

fn slot_hit(slot: DropGuideSlot) -> LogicalRect {
    match slot {
        DropGuideSlot::Center => rect(46.0, 46.0, 8.0, 8.0),
        DropGuideSlot::Edge(Edge::Left) => rect(34.0, 46.0, 8.0, 8.0),
        DropGuideSlot::Edge(Edge::Right) => rect(58.0, 46.0, 8.0, 8.0),
        DropGuideSlot::Edge(Edge::Top) => rect(46.0, 34.0, 8.0, 8.0),
        DropGuideSlot::Edge(Edge::Bottom) => rect(46.0, 58.0, 8.0, 8.0),
    }
}

fn slot_preview(slot: DropGuideSlot) -> LogicalRect {
    match slot {
        DropGuideSlot::Center => activation_rect(),
        DropGuideSlot::Edge(Edge::Left) => rect(20.0, 20.0, 20.0, 60.0),
        DropGuideSlot::Edge(Edge::Right) => rect(60.0, 20.0, 20.0, 60.0),
        DropGuideSlot::Edge(Edge::Top) => rect(20.0, 20.0, 60.0, 20.0),
        DropGuideSlot::Edge(Edge::Bottom) => rect(20.0, 60.0, 60.0, 20.0),
    }
}

fn edge_target_id(fixture: &Fixture, edge: Edge) -> DropTargetId {
    DropTargetId::OuterEdge {
        surface: TARGET_SURFACE,
        root: TARGET_ROOT,
        node: fixture.target_tabs,
        edge,
    }
}

fn center_guide(fixture: &Fixture, layer: SceneLayerKey) -> DropGuideTargetRecord {
    let slot = DropGuideSlot::Center;
    guide_target(
        DropTargetId::Center {
            surface: TARGET_SURFACE,
            root: TARGET_ROOT,
            tabs: fixture.target_tabs,
        },
        DockTarget::Center(
            fixture
                .engine
                .workspace()
                .capture_tab_target(TARGET_ROOT, fixture.target_tabs)
                .expect("center target must be current"),
        ),
        slot,
        layer,
    )
}

fn edge_guide(fixture: &Fixture, edge: Edge, layer: SceneLayerKey) -> DropGuideTargetRecord {
    let slot = DropGuideSlot::Edge(edge);
    guide_target(
        edge_target_id(fixture, edge),
        DockTarget::Edge(
            fixture
                .engine
                .workspace()
                .capture_edge_target(
                    TARGET_ROOT,
                    fixture.target_tabs,
                    edge,
                    DockFraction::new(0.35).expect("dock fraction must be valid"),
                )
                .expect("edge target must be current"),
        ),
        slot,
        layer,
    )
}

fn guide_target(
    id: DropTargetId,
    target: DockTarget,
    slot: DropGuideSlot,
    layer: SceneLayerKey,
) -> DropGuideTargetRecord {
    let hit = slot_hit(slot);
    DropGuideTargetRecord::new(
        DropTargetRecord::new(
            id,
            target,
            DropTargetAvailability::Available,
            HitRegion::new(hit),
            layer,
            DropVisual::new(slot_preview(slot)),
        ),
        hit,
    )
}

fn inner_five_cluster(fixture: &Fixture) -> DropGuideClusterRecord {
    let layer = SceneLayerKey::new(1);
    DropGuideClusterRecord::inner(
        TARGET_SURFACE,
        TARGET_ROOT,
        fixture.target_tabs,
        HitRegion::new(activation_rect()),
        layer,
        center_guide(fixture, layer),
        DropGuideEdgeSet::new(
            edge_guide(fixture, Edge::Left, layer),
            edge_guide(fixture, Edge::Right, layer),
            edge_guide(fixture, Edge::Top, layer),
            edge_guide(fixture, Edge::Bottom, layer),
        ),
    )
}

fn guide_scene(fixture: &Fixture) -> BuildingScene {
    let mut building =
        BuildingScene::new([SOURCE_SURFACE, TARGET_SURFACE]).expect("scene roster must be valid");
    building
        .insert_ready(ReadySurfaceScene::new(
            SOURCE_SURFACE,
            rect(0.0, 0.0, 100.0, 100.0),
        ))
        .expect("source scene must be unique");
    let mut target = ReadySurfaceScene::new(TARGET_SURFACE, rect(0.0, 0.0, 100.0, 100.0));
    target.push_drop_guide_cluster(inner_five_cluster(fixture));
    building
        .insert_ready(target)
        .expect("target scene must be unique");
    building
}

fn publish_scene(fixture: &mut Fixture) -> SceneStamp {
    let scene = guide_scene(fixture);
    fixture
        .engine
        .enqueue_scene(scene)
        .expect("scene publication must enqueue");
    let transition = fixture
        .engine
        .reduce_pending()
        .expect("guide scene must publish");
    assert!(matches!(
        transition.reduced_inputs()[0].outcome(),
        InputOutcome::ScenePublished {
            ready_surfaces: 2,
            bootstrap_surfaces: 0,
            ..
        }
    ));
    fixture
        .engine
        .scene()
        .expect("sealed guide scene must exist")
        .stamp()
}

fn arm_and_begin(fixture: &mut Fixture) -> DragSessionId {
    fixture
        .engine
        .enqueue_renderer_intent(RendererIntent::ArmDrag {
            pointer: POINTER,
            button: PointerButton::Primary,
            payload: fixture.payload(),
        })
        .expect("arm must enqueue");
    let armed = fixture.engine.reduce_pending().expect("arm must reduce");
    let session = match armed.reduced_inputs()[0].outcome() {
        InputOutcome::InteractionProcessed {
            outcome: InteractionOutcome::DragArmed { session, .. },
            ..
        } => *session,
        outcome => panic!("unexpected arm outcome: {outcome:?}"),
    };
    fixture
        .engine
        .enqueue_renderer_intent(RendererIntent::BeginDrag {
            session,
            pointer: POINTER,
            button: PointerButton::Primary,
        })
        .expect("begin must enqueue");
    let begun = fixture.engine.reduce_pending().expect("begin must reduce");
    assert!(matches!(
        begun.reduced_inputs()[0].outcome(),
        InputOutcome::InteractionProcessed {
            outcome: InteractionOutcome::DragBegan { session: current },
            ..
        } if *current == session
    ));
    session
}

fn observe(fixture: &mut Fixture, session: DragSessionId, at: LogicalPoint) -> EngineTransition {
    fixture
        .engine
        .enqueue_renderer_intent(RendererIntent::UpdateDrag {
            session,
            target: target_at(at),
            tear_off: None,
        })
        .expect("guide observation must enqueue");
    fixture
        .engine
        .reduce_pending()
        .expect("guide observation must reduce")
}

fn target_at(at: LogicalPoint) -> TargetAuthority {
    TargetAuthority::local(
        TARGET_SURFACE,
        Authority::Known(Some(SurfacePointer::new(TARGET_SURFACE, at))),
    )
}

fn point_for_edge(edge: Edge) -> LogicalPoint {
    match edge {
        Edge::Top => point(50.0, 38.0),
        Edge::Bottom => point(50.0, 62.0),
        Edge::Left | Edge::Right => panic!("test requires a vertical edge"),
    }
}

#[test]
fn activation_without_button_hit_publishes_complete_inner_five_affordance() {
    let mut fixture = Fixture::new(DockPolicy::default());
    let scene = publish_scene(&mut fixture);
    let session = arm_and_begin(&mut fixture);
    let transition = observe(&mut fixture, session, point(30.0, 30.0));

    assert!(matches!(
        transition.reduced_inputs()[0].outcome(),
        InputOutcome::InteractionProcessed {
            outcome: InteractionOutcome::PreviewUpdated {
                preview: None,
                status: PreviewResolutionStatus::KnownNone,
                ..
            },
            ..
        }
    ));
    assert!(fixture.engine.interaction().preview().is_none());
    let affordance = fixture
        .engine
        .interaction()
        .drop_affordance()
        .expect("activation must publish guide affordance");
    assert_eq!(affordance.scene(), scene);
    assert!(affordance.active_target().is_none());
    assert_eq!(affordance.clusters().len(), 1);
    let cluster = &affordance.clusters()[0];
    assert!(matches!(
        cluster.id().scope,
        DropGuideScope::Inner(node) if node == fixture.target_tabs
    ));
    assert_eq!(cluster.targets().len(), 5);
    assert_eq!(
        cluster
            .targets()
            .iter()
            .map(dockspace::drop_resolver::DropAffordanceTarget::slot)
            .collect::<Vec<_>>(),
        vec![
            DropGuideSlot::Center,
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
        publish_scene(&mut fixture);
        let session = arm_and_begin(&mut fixture);
        let transition = observe(&mut fixture, session, point_for_edge(edge));
        assert!(matches!(
            transition.reduced_inputs()[0].outcome(),
            InputOutcome::InteractionProcessed {
                outcome: InteractionOutcome::PreviewUpdated {
                    preview: Some(_),
                    status: PreviewResolutionStatus::Resolved,
                    ..
                },
                ..
            }
        ));
        let active = fixture
            .engine
            .interaction()
            .drop_affordance()
            .and_then(dockspace::drop_resolver::DropAffordance::active_target)
            .expect("edge button must be active");
        assert_eq!(active.slot(), DropGuideSlot::Edge(edge));
        assert!(active.eligibility().is_eligible());
        let expected_target = edge_target_id(&fixture, edge);
        assert_eq!(active.target_id(), expected_target);
        assert!(matches!(
            fixture
                .engine
                .interaction()
                .preview()
                .expect("edge preview must exist")
                .visual(),
            PreviewVisual::Dock { target, rect, .. }
                if *target == expected_target && *rect == slot_preview(DropGuideSlot::Edge(edge))
        ));
    }
}

#[test]
fn rejected_active_guide_remains_visible_without_a_preview() {
    let mut policy = DockPolicy::default();
    policy.set_allow_edge_split(false);
    let mut fixture = Fixture::new(policy);
    publish_scene(&mut fixture);
    let session = arm_and_begin(&mut fixture);
    let transition = observe(&mut fixture, session, point_for_edge(Edge::Top));

    assert!(matches!(
        transition.reduced_inputs()[0].outcome(),
        InputOutcome::InteractionProcessed {
            outcome: InteractionOutcome::PreviewUpdated {
                preview: None,
                status: PreviewResolutionStatus::Rejected,
                ..
            },
            ..
        }
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
fn leaving_the_activation_and_cancelling_clear_the_affordance() {
    let mut fixture = Fixture::new(DockPolicy::default());
    publish_scene(&mut fixture);
    let session = arm_and_begin(&mut fixture);
    observe(&mut fixture, session, point(30.0, 30.0));
    assert!(fixture.engine.interaction().drop_affordance().is_some());

    let left = observe(&mut fixture, session, point(90.0, 90.0));
    assert!(matches!(
        left.reduced_inputs()[0].outcome(),
        InputOutcome::InteractionProcessed {
            outcome: InteractionOutcome::PreviewUpdated {
                preview: None,
                status: PreviewResolutionStatus::KnownNone,
                ..
            },
            ..
        }
    ));
    assert!(fixture.engine.interaction().drop_affordance().is_none());

    observe(&mut fixture, session, point(30.0, 30.0));
    assert!(fixture.engine.interaction().drop_affordance().is_some());
    fixture
        .engine
        .enqueue_renderer_intent(RendererIntent::CancelDrag {
            session,
            reason: InteractionCancelReason::Escape,
        })
        .expect("cancel must enqueue");
    let cancelled = fixture.engine.reduce_pending().expect("cancel must reduce");
    assert!(matches!(
        cancelled.reduced_inputs()[0].outcome(),
        InputOutcome::InteractionProcessed {
            outcome: InteractionOutcome::Cancelled {
                reason: InteractionCancelReason::Escape,
                ..
            },
            ..
        }
    ));
    assert_eq!(
        fixture.engine.interaction().status(),
        InteractionStatus::Idle
    );
    assert!(fixture.engine.interaction().drop_affordance().is_none());
    assert!(fixture.engine.interaction().preview().is_none());
}

#[test]
fn scene_refresh_rebuilds_affordance_and_preview_stamps() {
    let mut fixture = Fixture::new(DockPolicy::default());
    let first_scene = publish_scene(&mut fixture);
    let session = arm_and_begin(&mut fixture);
    observe(&mut fixture, session, point_for_edge(Edge::Top));
    let first_affordance_scene = fixture
        .engine
        .interaction()
        .drop_affordance()
        .expect("first affordance must exist")
        .scene();
    let first_preview = fixture
        .engine
        .interaction()
        .preview()
        .expect("first preview must exist")
        .token();
    assert_eq!(first_affordance_scene, first_scene);
    assert_eq!(first_preview.scene(), first_scene);

    let refreshed_scene = publish_scene(&mut fixture);
    assert_ne!(refreshed_scene, first_scene);
    let refreshed_affordance = fixture
        .engine
        .interaction()
        .drop_affordance()
        .expect("refresh must rebuild affordance");
    let refreshed_preview = fixture
        .engine
        .interaction()
        .preview()
        .expect("refresh must rebuild preview")
        .token();
    assert_eq!(refreshed_affordance.scene(), refreshed_scene);
    assert_eq!(refreshed_preview.scene(), refreshed_scene);
    assert_ne!(refreshed_preview, first_preview);
}

#[test]
fn top_and_bottom_acknowledged_releases_commit_the_painted_target_and_conserve_items() {
    for edge in [Edge::Top, Edge::Bottom] {
        let mut fixture = Fixture::new(DockPolicy::default());
        publish_scene(&mut fixture);
        let before_items = fixture.engine.workspace().item_multiset();
        let session = arm_and_begin(&mut fixture);
        let at = point_for_edge(edge);
        observe(&mut fixture, session, at);
        let active_target = fixture
            .engine
            .interaction()
            .drop_affordance()
            .and_then(dockspace::drop_resolver::DropAffordance::active_target)
            .expect("delivery guide must be active")
            .target_id();
        let preview = fixture
            .engine
            .interaction()
            .preview()
            .expect("delivery preview must exist");
        assert!(matches!(
            preview.visual(),
            PreviewVisual::Dock { target, .. } if *target == active_target
        ));
        let acknowledgement = preview.acknowledgement();

        fixture
            .engine
            .enqueue_renderer_intent(RendererIntent::ReleaseDrag {
                session,
                pointer: POINTER,
                button: PointerButton::Primary,
                button_state: Authority::Known(PointerButtonState::Released),
                target: target_at(at),
                tear_off: None,
            })
            .expect("release must enqueue");
        fixture
            .engine
            .enqueue_renderer_intent(RendererIntent::AcknowledgePreview(acknowledgement))
            .expect("acknowledgement must enqueue");
        let delivered = fixture
            .engine
            .reduce_pending()
            .expect("acknowledged guide release must reduce");
        assert!(matches!(
            delivered.reduced_inputs()[0].outcome(),
            InputOutcome::InteractionProcessed {
                outcome: InteractionOutcome::PreviewAcknowledged { .. },
                ..
            }
        ));
        assert!(matches!(
            delivered.reduced_inputs()[1].outcome(),
            InputOutcome::InteractionProcessed {
                outcome: InteractionOutcome::DragDelivered { .. },
                ..
            }
        ));
        assert!(delivered.changed());
        assert_eq!(fixture.engine.workspace().item_multiset(), before_items);
        assert_eq!(
            fixture.engine.interaction().status(),
            InteractionStatus::Idle
        );
        assert!(fixture.engine.interaction().drop_affordance().is_none());

        let target_root = fixture
            .engine
            .workspace()
            .root(TARGET_ROOT)
            .expect("target root must remain present");
        let (axis, children) = match fixture.engine.workspace().node(target_root.node) {
            Some(Node::Split { axis, children, .. }) => (*axis, children),
            node => panic!("target root must become a split, got {node:?}"),
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
