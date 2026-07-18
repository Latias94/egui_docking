use dockspace::command::{DockFraction, DockTarget, Edge, MovePayload};
use dockspace::drop_target::{
    DropTargetAvailability, DropTargetId, DropTargetRecord, DropVisual, SceneLayerKey,
};
use dockspace::engine::{DockEngine, EngineInput};
use dockspace::geometry::{LogicalPoint, LogicalRect};
use dockspace::graph::{Axis, Node, RootRecord, SplitWeight, SurfacePresentation, Workspace};
use dockspace::hit_region::HitRegion;
use dockspace::ids::{ItemId, NodeId, RootId, SurfaceId};
use dockspace::intent::{
    Authority, AuthorityUnavailableReason, PointerButton, PointerButtonState, PointerId,
    RendererIntent, SurfacePointer, TargetAuthority,
};
use dockspace::interaction::{
    DragGeneration, DragSessionId, InteractionCancelReason, InteractionOutcome,
    InteractionRejection, InteractionStatus,
};
use dockspace::policy::DockPolicy;
use dockspace::scene::{
    BuildingScene, NodeSceneId, ReadySurfaceScene, SceneBuildError, SemanticRect,
};
use dockspace::transition::InputOutcome;

const ROOT_A: RootId = RootId::new(1);
const ROOT_B: RootId = RootId::new(2);
const SURFACE_A: SurfaceId = SurfaceId::new(1);
const SURFACE_B: SurfaceId = SurfaceId::new(2);
const POINTER: PointerId = PointerId::new(1);

struct Fixture {
    engine: DockEngine,
    tabs_a: NodeId,
    tabs_b: NodeId,
}

fn point(x: f64, y: f64) -> LogicalPoint {
    LogicalPoint::new(x, y).expect("test point must be valid")
}

fn rect(x: f64, y: f64, width: f64, height: f64) -> LogicalRect {
    LogicalRect::new(x, y, width, height).expect("test rectangle must be valid")
}

fn fixture() -> Fixture {
    let mut builder = Workspace::builder();
    let tabs_a = builder.insert_node(Node::tabs([ItemId::new(1), ItemId::new(2)]));
    let tabs_b = builder.insert_node(Node::tabs([ItemId::new(3)]));
    builder.set_root(ROOT_A, RootRecord::new(tabs_a));
    builder.set_root(ROOT_B, RootRecord::new(tabs_b));
    builder.set_surface(SURFACE_A, SurfacePresentation::new(ROOT_A));
    builder.set_surface(SURFACE_B, SurfacePresentation::new(ROOT_B));
    let workspace = builder.build().expect("test workspace must be valid");
    let engine =
        DockEngine::new(workspace, DockPolicy::default()).expect("test engine must be valid");
    Fixture {
        engine,
        tabs_a,
        tabs_b,
    }
}

fn ready_scene(fixture: &Fixture) -> BuildingScene {
    let mut building =
        BuildingScene::new([SURFACE_A, SURFACE_B]).expect("test roster must be unique");
    building
        .insert_ready(ReadySurfaceScene::new(
            SURFACE_A,
            rect(0.0, 0.0, 300.0, 200.0),
        ))
        .expect("source surface must be ready");

    let center = fixture
        .engine
        .workspace()
        .capture_tab_target(ROOT_B, fixture.tabs_b)
        .expect("center target must be current");
    let edge = fixture
        .engine
        .workspace()
        .capture_edge_target(
            ROOT_B,
            fixture.tabs_b,
            Edge::Right,
            DockFraction::new(0.3).expect("fraction must be valid"),
        )
        .expect("edge target must be current");
    let mut target = ReadySurfaceScene::new(SURFACE_B, rect(0.0, 0.0, 300.0, 200.0));
    target.push_drop_target(DropTargetRecord::new(
        DropTargetId::Center {
            surface: SURFACE_B,
            root: ROOT_B,
            tabs: fixture.tabs_b,
        },
        DockTarget::Center(center),
        DropTargetAvailability::Available,
        HitRegion::new(rect(0.0, 0.0, 100.0, 100.0)),
        SceneLayerKey::new(1),
        DropVisual::new(rect(0.0, 0.0, 100.0, 100.0)),
    ));
    target.push_drop_target(DropTargetRecord::new(
        DropTargetId::InnerEdge {
            surface: SURFACE_B,
            root: ROOT_B,
            node: fixture.tabs_b,
            edge: Edge::Right,
        },
        DockTarget::Edge(edge),
        DropTargetAvailability::Available,
        HitRegion::new(rect(100.0, 0.0, 100.0, 100.0)),
        SceneLayerKey::new(1),
        DropVisual::new(rect(150.0, 0.0, 50.0, 100.0)),
    ));
    building
        .insert_ready(target)
        .expect("target surface must be ready");
    building
}

fn publish_ready_scene(fixture: &mut Fixture) {
    let scene = ready_scene(fixture);
    fixture
        .engine
        .enqueue_scene(scene)
        .expect("scene input sequence must be available");
    let transition = fixture
        .engine
        .reduce_pending()
        .expect("scene publication must succeed");
    assert!(matches!(
        transition.reduced_inputs()[0].outcome(),
        InputOutcome::ScenePublished {
            ready_surfaces: 2,
            bootstrap_surfaces: 0,
            ..
        }
    ));
}

fn source_payload(fixture: &Fixture) -> MovePayload {
    MovePayload::Item(
        fixture
            .engine
            .workspace()
            .capture_item_source(ROOT_A, fixture.tabs_a, ItemId::new(1))
            .expect("source item must be current"),
    )
}

fn arm(fixture: &mut Fixture) -> DragSessionId {
    let payload = source_payload(fixture);
    fixture
        .engine
        .enqueue_renderer_intent(RendererIntent::ArmDrag {
            pointer: POINTER,
            button: PointerButton::Primary,
            payload,
        })
        .expect("arm sequence must be available");
    let transition = fixture.engine.reduce_pending().expect("arm must reduce");
    match transition.reduced_inputs()[0].outcome() {
        InputOutcome::InteractionProcessed {
            outcome: InteractionOutcome::DragArmed { session, .. },
            ..
        } => *session,
        outcome => panic!("unexpected arm outcome: {outcome:?}"),
    }
}

fn begin_and_hover(fixture: &mut Fixture, session: DragSessionId, x: f64) {
    fixture
        .engine
        .enqueue_renderer_intent(RendererIntent::BeginDrag {
            session,
            pointer: POINTER,
            button: PointerButton::Primary,
        })
        .expect("begin sequence must be available");
    fixture
        .engine
        .enqueue_renderer_intent(RendererIntent::UpdateDrag {
            session,
            target: TargetAuthority::local(
                SURFACE_B,
                Authority::Known(Some(SurfacePointer::new(SURFACE_B, point(x, 50.0)))),
            ),
            tear_off: None,
        })
        .expect("update sequence must be available");
    fixture
        .engine
        .reduce_pending()
        .expect("begin and update must reduce");
    assert!(fixture.engine.interaction().preview().is_some());
}

fn release_intent(session: DragSessionId, x: f64) -> RendererIntent {
    release_with(
        session,
        POINTER,
        PointerButton::Primary,
        Authority::Known(PointerButtonState::Released),
        x,
    )
}

#[test]
fn local_target_cannot_claim_a_different_observer_surface() {
    let mut fixture = fixture();
    publish_ready_scene(&mut fixture);
    let session = arm(&mut fixture);
    fixture
        .engine
        .enqueue_renderer_intent(RendererIntent::BeginDrag {
            session,
            pointer: POINTER,
            button: PointerButton::Primary,
        })
        .expect("begin sequence must be available");
    fixture
        .engine
        .enqueue_renderer_intent(RendererIntent::UpdateDrag {
            session,
            target: TargetAuthority::local(
                SURFACE_A,
                Authority::Known(Some(SurfacePointer::new(SURFACE_B, point(50.0, 50.0)))),
            ),
            tear_off: None,
        })
        .expect("forged local observation must enqueue");

    let transition = fixture
        .engine
        .reduce_pending()
        .expect("forged local observation must cancel nonfatally");
    assert!(matches!(
        transition.reduced_inputs()[1].outcome(),
        InputOutcome::InteractionProcessed {
            outcome: InteractionOutcome::Cancelled {
                reason: InteractionCancelReason::UnknownTargetAuthority,
                ..
            },
            ..
        }
    ));
    assert_eq!(
        fixture.engine.interaction().status(),
        InteractionStatus::Idle
    );
    assert!(fixture.engine.interaction().preview().is_none());
}

fn release_with(
    session: DragSessionId,
    pointer: PointerId,
    button: PointerButton,
    button_state: Authority<PointerButtonState>,
    x: f64,
) -> RendererIntent {
    RendererIntent::ReleaseDrag {
        session,
        pointer,
        button,
        button_state,
        target: TargetAuthority::local(
            SURFACE_B,
            Authority::Known(Some(SurfacePointer::new(SURFACE_B, point(x, 50.0)))),
        ),
        tear_off: None,
    }
}

fn run_ack_release(release_first: bool) -> Fixture {
    let mut fixture = fixture();
    publish_ready_scene(&mut fixture);
    let session = arm(&mut fixture);
    begin_and_hover(&mut fixture, session, 50.0);
    let acknowledgement = fixture
        .engine
        .interaction()
        .preview()
        .expect("preview must exist")
        .acknowledgement();
    let acknowledgement = RendererIntent::AcknowledgePreview(acknowledgement);
    let release = release_intent(session, 50.0);
    if release_first {
        fixture
            .engine
            .enqueue_renderer_intent(release)
            .expect("release sequence must be available");
        fixture
            .engine
            .enqueue_renderer_intent(acknowledgement)
            .expect("ack sequence must be available");
    } else {
        fixture
            .engine
            .enqueue_renderer_intent(acknowledgement)
            .expect("ack sequence must be available");
        fixture
            .engine
            .enqueue_renderer_intent(release)
            .expect("release sequence must be available");
    }
    let transition = fixture
        .engine
        .reduce_pending()
        .expect("acknowledged release must reduce");
    assert!(matches!(
        transition.reduced_inputs()[0].outcome(),
        InputOutcome::InteractionProcessed {
            outcome: InteractionOutcome::PreviewAcknowledged { .. },
            ..
        }
    ));
    assert!(matches!(
        transition.reduced_inputs()[1].outcome(),
        InputOutcome::InteractionProcessed {
            outcome: InteractionOutcome::DragDelivered { .. },
            ..
        }
    ));
    assert!(transition.changed());
    assert!(transition.published_state_changed());
    fixture
}

#[test]
fn paint_acknowledgement_precedes_release_independent_of_callback_order() {
    let release_first = run_ack_release(true);
    let acknowledgement_first = run_ack_release(false);

    assert_eq!(
        release_first.engine.workspace(),
        acknowledgement_first.engine.workspace()
    );
    assert_eq!(
        release_first.engine.version(),
        acknowledgement_first.engine.version()
    );
    assert_eq!(
        release_first.engine.interaction().status(),
        InteractionStatus::Idle
    );
    assert!(release_first.engine.scene().is_none());
    assert!(matches!(
        release_first.engine.workspace().node(release_first.tabs_b),
        Some(Node::Tabs { items, .. }) if items == &[ItemId::new(3), ItemId::new(1)]
    ));
}

#[test]
fn unpainted_release_is_consumed_once_without_mutation() {
    let mut fixture = fixture();
    publish_ready_scene(&mut fixture);
    let session = arm(&mut fixture);
    begin_and_hover(&mut fixture, session, 50.0);
    let before = fixture.engine.workspace().clone();
    let version = fixture.engine.version();

    fixture
        .engine
        .enqueue_renderer_intent(release_intent(session, 50.0))
        .expect("release sequence must be available");
    let transition = fixture
        .engine
        .reduce_pending()
        .expect("unpainted release is a nonfatal rejection");
    assert!(matches!(
        transition.reduced_inputs()[0].outcome(),
        InputOutcome::InteractionProcessed {
            outcome: InteractionOutcome::Rejected(InteractionRejection::PreviewNotPainted),
            ..
        }
    ));
    assert_eq!(fixture.engine.workspace(), &before);
    assert_eq!(fixture.engine.version(), version);
    assert_eq!(
        fixture.engine.interaction().status(),
        InteractionStatus::Idle
    );

    fixture
        .engine
        .enqueue_renderer_intent(release_intent(session, 50.0))
        .expect("duplicate sequence must be available");
    let duplicate = fixture
        .engine
        .reduce_pending()
        .expect("duplicate release is nonfatal");
    assert!(matches!(
        duplicate.reduced_inputs()[0].outcome(),
        InputOutcome::InteractionProcessed {
            outcome: InteractionOutcome::Rejected(InteractionRejection::DuplicateRelease {
                session: repeated
            }),
            ..
        } if *repeated == session
    ));

    fixture
        .engine
        .enqueue_renderer_intent(RendererIntent::UpdateDrag {
            session,
            target: TargetAuthority::local(
                SURFACE_B,
                Authority::Known(Some(SurfacePointer::new(SURFACE_B, point(50.0, 50.0)))),
            ),
            tear_off: None,
        })
        .expect("late update sequence must be available");
    let late_update = fixture
        .engine
        .reduce_pending()
        .expect("late update is a nonfatal rejection");
    assert!(matches!(
        late_update.reduced_inputs()[0].outcome(),
        InputOutcome::InteractionProcessed {
            outcome: InteractionOutcome::Rejected(InteractionRejection::SessionConsumed {
                session: consumed
            }),
            ..
        } if *consumed == session
    ));
}

#[test]
fn release_cannot_commit_a_target_different_from_the_painted_preview() {
    let mut fixture = fixture();
    publish_ready_scene(&mut fixture);
    let session = arm(&mut fixture);
    begin_and_hover(&mut fixture, session, 50.0);
    let before = fixture.engine.workspace().clone();
    let acknowledgement = fixture
        .engine
        .interaction()
        .preview()
        .expect("center preview must exist")
        .acknowledgement();
    fixture
        .engine
        .enqueue_renderer_intent(RendererIntent::AcknowledgePreview(acknowledgement))
        .expect("ack sequence must be available");
    fixture
        .engine
        .enqueue_renderer_intent(release_intent(session, 150.0))
        .expect("release sequence must be available");

    let transition = fixture
        .engine
        .reduce_pending()
        .expect("target change is a nonfatal rejection");
    assert!(matches!(
        transition.reduced_inputs()[1].outcome(),
        InputOutcome::InteractionProcessed {
            outcome: InteractionOutcome::Rejected(InteractionRejection::TargetChanged),
            ..
        }
    ));
    assert_eq!(fixture.engine.workspace(), &before);
    assert_eq!(
        fixture.engine.interaction().status(),
        InteractionStatus::Idle
    );
}

#[test]
fn stale_preview_acknowledgement_cannot_mark_a_replacement_preview_as_painted() {
    let mut fixture = fixture();
    publish_ready_scene(&mut fixture);
    let session = arm(&mut fixture);
    begin_and_hover(&mut fixture, session, 50.0);
    let stale = fixture
        .engine
        .interaction()
        .preview()
        .expect("center preview must exist")
        .acknowledgement();
    fixture
        .engine
        .enqueue_renderer_intent(RendererIntent::UpdateDrag {
            session,
            target: TargetAuthority::local(
                SURFACE_B,
                Authority::Known(Some(SurfacePointer::new(SURFACE_B, point(150.0, 50.0)))),
            ),
            tear_off: None,
        })
        .expect("replacement preview sequence must be available");
    fixture
        .engine
        .reduce_pending()
        .expect("replacement preview must reduce");
    let current_token = fixture
        .engine
        .interaction()
        .preview()
        .expect("edge preview must exist")
        .token();
    assert_ne!(current_token, stale.token());
    fixture
        .engine
        .enqueue_renderer_intent(RendererIntent::AcknowledgePreview(stale))
        .expect("stale acknowledgement sequence must be available");

    let transition = fixture
        .engine
        .reduce_pending()
        .expect("stale acknowledgement is a nonfatal rejection");
    assert!(matches!(
        transition.reduced_inputs()[0].outcome(),
        InputOutcome::InteractionProcessed {
            outcome: InteractionOutcome::Rejected(
                InteractionRejection::PreviewAcknowledgementMismatch
            ),
            ..
        }
    ));
    assert_eq!(
        fixture
            .engine
            .interaction()
            .preview()
            .expect("current preview must remain active")
            .token(),
        current_token
    );
}

#[test]
fn release_binding_and_button_authority_fail_closed_without_inference() {
    let mut fixture = fixture();
    publish_ready_scene(&mut fixture);
    let session = arm(&mut fixture);
    begin_and_hover(&mut fixture, session, 50.0);
    let intents = [
        release_with(
            session,
            PointerId::new(2),
            PointerButton::Primary,
            Authority::Known(PointerButtonState::Released),
            50.0,
        ),
        release_with(
            session,
            POINTER,
            PointerButton::Secondary,
            Authority::Known(PointerButtonState::Released),
            50.0,
        ),
        release_with(
            session,
            POINTER,
            PointerButton::Primary,
            Authority::Known(PointerButtonState::Pressed),
            50.0,
        ),
        release_with(
            session,
            POINTER,
            PointerButton::Primary,
            Authority::Unknown(AuthorityUnavailableReason::NotReported),
            50.0,
        ),
    ];
    for intent in intents {
        fixture
            .engine
            .enqueue_renderer_intent(intent)
            .expect("release sequence must be available");
    }

    let transition = fixture
        .engine
        .reduce_pending()
        .expect("release rejections and cancellation are nonfatal");
    let outcomes: Vec<_> = transition
        .reduced_inputs()
        .iter()
        .map(dockspace::transition::ReducedInput::outcome)
        .collect();
    assert!(matches!(
        outcomes[0],
        InputOutcome::InteractionProcessed {
            outcome: InteractionOutcome::Rejected(InteractionRejection::PointerMismatch),
            ..
        }
    ));
    assert!(matches!(
        outcomes[1],
        InputOutcome::InteractionProcessed {
            outcome: InteractionOutcome::Rejected(InteractionRejection::ButtonMismatch),
            ..
        }
    ));
    assert!(matches!(
        outcomes[2],
        InputOutcome::InteractionProcessed {
            outcome: InteractionOutcome::Rejected(InteractionRejection::ButtonStillPressed),
            ..
        }
    ));
    assert!(matches!(
        outcomes[3],
        InputOutcome::InteractionProcessed {
            outcome: InteractionOutcome::Cancelled {
                reason: InteractionCancelReason::UnknownButtonState,
                ..
            },
            ..
        }
    ));
    assert_eq!(
        fixture.engine.interaction().status(),
        InteractionStatus::Idle
    );
}

#[test]
fn release_without_an_active_session_is_a_nonfatal_rejection() {
    let mut fixture = fixture();
    let session = DragSessionId::new(fixture.engine.version().epoch(), DragGeneration::new(1));
    fixture
        .engine
        .enqueue_renderer_intent(release_intent(session, 50.0))
        .expect("release sequence must be available");

    let transition = fixture
        .engine
        .reduce_pending()
        .expect("inactive release is a nonfatal rejection");
    assert!(matches!(
        transition.reduced_inputs()[0].outcome(),
        InputOutcome::InteractionProcessed {
            outcome: InteractionOutcome::Rejected(InteractionRejection::NoActiveGesture),
            ..
        }
    ));
}

#[test]
fn unknown_target_authority_cancels_without_geometry_inference() {
    let mut fixture = fixture();
    publish_ready_scene(&mut fixture);
    let session = arm(&mut fixture);
    fixture
        .engine
        .enqueue_renderer_intent(RendererIntent::BeginDrag {
            session,
            pointer: POINTER,
            button: PointerButton::Primary,
        })
        .expect("begin sequence must be available");
    fixture
        .engine
        .enqueue_renderer_intent(RendererIntent::UpdateDrag {
            session,
            target: TargetAuthority::local(
                SURFACE_B,
                Authority::Unknown(dockspace::intent::AuthorityUnavailableReason::NotReported),
            ),
            tear_off: None,
        })
        .expect("unknown observation sequence must be available");

    let transition = fixture
        .engine
        .reduce_pending()
        .expect("unknown target must cancel nonfatally");
    assert!(matches!(
        transition.reduced_inputs()[1].outcome(),
        InputOutcome::InteractionProcessed {
            outcome: InteractionOutcome::Cancelled {
                reason: InteractionCancelReason::UnknownTargetAuthority,
                ..
            },
            ..
        }
    ));
    assert_eq!(
        fixture.engine.interaction().status(),
        InteractionStatus::Idle
    );
}

#[test]
fn bootstrap_scene_cancels_an_active_drag_instead_of_reusing_old_geometry() {
    let mut fixture = fixture();
    publish_ready_scene(&mut fixture);
    let session = arm(&mut fixture);
    begin_and_hover(&mut fixture, session, 50.0);
    let bootstrap = BuildingScene::new([SURFACE_A, SURFACE_B]).expect("test roster must be unique");
    fixture
        .engine
        .enqueue_scene(bootstrap)
        .expect("scene sequence must be available");

    let transition = fixture
        .engine
        .reduce_pending()
        .expect("bootstrap publication must succeed");
    assert!(matches!(
        transition.reduced_inputs()[0].outcome(),
        InputOutcome::ScenePublished {
            ready_surfaces: 0,
            bootstrap_surfaces: 2,
            ..
        }
    ));
    assert_eq!(
        fixture.engine.interaction().status(),
        InteractionStatus::Idle
    );
    assert!(transition.interaction_events().iter().any(|event| matches!(
        event.kind(),
        dockspace::interaction::InteractionEventKind::Cancelled {
            reason: InteractionCancelReason::SceneUnavailable,
            ..
        }
    )));
}

#[test]
fn malformed_scene_clears_a_painted_preview_instead_of_reusing_old_geometry() {
    let mut fixture = fixture();
    publish_ready_scene(&mut fixture);
    let session = arm(&mut fixture);
    begin_and_hover(&mut fixture, session, 50.0);
    let acknowledgement = fixture
        .engine
        .interaction()
        .preview()
        .expect("preview must exist")
        .acknowledgement();
    fixture
        .engine
        .enqueue_renderer_intent(RendererIntent::AcknowledgePreview(acknowledgement))
        .expect("acknowledgement sequence must be available");
    fixture
        .engine
        .reduce_pending()
        .expect("acknowledgement must reduce");
    let before = fixture.engine.workspace().clone();

    let mut malformed =
        BuildingScene::new([SURFACE_A, SURFACE_B]).expect("test roster must be unique");
    let mut source = ReadySurfaceScene::new(SURFACE_A, rect(0.0, 0.0, 300.0, 200.0));
    source.push_node(SemanticRect::new(
        NodeSceneId {
            root: ROOT_B,
            node: fixture.tabs_b,
        },
        rect(0.0, 0.0, 100.0, 100.0),
        SceneLayerKey::new(0),
    ));
    malformed
        .insert_ready(source)
        .expect("malformed facts remain structurally unique");
    fixture
        .engine
        .enqueue_scene(malformed)
        .expect("malformed scene sequence must be available");

    let transition = fixture
        .engine
        .reduce_pending()
        .expect("malformed scene is a nonfatal rejection");
    assert!(matches!(
        transition.reduced_inputs()[0].outcome(),
        InputOutcome::SceneRejected {
            error: SceneBuildError::InvalidNodeSemantic { .. }
        }
    ));
    assert!(fixture.engine.scene().is_none());
    assert_eq!(
        fixture.engine.interaction().status(),
        InteractionStatus::Idle
    );
    assert_eq!(fixture.engine.workspace(), &before);
    assert!(transition.interaction_events().iter().any(|event| matches!(
        event.kind(),
        dockspace::interaction::InteractionEventKind::Cancelled {
            reason: InteractionCancelReason::SceneUnavailable,
            ..
        }
    )));

    fixture
        .engine
        .enqueue_renderer_intent(release_intent(session, 50.0))
        .expect("stale release sequence must be available");
    let release = fixture
        .engine
        .reduce_pending()
        .expect("stale release is a nonfatal rejection");
    assert!(matches!(
        release.reduced_inputs()[0].outcome(),
        InputOutcome::InteractionProcessed {
            outcome: InteractionOutcome::Rejected(InteractionRejection::NoActiveGesture),
            ..
        }
    ));
    assert_eq!(fixture.engine.workspace(), &before);
}

#[test]
fn workspace_restore_invalidates_scene_and_drag_epoch_before_renderer_intents() {
    let mut fixture = fixture();
    publish_ready_scene(&mut fixture);
    let session = arm(&mut fixture);
    begin_and_hover(&mut fixture, session, 50.0);
    let replacement = fixture.engine.workspace().clone();
    fixture
        .engine
        .enqueue_renderer_intent(release_intent(session, 50.0))
        .expect("release sequence must be available");
    fixture
        .engine
        .enqueue_workspace_replacement(replacement)
        .expect("replacement sequence must be available");

    let transition = fixture
        .engine
        .reduce_pending()
        .expect("replacement must commit before release");
    assert!(matches!(
        transition.reduced_inputs()[0].outcome(),
        InputOutcome::WorkspaceReplaced { .. }
    ));
    assert!(matches!(
        transition.reduced_inputs()[1].outcome(),
        InputOutcome::StaleRejected { .. }
    ));
    assert_eq!(
        fixture.engine.interaction().status(),
        InteractionStatus::Idle
    );
    assert!(fixture.engine.scene().is_none());
    assert_eq!(fixture.engine.version().epoch().get(), 1);
}

#[test]
fn policy_change_invalidates_the_scene_and_active_drag() {
    let mut fixture = fixture();
    publish_ready_scene(&mut fixture);
    let session = arm(&mut fixture);
    begin_and_hover(&mut fixture, session, 50.0);
    let expected = fixture.engine.version();
    let mut policy = fixture.engine.policy().clone();
    policy.set_allow_tab_merge(false);
    fixture
        .engine
        .enqueue(EngineInput::ReplacePolicy { expected, policy })
        .expect("policy sequence must be available");

    let transition = fixture
        .engine
        .reduce_pending()
        .expect("policy replacement must reduce");
    assert!(matches!(
        transition.reduced_inputs()[0].outcome(),
        InputOutcome::PolicyReplaced { changed: true, .. }
    ));
    assert!(fixture.engine.scene().is_none());
    assert_eq!(
        fixture.engine.interaction().status(),
        InteractionStatus::Idle
    );
    assert!(transition.interaction_events().iter().any(|event| matches!(
        event.kind(),
        dockspace::interaction::InteractionEventKind::Cancelled {
            reason: InteractionCancelReason::PolicyChanged,
            ..
        }
    )));
}

#[test]
fn a_new_gesture_explicitly_replaces_the_previous_generation() {
    let mut fixture = fixture();
    publish_ready_scene(&mut fixture);
    let first = arm(&mut fixture);
    let second = arm(&mut fixture);

    assert_ne!(first, second);
    assert_eq!(
        fixture.engine.interaction().status(),
        InteractionStatus::Armed { session: second }
    );
}

fn resize_fixture() -> (DockEngine, NodeId) {
    let mut builder = Workspace::builder();
    let left = builder.insert_node(Node::tabs([ItemId::new(1)]));
    let right = builder.insert_node(Node::tabs([ItemId::new(2)]));
    let split = builder.insert_node(
        Node::equal_split(Axis::Horizontal, [left, right]).expect("split must be valid"),
    );
    builder.set_root(ROOT_A, RootRecord::new(split));
    builder.set_surface(SURFACE_A, SurfacePresentation::new(ROOT_A));
    let workspace = builder.build().expect("resize workspace must be valid");
    (
        DockEngine::new(workspace, DockPolicy::default()).expect("resize engine must be valid"),
        split,
    )
}

#[test]
fn resize_cancel_discards_the_validated_transient_override() {
    let (mut engine, split) = resize_fixture();
    let source = engine
        .workspace()
        .capture_node_source(ROOT_A, split)
        .expect("split source must be current");
    engine
        .enqueue_renderer_intent(RendererIntent::BeginResize {
            pointer: POINTER,
            button: PointerButton::Primary,
            split: source,
        })
        .expect("begin resize sequence must be available");
    let begin = engine.reduce_pending().expect("begin resize must reduce");
    let session = match begin.reduced_inputs()[0].outcome() {
        InputOutcome::InteractionProcessed {
            outcome: InteractionOutcome::ResizeBegan { session, .. },
            ..
        } => *session,
        outcome => panic!("unexpected resize outcome: {outcome:?}"),
    };
    let weights = SplitWeight::normalize([0.25, 0.75]).expect("weights must be valid");
    engine
        .enqueue_renderer_intent(RendererIntent::UpdateResize {
            session,
            weights: weights.clone(),
        })
        .expect("resize update sequence must be available");
    engine.reduce_pending().expect("resize update must reduce");
    assert_eq!(
        engine
            .interaction()
            .resize_weights()
            .map(|(_, _, current)| current),
        Some(weights.as_slice())
    );
    let before = engine.workspace().clone();
    engine
        .enqueue_renderer_intent(RendererIntent::CancelResize {
            session,
            reason: InteractionCancelReason::Escape,
        })
        .expect("resize cancel sequence must be available");
    engine.reduce_pending().expect("resize cancel must reduce");
    assert_eq!(engine.interaction().status(), InteractionStatus::Idle);
    assert_eq!(engine.workspace(), &before);
}

#[test]
fn resize_commits_only_on_authoritative_matching_release() {
    let (mut engine, split) = resize_fixture();
    let source = engine
        .workspace()
        .capture_node_source(ROOT_A, split)
        .expect("split source must be current");
    engine
        .enqueue_renderer_intent(RendererIntent::BeginResize {
            pointer: POINTER,
            button: PointerButton::Primary,
            split: source,
        })
        .expect("begin resize sequence must be available");
    let begin = engine.reduce_pending().expect("begin resize must reduce");
    let session = match begin.reduced_inputs()[0].outcome() {
        InputOutcome::InteractionProcessed {
            outcome: InteractionOutcome::ResizeBegan { session, .. },
            ..
        } => *session,
        outcome => panic!("unexpected resize outcome: {outcome:?}"),
    };
    let weights = SplitWeight::normalize([0.3, 0.7]).expect("weights must be valid");
    engine
        .enqueue_renderer_intent(RendererIntent::UpdateResize {
            session,
            weights: weights.clone(),
        })
        .expect("resize update sequence must be available");
    engine.reduce_pending().expect("resize update must reduce");
    engine
        .enqueue_renderer_intent(RendererIntent::ReleaseResize {
            session,
            pointer: POINTER,
            button: PointerButton::Primary,
            button_state: Authority::Known(PointerButtonState::Released),
        })
        .expect("resize release sequence must be available");
    let release = engine.reduce_pending().expect("resize release must commit");
    assert!(matches!(
        release.reduced_inputs()[0].outcome(),
        InputOutcome::InteractionProcessed {
            outcome: InteractionOutcome::ResizeDelivered { changed: true, .. },
            ..
        }
    ));
    assert!(matches!(
        engine.workspace().node(split),
        Some(Node::Split { weights: current, .. }) if current == &weights
    ));
    assert_eq!(engine.version().revision().get(), 1);
}
