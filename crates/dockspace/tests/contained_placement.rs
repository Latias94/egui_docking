use dockspace::command::{MovePayload, WorkspaceCommand};
use dockspace::engine::DockEngine;
use dockspace::error::CommandError;
use dockspace::geometry::{LogicalPoint, LogicalRect, LogicalSize};
use dockspace::graph::{ContainedFloating, Node, RootRecord, SurfacePresentation, Workspace};
use dockspace::ids::{FloatingPresentationId, ItemId, NodeId, RootId, SurfaceId};
use dockspace::intent::{
    Authority, ContainedPlacementUnavailable, ContainedTearOffProposal, PointerButton, PointerId,
    RendererIntent, TargetAuthority, TearOffRequest,
};
use dockspace::interaction::{
    DragSessionId, InteractionOutcome, InteractionRejection, PreviewResolutionStatus,
};
use dockspace::policy::DockPolicy;
use dockspace::scene::{BuildingScene, ReadySurfaceScene};
use dockspace::transition::InputOutcome;
use dockspace::viewport::{ViewportRole, WindowToken};

const SURFACE_A: SurfaceId = SurfaceId::new(1);
const SURFACE_B: SurfaceId = SurfaceId::new(2);
const SURFACE_MISSING: SurfaceId = SurfaceId::new(3);
const ROOT_A: RootId = RootId::new(1);
const ROOT_B: RootId = RootId::new(2);
const ROOT_FLOATING: RootId = RootId::new(3);
const ROOT_NEW: RootId = RootId::new(4);
const FLOATING: FloatingPresentationId = FloatingPresentationId::new(3);
const FLOATING_NEW: FloatingPresentationId = FloatingPresentationId::new(4);
const POINTER: PointerId = PointerId::new(1);

const INITIAL_X: f64 = 120.0;
const INITIAL_Y: f64 = 70.0;

struct Fixture {
    engine: DockEngine,
    tabs_a: NodeId,
    tabs_b: NodeId,
}

fn rect(x: f64, y: f64, width: f64, height: f64) -> LogicalRect {
    LogicalRect::new(x, y, width, height).expect("test rectangle must be valid")
}

fn size(width: f64, height: f64) -> LogicalSize {
    LogicalSize::new(width, height).expect("test size must be valid")
}

fn initial_rect() -> LogicalRect {
    rect(INITIAL_X, INITIAL_Y, 120.0, 90.0)
}

fn fixture() -> Fixture {
    fixture_at(initial_rect())
}

fn fixture_at(contained_rect: LogicalRect) -> Fixture {
    let mut builder = Workspace::builder();
    let tabs_a = builder.insert_node(Node::tabs([ItemId::new(1), ItemId::new(2)]));
    let tabs_b = builder.insert_node(Node::tabs([ItemId::new(3)]));
    let floating_tabs = builder.insert_node(Node::tabs([ItemId::new(4)]));
    builder.set_root(ROOT_A, RootRecord::new(tabs_a));
    builder.set_root(ROOT_B, RootRecord::new(tabs_b));
    builder.set_root(ROOT_FLOATING, RootRecord::new(floating_tabs));
    builder.set_surface(SURFACE_A, SurfacePresentation::new(ROOT_A));
    builder.set_surface(SURFACE_B, SurfacePresentation::new(ROOT_B));
    builder.set_contained_floating(ContainedFloating::new(
        FLOATING,
        ROOT_FLOATING,
        SURFACE_A,
        contained_rect,
        1,
    ));
    builder
        .attach_contained(SURFACE_A, FLOATING)
        .expect("surface A must exist");
    let workspace = builder.build().expect("test workspace must be valid");
    let engine =
        DockEngine::new(workspace, DockPolicy::default()).expect("test engine must be valid");
    Fixture {
        engine,
        tabs_a,
        tabs_b,
    }
}

fn ready_scene() -> BuildingScene {
    let mut scene = BuildingScene::new([SURFACE_A, SURFACE_B]).expect("test roster must be unique");
    scene
        .insert_ready(ReadySurfaceScene::new(
            SURFACE_A,
            rect(100.0, 50.0, 300.0, 200.0),
        ))
        .expect("surface A facts must be unique");
    scene
        .insert_ready(ReadySurfaceScene::new(
            SURFACE_B,
            rect(-200.0, -100.0, 200.0, 150.0),
        ))
        .expect("surface B facts must be unique");
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

fn arm_and_begin(fixture: &mut Fixture) -> DragSessionId {
    let payload = MovePayload::Item(
        fixture
            .engine
            .workspace()
            .capture_item_source(ROOT_B, fixture.tabs_b, ItemId::new(3))
            .expect("source item must be current"),
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
fn clamp_is_exact_at_each_edge_and_for_oversize_rectangles() {
    let mut fixture = fixture();
    publish_ready_scene(&mut fixture.engine);

    let top_left = fixture
        .engine
        .contained_placement(SURFACE_A, rect(-50.0, -80.0, 100.0, 80.0), size(0.0, 0.0))
        .expect("top-left placement must be authorized");
    assert_eq!(top_left.clamped_rect(), rect(100.0, 50.0, 100.0, 80.0));

    let bottom_right = fixture
        .engine
        .contained_placement(SURFACE_A, rect(500.0, 400.0, 100.0, 80.0), size(0.0, 0.0))
        .expect("bottom-right placement must be authorized");
    assert_eq!(bottom_right.clamped_rect(), rect(300.0, 170.0, 100.0, 80.0));

    let minimum_expanded = fixture
        .engine
        .contained_placement(SURFACE_A, rect(390.0, 240.0, 20.0, 10.0), size(80.0, 60.0))
        .expect("minimum-expanded placement must be authorized");
    assert_eq!(
        minimum_expanded.clamped_rect(),
        rect(320.0, 190.0, 80.0, 60.0)
    );

    let oversize = fixture
        .engine
        .contained_placement(
            SURFACE_A,
            rect(-1_000.0, 900.0, 500.0, 400.0),
            size(600.0, 500.0),
        )
        .expect("oversize placement must be authorized");
    assert_eq!(oversize.clamped_rect(), rect(100.0, 50.0, 300.0, 200.0));
}

#[test]
fn placement_requires_a_current_ready_surface() {
    let mut fixture = fixture();
    assert_eq!(
        fixture
            .engine
            .contained_placement(SURFACE_A, initial_rect(), size(0.0, 0.0)),
        Err(ContainedPlacementUnavailable::SceneUnavailable)
    );

    let mut bootstrap =
        BuildingScene::new([SURFACE_A, SURFACE_B]).expect("test roster must be unique");
    bootstrap
        .insert_ready(ReadySurfaceScene::new(
            SURFACE_A,
            rect(100.0, 50.0, 300.0, 200.0),
        ))
        .expect("surface A facts must be unique");
    fixture
        .engine
        .enqueue_scene(bootstrap)
        .expect("bootstrap scene must enqueue");
    fixture
        .engine
        .reduce_pending()
        .expect("bootstrap scene must publish");

    assert_eq!(
        fixture
            .engine
            .contained_placement(SURFACE_B, initial_rect(), size(0.0, 0.0)),
        Err(ContainedPlacementUnavailable::BootstrapSurface { surface: SURFACE_B })
    );
    assert_eq!(
        fixture
            .engine
            .contained_placement(SURFACE_MISSING, initial_rect(), size(0.0, 0.0)),
        Err(ContainedPlacementUnavailable::MissingSurface {
            surface: SURFACE_MISSING,
        })
    );
}

#[test]
fn finite_corners_with_an_infinite_span_are_rejected() {
    let mut fixture = fixture();
    publish_ready_scene(&mut fixture.engine);
    let requested = LogicalRect::from_min_max(
        LogicalPoint::new(-f64::MAX, 0.0).expect("minimum point must be finite"),
        LogicalPoint::new(f64::MAX, 10.0).expect("maximum point must be finite"),
    )
    .expect("ordered finite corners must construct a rectangle");

    assert_eq!(
        fixture
            .engine
            .contained_placement(SURFACE_A, requested, size(0.0, 0.0)),
        Err(ContainedPlacementUnavailable::UnrepresentableGeometry { surface: SURFACE_A })
    );
}

#[test]
fn current_proof_commits_the_exact_clamped_rect() {
    let mut fixture = fixture();
    publish_ready_scene(&mut fixture.engine);
    let placement = fixture
        .engine
        .contained_placement(SURFACE_A, rect(390.0, 240.0, 80.0, 60.0), size(0.0, 0.0))
        .expect("placement must be authorized");

    let outcome = interaction_outcome(
        &mut fixture.engine,
        RendererIntent::ApplyContainedPlacement {
            root: ROOT_FLOATING,
            floating: FLOATING,
            expected_rect: initial_rect(),
            placement,
        },
    );

    assert!(matches!(
        outcome,
        InteractionOutcome::ContainedPlacementApplied { changed: true, .. }
    ));
    assert_eq!(
        fixture
            .engine
            .workspace()
            .contained_floating(FLOATING)
            .expect("floating must remain present")
            .rect,
        rect(320.0, 190.0, 80.0, 60.0)
    );
}

#[test]
fn one_shot_placement_reconciles_persisted_geometry_to_new_scene_bounds() {
    let persisted = rect(20.0, 10.0, 500.0, 400.0);
    let mut fixture = fixture_at(persisted);
    publish_ready_scene(&mut fixture.engine);
    let placement = fixture
        .engine
        .contained_placement(SURFACE_A, persisted, size(80.0, 60.0))
        .expect("new scene bounds must authorize reconciliation");

    let outcome = interaction_outcome(
        &mut fixture.engine,
        RendererIntent::ApplyContainedPlacement {
            root: ROOT_FLOATING,
            floating: FLOATING,
            expected_rect: persisted,
            placement,
        },
    );

    assert!(matches!(
        outcome,
        InteractionOutcome::ContainedPlacementApplied { changed: true, .. }
    ));
    assert_eq!(
        fixture
            .engine
            .workspace()
            .contained_floating(FLOATING)
            .expect("floating must remain present")
            .rect,
        rect(100.0, 50.0, 300.0, 200.0)
    );
}

#[test]
fn scene_rollover_rejects_old_rect_and_tear_off_proofs_atomically() {
    let mut fixture = fixture();
    publish_ready_scene(&mut fixture.engine);
    let old_placement = fixture
        .engine
        .contained_placement(SURFACE_A, rect(200.0, 100.0, 100.0, 80.0), size(0.0, 0.0))
        .expect("placement must be authorized");
    let old_proposal = ContainedTearOffProposal::new(ROOT_NEW, FLOATING_NEW, old_placement, 2);
    let session = arm_and_begin(&mut fixture);
    publish_ready_scene(&mut fixture.engine);
    let before = fixture.engine.workspace().clone();
    let version = fixture.engine.version();

    let rect_outcome = interaction_outcome(
        &mut fixture.engine,
        RendererIntent::ApplyContainedPlacement {
            root: ROOT_FLOATING,
            floating: FLOATING,
            expected_rect: initial_rect(),
            placement: old_placement,
        },
    );
    assert!(matches!(
        rect_outcome,
        InteractionOutcome::Rejected(InteractionRejection::StaleScene)
    ));

    let tear_off_outcome = interaction_outcome(
        &mut fixture.engine,
        RendererIntent::UpdateDrag {
            session,
            target: TargetAuthority::local(SURFACE_B, Authority::Known(None)),
            tear_off: Some(TearOffRequest::Contained(old_proposal)),
        },
    );
    assert!(matches!(
        tear_off_outcome,
        InteractionOutcome::PreviewUpdated {
            preview: None,
            status: PreviewResolutionStatus::Rejected,
            ..
        }
    ));
    assert_eq!(fixture.engine.workspace(), &before);
    assert_eq!(fixture.engine.version(), version);
}

#[test]
fn viewport_registration_rejects_a_recovery_from_an_old_scene() {
    let mut fixture = fixture();
    publish_ready_scene(&mut fixture.engine);
    let placement = fixture
        .engine
        .contained_placement(SURFACE_A, rect(150.0, 90.0, 120.0, 90.0), size(0.0, 0.0))
        .expect("recovery placement must be authorized");
    let recovery = ContainedTearOffProposal::new(ROOT_B, FLOATING_NEW, placement, 2);
    publish_ready_scene(&mut fixture.engine);
    let before = fixture.engine.workspace().clone();
    let version = fixture.engine.version();

    fixture
        .engine
        .enqueue_viewport_registration(
            SURFACE_B,
            WindowToken::new(22),
            ViewportRole::Child,
            Some(recovery),
        )
        .expect("registration must enqueue");
    let transition = fixture
        .engine
        .reduce_pending()
        .expect("registration rejection must reduce");

    assert!(matches!(
        transition.reduced_inputs()[0].outcome(),
        InputOutcome::ViewportRegistrationRejected { surface } if *surface == SURFACE_B
    ));
    assert!(fixture.engine.viewport().viewport(SURFACE_B).is_none());
    assert_eq!(fixture.engine.workspace(), &before);
    assert_eq!(fixture.engine.version(), version);
}

#[test]
fn workspace_change_and_surface_mismatch_leave_contained_geometry_unchanged() {
    let mut fixture = fixture();
    publish_ready_scene(&mut fixture.engine);
    let stale_after_mutation = fixture
        .engine
        .contained_placement(SURFACE_A, rect(200.0, 100.0, 100.0, 80.0), size(0.0, 0.0))
        .expect("placement must be authorized");
    let select = fixture
        .engine
        .workspace()
        .capture_item_source(ROOT_A, fixture.tabs_a, ItemId::new(2))
        .expect("selection source must be current");
    fixture
        .engine
        .enqueue_command(WorkspaceCommand::Select { source: select })
        .expect("selection must enqueue");
    fixture
        .engine
        .reduce_pending()
        .expect("selection must reduce");
    let after_selection = fixture.engine.workspace().clone();

    let stale_outcome = interaction_outcome(
        &mut fixture.engine,
        RendererIntent::ApplyContainedPlacement {
            root: ROOT_FLOATING,
            floating: FLOATING,
            expected_rect: initial_rect(),
            placement: stale_after_mutation,
        },
    );
    assert!(matches!(
        stale_outcome,
        InteractionOutcome::Rejected(InteractionRejection::StaleScene)
    ));
    assert_eq!(fixture.engine.workspace(), &after_selection);

    publish_ready_scene(&mut fixture.engine);
    let wrong_surface = fixture
        .engine
        .contained_placement(SURFACE_B, rect(-150.0, -80.0, 100.0, 80.0), size(0.0, 0.0))
        .expect("surface B placement must be authorized");
    let version = fixture.engine.version();
    let before_mismatch = fixture.engine.workspace().clone();
    let mismatch_outcome = interaction_outcome(
        &mut fixture.engine,
        RendererIntent::ApplyContainedPlacement {
            root: ROOT_FLOATING,
            floating: FLOATING,
            expected_rect: initial_rect(),
            placement: wrong_surface,
        },
    );
    assert!(matches!(
        mismatch_outcome,
        InteractionOutcome::Rejected(InteractionRejection::CommandRejected(
            CommandError::FloatingPresentationMismatch { .. }
        ))
    ));
    assert_eq!(fixture.engine.workspace(), &before_mismatch);
    assert_eq!(fixture.engine.version(), version);
}

#[test]
fn stale_expected_rect_rejects_the_whole_update() {
    let mut fixture = fixture();
    publish_ready_scene(&mut fixture.engine);
    let placement = fixture
        .engine
        .contained_placement(SURFACE_A, rect(200.0, 100.0, 100.0, 80.0), size(0.0, 0.0))
        .expect("placement must be authorized");
    let before = fixture.engine.workspace().clone();
    let version = fixture.engine.version();

    let outcome = interaction_outcome(
        &mut fixture.engine,
        RendererIntent::ApplyContainedPlacement {
            root: ROOT_FLOATING,
            floating: FLOATING,
            expected_rect: rect(0.0, 0.0, 1.0, 1.0),
            placement,
        },
    );

    assert!(matches!(
        outcome,
        InteractionOutcome::Rejected(InteractionRejection::CommandRejected(
            CommandError::StaleContainedRect { .. }
        ))
    ));
    assert_eq!(fixture.engine.workspace(), &before);
    assert_eq!(fixture.engine.version(), version);
}
