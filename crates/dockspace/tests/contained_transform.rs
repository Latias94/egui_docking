use dockspace::command::{MovePayload, WorkspaceCommand};
use dockspace::engine::{DockEngine, EngineInput};
use dockspace::error::CommandError;
use dockspace::geometry::{LogicalPoint, LogicalRect, LogicalSize};
use dockspace::graph::{Axis, ContainedFloating, Node, RootRecord, SurfacePresentation, Workspace};
use dockspace::ids::{FloatingPresentationId, ItemId, NodeId, RootId, SurfaceId};
use dockspace::intent::{
    Authority, AuthorityUnavailableReason, ContainedHorizontalResizeEdge, ContainedResizeEdges,
    ContainedTransformKind, ContainedVerticalResizeEdge, PointerButton, PointerButtonState,
    PointerId, RendererIntent,
};
use dockspace::interaction::{
    ContainedTransformPreview, ContainedTransformSessionId, DragSessionId, InteractionCancelReason,
    InteractionOutcome, InteractionRejection, InteractionStatus,
};
use dockspace::policy::{DockPolicy, PolicyRejection};
use dockspace::scene::{BuildingScene, ReadySurfaceScene};
use dockspace::transition::InputOutcome;

const SURFACE: SurfaceId = SurfaceId::new(1);
const MAIN_ROOT: RootId = RootId::new(1);
const FLOATING_ROOT: RootId = RootId::new(2);
const FLOATING: FloatingPresentationId = FloatingPresentationId::new(1);
const POINTER: PointerId = PointerId::new(1);
const OTHER_POINTER: PointerId = PointerId::new(2);

struct Fixture {
    engine: DockEngine,
    main_tabs: NodeId,
    main_split: NodeId,
}

fn rect(x: f64, y: f64, width: f64, height: f64) -> LogicalRect {
    LogicalRect::new(x, y, width, height).expect("test rectangle must be valid")
}

fn size(width: f64, height: f64) -> LogicalSize {
    LogicalSize::new(width, height).expect("test size must be valid")
}

fn point(x: f64, y: f64) -> LogicalPoint {
    LogicalPoint::new(x, y).expect("test point must be valid")
}

fn initial_rect() -> LogicalRect {
    rect(120.0, 70.0, 120.0, 90.0)
}

fn fixture_with(initial: LogicalRect, policy: DockPolicy) -> Fixture {
    let mut builder = Workspace::builder();
    let main_tabs = builder.insert_node(Node::tabs([ItemId::new(1), ItemId::new(2)]));
    let peer_tabs = builder.insert_node(Node::tabs([ItemId::new(3)]));
    let main_split = builder.insert_node(
        Node::split(Axis::Horizontal, [main_tabs, peer_tabs], [1.0, 1.0])
            .expect("test split must be valid"),
    );
    let floating_tabs = builder.insert_node(Node::tabs([ItemId::new(4)]));
    builder.set_root(MAIN_ROOT, RootRecord::new(main_split));
    builder.set_root(FLOATING_ROOT, RootRecord::new(floating_tabs));
    builder.set_surface(SURFACE, SurfacePresentation::new(MAIN_ROOT));
    builder.set_contained_floating(ContainedFloating::new(
        FLOATING,
        FLOATING_ROOT,
        SURFACE,
        initial,
        7,
    ));
    builder
        .attach_contained(SURFACE, FLOATING)
        .expect("test surface must exist");
    let workspace = builder.build().expect("test workspace must be valid");
    Fixture {
        engine: DockEngine::new(workspace, policy).expect("test engine must be valid"),
        main_tabs,
        main_split,
    }
}

fn fixture() -> Fixture {
    fixture_with(initial_rect(), DockPolicy::default())
}

fn ready_scene() -> BuildingScene {
    let mut scene = BuildingScene::new([SURFACE]).expect("test roster must be unique");
    scene
        .insert_ready(ReadySurfaceScene::new(
            SURFACE,
            rect(100.0, 50.0, 300.0, 200.0),
        ))
        .expect("surface facts must be unique");
    scene
}

fn publish_ready_scene(engine: &mut DockEngine) {
    engine
        .enqueue_scene(ready_scene())
        .expect("scene must enqueue");
    engine.reduce_pending().expect("scene must publish");
}

fn interaction_outcome(engine: &mut DockEngine, intent: RendererIntent) -> InteractionOutcome {
    engine
        .enqueue_renderer_intent(intent)
        .expect("renderer intent must enqueue");
    let transition = engine
        .reduce_pending()
        .expect("renderer intent must reduce");
    match transition.reduced_inputs()[0].outcome() {
        InputOutcome::InteractionProcessed { outcome, .. } => outcome.clone(),
        outcome => panic!("unexpected input outcome: {outcome:?}"),
    }
}

fn begin_transform(
    engine: &mut DockEngine,
    kind: ContainedTransformKind,
    initial_pointer: LogicalPoint,
) -> ContainedTransformSessionId {
    let outcome = interaction_outcome(
        engine,
        RendererIntent::BeginContainedTransform {
            surface: SURFACE,
            root: FLOATING_ROOT,
            floating: FLOATING,
            pointer: POINTER,
            button: PointerButton::Primary,
            initial_pointer,
            kind,
            minimum_size: size(40.0, 30.0),
        },
    );
    let InteractionOutcome::ContainedTransformBegan { session, .. } = outcome else {
        panic!("unexpected begin outcome: {outcome:?}");
    };
    session
}

fn update_transform(
    engine: &mut DockEngine,
    session: ContainedTransformSessionId,
    current_pointer: LogicalPoint,
) -> ContainedTransformPreview {
    let outcome = interaction_outcome(
        engine,
        RendererIntent::UpdateContainedTransform {
            session,
            current_pointer,
        },
    );
    let InteractionOutcome::ContainedTransformPreviewUpdated { preview, .. } = outcome else {
        panic!("unexpected update outcome: {outcome:?}");
    };
    preview
}

fn acknowledge_preview(engine: &mut DockEngine, preview: ContainedTransformPreview) {
    let outcome = interaction_outcome(
        engine,
        RendererIntent::AcknowledgeContainedTransformPreview(preview.acknowledgement()),
    );
    assert!(matches!(
        outcome,
        InteractionOutcome::ContainedTransformPreviewAcknowledged { changed: true, .. }
    ));
}

fn release_transform(
    engine: &mut DockEngine,
    session: ContainedTransformSessionId,
    pointer: PointerId,
    button: PointerButton,
    button_state: Authority<PointerButtonState>,
) -> InteractionOutcome {
    interaction_outcome(
        engine,
        RendererIntent::ReleaseContainedTransform {
            session,
            pointer,
            button,
            button_state,
        },
    )
}

#[test]
fn move_uses_frozen_rect_and_absolute_pointer_then_delivers_once() {
    let mut fixture = fixture();
    publish_ready_scene(&mut fixture.engine);
    let initial_pointer = point(150.0, 100.0);
    let session = begin_transform(
        &mut fixture.engine,
        ContainedTransformKind::Move,
        initial_pointer,
    );

    let view = fixture
        .engine
        .interaction()
        .active_contained_transform_view()
        .expect("transform view must be published");
    assert_eq!(view.session(), session);
    assert_eq!(view.source_rect(), initial_rect());
    assert_eq!(view.initial_pointer(), initial_pointer);
    assert_eq!(view.current_pointer(), initial_pointer);
    assert_eq!(view.preview(), None);

    assert_eq!(
        update_transform(&mut fixture.engine, session, point(170.0, 120.0)).rect(),
        rect(140.0, 90.0, 120.0, 90.0)
    );
    let preview = update_transform(&mut fixture.engine, session, point(180.0, 110.0));
    assert_eq!(preview.rect(), rect(150.0, 80.0, 120.0, 90.0));
    acknowledge_preview(&mut fixture.engine, preview);

    assert!(matches!(
        release_transform(
            &mut fixture.engine,
            session,
            POINTER,
            PointerButton::Primary,
            Authority::Known(PointerButtonState::Pressed),
        ),
        InteractionOutcome::Rejected(InteractionRejection::ButtonStillPressed)
    ));
    assert_eq!(
        fixture.engine.interaction().status(),
        InteractionStatus::ContainedTransforming { session }
    );

    assert!(matches!(
        release_transform(
            &mut fixture.engine,
            session,
            POINTER,
            PointerButton::Primary,
            Authority::Known(PointerButtonState::Released),
        ),
        InteractionOutcome::ContainedTransformDelivered { changed: true, .. }
    ));
    let stored = fixture
        .engine
        .workspace()
        .contained_floating(FLOATING)
        .expect("floating must remain present");
    assert_eq!(stored.rect, preview.rect());
    assert_eq!(stored.z_order, 7, "transform must not implicitly raise");
    assert_eq!(
        fixture.engine.interaction().status(),
        InteractionStatus::Idle
    );

    assert!(matches!(
        release_transform(
            &mut fixture.engine,
            session,
            POINTER,
            PointerButton::Primary,
            Authority::Known(PointerButtonState::Released),
        ),
        InteractionOutcome::Rejected(
            InteractionRejection::DuplicateContainedTransformRelease { .. }
        )
    ));
}

#[test]
fn resize_edges_hold_opposite_anchors_and_clamp_to_minimum_and_bounds() {
    let cases = [
        (
            ContainedTransformKind::Resize(ContainedResizeEdges::horizontal(
                ContainedHorizontalResizeEdge::Left,
            )),
            point(400.0, 100.0),
            rect(200.0, 70.0, 40.0, 90.0),
        ),
        (
            ContainedTransformKind::Resize(ContainedResizeEdges::horizontal(
                ContainedHorizontalResizeEdge::Right,
            )),
            point(500.0, 100.0),
            rect(120.0, 70.0, 280.0, 90.0),
        ),
        (
            ContainedTransformKind::Resize(ContainedResizeEdges::vertical(
                ContainedVerticalResizeEdge::Top,
            )),
            point(150.0, -100.0),
            rect(120.0, 50.0, 120.0, 110.0),
        ),
        (
            ContainedTransformKind::Resize(ContainedResizeEdges::vertical(
                ContainedVerticalResizeEdge::Bottom,
            )),
            point(150.0, 400.0),
            rect(120.0, 70.0, 120.0, 180.0),
        ),
        (
            ContainedTransformKind::Resize(ContainedResizeEdges::corner(
                ContainedHorizontalResizeEdge::Left,
                ContainedVerticalResizeEdge::Top,
            )),
            point(0.0, 0.0),
            rect(100.0, 50.0, 140.0, 110.0),
        ),
    ];

    for (kind, current_pointer, expected) in cases {
        let mut fixture = fixture();
        publish_ready_scene(&mut fixture.engine);
        let session = begin_transform(&mut fixture.engine, kind, point(150.0, 100.0));
        let preview = update_transform(&mut fixture.engine, session, current_pointer);
        assert_eq!(preview.rect(), expected);
    }
}

#[test]
fn begin_rejects_unreconciled_geometry_and_disabled_policy() {
    let mut outside = fixture_with(rect(80.0, 40.0, 120.0, 90.0), DockPolicy::default());
    publish_ready_scene(&mut outside.engine);
    assert!(matches!(
        interaction_outcome(
            &mut outside.engine,
            RendererIntent::BeginContainedTransform {
                surface: SURFACE,
                root: FLOATING_ROOT,
                floating: FLOATING,
                pointer: POINTER,
                button: PointerButton::Primary,
                initial_pointer: point(100.0, 100.0),
                kind: ContainedTransformKind::Move,
                minimum_size: size(40.0, 30.0),
            },
        ),
        InteractionOutcome::Rejected(
            InteractionRejection::ContainedTransformInitialRectUnavailable
        )
    ));

    let mut policy = DockPolicy::default();
    policy.set_allow_contained_floating(false);
    let mut disabled = fixture_with(initial_rect(), policy);
    publish_ready_scene(&mut disabled.engine);
    assert!(matches!(
        interaction_outcome(
            &mut disabled.engine,
            RendererIntent::BeginContainedTransform {
                surface: SURFACE,
                root: FLOATING_ROOT,
                floating: FLOATING,
                pointer: POINTER,
                button: PointerButton::Primary,
                initial_pointer: point(100.0, 100.0),
                kind: ContainedTransformKind::Move,
                minimum_size: size(40.0, 30.0),
            },
        ),
        InteractionOutcome::Rejected(InteractionRejection::CommandRejected(CommandError::Policy(
            PolicyRejection::ContainedFloatingDisabled
        )))
    ));
}

#[test]
fn scene_rollover_republishes_a_new_unpainted_token() {
    let mut fixture = fixture();
    publish_ready_scene(&mut fixture.engine);
    let session = begin_transform(
        &mut fixture.engine,
        ContainedTransformKind::Move,
        point(150.0, 100.0),
    );
    let old_preview = update_transform(&mut fixture.engine, session, point(180.0, 120.0));
    acknowledge_preview(&mut fixture.engine, old_preview);

    publish_ready_scene(&mut fixture.engine);
    let current_preview = *fixture
        .engine
        .interaction()
        .contained_transform_preview()
        .expect("rollover must republish the preview");
    assert_eq!(current_preview.rect(), old_preview.rect());
    assert_ne!(current_preview.token(), old_preview.token());
    assert_ne!(current_preview.token().scene(), old_preview.token().scene());

    assert!(matches!(
        interaction_outcome(
            &mut fixture.engine,
            RendererIntent::AcknowledgeContainedTransformPreview(old_preview.acknowledgement()),
        ),
        InteractionOutcome::Rejected(
            InteractionRejection::ContainedTransformPreviewAcknowledgementMismatch
        )
    ));
    acknowledge_preview(&mut fixture.engine, current_preview);
    assert!(matches!(
        release_transform(
            &mut fixture.engine,
            session,
            POINTER,
            PointerButton::Primary,
            Authority::Known(PointerButtonState::Released),
        ),
        InteractionOutcome::ContainedTransformDelivered { changed: true, .. }
    ));
}

#[test]
fn release_errors_are_typed_and_do_not_leak_a_live_session() {
    let mut missing = fixture();
    publish_ready_scene(&mut missing.engine);
    let missing_session = begin_transform(
        &mut missing.engine,
        ContainedTransformKind::Move,
        point(150.0, 100.0),
    );
    assert!(matches!(
        release_transform(
            &mut missing.engine,
            missing_session,
            POINTER,
            PointerButton::Primary,
            Authority::Known(PointerButtonState::Released),
        ),
        InteractionOutcome::Rejected(InteractionRejection::PreviewMissing)
    ));
    assert_eq!(
        missing.engine.interaction().status(),
        InteractionStatus::Idle
    );

    let mut unpainted = fixture();
    publish_ready_scene(&mut unpainted.engine);
    let unpainted_session = begin_transform(
        &mut unpainted.engine,
        ContainedTransformKind::Move,
        point(150.0, 100.0),
    );
    update_transform(
        &mut unpainted.engine,
        unpainted_session,
        point(160.0, 110.0),
    );
    assert!(matches!(
        release_transform(
            &mut unpainted.engine,
            unpainted_session,
            POINTER,
            PointerButton::Primary,
            Authority::Known(PointerButtonState::Released),
        ),
        InteractionOutcome::Rejected(InteractionRejection::PreviewNotPainted)
    ));
    assert!(matches!(
        release_transform(
            &mut unpainted.engine,
            unpainted_session,
            POINTER,
            PointerButton::Primary,
            Authority::Known(PointerButtonState::Released),
        ),
        InteractionOutcome::Rejected(
            InteractionRejection::DuplicateContainedTransformRelease { .. }
        )
    ));
}

#[test]
fn binding_mismatch_retains_session_and_unknown_authority_cancels_it() {
    let mut fixture = fixture();
    publish_ready_scene(&mut fixture.engine);
    let session = begin_transform(
        &mut fixture.engine,
        ContainedTransformKind::Move,
        point(150.0, 100.0),
    );

    assert!(matches!(
        release_transform(
            &mut fixture.engine,
            session,
            OTHER_POINTER,
            PointerButton::Primary,
            Authority::Known(PointerButtonState::Released),
        ),
        InteractionOutcome::Rejected(InteractionRejection::PointerMismatch)
    ));
    assert!(matches!(
        release_transform(
            &mut fixture.engine,
            session,
            POINTER,
            PointerButton::Secondary,
            Authority::Known(PointerButtonState::Released),
        ),
        InteractionOutcome::Rejected(InteractionRejection::ButtonMismatch)
    ));
    assert_eq!(
        fixture.engine.interaction().status(),
        InteractionStatus::ContainedTransforming { session }
    );

    assert!(matches!(
        release_transform(
            &mut fixture.engine,
            session,
            POINTER,
            PointerButton::Primary,
            Authority::Unknown(AuthorityUnavailableReason::NotReported),
        ),
        InteractionOutcome::Cancelled {
            reason: InteractionCancelReason::UnknownButtonState,
            ..
        }
    ));
    assert_eq!(
        fixture.engine.interaction().status(),
        InteractionStatus::Idle
    );
}

fn arm_and_begin_drag(fixture: &mut Fixture) -> DragSessionId {
    let payload = MovePayload::Item(
        fixture
            .engine
            .workspace()
            .capture_item_source(MAIN_ROOT, fixture.main_tabs, ItemId::new(1))
            .expect("drag source must be current"),
    );
    let armed = interaction_outcome(
        &mut fixture.engine,
        RendererIntent::ArmDrag {
            pointer: POINTER,
            button: PointerButton::Primary,
            payload,
        },
    );
    let InteractionOutcome::DragArmed { session, .. } = armed else {
        panic!("unexpected arm outcome: {armed:?}");
    };
    let begun = interaction_outcome(
        &mut fixture.engine,
        RendererIntent::BeginDrag {
            session,
            pointer: POINTER,
            button: PointerButton::Primary,
        },
    );
    assert!(matches!(begun, InteractionOutcome::DragBegan { .. }));
    session
}

#[test]
fn contained_transform_is_mutually_exclusive_with_drag_and_split_resize() {
    let mut fixture = fixture();
    publish_ready_scene(&mut fixture.engine);
    let drag = arm_and_begin_drag(&mut fixture);
    assert_eq!(
        fixture.engine.interaction().status(),
        InteractionStatus::Dragging { session: drag }
    );
    let transform = interaction_outcome(
        &mut fixture.engine,
        RendererIntent::BeginContainedTransform {
            surface: SURFACE,
            root: FLOATING_ROOT,
            floating: FLOATING,
            pointer: POINTER,
            button: PointerButton::Primary,
            initial_pointer: point(150.0, 100.0),
            kind: ContainedTransformKind::Move,
            minimum_size: size(40.0, 30.0),
        },
    );
    assert!(matches!(
        transform,
        InteractionOutcome::ContainedTransformBegan {
            replaced: Some(InteractionStatus::Dragging { .. }),
            ..
        }
    ));

    let split = fixture
        .engine
        .workspace()
        .capture_node_source(MAIN_ROOT, fixture.main_split)
        .expect("split source must be current");
    let resize = interaction_outcome(
        &mut fixture.engine,
        RendererIntent::BeginResize {
            pointer: POINTER,
            button: PointerButton::Primary,
            split,
        },
    );
    assert!(matches!(
        resize,
        InteractionOutcome::ResizeBegan {
            replaced: Some(InteractionStatus::ContainedTransforming { .. }),
            ..
        }
    ));
}

#[test]
fn workspace_and_policy_changes_invalidate_the_transform() {
    let mut workspace_change = fixture();
    publish_ready_scene(&mut workspace_change.engine);
    begin_transform(
        &mut workspace_change.engine,
        ContainedTransformKind::Move,
        point(150.0, 100.0),
    );
    let select = workspace_change
        .engine
        .workspace()
        .capture_item_source(MAIN_ROOT, workspace_change.main_tabs, ItemId::new(2))
        .expect("selection source must be current");
    workspace_change
        .engine
        .enqueue_command(WorkspaceCommand::Select { source: select })
        .expect("command must enqueue");
    workspace_change
        .engine
        .reduce_pending()
        .expect("command must reduce");
    assert_eq!(
        workspace_change.engine.interaction().status(),
        InteractionStatus::Idle
    );

    let mut policy_change = fixture();
    publish_ready_scene(&mut policy_change.engine);
    begin_transform(
        &mut policy_change.engine,
        ContainedTransformKind::Move,
        point(150.0, 100.0),
    );
    let mut policy = policy_change.engine.policy().clone();
    policy.set_allow_native_surfaces(true);
    policy_change
        .engine
        .enqueue(EngineInput::ReplacePolicy {
            expected: policy_change.engine.version(),
            policy,
        })
        .expect("policy must enqueue");
    policy_change
        .engine
        .reduce_pending()
        .expect("policy must reduce");
    assert_eq!(
        policy_change.engine.interaction().status(),
        InteractionStatus::Idle
    );
}

#[test]
fn non_finite_absolute_delta_is_rejected_without_mutating_preview() {
    let mut fixture = fixture();
    publish_ready_scene(&mut fixture.engine);
    let session = begin_transform(
        &mut fixture.engine,
        ContainedTransformKind::Move,
        point(-f64::MAX, 100.0),
    );
    let outcome = interaction_outcome(
        &mut fixture.engine,
        RendererIntent::UpdateContainedTransform {
            session,
            current_pointer: point(f64::MAX, 100.0),
        },
    );
    assert!(matches!(
        outcome,
        InteractionOutcome::Rejected(InteractionRejection::ContainedTransformGeometryUnavailable)
    ));
    assert!(
        fixture
            .engine
            .interaction()
            .contained_transform_preview()
            .is_none()
    );
    assert_eq!(
        fixture.engine.interaction().status(),
        InteractionStatus::ContainedTransforming { session }
    );
}

#[test]
fn explicit_cancel_reports_the_exact_reason() {
    let mut fixture = fixture();
    publish_ready_scene(&mut fixture.engine);
    let session = begin_transform(
        &mut fixture.engine,
        ContainedTransformKind::Move,
        point(150.0, 100.0),
    );
    assert!(matches!(
        interaction_outcome(
            &mut fixture.engine,
            RendererIntent::CancelContainedTransform {
                session,
                reason: InteractionCancelReason::Escape,
            },
        ),
        InteractionOutcome::Cancelled {
            reason: InteractionCancelReason::Escape,
            ..
        }
    ));
    assert_eq!(
        fixture.engine.interaction().status(),
        InteractionStatus::Idle
    );
}
