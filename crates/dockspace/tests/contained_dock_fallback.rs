use dockspace::command::{CommandOutcome, DockTarget, MovePayload};
use dockspace::drop_target::{
    DropTargetAvailability, DropTargetId, DropTargetRecord, DropVisual, SceneLayerKey,
};
use dockspace::engine::DockEngine;
use dockspace::geometry::{LogicalPoint, LogicalRect, LogicalSize};
use dockspace::graph::{ContainedFloating, Node, RootRecord, SurfacePresentation, Workspace};
use dockspace::hit_region::HitRegion;
use dockspace::ids::{FloatingPresentationId, ItemId, NodeId, RootId, SurfaceId};
use dockspace::intent::{
    Authority, AuthorityUnavailableReason, ContainedTearOffProposal, PointerButton,
    PointerButtonState, PointerId, RendererIntent, SurfacePointer, TargetAuthority, TearOffRequest,
};
use dockspace::interaction::{
    DragSessionId, InteractionCancelReason, InteractionDelivery, InteractionOutcome,
    InteractionRejection, InteractionStatus, PreviewResolutionStatus, PreviewVisual,
    WorkspaceDeliveryKind,
};
use dockspace::policy::DockPolicy;
use dockspace::scene::{BuildingScene, ReadySurfaceScene};
use dockspace::transition::InputOutcome;

const SURFACE: SurfaceId = SurfaceId::new(1);
const BOOTSTRAP_SURFACE: SurfaceId = SurfaceId::new(2);
const MAIN_ROOT: RootId = RootId::new(1);
const BOOTSTRAP_ROOT: RootId = RootId::new(2);
const FLOATING_ROOT: RootId = RootId::new(3);
const FLOATING: FloatingPresentationId = FloatingPresentationId::new(3);
const POINTER: PointerId = PointerId::new(1);
const FLOATING_Z_ORDER: u64 = 11;

struct Fixture {
    engine: DockEngine,
    main_tabs: NodeId,
    floating_tabs: NodeId,
}

fn point(x: f64, y: f64) -> LogicalPoint {
    LogicalPoint::new(x, y).expect("test point must be valid")
}

fn rect(x: f64, y: f64, width: f64, height: f64) -> LogicalRect {
    LogicalRect::new(x, y, width, height).expect("test rectangle must be valid")
}

fn size(width: f64, height: f64) -> LogicalSize {
    LogicalSize::new(width, height).expect("test size must be valid")
}

fn initial_floating_rect() -> LogicalRect {
    rect(300.0, 200.0, 240.0, 180.0)
}

fn moved_floating_rect() -> LogicalRect {
    rect(420.0, 300.0, 240.0, 180.0)
}

fn fixture(policy: DockPolicy) -> Fixture {
    fixture_with_floating_items(policy, &[3])
}

fn fixture_with_floating_items(policy: DockPolicy, floating_items: &[u64]) -> Fixture {
    let mut builder = Workspace::builder();
    let main_tabs = builder.insert_node(Node::tabs([ItemId::new(1)]));
    let bootstrap_tabs = builder.insert_node(Node::tabs([ItemId::new(2)]));
    let floating_tabs =
        builder.insert_node(Node::tabs(floating_items.iter().copied().map(ItemId::new)));
    builder.set_root(MAIN_ROOT, RootRecord::new(main_tabs));
    builder.set_root(BOOTSTRAP_ROOT, RootRecord::new(bootstrap_tabs));
    builder.set_root(FLOATING_ROOT, RootRecord::new(floating_tabs));
    builder.set_surface(SURFACE, SurfacePresentation::new(MAIN_ROOT));
    builder.set_surface(BOOTSTRAP_SURFACE, SurfacePresentation::new(BOOTSTRAP_ROOT));
    builder.set_contained_floating(ContainedFloating::new(
        FLOATING,
        FLOATING_ROOT,
        SURFACE,
        initial_floating_rect(),
        FLOATING_Z_ORDER,
    ));
    builder
        .attach_contained(SURFACE, FLOATING)
        .expect("host surface must exist");
    let workspace = builder.build().expect("test workspace must be valid");
    let engine = DockEngine::new(workspace, policy).expect("test engine must be valid");
    Fixture {
        engine,
        main_tabs,
        floating_tabs,
    }
}

fn ready_surface_with_exact_target(fixture: &Fixture) -> ReadySurfaceScene {
    let mut ready = ReadySurfaceScene::new(SURFACE, rect(0.0, 0.0, 800.0, 600.0));
    let target = fixture
        .engine
        .workspace()
        .capture_tab_target(MAIN_ROOT, fixture.main_tabs)
        .expect("main tabs target must be current");
    ready.push_drop_target(DropTargetRecord::new(
        DropTargetId::Center {
            surface: SURFACE,
            root: MAIN_ROOT,
            tabs: fixture.main_tabs,
        },
        DockTarget::Center(target),
        DropTargetAvailability::Available,
        HitRegion::new(rect(20.0, 20.0, 80.0, 80.0)),
        SceneLayerKey::new(1),
        DropVisual::new(rect(20.0, 20.0, 80.0, 80.0)),
    ));
    ready
}

fn publish_scene(fixture: &mut Fixture, bootstrap_second_surface: bool) {
    let mut scene = BuildingScene::new([SURFACE, BOOTSTRAP_SURFACE])
        .expect("test surface roster must be unique");
    scene
        .insert_ready(ready_surface_with_exact_target(fixture))
        .expect("primary surface facts must be valid");
    if !bootstrap_second_surface {
        scene
            .insert_ready(ReadySurfaceScene::new(
                BOOTSTRAP_SURFACE,
                rect(0.0, 0.0, 400.0, 300.0),
            ))
            .expect("secondary surface facts must be valid");
    }
    fixture
        .engine
        .enqueue_scene(scene)
        .expect("scene sequence must be available");
    fixture.engine.reduce_pending().expect("scene must publish");
}

fn process(fixture: &mut Fixture, intent: RendererIntent) -> InteractionOutcome {
    fixture
        .engine
        .enqueue_renderer_intent(intent)
        .expect("renderer intent sequence must be available");
    let transition = fixture
        .engine
        .reduce_pending()
        .expect("renderer intent must reduce");
    match transition.reduced_inputs()[0].outcome() {
        InputOutcome::InteractionProcessed { outcome, .. } => outcome.clone(),
        outcome => panic!("unexpected input outcome: {outcome:?}"),
    }
}

fn arm_and_begin(fixture: &mut Fixture) -> DragSessionId {
    let payload = MovePayload::Item(
        fixture
            .engine
            .workspace()
            .capture_item_source(FLOATING_ROOT, fixture.floating_tabs, ItemId::new(3))
            .expect("complete floating source must be current"),
    );
    arm_and_begin_with_payload(fixture, payload)
}

fn arm_and_begin_subtree(fixture: &mut Fixture) -> DragSessionId {
    let payload = MovePayload::Subtree(
        fixture
            .engine
            .workspace()
            .capture_node_source(FLOATING_ROOT, fixture.floating_tabs)
            .expect("complete floating subtree must be current"),
    );
    arm_and_begin_with_payload(fixture, payload)
}

fn arm_and_begin_with_payload(fixture: &mut Fixture, payload: MovePayload) -> DragSessionId {
    let armed = process(
        fixture,
        RendererIntent::ArmDrag {
            pointer: POINTER,
            button: PointerButton::Primary,
            payload,
        },
    );
    let InteractionOutcome::DragArmed { session, .. } = armed else {
        panic!("unexpected arm outcome: {armed:?}");
    };
    let begun = process(
        fixture,
        RendererIntent::BeginDrag {
            session,
            pointer: POINTER,
            button: PointerButton::Primary,
        },
    );
    assert!(matches!(begun, InteractionOutcome::DragBegan { .. }));
    session
}

fn contained_fallback(fixture: &Fixture) -> TearOffRequest {
    contained_fallback_at(fixture, moved_floating_rect(), FLOATING_Z_ORDER)
}

fn contained_fallback_at(
    fixture: &Fixture,
    requested_rect: LogicalRect,
    z_order: u64,
) -> TearOffRequest {
    let placement = fixture
        .engine
        .contained_placement(SURFACE, requested_rect, size(0.0, 0.0))
        .expect("ready host surface must authorize the fallback placement");
    TearOffRequest::Contained(ContainedTearOffProposal::new(
        FLOATING_ROOT,
        FLOATING,
        placement,
        z_order,
    ))
}

fn target_at(surface: SurfaceId, position: LogicalPoint) -> TargetAuthority {
    TargetAuthority::local(
        surface,
        Authority::Known(Some(SurfacePointer::new(surface, position))),
    )
}

fn update_drag(
    fixture: &mut Fixture,
    session: DragSessionId,
    target: TargetAuthority,
    fallback: Option<TearOffRequest>,
) -> InteractionOutcome {
    process(
        fixture,
        RendererIntent::UpdateDrag {
            session,
            target,
            tear_off: fallback,
        },
    )
}

fn acknowledge_and_release(
    fixture: &mut Fixture,
    session: DragSessionId,
    target: TargetAuthority,
    fallback: Option<TearOffRequest>,
) -> InteractionOutcome {
    let acknowledgement = fixture
        .engine
        .interaction()
        .preview()
        .expect("a preview must exist before release")
        .acknowledgement();
    fixture
        .engine
        .enqueue_renderer_intent(RendererIntent::ReleaseDrag {
            session,
            pointer: POINTER,
            button: PointerButton::Primary,
            button_state: Authority::Known(PointerButtonState::Released),
            target,
            tear_off: fallback,
        })
        .expect("release sequence must be available");
    fixture
        .engine
        .enqueue_renderer_intent(RendererIntent::AcknowledgePreview(acknowledgement))
        .expect("acknowledgement sequence must be available");
    let transition = fixture
        .engine
        .reduce_pending()
        .expect("acknowledgement and release must reduce");
    assert!(matches!(
        transition.reduced_inputs()[0].outcome(),
        InputOutcome::InteractionProcessed {
            outcome: InteractionOutcome::PreviewAcknowledged { .. },
            ..
        }
    ));
    match transition.reduced_inputs()[1].outcome() {
        InputOutcome::InteractionProcessed { outcome, .. } => outcome.clone(),
        outcome => panic!("unexpected release outcome: {outcome:?}"),
    }
}

#[test]
fn exact_dock_wins_over_an_explicit_contained_fallback() {
    let mut fixture = fixture(DockPolicy::default());
    publish_scene(&mut fixture, false);
    let session = arm_and_begin(&mut fixture);
    let fallback = contained_fallback(&fixture);
    let exact_target = target_at(SURFACE, point(40.0, 40.0));

    let preview = update_drag(
        &mut fixture,
        session,
        exact_target.clone(),
        Some(fallback.clone()),
    );
    assert!(matches!(
        preview,
        InteractionOutcome::PreviewUpdated {
            preview: Some(ref preview),
            status: PreviewResolutionStatus::Resolved,
            ..
        } if matches!(preview.visual(), PreviewVisual::Dock { target, .. }
            if *target == DropTargetId::Center {
                surface: SURFACE,
                root: MAIN_ROOT,
                tabs: fixture.main_tabs,
            })
    ));

    let delivery = acknowledge_and_release(&mut fixture, session, exact_target, Some(fallback));
    assert!(matches!(
        delivery,
        InteractionOutcome::DragDelivered {
            delivery: InteractionDelivery::Workspace {
                kind: WorkspaceDeliveryKind::Dock,
                changed: true,
                ..
            },
            ..
        }
    ));
    assert!(fixture.engine.workspace().root(FLOATING_ROOT).is_none());
    assert!(
        fixture
            .engine
            .workspace()
            .contained_floating(FLOATING)
            .is_none()
    );
}

#[test]
fn known_none_moves_the_same_contained_presentation_atomically() {
    let mut fixture = fixture(DockPolicy::default());
    publish_scene(&mut fixture, false);
    let session = arm_and_begin(&mut fixture);
    let fallback = contained_fallback(&fixture);
    let no_exact_target = target_at(SURFACE, point(200.0, 150.0));

    let preview = update_drag(
        &mut fixture,
        session,
        no_exact_target.clone(),
        Some(fallback.clone()),
    );
    assert!(matches!(
        preview,
        InteractionOutcome::PreviewUpdated {
            preview: Some(ref preview),
            status: PreviewResolutionStatus::Resolved,
            ..
        } if matches!(
            preview.visual(),
            PreviewVisual::Contained {
                surface: SURFACE,
                rect,
                fallback: false,
            } if *rect == moved_floating_rect()
        )
    ));

    let delivery = acknowledge_and_release(&mut fixture, session, no_exact_target, Some(fallback));
    assert!(matches!(
        delivery,
        InteractionOutcome::DragDelivered {
            delivery: InteractionDelivery::Workspace {
                kind: WorkspaceDeliveryKind::Contained,
                outcome: CommandOutcome::ContainedRectUpdated {
                    floating: FLOATING,
                    changed: true,
                },
                changed: true,
            },
            ..
        }
    ));
    assert!(fixture.engine.workspace().root(FLOATING_ROOT).is_some());
    let floating = fixture
        .engine
        .workspace()
        .contained_floating(FLOATING)
        .expect("the same contained presentation must remain");
    assert_eq!(floating.root, FLOATING_ROOT);
    assert_eq!(floating.surface, SURFACE);
    assert_eq!(floating.rect, moved_floating_rect());
    assert_eq!(floating.z_order, FLOATING_Z_ORDER);
}

#[test]
fn known_none_moves_a_complete_subtree_without_rehoming_its_presentation() {
    let mut fixture = fixture(DockPolicy::default());
    publish_scene(&mut fixture, false);
    let session = arm_and_begin_subtree(&mut fixture);
    let fallback = contained_fallback(&fixture);
    let no_exact_target = target_at(SURFACE, point(200.0, 150.0));

    let preview = update_drag(
        &mut fixture,
        session,
        no_exact_target.clone(),
        Some(fallback.clone()),
    );
    assert!(matches!(
        preview,
        InteractionOutcome::PreviewUpdated {
            preview: Some(ref preview),
            status: PreviewResolutionStatus::Resolved,
            ..
        } if matches!(preview.visual(), PreviewVisual::Contained { fallback: false, .. })
    ));
    let delivery = acknowledge_and_release(&mut fixture, session, no_exact_target, Some(fallback));

    assert!(matches!(
        delivery,
        InteractionOutcome::DragDelivered {
            delivery: InteractionDelivery::Workspace {
                outcome: CommandOutcome::ContainedRectUpdated {
                    floating: FLOATING,
                    changed: true,
                },
                ..
            },
            ..
        }
    ));
    assert_eq!(
        fixture
            .engine
            .workspace()
            .contained_floating(FLOATING)
            .expect("same presentation must remain")
            .z_order,
        FLOATING_Z_ORDER
    );
}

#[test]
fn partial_payload_and_changed_stacking_fact_cannot_update_the_same_presentation() {
    let mut partial = fixture_with_floating_items(DockPolicy::default(), &[3, 4]);
    publish_scene(&mut partial, false);
    let partial_session = arm_and_begin(&mut partial);
    let partial_fallback = contained_fallback(&partial);
    let partial_outcome = update_drag(
        &mut partial,
        partial_session,
        target_at(SURFACE, point(200.0, 150.0)),
        Some(partial_fallback),
    );
    assert!(matches!(
        partial_outcome,
        InteractionOutcome::PreviewUpdated {
            preview: None,
            status: PreviewResolutionStatus::Rejected,
            ..
        }
    ));

    let mut changed_stack = fixture(DockPolicy::default());
    publish_scene(&mut changed_stack, false);
    let changed_stack_session = arm_and_begin(&mut changed_stack);
    let changed_stack_fallback =
        contained_fallback_at(&changed_stack, moved_floating_rect(), FLOATING_Z_ORDER + 1);
    let changed_stack_outcome = update_drag(
        &mut changed_stack,
        changed_stack_session,
        target_at(SURFACE, point(200.0, 150.0)),
        Some(changed_stack_fallback),
    );
    assert!(matches!(
        changed_stack_outcome,
        InteractionOutcome::PreviewUpdated {
            preview: None,
            status: PreviewResolutionStatus::Rejected,
            ..
        }
    ));
}

#[test]
fn known_none_without_an_explicit_fallback_remains_known_none() {
    let mut fixture = fixture(DockPolicy::default());
    publish_scene(&mut fixture, false);
    let session = arm_and_begin(&mut fixture);

    let outcome = update_drag(
        &mut fixture,
        session,
        target_at(SURFACE, point(200.0, 150.0)),
        None,
    );

    assert!(matches!(
        outcome,
        InteractionOutcome::PreviewUpdated {
            preview: None,
            status: PreviewResolutionStatus::KnownNone,
            ..
        }
    ));
    assert_eq!(
        fixture
            .engine
            .workspace()
            .contained_floating(FLOATING)
            .expect("floating must remain unchanged")
            .rect,
        initial_floating_rect()
    );
}

#[test]
fn rejected_exact_target_never_falls_back_to_contained_movement() {
    let mut policy = DockPolicy::default();
    policy.set_allow_tab_merge(false);
    let mut fixture = fixture(policy);
    publish_scene(&mut fixture, false);
    let session = arm_and_begin(&mut fixture);
    let fallback = contained_fallback(&fixture);

    let outcome = update_drag(
        &mut fixture,
        session,
        target_at(SURFACE, point(40.0, 40.0)),
        Some(fallback),
    );

    assert!(matches!(
        outcome,
        InteractionOutcome::PreviewUpdated {
            preview: None,
            status: PreviewResolutionStatus::Rejected,
            ..
        }
    ));
    assert_eq!(
        fixture
            .engine
            .workspace()
            .contained_floating(FLOATING)
            .expect("rejected exact target must not move the floating")
            .rect,
        initial_floating_rect()
    );
}

#[test]
fn unavailable_and_unknown_target_authority_never_use_the_fallback() {
    let mut unavailable = fixture(DockPolicy::default());
    publish_scene(&mut unavailable, true);
    let unavailable_session = arm_and_begin(&mut unavailable);
    let unavailable_fallback = contained_fallback(&unavailable);
    let unavailable_outcome = update_drag(
        &mut unavailable,
        unavailable_session,
        target_at(BOOTSTRAP_SURFACE, point(40.0, 40.0)),
        Some(unavailable_fallback),
    );
    assert!(matches!(
        unavailable_outcome,
        InteractionOutcome::Cancelled {
            reason: InteractionCancelReason::SceneUnavailable,
            ..
        }
    ));
    assert_eq!(
        unavailable.engine.interaction().status(),
        InteractionStatus::Idle
    );
    assert_eq!(
        unavailable
            .engine
            .workspace()
            .contained_floating(FLOATING)
            .expect("unavailable target must not move the floating")
            .rect,
        initial_floating_rect()
    );

    let mut unknown = fixture(DockPolicy::default());
    publish_scene(&mut unknown, false);
    let unknown_session = arm_and_begin(&mut unknown);
    let unknown_fallback = contained_fallback(&unknown);
    let unknown_outcome = update_drag(
        &mut unknown,
        unknown_session,
        TargetAuthority::local(
            SURFACE,
            Authority::Unknown(AuthorityUnavailableReason::NotReported),
        ),
        Some(unknown_fallback),
    );
    assert!(matches!(
        unknown_outcome,
        InteractionOutcome::Cancelled {
            reason: InteractionCancelReason::UnknownTargetAuthority,
            ..
        }
    ));
    assert_eq!(
        unknown.engine.interaction().status(),
        InteractionStatus::Idle
    );
    assert_eq!(
        unknown
            .engine
            .workspace()
            .contained_floating(FLOATING)
            .expect("unknown target must not move the floating")
            .rect,
        initial_floating_rect()
    );
}

#[test]
fn release_candidate_cannot_replace_the_painted_contained_fallback() {
    let mut fixture = fixture(DockPolicy::default());
    publish_scene(&mut fixture, false);
    let session = arm_and_begin(&mut fixture);
    let fallback = contained_fallback(&fixture);
    let before = fixture.engine.workspace().clone();
    let no_exact_target = target_at(SURFACE, point(200.0, 150.0));
    let preview = update_drag(
        &mut fixture,
        session,
        no_exact_target,
        Some(fallback.clone()),
    );
    assert!(matches!(
        preview,
        InteractionOutcome::PreviewUpdated {
            preview: Some(ref preview),
            ..
        } if matches!(preview.visual(), PreviewVisual::Contained { fallback: false, .. })
    ));

    let outcome = acknowledge_and_release(
        &mut fixture,
        session,
        target_at(SURFACE, point(40.0, 40.0)),
        Some(fallback),
    );

    assert!(matches!(
        outcome,
        InteractionOutcome::Rejected(InteractionRejection::TargetChanged)
    ));
    assert_eq!(fixture.engine.workspace(), &before);
    assert_eq!(
        fixture.engine.interaction().status(),
        InteractionStatus::Idle
    );
}

#[test]
fn release_cannot_change_the_painted_contained_fallback_placement() {
    let mut fixture = fixture(DockPolicy::default());
    publish_scene(&mut fixture, false);
    let session = arm_and_begin(&mut fixture);
    let painted = contained_fallback(&fixture);
    let changed =
        contained_fallback_at(&fixture, rect(120.0, 320.0, 240.0, 180.0), FLOATING_Z_ORDER);
    let no_exact_target = target_at(SURFACE, point(200.0, 150.0));
    let preview = update_drag(
        &mut fixture,
        session,
        no_exact_target.clone(),
        Some(painted),
    );
    assert!(matches!(
        preview,
        InteractionOutcome::PreviewUpdated {
            preview: Some(ref preview),
            ..
        } if matches!(preview.visual(), PreviewVisual::Contained { .. })
    ));
    let before = fixture.engine.workspace().clone();

    let outcome = acknowledge_and_release(&mut fixture, session, no_exact_target, Some(changed));

    assert!(matches!(
        outcome,
        InteractionOutcome::Rejected(InteractionRejection::TargetChanged)
    ));
    assert_eq!(fixture.engine.workspace(), &before);
}

#[test]
fn rejected_exact_candidate_clears_the_old_fallback_proof_before_release() {
    let mut policy = DockPolicy::default();
    policy.set_allow_tab_merge(false);
    let mut fixture = fixture(policy);
    publish_scene(&mut fixture, false);
    let session = arm_and_begin(&mut fixture);
    let fallback = contained_fallback(&fixture);
    let no_exact_target = target_at(SURFACE, point(200.0, 150.0));
    let preview = update_drag(
        &mut fixture,
        session,
        no_exact_target.clone(),
        Some(fallback.clone()),
    );
    assert!(matches!(
        preview,
        InteractionOutcome::PreviewUpdated {
            preview: Some(ref preview),
            ..
        } if matches!(preview.visual(), PreviewVisual::Contained { .. })
    ));
    let stale_acknowledgement = fixture
        .engine
        .interaction()
        .preview()
        .expect("fallback preview must exist")
        .acknowledgement();

    let rejected = update_drag(
        &mut fixture,
        session,
        target_at(SURFACE, point(40.0, 40.0)),
        Some(fallback.clone()),
    );
    assert!(matches!(
        rejected,
        InteractionOutcome::PreviewUpdated {
            preview: None,
            status: PreviewResolutionStatus::Rejected,
            ..
        }
    ));
    let before = fixture.engine.workspace().clone();
    fixture
        .engine
        .enqueue_renderer_intent(RendererIntent::ReleaseDrag {
            session,
            pointer: POINTER,
            button: PointerButton::Primary,
            button_state: Authority::Known(PointerButtonState::Released),
            target: no_exact_target,
            tear_off: Some(fallback),
        })
        .expect("release sequence must be available");
    fixture
        .engine
        .enqueue_renderer_intent(RendererIntent::AcknowledgePreview(stale_acknowledgement))
        .expect("stale acknowledgement sequence must be available");

    let transition = fixture
        .engine
        .reduce_pending()
        .expect("stale acknowledgement and release must reduce");
    assert!(matches!(
        transition.reduced_inputs()[0].outcome(),
        InputOutcome::InteractionProcessed {
            outcome: InteractionOutcome::Rejected(
                InteractionRejection::PreviewAcknowledgementMismatch
            ),
            ..
        }
    ));
    assert!(matches!(
        transition.reduced_inputs()[1].outcome(),
        InputOutcome::InteractionProcessed {
            outcome: InteractionOutcome::Rejected(InteractionRejection::PreviewMissing),
            ..
        }
    ));
    assert_eq!(fixture.engine.workspace(), &before);
}
