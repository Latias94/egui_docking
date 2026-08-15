use super::support;

use dockspace::command::WorkspaceCommand;
use dockspace::engine::{DockEngine, EngineInput};
use dockspace::error::CommandError;
use dockspace::geometry::{LogicalPoint, LogicalRect, LogicalSize};
use dockspace::graph::{ContainedFloating, Node, RootRecord, SurfacePresentation, Workspace};
use dockspace::ids::{
    FloatingPresentationId, ItemId, NodeId, RootId, StableInputSourceId, SurfaceId,
};
use dockspace::intent::ContainedPlacementUnavailable;
use dockspace::interaction::{InteractionOutcome, InteractionRejection};
use dockspace::model::{DockspaceActionOutcome, DockspaceActionRejection};
use dockspace::policy::DockPolicy;
use dockspace::presentation_config::DockPresentationConfig;
use dockspace::scene::SurfaceScene;
use dockspace::surface_recovery::{
    ConvertedMainRecovery, RootRecoveryAnchor, SurfaceRecoveryTarget,
};
use dockspace::transition::{InputOutcome, SurfaceContributionOutcome};
use dockspace::viewport::{ViewportRole, WindowToken};
use support::{
    MeasurementProfile, TestPresentationHost, install_surface_projection, measurements,
    publish_surface, publish_surfaces, submit_input,
};

const SURFACE_A: SurfaceId = SurfaceId::new(1);
const SURFACE_B: SurfaceId = SurfaceId::new(2);
const SURFACE_MISSING: SurfaceId = SurfaceId::new(3);
const ROOT_A: RootId = RootId::new(1);
const ROOT_B: RootId = RootId::new(2);
const ROOT_FLOATING: RootId = RootId::new(3);
const FLOATING: FloatingPresentationId = FloatingPresentationId::new(3);
const FLOATING_NEW: FloatingPresentationId = FloatingPresentationId::new(4);
const CONTAINED_INPUT_SOURCE: StableInputSourceId = StableInputSourceId::new(0xC07A);

const INITIAL_X: f64 = 120.0;
const INITIAL_Y: f64 = 70.0;

struct Fixture {
    engine: DockEngine,
    host: TestPresentationHost,
    tabs_a: NodeId,
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

fn compact_presentation_config() -> DockPresentationConfig {
    DockPresentationConfig::builder()
        .tab_bar_height(1.0)
        .tab_group_grip_extent(1.0)
        .tab_min_width(3.0)
        .tab_close_extent(1.0)
        .guide_extent(1.0)
        .guide_gap(0.0)
        .guide_hit_padding(0.0)
        .guide_outer_inset(1.0)
        .floating_title_height(1.0)
        .floating_border_width(0.0)
        .minimum_pane_size(3.0, 3.0)
        .minimum_floating_size(1.0, 1.0)
        .build()
        .expect("compact presentation geometry must be valid")
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
    builder.set_surface(SURFACE_A, SurfacePresentation::with_main(ROOT_A));
    builder.set_surface(SURFACE_B, SurfacePresentation::with_main(ROOT_B));
    builder.set_contained_floating(
        FLOATING,
        ContainedFloating::new(ROOT_FLOATING, contained_rect),
    );
    builder
        .attach_contained(SURFACE_A, FLOATING)
        .expect("surface A must exist");
    let workspace = builder.build().expect("test workspace must be valid");
    let mut engine = DockEngine::new_with_presentation_config(
        workspace,
        DockPolicy::default(),
        compact_presentation_config(),
    )
    .expect("test engine must be valid");
    let host = TestPresentationHost::new(&mut engine);
    Fixture {
        engine,
        host,
        tabs_a,
    }
}

fn publish_ready_scene(engine: &mut DockEngine, host: &mut TestPresentationHost) {
    publish_surfaces(
        engine,
        host,
        [
            (SURFACE_A, rect(100.0, 50.0, 300.0, 200.0)),
            (SURFACE_B, rect(-200.0, -100.0, 200.0, 150.0)),
        ],
    );
}

fn contained_placement_outcome(
    engine: &mut DockEngine,
    host: &mut TestPresentationHost,
    root: RootId,
    floating: FloatingPresentationId,
    expected_rect: LogicalRect,
    placement: dockspace::intent::ContainedPlacementProof,
) -> InteractionOutcome {
    let expected = engine.version();
    let transition = submit_input(
        engine,
        host,
        CONTAINED_INPUT_SOURCE,
        EngineInput::ApplyContainedPlacement {
            expected,
            root,
            floating,
            expected_rect,
            placement,
        },
    )
    .expect("contained placement input must reduce");
    match transition.reduced_inputs()[0].outcome() {
        InputOutcome::InteractionProcessed { outcome, .. } => outcome.clone(),
        outcome => panic!("unexpected input outcome: {outcome:?}"),
    }
}

fn register_root_viewport(
    engine: &mut DockEngine,
    host: &mut TestPresentationHost,
    surface: SurfaceId,
    token: WindowToken,
) -> RootRecoveryAnchor {
    let expected = engine.version();
    let provider = host.platform_provider();
    let transition = submit_input(
        engine,
        host,
        CONTAINED_INPUT_SOURCE,
        EngineInput::RegisterViewport {
            provider,
            expected,
            surface,
            token,
            role: ViewportRole::Root,
            recovery_target: None,
        },
    )
    .expect("root viewport registration must reduce");
    assert!(matches!(
        transition.reduced_inputs()[0].outcome(),
        InputOutcome::ViewportRegistered { binding }
            if binding.surface() == surface && binding.token() == token
    ));
    engine
        .root_recovery_anchor(surface)
        .expect("registered root viewport must own a recovery anchor")
}

fn foreign_root_recovery_anchor(surface: SurfaceId) -> RootRecoveryAnchor {
    let mut foreign = fixture();
    register_root_viewport(
        &mut foreign.engine,
        &mut foreign.host,
        surface,
        WindowToken::new(900),
    )
}

#[test]
fn clamp_is_exact_at_each_edge_and_for_oversize_rectangles() {
    let mut fixture = fixture();
    publish_surface(
        &mut fixture.engine,
        &mut fixture.host,
        SURFACE_A,
        rect(100.0, 50.0, 300.0, 200.0),
    );

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
        Err(ContainedPlacementUnavailable::BootstrapSurface { surface: SURFACE_A })
    );

    install_surface_projection(
        &mut fixture.engine,
        &mut fixture.host,
        SURFACE_A,
        rect(100.0, 50.0, 300.0, 200.0),
    );
    assert_eq!(
        fixture
            .engine
            .contained_placement(SURFACE_A, initial_rect(), size(0.0, 0.0)),
        Err(ContainedPlacementUnavailable::PendingPaintSurface { surface: SURFACE_A })
    );

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

    publish_ready_scene(&mut fixture.engine, &mut fixture.host);
    assert!(
        fixture
            .engine
            .contained_placement(SURFACE_A, initial_rect(), size(0.0, 0.0))
            .is_ok(),
        "only geometry confirmed painted before reduction may authorize placement"
    );
}

#[test]
fn finite_corners_with_an_infinite_span_are_rejected() {
    let mut fixture = fixture();
    publish_ready_scene(&mut fixture.engine, &mut fixture.host);
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
    publish_ready_scene(&mut fixture.engine, &mut fixture.host);
    let placement = fixture
        .engine
        .contained_placement(SURFACE_A, rect(390.0, 240.0, 80.0, 60.0), size(0.0, 0.0))
        .expect("placement must be authorized");

    let outcome = contained_placement_outcome(
        &mut fixture.engine,
        &mut fixture.host,
        ROOT_FLOATING,
        FLOATING,
        initial_rect(),
        placement,
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
fn bring_into_view_requires_ready_geometry_and_preserves_stacking() {
    let persisted = rect(520.0, 400.0, 120.0, 90.0);
    let expected = rect(280.0, 160.0, 120.0, 90.0);
    let mut fixture = fixture_at(persisted);
    let item = ItemId::new(4);
    let before = fixture.engine.workspace().clone();
    let before_version = fixture.engine.version();

    let prepared = fixture.engine.prepare_bring_contained_into_view(item);
    let input = fixture
        .engine
        .accept_prepared_action(prepared)
        .expect("the prepared action belongs to this engine");
    let transition = submit_input(
        &mut fixture.engine,
        &mut fixture.host,
        CONTAINED_INPUT_SOURCE,
        input,
    )
    .expect("missing ready geometry is a typed product rejection");
    assert!(matches!(
        transition.reduced_inputs()[0].outcome(),
        InputOutcome::ProductActionRejected {
            reason: DockspaceActionRejection::PresentationUnavailable { surface: SURFACE_A },
            version,
        } if *version == before_version
    ));
    assert_eq!(fixture.engine.workspace(), &before);
    assert_eq!(fixture.engine.version(), before_version);
    assert!(transition.events().is_empty());

    publish_ready_scene(&mut fixture.engine, &mut fixture.host);
    let roster = fixture
        .engine
        .workspace()
        .surface(SURFACE_A)
        .expect("surface A remains available")
        .contained
        .clone();
    let prepared = fixture.engine.prepare_bring_contained_into_view(item);
    let input = fixture
        .engine
        .accept_prepared_action(prepared)
        .expect("the prepared action belongs to this engine");
    let transition = submit_input(
        &mut fixture.engine,
        &mut fixture.host,
        CONTAINED_INPUT_SOURCE,
        input,
    )
    .expect("bring-into-view commits against current presentation facts");
    assert!(matches!(
        transition.reduced_inputs()[0].outcome(),
        InputOutcome::ProductActionProcessed {
            outcome: DockspaceActionOutcome::ContainedBoundsUpdated {
                root: ROOT_FLOATING,
                surface: SURFACE_A,
                floating: FLOATING,
                changed: true,
            },
            ..
        }
    ));
    assert_eq!(
        fixture
            .engine
            .workspace()
            .contained_floating(FLOATING)
            .expect("the floating presentation remains available")
            .rect,
        expected,
    );
    assert_eq!(
        fixture
            .engine
            .workspace()
            .surface(SURFACE_A)
            .expect("surface A remains available")
            .contained,
        roster,
        "bring-into-view must not raise or reorder contained presentations",
    );
    assert_eq!(transition.events().len(), 1);

    publish_ready_scene(&mut fixture.engine, &mut fixture.host);
    let before_noop = fixture.engine.version();
    let prepared = fixture.engine.prepare_bring_contained_into_view(item);
    let input = fixture
        .engine
        .accept_prepared_action(prepared)
        .expect("the prepared action belongs to this engine");
    let transition = submit_input(
        &mut fixture.engine,
        &mut fixture.host,
        CONTAINED_INPUT_SOURCE,
        input,
    )
    .expect("an already-visible presentation produces a checked no-op");
    assert!(matches!(
        transition.reduced_inputs()[0].outcome(),
        InputOutcome::ProductActionProcessed {
            outcome: DockspaceActionOutcome::ContainedBoundsUpdated { changed: false, .. },
            version,
        } if *version == before_noop
    ));
    assert_eq!(fixture.engine.version(), before_noop);
    assert!(transition.events().is_empty());
}

#[test]
fn surface_contribution_clips_projection_without_rewriting_durable_geometry() {
    let persisted = rect(20.0, 10.0, 500.0, 400.0);
    let mut fixture = fixture_at(persisted);
    let before_version = fixture.engine.version();
    let before_workspace = fixture.engine.workspace().clone();
    let before_interaction = fixture.engine.interaction().status();
    let token = fixture
        .engine
        .begin_surface_contribution(SURFACE_A)
        .expect("surface A contribution should begin");
    let measurements = measurements(
        &fixture.engine,
        SURFACE_A,
        rect(100.0, 50.0, 300.0, 200.0),
        MeasurementProfile::default(),
    );
    let contribution = fixture
        .engine
        .prepare_surface_contribution(token, measurements)
        .expect("complete surface measurements prepare successfully");
    let mut frame = fixture.host.begin(&fixture.engine);
    frame
        .push_surface_contribution(contribution)
        .expect("one tick carries one surface contribution");
    support::complete_host_frame_with_retained_or_unavailable(&fixture.engine, &mut frame);
    let transition = fixture.host.finish(frame, &mut fixture.engine);
    let stamp = transition
        .surface_contributions()
        .iter()
        .find_map(|outcome| match outcome {
            SurfaceContributionOutcome::Ready { surface, stamp, .. } if *surface == SURFACE_A => {
                Some(*stamp)
            }
            _ => None,
        })
        .unwrap_or_else(|| {
            panic!(
                "expected a ready contribution for {SURFACE_A}, got {:?}",
                transition.surface_contributions()
            )
        });
    let ready = fixture
        .engine
        .scene()
        .surface(SURFACE_A)
        .and_then(SurfaceScene::ready)
        .expect("the complete contribution must install a ready projection");
    assert_eq!(ready.stamp(), stamp);
    assert!(
        ready.paint_fallback().is_none(),
        "measurement alone must not manufacture painted interaction authority"
    );
    let contained = ready
        .plan()
        .contained_records()
        .iter()
        .find(|record| record.floating() == FLOATING)
        .expect("the visible portion of the contained root must be projected");
    assert_eq!(
        contained.outer_bounds(),
        rect(100.0, 50.0, 300.0, 200.0),
        "projection geometry is the exact surface-clipped durable rectangle"
    );
    assert_eq!(fixture.engine.workspace(), &before_workspace);
    assert_eq!(fixture.engine.version(), before_version);
    assert!(fixture.engine.scene().ready_surface(SURFACE_A).is_none());
    assert_eq!(fixture.engine.interaction().status(), before_interaction);
    assert_eq!(
        fixture
            .engine
            .workspace()
            .contained_floating(FLOATING)
            .expect("floating must remain present")
            .rect,
        persisted,
        "surface measurements must never become an implicit workspace command"
    );
}

#[test]
fn scene_rollover_rejects_old_rect_proof_and_reauthorizes_the_same_request() {
    let mut fixture = fixture();
    publish_ready_scene(&mut fixture.engine, &mut fixture.host);
    let requested = rect(200.0, 100.0, 100.0, 80.0);
    let minimum = size(0.0, 0.0);
    let old_placement = fixture
        .engine
        .contained_placement(SURFACE_A, requested, minimum)
        .expect("placement must be authorized");
    publish_ready_scene(&mut fixture.engine, &mut fixture.host);
    let before = fixture.engine.workspace().clone();
    let version = fixture.engine.version();

    let rect_outcome = contained_placement_outcome(
        &mut fixture.engine,
        &mut fixture.host,
        ROOT_FLOATING,
        FLOATING,
        initial_rect(),
        old_placement,
    );
    assert!(matches!(
        rect_outcome,
        InteractionOutcome::Rejected(InteractionRejection::StaleScene)
    ));

    let fresh_placement = fixture
        .engine
        .contained_placement(SURFACE_A, requested, minimum)
        .expect("the same request must be reauthorized against the current scene");
    assert_eq!(
        fresh_placement.requested_rect(),
        old_placement.requested_rect()
    );
    assert_eq!(fresh_placement.minimum_size(), old_placement.minimum_size());
    assert_eq!(fresh_placement.clamped_rect(), old_placement.clamped_rect());
    assert_ne!(fresh_placement.scene(), old_placement.scene());
    assert_eq!(fixture.engine.workspace(), &before);
    assert_eq!(fixture.engine.version(), version);
}

#[test]
fn viewport_registration_retains_recovery_intent_without_the_old_scene_proof() {
    let mut fixture = fixture();
    let anchor = register_root_viewport(
        &mut fixture.engine,
        &mut fixture.host,
        SURFACE_A,
        WindowToken::new(21),
    );
    let recovery_target = SurfaceRecoveryTarget::with_converted_main(
        anchor,
        ConvertedMainRecovery::new(ROOT_B, FLOATING_NEW, size(0.0, 0.0)),
    );
    let before = fixture.engine.workspace().clone();
    let version = fixture.engine.version();
    let provider = fixture.host.platform_provider();

    let transition = submit_input(
        &mut fixture.engine,
        &mut fixture.host,
        CONTAINED_INPUT_SOURCE,
        EngineInput::RegisterViewport {
            provider,
            expected: version,
            surface: SURFACE_B,
            token: WindowToken::new(22),
            role: ViewportRole::Child,
            recovery_target: Some(recovery_target),
        },
    )
    .expect("registration must reduce");

    assert!(matches!(
        transition.reduced_inputs()[0].outcome(),
        InputOutcome::ViewportRegistered { binding }
            if binding.surface() == SURFACE_B && binding.token() == WindowToken::new(22)
    ));
    assert!(fixture.engine.viewport().viewport(SURFACE_B).is_some());
    assert_eq!(fixture.engine.workspace(), &before);
    assert_eq!(fixture.engine.version(), version);
}

#[test]
fn viewport_registration_rejects_role_shape_host_and_floating_mismatches() {
    #[derive(Clone, Copy)]
    enum InvalidRecoveryContract {
        RootWithTarget,
        ChildWithoutTarget,
        RootedChildWithoutMainConversion,
        SelfHost,
        ReservedFloating,
        WrongMainRoot,
    }

    let cases = [
        InvalidRecoveryContract::RootWithTarget,
        InvalidRecoveryContract::ChildWithoutTarget,
        InvalidRecoveryContract::RootedChildWithoutMainConversion,
        InvalidRecoveryContract::SelfHost,
        InvalidRecoveryContract::ReservedFloating,
        InvalidRecoveryContract::WrongMainRoot,
    ];

    for (index, case) in cases.into_iter().enumerate() {
        let mut fixture = fixture();
        let anchor = register_root_viewport(
            &mut fixture.engine,
            &mut fixture.host,
            SURFACE_A,
            WindowToken::new(10 + index as u64),
        );
        let (role, recovery_target) = match case {
            InvalidRecoveryContract::RootWithTarget => (
                ViewportRole::Root,
                Some(SurfaceRecoveryTarget::with_converted_main(
                    anchor,
                    ConvertedMainRecovery::new(ROOT_B, FLOATING_NEW, size(0.0, 0.0)),
                )),
            ),
            InvalidRecoveryContract::ChildWithoutTarget => (ViewportRole::Child, None),
            InvalidRecoveryContract::RootedChildWithoutMainConversion => (
                ViewportRole::Child,
                Some(SurfaceRecoveryTarget::forest_only(anchor)),
            ),
            InvalidRecoveryContract::SelfHost => (
                ViewportRole::Child,
                Some(SurfaceRecoveryTarget::with_converted_main(
                    foreign_root_recovery_anchor(SURFACE_B),
                    ConvertedMainRecovery::new(ROOT_B, FLOATING_NEW, size(0.0, 0.0)),
                )),
            ),
            InvalidRecoveryContract::ReservedFloating => (
                ViewportRole::Child,
                Some(SurfaceRecoveryTarget::with_converted_main(
                    anchor,
                    ConvertedMainRecovery::new(ROOT_B, FLOATING, size(0.0, 0.0)),
                )),
            ),
            InvalidRecoveryContract::WrongMainRoot => (
                ViewportRole::Child,
                Some(SurfaceRecoveryTarget::with_converted_main(
                    anchor,
                    ConvertedMainRecovery::new(ROOT_A, FLOATING_NEW, size(0.0, 0.0)),
                )),
            ),
        };
        let version = fixture.engine.version();
        let provider = fixture.host.platform_provider();
        let transition = submit_input(
            &mut fixture.engine,
            &mut fixture.host,
            CONTAINED_INPUT_SOURCE,
            EngineInput::RegisterViewport {
                provider,
                expected: version,
                surface: SURFACE_B,
                token: WindowToken::new(100 + index as u64),
                role,
                recovery_target,
            },
        )
        .expect("invalid registration must reduce without mutating coordinator state");

        assert!(matches!(
            transition.reduced_inputs()[0].outcome(),
            InputOutcome::ViewportRegistrationRejected { surface: SURFACE_B }
        ));
        assert!(fixture.engine.viewport().viewport(SURFACE_B).is_none());
        assert_eq!(fixture.engine.version(), version);
    }
}

#[test]
fn workspace_change_and_surface_mismatch_leave_contained_geometry_unchanged() {
    let mut fixture = fixture();
    publish_ready_scene(&mut fixture.engine, &mut fixture.host);
    let stale_after_mutation = fixture
        .engine
        .contained_placement(SURFACE_A, rect(200.0, 100.0, 100.0, 80.0), size(0.0, 0.0))
        .expect("placement must be authorized");
    let select = fixture
        .engine
        .workspace()
        .capture_item_source(ROOT_A, fixture.tabs_a, ItemId::new(2))
        .expect("selection source must be current");
    let expected = fixture.engine.version();
    submit_input(
        &mut fixture.engine,
        &mut fixture.host,
        CONTAINED_INPUT_SOURCE,
        EngineInput::WorkspaceCommand {
            expected,
            command: WorkspaceCommand::Select { source: select },
        },
    )
    .expect("selection must reduce");
    let after_selection = fixture.engine.workspace().clone();

    let stale_outcome = contained_placement_outcome(
        &mut fixture.engine,
        &mut fixture.host,
        ROOT_FLOATING,
        FLOATING,
        initial_rect(),
        stale_after_mutation,
    );
    assert!(matches!(
        stale_outcome,
        InteractionOutcome::Rejected(InteractionRejection::StaleScene)
    ));
    assert_eq!(fixture.engine.workspace(), &after_selection);

    publish_ready_scene(&mut fixture.engine, &mut fixture.host);
    let wrong_surface = fixture
        .engine
        .contained_placement(SURFACE_B, rect(-150.0, -80.0, 100.0, 80.0), size(0.0, 0.0))
        .expect("surface B placement must be authorized");
    let version = fixture.engine.version();
    let before_mismatch = fixture.engine.workspace().clone();
    let mismatch_outcome = contained_placement_outcome(
        &mut fixture.engine,
        &mut fixture.host,
        ROOT_FLOATING,
        FLOATING,
        initial_rect(),
        wrong_surface,
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
    publish_ready_scene(&mut fixture.engine, &mut fixture.host);
    let placement = fixture
        .engine
        .contained_placement(SURFACE_A, rect(200.0, 100.0, 100.0, 80.0), size(0.0, 0.0))
        .expect("placement must be authorized");
    let before = fixture.engine.workspace().clone();
    let version = fixture.engine.version();

    let outcome = contained_placement_outcome(
        &mut fixture.engine,
        &mut fixture.host,
        ROOT_FLOATING,
        FLOATING,
        rect(0.0, 0.0, 1.0, 1.0),
        placement,
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
