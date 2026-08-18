//! Native lifecycle, recovery, contained geometry, and vacancy tests.

use super::*;

use crate::model::{DockPlacement, DockspaceActionOutcome, DockspaceActionRejection};
use crate::runtime::{DockspaceSession, SurfaceUnavailableReason};

mod staging_resource;

#[test]
fn zero_area_ready_surface_is_rejected_and_cannot_authorize_contained_placement() {
    let mut fixture = counter_fixture();
    let contribution = begin_surface_measurement(
        &fixture.engine,
        SOURCE_SURFACE,
        LogicalRect::new(0.0, 0.0, 0.0, 100.0)
            .expect("zero-width bounds remain representable geometry"),
    );
    let transition =
        submit_surface_measurement(&mut fixture.engine, fixture.presentation_host, contribution);
    assert!(matches!(
        transition
            .surface_contributions()
            .iter()
            .find(|outcome| outcome.surface() == SOURCE_SURFACE),
        Some(SurfaceContributionOutcome::Unavailable {
            surface: SOURCE_SURFACE,
            reason: SurfaceContributionUnavailableReason::EmptyBounds,
            ..
        })
    ));

    assert_eq!(
        fixture.engine.contained_placement(
            SOURCE_SURFACE,
            test_rect(),
            LogicalSize::new(0.0, 0.0).expect("minimum size must be valid"),
        ),
        Err(ContainedPlacementUnavailable::BootstrapSurface {
            surface: SOURCE_SURFACE,
        })
    );
}

#[test]
fn contained_clamp_never_authorizes_a_non_positive_durable_rect() {
    let minimum = LogicalSize::new(0.0, 0.0).expect("minimum size must be valid");
    for requested in [
        LogicalRect::new(0.0, 0.0, 0.0, 40.0)
            .expect("zero-width request remains representable geometry"),
        LogicalRect::new(0.0, 0.0, 40.0, 0.0)
            .expect("zero-height request remains representable geometry"),
    ] {
        assert_eq!(
            clamp_contained_rect(SOURCE_SURFACE, test_rect(), requested, minimum),
            Err(ContainedPlacementUnavailable::UnrepresentableGeometry {
                surface: SOURCE_SURFACE,
            })
        );
    }

    let huge_bounds = LogicalRect::from_min_max(
        LogicalPoint::new(f64::MAX / 2.0, 0.0).expect("minimum corner must be finite"),
        LogicalPoint::new(f64::MAX, 100.0).expect("maximum corner must be finite"),
    )
    .expect("huge positive bounds must remain representable");
    assert_eq!(
        clamp_contained_rect(
            SOURCE_SURFACE,
            huge_bounds,
            LogicalRect::new(0.0, 0.0, 1.0, 40.0).expect("positive request must be valid"),
            minimum,
        ),
        Err(ContainedPlacementUnavailable::UnrepresentableGeometry {
            surface: SOURCE_SURFACE,
        })
    );
}

fn background_bounds() -> LogicalRect {
    LogicalRect::new(0.0, 0.0, 400.0, 300.0).expect("background bounds must be valid")
}

fn background_contained_rect() -> LogicalRect {
    LogicalRect::new(220.0, 160.0, 120.0, 120.0).expect("contained rectangle must be valid")
}

fn background_fixture(policy: DockPolicy) -> BackgroundFixture {
    let mut builder = Workspace::builder();
    let source_tabs = builder.insert_node(Node::tabs([ItemId::new(1), ItemId::new(2)]));
    let contained_tabs = builder.insert_node(Node::tabs([ItemId::new(3)]));
    let contained = FloatingPresentationId::new(2);
    builder.set_root(SOURCE_ROOT, RootRecord::new(source_tabs));
    builder.set_root(TARGET_ROOT, RootRecord::new(contained_tabs));
    builder.set_surface(SOURCE_SURFACE, SurfacePresentation::with_main(SOURCE_ROOT));
    builder.set_surface(TARGET_SURFACE, SurfacePresentation::rootless());
    builder.set_contained_floating(
        contained,
        ContainedFloating::new(TARGET_ROOT, background_contained_rect()),
    );
    builder
        .attach_contained(TARGET_SURFACE, contained)
        .expect("rootless target surface must exist");
    let workspace = builder.build().expect("background workspace must be valid");
    let mut engine = DockEngine::new(workspace, policy).expect("background engine must be valid");
    let presentation_host = engine
        .create_presentation_host()
        .expect("background presentation host must mint");
    let mut fixture = BackgroundFixture {
        engine,
        presentation_host,
        source_tabs,
    };
    publish_background_scene(&mut fixture);
    fixture
}

fn publish_background_scene(fixture: &mut BackgroundFixture) {
    publish_surface_projection(
        &mut fixture.engine,
        fixture.presentation_host,
        SOURCE_SURFACE,
        background_bounds(),
    );
    publish_surface_projection(
        &mut fixture.engine,
        fixture.presentation_host,
        TARGET_SURFACE,
        background_bounds(),
    );
}

#[test]
fn prepared_surface_plan_is_compiled_once_and_installed_without_recompilation() {
    let mut fixture = background_fixture(DockPolicy::default());
    crate::scene_compiler::reset_surface_compilation_count();
    let token = fixture
        .engine
        .begin_surface_contribution(SOURCE_SURFACE)
        .expect("source surface contribution begins");
    let measurements = surface_measurements(&fixture.engine, SOURCE_SURFACE, background_bounds());

    let prepared = fixture
        .engine
        .prepare_surface_contribution(token, measurements)
        .expect("complete measurements prepare successfully");

    assert!(matches!(
        prepared.paint_candidate(),
        PreparedSurfacePaintCandidate::Ready(_)
    ));
    assert_eq!(crate::scene_compiler::surface_compilation_count(), 1);
    let transition =
        submit_surface_measurement(&mut fixture.engine, fixture.presentation_host, prepared);
    assert!(matches!(
        transition
            .surface_contributions()
            .iter()
            .find(|outcome| outcome.surface() == SOURCE_SURFACE),
        Some(SurfaceContributionOutcome::Ready {
            surface: SOURCE_SURFACE,
            ..
        })
    ));
    assert_eq!(
        crate::scene_compiler::surface_compilation_count(),
        1,
        "reduction must install the prepared plan rather than compiling it again"
    );
}

fn background_payload(fixture: &BackgroundFixture, item: ItemId) -> MovePayload {
    MovePayload::Item(
        fixture
            .engine
            .workspace()
            .capture_item_source(SOURCE_ROOT, fixture.source_tabs, item)
            .expect("background source item must be current"),
    )
}

#[test]
fn engine_presented_drop_reuses_the_requirement_workspace_index() {
    let fixture = background_fixture(DockPolicy::default());
    let source = background_payload(&fixture, ItemId::new(1));
    let projection = fixture
        .engine
        .interaction_projection(TARGET_SURFACE)
        .expect("rootless target projection must be receiver-authoritative");
    let presentation = JournalSurfacePresentation::from_interaction(projection);
    let point = LogicalPoint::new(10.0, 10.0).expect("background point must be finite");

    crate::drop_resolver::structural_work::reset();
    let query = fixture
        .engine
        .resolve_presented_drop_with_current_index(
            &presentation,
            None,
            fixture.engine.policy_snapshot(),
            DragSessionId::new(fixture.engine.version().epoch(), DragGeneration::new(1)),
            source,
            Some(crate::intent::SurfaceBackgroundRootOffer::new(RootId::new(
                1_000,
            ))),
            point,
        )
        .expect("current engine presentation must resolve without rebuilding its index");
    assert!(matches!(
        query.resolution(),
        DropResolution::Resolved(resolved)
            if resolved.target_id()
                == crate::drop_target::DropTargetId::SurfaceBackground {
                    surface: TARGET_SURFACE,
                }
    ));

    let work = crate::drop_resolver::structural_work::snapshot();
    assert_eq!(work.drop_targets_assessed, 1);
    assert_eq!(work.geometric_winners, 1);
    assert_eq!(work.transaction_prepares, 1);
    assert_eq!(work.root_fingerprint_builds, 2);
    assert_eq!(work.root_fingerprint_node_visits, 2);
}

struct DurableRectFixture {
    engine: DockEngine,
    presentation_host: PresentationHostLease,
    surface: SurfaceId,
    first: FloatingPresentationId,
    second: FloatingPresentationId,
}

fn durable_rect_fixture() -> DurableRectFixture {
    let surface = SurfaceId::new(60);
    let first = FloatingPresentationId::new(62);
    let second = FloatingPresentationId::new(61);
    let first_root = RootId::new(62);
    let second_root = RootId::new(61);
    let mut builder = Workspace::builder();
    let first_tabs = builder.insert_node(Node::tabs([ItemId::new(62)]));
    let second_tabs = builder.insert_node(Node::tabs([ItemId::new(61)]));
    builder.set_root(first_root, RootRecord::new(first_tabs));
    builder.set_root(second_root, RootRecord::new(second_tabs));
    builder.set_surface(surface, SurfacePresentation::rootless());
    builder.set_contained_floating(
        first,
        ContainedFloating::new(
            first_root,
            LogicalRect::new(220.0, 160.0, 120.0, 120.0)
                .expect("first contained rectangle must be valid"),
        ),
    );
    builder.set_contained_floating(
        second,
        ContainedFloating::new(
            second_root,
            LogicalRect::new(150.0, 100.0, 120.0, 120.0)
                .expect("second contained rectangle must be valid"),
        ),
    );
    builder
        .attach_contained(surface, first)
        .expect("first contained presentation must attach");
    builder
        .attach_contained(surface, second)
        .expect("second contained presentation must attach");
    let workspace = builder
        .build()
        .expect("durable-rectangle workspace must be valid");
    let mut engine = DockEngine::new(workspace, DockPolicy::default())
        .expect("durable-rectangle engine must be valid");
    let presentation_host = engine
        .create_presentation_host()
        .expect("durable-rectangle presentation host must mint");
    DurableRectFixture {
        engine,
        presentation_host,
        surface,
        first,
        second,
    }
}

fn submit_durable_rect_projection(
    fixture: &mut DurableRectFixture,
    bounds: LogicalRect,
) -> EngineTransition {
    let contribution = begin_surface_measurement(&fixture.engine, fixture.surface, bounds);
    submit_surface_measurement(&mut fixture.engine, fixture.presentation_host, contribution)
}

fn publish_initial_durable_rect_projection(fixture: &mut DurableRectFixture) {
    publish_surface_projection(
        &mut fixture.engine,
        fixture.presentation_host,
        fixture.surface,
        LogicalRect::new(0.0, 0.0, 400.0, 300.0).expect("initial surface bounds must be valid"),
    );
}

#[test]
fn surface_resize_clips_presentation_without_rewriting_durable_contained_rects() {
    let mut fixture = durable_rect_fixture();
    publish_initial_durable_rect_projection(&mut fixture);
    let before_workspace = fixture.engine.workspace().clone();
    let before_version = fixture.engine.version();
    let initial_plan = fixture
        .engine
        .scene()
        .ready_surface(fixture.surface)
        .expect("initial projection must be painted")
        .plan();
    let initial_identity = initial_plan
        .contained_records()
        .iter()
        .map(|record| {
            (
                record.floating(),
                record.root(),
                record.ordinal(),
                record.layer(),
            )
        })
        .collect::<Vec<_>>();
    let initial_outer_bounds = initial_plan
        .contained_records()
        .iter()
        .map(crate::scene::ContainedRecord::outer_bounds)
        .collect::<Vec<_>>();
    assert_eq!(
        initial_outer_bounds,
        [
            fixture
                .engine
                .workspace()
                .contained_floating(fixture.first)
                .expect("first durable floating must exist")
                .rect,
            fixture
                .engine
                .workspace()
                .contained_floating(fixture.second)
                .expect("second durable floating must exist")
                .rect,
        ]
    );

    let shrunken_bounds =
        LogicalRect::new(0.0, 0.0, 200.0, 150.0).expect("shrunken bounds must be valid");
    let shrunken = submit_durable_rect_projection(&mut fixture, shrunken_bounds);
    assert!(matches!(
        shrunken.surface_contributions(),
        [SurfaceContributionOutcome::Ready {
            surface,
            ..
        }] if *surface == fixture.surface
    ));
    assert!(shrunken.events().is_empty());
    assert_eq!(fixture.engine.workspace(), &before_workspace);
    assert_eq!(fixture.engine.version(), before_version);

    let pending_shrunken = fixture
        .engine
        .scene()
        .surface(fixture.surface)
        .and_then(SurfaceScene::paint_projection)
        .expect("shrunken projection must be paintable");
    assert_eq!(pending_shrunken.plan().bounds(), shrunken_bounds);
    assert_eq!(
        pending_shrunken
            .plan()
            .contained_records()
            .iter()
            .map(|record| (record.floating(), record.outer_bounds()))
            .collect::<Vec<_>>(),
        [(
            fixture.second,
            LogicalRect::new(150.0, 100.0, 50.0, 50.0)
                .expect("partially clipped bounds must remain representable"),
        )],
        "the fully hidden floating is omitted while the visible subset is clipped"
    );
    assert_eq!(
        fixture
            .engine
            .scene()
            .ready_surface(fixture.surface)
            .expect("the prior painted projection remains hit authority")
            .plan()
            .bounds(),
        LogicalRect::new(0.0, 0.0, 400.0, 300.0).expect("initial surface bounds must be valid")
    );
    publish_surface_projection(
        &mut fixture.engine,
        fixture.presentation_host,
        fixture.surface,
        shrunken_bounds,
    );
    assert_eq!(
        fixture
            .engine
            .scene()
            .ready_surface(fixture.surface)
            .expect("the painted shrunken projection becomes hit authority")
            .plan()
            .bounds(),
        shrunken_bounds
    );

    let expanded_bounds =
        LogicalRect::new(0.0, 0.0, 400.0, 300.0).expect("expanded surface bounds must be valid");
    let expanded = submit_durable_rect_projection(&mut fixture, expanded_bounds);
    assert!(matches!(
        expanded.surface_contributions(),
        [SurfaceContributionOutcome::Ready {
            surface,
            ..
        }] if *surface == fixture.surface
    ));
    assert!(expanded.events().is_empty());
    assert_eq!(fixture.engine.workspace(), &before_workspace);
    assert_eq!(fixture.engine.version(), before_version);

    let pending_expanded = fixture
        .engine
        .scene()
        .surface(fixture.surface)
        .and_then(SurfaceScene::paint_projection)
        .expect("expanded projection must be paintable");
    assert_eq!(
        pending_expanded
            .plan()
            .contained_records()
            .iter()
            .map(|record| {
                (
                    record.floating(),
                    record.root(),
                    record.ordinal(),
                    record.layer(),
                )
            })
            .collect::<Vec<_>>(),
        initial_identity
    );
    assert_eq!(
        pending_expanded
            .plan()
            .contained_records()
            .iter()
            .map(crate::scene::ContainedRecord::outer_bounds)
            .collect::<Vec<_>>(),
        initial_outer_bounds
    );
    publish_surface_projection(
        &mut fixture.engine,
        fixture.presentation_host,
        fixture.surface,
        expanded_bounds,
    );
    assert_eq!(
        fixture
            .engine
            .scene()
            .ready_surface(fixture.surface)
            .expect("the painted expanded projection becomes hit authority")
            .plan()
            .contained_records()
            .iter()
            .map(crate::scene::ContainedRecord::outer_bounds)
            .collect::<Vec<_>>(),
        initial_outer_bounds
    );
}

fn background_source_binding(fixture: &BackgroundFixture) -> ViewportBinding {
    fixture
        .engine
        .viewport()
        .viewport(SOURCE_SURFACE)
        .expect("native background source viewport must be registered")
        .binding()
}

fn background_platform_snapshot(
    source_binding: ViewportBinding,
    content_bounds: Option<PhysicalRect>,
    focus_generation: u64,
    close_requested: bool,
) -> PlatformSnapshot {
    const WORK_AREA: WorkAreaToken = WorkAreaToken::new(93);
    let mut capabilities = PlatformCapabilities::default();
    capabilities.set_native_window_lifecycle(PlatformCapability::Supported);
    capabilities.set_authoritative_inventory(PlatformCapability::Supported);
    capabilities.set_global_window_placement(PlatformCapability::Supported);
    capabilities.set_work_area(PlatformCapability::Supported);
    capabilities.set_global_focus_observation(PlatformCapability::Supported);
    let observed = content_bounds.into_iter().map(|content_bounds| {
        ObservedWindow::new(source_binding)
            .with_coordinate_observation(WindowCoordinateObservation::new(
                source_binding,
                CoordinateObservationGeneration::new(focus_generation),
                Authority::Known(content_bounds),
                Authority::Known(
                    PhysicalRect::new(-8.0, -30.0, 416.0, 338.0)
                        .expect("source outer bounds must be valid"),
                ),
                Authority::Known(ScaleFactor::new(1.0).expect("source scale must be valid")),
                Authority::Known(ScaleFactor::new(1.0).expect("source scale must be valid")),
            ))
            .with_close_requested(Authority::Known(close_requested))
            .with_presentation_observation(WindowPresentationObservation::new(
                source_binding,
                crate::viewport::PresentationObservationGeneration::new(focus_generation),
                Authority::Known(WindowPresentationState::Visible),
                PresentationEffectAcknowledgement::known(None),
            ))
    });
    test_platform_snapshot(
        capabilities,
        FocusObservationEnvelope::new(
            FocusObservationGeneration::new(focus_generation),
            Authority::Known(GlobalFocusedWindow::Foreign),
            Authority::Known(None),
        ),
        observed.collect(),
        Vec::new(),
        known_work_area_observation(
            focus_generation,
            vec![ObservedWorkArea::new(
                WORK_AREA,
                PhysicalRect::new(0.0, 0.0, 1920.0, 1080.0).expect("work area must be valid"),
                ScaleFactor::new(1.0).expect("work-area scale must be valid"),
            )],
        ),
    )
    .expect("native platform facts must be canonical")
}

fn publish_native_background_platform(fixture: &mut BackgroundFixture) -> WorkAreaToken {
    const SOURCE_WINDOW: WindowToken = WindowToken::new(93);
    const WORK_AREA: WorkAreaToken = WorkAreaToken::new(93);

    let provider = test_platform_provider(&mut fixture.engine);
    let expected = fixture.engine.version();
    submit_test_input(
        &mut fixture.engine,
        fixture.presentation_host,
        EngineInput::RegisterViewport {
            provider,
            expected,
            surface: SOURCE_SURFACE,
            token: SOURCE_WINDOW,
            role: ViewportRole::Root,
            recovery_target: None,
        },
    )
    .expect("source viewport registration must reduce");
    let snapshot = background_platform_snapshot(
        background_source_binding(fixture),
        Some(
            PhysicalRect::new(0.0, 0.0, 400.0, 300.0)
                .expect("source physical bounds must be valid"),
        ),
        1,
        false,
    );
    let expected_epoch = fixture.engine.version().epoch();
    submit_test_input(
        &mut fixture.engine,
        fixture.presentation_host,
        EngineInput::PublishPlatformSnapshot {
            provider,
            expected_epoch,
            snapshot,
        },
    )
    .expect("native platform snapshot must reduce");
    publish_background_scene(fixture);
    WORK_AREA
}

#[test]
fn native_surface_without_coordinate_authority_prepares_only_unavailable_paint() {
    const PENDING_WINDOW: WindowToken = WindowToken::new(94);
    let mut fixture = background_fixture(DockPolicy::default());
    let provider = test_platform_provider(&mut fixture.engine);
    let expected = fixture.engine.version();
    submit_test_input(
        &mut fixture.engine,
        fixture.presentation_host,
        EngineInput::RegisterViewport {
            provider,
            expected,
            surface: SOURCE_SURFACE,
            token: PENDING_WINDOW,
            role: ViewportRole::Root,
            recovery_target: None,
        },
    )
    .expect("pending native viewport registration reduces");
    let token = fixture
        .engine
        .begin_surface_contribution(SOURCE_SURFACE)
        .expect("registered surface contribution begins");
    let measurements = surface_measurements(&fixture.engine, SOURCE_SURFACE, background_bounds());

    let prepared = fixture
        .engine
        .prepare_surface_contribution(token, measurements)
        .expect("valid measurements prepare despite unavailable coordinates");

    assert!(matches!(
        prepared.paint_candidate(),
        PreparedSurfacePaintCandidate::Unavailable {
            reason: SurfaceContributionUnavailableReason::CoordinateAuthorityUnavailable,
        }
    ));
}

#[test]
fn headless_contribution_cannot_publish_after_native_registration() {
    const LATE_WINDOW: WindowToken = WindowToken::new(95);
    let mut fixture = background_fixture(DockPolicy::default());
    let contribution =
        begin_surface_measurement(&fixture.engine, SOURCE_SURFACE, background_bounds());
    let provider = test_platform_provider(&mut fixture.engine);
    let expected = fixture.engine.version();
    submit_test_input(
        &mut fixture.engine,
        fixture.presentation_host,
        EngineInput::RegisterViewport {
            provider,
            expected,
            surface: SOURCE_SURFACE,
            token: LATE_WINDOW,
            role: ViewportRole::Root,
            recovery_target: None,
        },
    )
    .expect("late registration must reduce");
    let before_workspace = fixture.engine.workspace().clone();
    let before_version = fixture.engine.version();
    let before_scene = fixture.engine.scene().clone();
    let before_interaction = fixture.engine.interaction().clone();

    let transition =
        submit_surface_measurement(&mut fixture.engine, fixture.presentation_host, contribution);

    assert!(matches!(
        transition
            .surface_contributions()
            .iter()
            .find(|outcome| outcome.surface() == SOURCE_SURFACE),
        Some(SurfaceContributionOutcome::Rejected {
            surface: SOURCE_SURFACE,
            reason: SurfaceContributionRejection::StaleBase { .. },
        })
    ));
    assert!(transition.events().is_empty());
    assert!(transition.interaction_events().is_empty());
    assert_eq!(fixture.engine.workspace(), &before_workspace);
    assert_eq!(fixture.engine.version(), before_version);
    assert_eq!(fixture.engine.scene(), &before_scene);
    assert_eq!(fixture.engine.interaction(), &before_interaction);
}

#[test]
fn native_contribution_requires_an_explicit_tombstone_before_binding_is_removed() {
    let mut fixture = background_fixture(DockPolicy::default());
    publish_native_background_platform(&mut fixture);
    let stale_contribution = begin_surface_measurement(
        &fixture.engine,
        SOURCE_SURFACE,
        LogicalRect::new(0.0, 0.0, 200.0, 150.0).expect("shrunken bounds must be valid"),
    );
    let expected_epoch = fixture.engine.version().epoch();
    let source_binding = background_source_binding(&fixture);
    let provider = test_platform_provider(&mut fixture.engine);
    submit_test_input(
        &mut fixture.engine,
        fixture.presentation_host,
        EngineInput::PublishPlatformSnapshot {
            provider,
            expected_epoch,
            snapshot: background_platform_snapshot(source_binding, None, 2, false),
        },
    )
    .expect("authoritative inventory absence must reduce");
    assert!(
        fixture.engine.viewport().viewport(SOURCE_SURFACE).is_some(),
        "inventory absence is Missing, not a destruction tombstone"
    );
    let before_workspace = fixture.engine.workspace().clone();
    let before_version = fixture.engine.version();
    let before_scene = fixture.engine.scene().clone();
    let before_interaction = fixture.engine.interaction().clone();

    let transition = submit_surface_measurement(
        &mut fixture.engine,
        fixture.presentation_host,
        stale_contribution,
    );

    assert!(matches!(
        transition
            .surface_contributions()
            .iter()
            .find(|outcome| outcome.surface() == SOURCE_SURFACE),
        Some(SurfaceContributionOutcome::Rejected {
            surface: SOURCE_SURFACE,
            reason: SurfaceContributionRejection::StaleBase { .. },
        })
    ));
    assert!(transition.events().is_empty());
    assert!(transition.interaction_events().is_empty());
    assert_eq!(fixture.engine.workspace(), &before_workspace);
    assert_eq!(fixture.engine.version(), before_version);
    assert_eq!(fixture.engine.scene(), &before_scene);
    assert_eq!(fixture.engine.interaction(), &before_interaction);
}

fn install_pending_native_root_reservation(
    fixture: &mut BackgroundFixture,
    reserved_root: RootId,
) -> crate::frame::NativeCreateRequest {
    let work_area = publish_native_background_platform(fixture);
    install_pending_native_root_reservation_with_work_area(fixture, reserved_root, work_area)
}

fn install_pending_native_root_reservation_with_work_area(
    fixture: &mut BackgroundFixture,
    reserved_root: RootId,
    work_area: WorkAreaToken,
) -> crate::frame::NativeCreateRequest {
    const NATIVE_SURFACE: SurfaceId = SurfaceId::new(93);
    const RECOVERY_FLOATING: FloatingPresentationId = FloatingPresentationId::new(93);

    let placement = fixture
        .engine
        .viewport_placement(
            SOURCE_SURFACE,
            LogicalRect::new(50.0, 50.0, 300.0, 220.0)
                .expect("native logical placement must be valid"),
            work_area,
        )
        .expect("native placement proof must be current");
    let payload = background_payload(fixture, ItemId::new(1));
    let command = WorkspaceCommand::CreateSurfaceRoot {
        surface: NATIVE_SURFACE,
        root: reserved_root,
        content: RootContent::Move(payload.clone()),
    };
    let converted_main = crate::surface_recovery::ConvertedMainRecovery::new(
        reserved_root,
        RECOVERY_FLOATING,
        LogicalSize::new(0.0, 0.0).expect("recovery minimum must be valid"),
    );
    let proposal =
        NativeTearOffProposal::new(NATIVE_SURFACE, reserved_root, placement, converted_main)
            .expect("native recovery contract must be valid");
    let anchor = fixture
        .engine
        .root_recovery_anchor(SOURCE_SURFACE)
        .expect("registered source root must own a recovery anchor");
    let target = SurfaceRecoveryTarget::with_converted_main(anchor, converted_main);
    let mut future = fixture.engine.workspace().clone();
    WorkspaceTransaction::from_commands([command.clone()])
        .apply(&mut future, fixture.engine.policy_snapshot())
        .expect("native command must produce a future workspace");
    let id = fixture
        .engine
        .next_surface_recovery_obligation_id(InputSequence::new(93))
        .expect("test obligation identity must advance");
    let obligation = fixture
        .engine
        .authorize_surface_recovery_obligation(
            InputSequence::new(93),
            id,
            &future,
            NATIVE_SURFACE,
            target,
            fixture.engine.policy_snapshot(),
        )
        .expect("native recovery must be authorized");
    let focus_causal = mint_test_focus_causal(&mut fixture.engine);
    let source_presentation = fixture
        .engine
        .interaction_authority(SOURCE_SURFACE)
        .expect("source surface must have presented interaction authority");
    let prepared = crate::frame::PreparedNativeCreate::new(
        source_presentation,
        payload,
        fixture.engine.version().epoch(),
        command,
        crate::frame::NativeCreateProposal::from_tear_off(&proposal),
        obligation,
        focus_causal,
    );
    let request = fixture
        .engine
        .viewport
        .start_native_create(prepared)
        .expect("native create reservation must start");
    assert!(fixture.engine.viewport_focus.reserve_activation_causal(
        request.saga(),
        focus_causal,
        ViewportActivationRequest::tear_off_committed(
            request.binding(),
            PaneFocusDisposition::Clear,
        ),
    ));
    request
}

#[test]
fn product_main_root_skips_a_pending_native_root_reservation() {
    let mut policy = DockPolicy::default();
    policy.set_allow_native_surfaces(true);
    let mut fixture = background_fixture(policy);
    let reserved_root = RootId::new(94);
    install_pending_native_root_reservation(&mut fixture, reserved_root);
    let expected = fixture.engine.version();

    let transition = submit_test_input(
        &mut fixture.engine,
        fixture.presentation_host,
        EngineInput::OpenItem {
            expected,
            item: ItemId::new(99),
            placement: DockPlacement::Main(TARGET_SURFACE),
        },
    )
    .expect("product main-root action must reduce beside the pending native saga");
    let prepared_root = RootId::new(reserved_root.get() + 1);

    assert!(matches!(
        transition.reduced_inputs()[0].outcome(),
        InputOutcome::ProductActionProcessed {
            outcome: DockspaceActionOutcome::Opened {
                item,
                root,
            },
            ..
        } if *item == ItemId::new(99) && *root == prepared_root
    ));
    assert_eq!(
        fixture
            .engine
            .workspace()
            .surface(TARGET_SURFACE)
            .and_then(|surface| surface.main_root),
        Some(prepared_root),
    );
}

#[test]
fn pending_newer_native_reservation_preserves_ready_older_owner() {
    let mut policy = DockPolicy::default();
    policy.set_allow_native_surfaces(true);
    let mut fixture = background_fixture(policy);
    let work_area = publish_native_background_platform(&mut fixture);
    let source_binding = background_source_binding(&fixture);
    let older_owner = NativeCreateSagaId::new(200);
    let older_causal = mint_test_focus_causal(&mut fixture.engine);
    assert!(fixture.engine.viewport_focus.reserve_activation_causal(
        older_owner,
        older_causal,
        ViewportActivationRequest::tear_off_committed(source_binding, PaneFocusDisposition::Clear,),
    ));
    let newer = install_pending_native_root_reservation_with_work_area(
        &mut fixture,
        RootId::new(97),
        work_area,
    );
    assert_eq!(
        fixture
            .engine
            .viewport_focus
            .winning_activation_reservation()
            .map(|(owner, _, _)| owner),
        Some(newer.saga())
    );

    fixture
        .engine
        .settle_native_activation_reservations(&mut Vec::new())
        .expect("a pending newer owner must defer reservation settlement");

    let owners = fixture
        .engine
        .viewport_focus
        .activation_reservations()
        .into_iter()
        .map(|(owner, _, _)| owner)
        .collect::<Vec<_>>();
    assert_eq!(owners, vec![newer.saga(), older_owner]);
    assert!(
        fixture
            .engine
            .viewport_focus
            .cancel_activation_reservation(newer.saga())
    );
    assert_eq!(
        fixture
            .engine
            .viewport_focus
            .winning_activation_reservation()
            .map(|(owner, _, _)| owner),
        Some(older_owner),
        "cancelling the pending winner must reveal the retained older claim"
    );
}

#[test]
fn terminal_missing_newer_reservation_is_pruned_before_older_owner_settlement() {
    let mut policy = DockPolicy::default();
    policy.set_allow_native_surfaces(true);
    let mut fixture = background_fixture(policy);
    publish_native_background_platform(&mut fixture);
    let source_binding = background_source_binding(&fixture);
    let older_owner = NativeCreateSagaId::new(201);
    let older_causal = mint_test_focus_causal(&mut fixture.engine);
    assert!(fixture.engine.viewport_focus.reserve_activation_causal(
        older_owner,
        older_causal,
        ViewportActivationRequest::tear_off_committed(source_binding, PaneFocusDisposition::Clear,),
    ));
    let missing_binding = ViewportBinding::new(
        source_binding.authority_domain(),
        source_binding.epoch(),
        SurfaceId::new(202),
        WindowToken::new(202),
        WindowIncarnation::new(1),
    );
    let terminal_owner = NativeCreateSagaId::new(202);
    let newer_causal = mint_test_focus_causal(&mut fixture.engine);
    assert!(
        fixture.engine.viewport_focus.reserve_activation_causal(
            terminal_owner,
            newer_causal,
            ViewportActivationRequest::tear_off_committed(
                missing_binding,
                PaneFocusDisposition::Clear,
            ),
        )
    );

    fixture
        .engine
        .settle_native_activation_reservations(&mut Vec::new())
        .expect("terminal reservation pruning must preserve the older claim");

    assert_eq!(
        fixture
            .engine
            .viewport_focus
            .activation_reservations()
            .into_iter()
            .map(|(owner, _, _)| owner)
            .collect::<Vec<_>>(),
        vec![older_owner]
    );
    assert_eq!(
        fixture
            .engine
            .viewport_focus
            .winning_activation_reservation()
            .map(|(owner, _, _)| owner),
        Some(older_owner)
    );
}

#[test]
fn direct_surface_close_rejects_a_staging_native_create_without_a_plan() {
    let mut policy = DockPolicy::default();
    policy.set_allow_native_surfaces(true);
    let mut fixture = background_fixture(policy);
    let request = install_pending_native_root_reservation(&mut fixture, RootId::new(96));
    let edge = NativeCloseEdge::from_authoritative_requested(
        fixture.engine.authority_domain,
        request.binding(),
        CloseObservationGeneration::new(1),
        fixture.engine.viewport().registry().inventory_generation(),
    );
    let version = fixture.engine.version();
    let policy = fixture.engine.policy.clone();

    let outcome = fixture
        .engine
        .reduce_surface_close_request(
            InputSequence::new(96),
            version,
            edge,
            SurfaceCloseRequest::RetainLayout,
            version,
            &policy,
        )
        .expect("pre-admission close rejection must be a normal reducer outcome");

    assert!(matches!(
        outcome,
        InputOutcome::SurfaceCloseRejected {
            edge: actual,
            reason: SurfaceCloseRequestRejection::StagingBinding { binding },
            ..
        } if actual == edge && binding == request.binding()
    ));
    assert_eq!(fixture.engine.active_close_plans().count(), 0);
    assert!(
        fixture
            .engine
            .viewport()
            .native_create_saga(request.saga())
            .is_some()
    );
}

fn background_native_window_snapshot(
    fixture: &mut BackgroundFixture,
    request: crate::frame::NativeCreateRequest,
    presentation: WindowPresentationState,
    generation: u64,
    focus_control: bool,
) -> PlatformSnapshot {
    background_native_window_snapshot_with_focus(
        fixture,
        request,
        presentation,
        generation,
        focus_control,
        GlobalFocusedWindow::Foreign,
    )
}

fn background_native_window_snapshot_with_focus(
    fixture: &mut BackgroundFixture,
    request: crate::frame::NativeCreateRequest,
    presentation: WindowPresentationState,
    generation: u64,
    focus_control: bool,
    focused: GlobalFocusedWindow,
) -> PlatformSnapshot {
    let mut capabilities = PlatformCapabilities::default();
    capabilities.set_native_window_lifecycle(PlatformCapability::Supported);
    capabilities.set_authoritative_inventory(PlatformCapability::Supported);
    capabilities.set_global_window_placement(PlatformCapability::Supported);
    capabilities.set_work_area(PlatformCapability::Supported);
    capabilities.set_global_focus_observation(PlatformCapability::Supported);
    if focus_control {
        capabilities.set_window_activation_control(PlatformCapability::Supported);
    }
    background_native_window_snapshot_with_capabilities(
        fixture,
        request,
        presentation,
        generation,
        focused,
        capabilities,
    )
}

fn background_native_window_snapshot_with_capabilities(
    fixture: &mut BackgroundFixture,
    request: crate::frame::NativeCreateRequest,
    presentation: WindowPresentationState,
    generation: u64,
    focused: GlobalFocusedWindow,
    capabilities: PlatformCapabilities,
) -> PlatformSnapshot {
    const WORK_AREA: WorkAreaToken = WorkAreaToken::new(93);
    let source = ObservedWindow::new(background_source_binding(fixture))
        .with_coordinate_observation(WindowCoordinateObservation::new(
            background_source_binding(fixture),
            CoordinateObservationGeneration::new(generation),
            Authority::Known(
                PhysicalRect::new(0.0, 0.0, 400.0, 300.0)
                    .expect("source physical bounds must be valid"),
            ),
            Authority::Known(
                PhysicalRect::new(-8.0, -30.0, 416.0, 338.0)
                    .expect("source outer bounds must be valid"),
            ),
            Authority::Known(ScaleFactor::new(1.0).expect("source scale must be valid")),
            Authority::Known(ScaleFactor::new(1.0).expect("source scale must be valid")),
        ))
        .with_presentation_observation(WindowPresentationObservation::new(
            background_source_binding(fixture),
            crate::viewport::PresentationObservationGeneration::new(generation),
            Authority::Known(WindowPresentationState::Visible),
            PresentationEffectAcknowledgement::known(None),
        ));
    let acknowledged_effect = fixture
        .engine
        .viewport()
        .native_create_saga(request.saga())
        .map(|saga| saga.phase().presentation_correlation_effect());
    let native = ObservedWindow::new(request.binding())
        .with_coordinate_observation(WindowCoordinateObservation::new(
            request.binding(),
            CoordinateObservationGeneration::new(generation),
            Authority::Known(
                PhysicalRect::new(500.0, 0.0, 300.0, 220.0)
                    .expect("native physical bounds must be valid"),
            ),
            Authority::Known(
                PhysicalRect::new(492.0, -30.0, 316.0, 258.0)
                    .expect("native outer bounds must be valid"),
            ),
            Authority::Known(ScaleFactor::new(1.0).expect("native scale must be valid")),
            Authority::Known(ScaleFactor::new(1.0).expect("native scale must be valid")),
        ))
        .with_presentation_observation(WindowPresentationObservation::new(
            request.binding(),
            crate::viewport::PresentationObservationGeneration::new(generation),
            Authority::Known(presentation),
            PresentationEffectAcknowledgement::known(acknowledged_effect),
        ));
    test_platform_snapshot(
        capabilities,
        FocusObservationEnvelope::new(
            FocusObservationGeneration::new(generation),
            Authority::Known(focused),
            Authority::Known(None),
        ),
        vec![source, native],
        Vec::new(),
        known_work_area_observation(
            generation,
            vec![ObservedWorkArea::new(
                WORK_AREA,
                PhysicalRect::new(0.0, 0.0, 1920.0, 1080.0).expect("work area must be valid"),
                ScaleFactor::new(1.0).expect("work-area scale must be valid"),
            )],
        ),
    )
    .expect("native create snapshot must be canonical")
}

fn publish_background_native_window(
    fixture: &mut BackgroundFixture,
    request: crate::frame::NativeCreateRequest,
    presentation: WindowPresentationState,
    generation: u64,
) -> EngineTransition {
    let snapshot =
        background_native_window_snapshot(fixture, request, presentation, generation, false);
    let provider = test_platform_provider(&mut fixture.engine);
    let expected_epoch = fixture.engine.version().epoch();
    submit_test_input(
        &mut fixture.engine,
        fixture.presentation_host,
        EngineInput::PublishPlatformSnapshot {
            provider,
            expected_epoch,
            snapshot,
        },
    )
    .expect("native create snapshot must reduce")
}

fn current_native_staging_presentation(
    fixture: &BackgroundFixture,
    request: crate::frame::NativeCreateRequest,
    phase: crate::presentation_observation::NativeStagingPresentationPhase,
) -> NativeStagingPresentation {
    let saga = fixture
        .engine
        .viewport()
        .native_create_saga(request.saga())
        .expect("native create saga must remain pending");
    match saga.phase() {
        crate::frame::NativeCreatePhase::AwaitingPreShowPresentation { presentation, .. }
        | crate::frame::NativeCreatePhase::AwaitingPostShowPresentation { presentation, .. }
            if presentation.phase() == phase =>
        {
            Some(presentation)
        }
        _ => None,
    }
    .unwrap_or_else(|| {
        panic!(
            "native create must request {phase:?}, actual phase was {:?}",
            saga.phase()
        )
    })
}

fn emit_background_native_staging(
    fixture: &mut BackgroundFixture,
    presentation: NativeStagingPresentation,
) -> crate::presentation_observation::HostPresentationOutput {
    let mut frame = begin_test_host_frame(&fixture.engine, fixture.presentation_host);
    assert!(
        frame
            .view()
            .native_staging_presentations()
            .any(|current| current == presentation)
    );
    for obligation in frame
        .issue_presentation_obligations()
        .expect("native staging frame must issue its exact physical roster")
    {
        let disposition = if obligation.slot().native_staging() == Some(presentation) {
            HostPresentationDisposition::Painted(HostInteractionPresentation::default())
        } else {
            HostPresentationDisposition::Unavailable(
                HostPresentationUnavailableReason::OutputNotProduced,
            )
        };
        frame
            .settle_presentation_obligation(obligation, disposition)
            .expect("native staging physical obligation must resolve");
    }
    complete_surface_contribution_roster(&mut frame);
    let emitted = frame
        .finish(&mut fixture.engine)
        .expect("native staging output must emit");
    let outputs = emitted
        .presentation_emissions()
        .iter()
        .filter_map(|emission| {
            let output = emission.output();
            matches!(
                output.payload(),
                HostPresentationOutputPayload::NativeStaging {
                    presentation: actual,
                } if actual == presentation
            )
            .then_some(output)
        })
        .collect::<Vec<_>>();
    let [output] = outputs.as_slice() else {
        panic!("one exact native staging output must emit, got {outputs:?}");
    };
    assert_eq!(
        output.continuation(),
        crate::presentation_observation::HostPresentationContinuation::Presented,
        "native staging advances only after successful presentation",
    );
    *output
}

#[test]
fn product_runtime_exposes_and_settles_exact_native_staging_outputs() {
    let mut policy = DockPolicy::default();
    policy.set_allow_native_surfaces(true);
    let mut fixture = background_fixture(policy);
    let request = install_pending_native_root_reservation(&mut fixture, RootId::new(194));
    let create_effects = fixture.engine.viewport.take_new_effects();
    assert!(create_effects.iter().any(|effect| {
        effect.id() == request.effect()
            && matches!(
                effect.effect(),
                crate::effect::PlatformEffect::CreateWindow { binding, .. }
                    if *binding == request.binding()
            )
    }));
    publish_background_native_window(&mut fixture, request, WindowPresentationState::Hidden, 2);
    let expected = current_native_staging_presentation(
        &fixture,
        request,
        crate::presentation_observation::NativeStagingPresentationPhase::PreShow,
    );

    let mut session =
        DockspaceSession::from_engine_for_test(fixture.engine, fixture.presentation_host);
    let mut frame = session
        .begin_host_frame()
        .expect("runtime staging frame begins");
    let paints = frame.native_staging_paints();
    assert_eq!(paints.len(), 1, "expected {expected:?}");
    let paint = paints[0];
    assert_eq!(paint.surface(), request.binding().surface());
    assert_eq!(paint.binding().surface(), request.binding().surface());
    assert_eq!(
        paint.phase(),
        crate::runtime::NativeStagingPresentationPhase::PreShow
    );
    frame
        .confirm_native_staging_painted(paint)
        .expect("the exact staging request is painted once");
    frame
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .expect("semantic surfaces are explicitly deferred");
    let mut report = frame.commit().expect("runtime staging frame commits");
    assert!(report.take_painted_outputs().is_empty());
    let mut staging = report.take_painted_native_staging_outputs();
    assert_eq!(staging.len(), 1);
    let output = staging.pop().expect("one staging output was emitted");
    assert_eq!(output.surface(), paint.surface());
    assert_eq!(output.binding(), paint.binding());
    assert_eq!(output.phase(), paint.phase());
    session
        .report_native_staging_presentation(
            output,
            crate::runtime::SurfacePresentationResult::Presented,
        )
        .expect("the exact staging output is retained for observation");

    let mut observed = session
        .begin_host_frame()
        .expect("staging presentation observation frame begins");
    observed
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .expect("the observation frame settles semantic surfaces");
    let mut observed_report = observed
        .commit()
        .expect("presented staging output advances native bring-up");
    let effects = observed_report.take_native_effects();
    assert_eq!(effects.len(), 1);
    let effect = effects
        .into_iter()
        .next()
        .expect("pre-show presentation emits one show request");
    assert_eq!(
        effect.operation(),
        &crate::runtime::NativeEffectOperation::ShowWindow {
            binding: paint.binding(),
        }
    );
    assert!(matches!(
        effect.accepted(),
        Some(crate::runtime::NativeEffectAcknowledgement::Presentation(_))
    ));
}

fn observe_background_presentation_output(
    fixture: &mut BackgroundFixture,
    output: crate::presentation_observation::HostPresentationOutput,
) -> EngineTransition {
    let mut prelude = fixture
        .engine
        .begin_host_frame(fixture.presentation_host)
        .expect("native staging observation frame must begin");
    prelude
        .submit_presentation_observation(HostPresentationObservation::Batch(vec![
            HostPresentationObservationEntry::new(
                output.stream(),
                HostPresentationStreamObservation::Captured {
                    generation: HostPresentationCaptureGeneration::new(
                        output.key().ordinal_for_test(),
                    ),
                    progress: HostPresentationProgress::Retired {
                        settled_through: output.key(),
                        presented: Authority::Known(Some(output.key())),
                    },
                },
            ),
        ]))
        .expect("native staging observation must submit");
    let mut frame = prelude
        .seal(&fixture.engine)
        .expect("native staging observation frame must seal");
    complete_host_frame_with_explicit_surface_roster(&fixture.engine, &mut frame);
    frame
        .finish(&mut fixture.engine)
        .expect("native staging observation must reduce")
}

fn present_background_native_staging(
    fixture: &mut BackgroundFixture,
    presentation: NativeStagingPresentation,
) -> EngineTransition {
    let output = emit_background_native_staging(fixture, presentation);
    observe_background_presentation_output(fixture, output)
}

fn advance_pending_native_root_to_awaiting_visible(
    fixture: &mut BackgroundFixture,
    request: crate::frame::NativeCreateRequest,
) {
    let hidden =
        publish_background_native_window(fixture, request, WindowPresentationState::Hidden, 2);
    assert!(hidden.platform_effects().iter().all(|effect| {
        !matches!(
            effect.effect(),
            crate::effect::PlatformEffect::ShowWindow { binding, .. }
                if *binding == request.binding()
        )
    }));
    let pre_show = current_native_staging_presentation(
        fixture,
        request,
        crate::presentation_observation::NativeStagingPresentationPhase::PreShow,
    );
    let pre_show_presented = present_background_native_staging(fixture, pre_show);
    assert!(pre_show_presented.platform_effects().iter().any(|effect| {
        matches!(
            effect.effect(),
            crate::effect::PlatformEffect::ShowWindow { binding, .. }
                if *binding == request.binding()
        )
    }));
    let acknowledged =
        publish_background_native_window(fixture, request, WindowPresentationState::Hidden, 3);
    assert!(acknowledged.platform_effects().is_empty());
}

fn advance_pending_native_root_to_ownership_transfer(
    fixture: &mut BackgroundFixture,
    request: crate::frame::NativeCreateRequest,
) -> EngineTransition {
    advance_pending_native_root_to_awaiting_visible(fixture, request);
    let visible =
        publish_background_native_window(fixture, request, WindowPresentationState::Visible, 4);
    assert!(
        fixture
            .engine
            .workspace()
            .surface(request.binding().surface())
            .is_none()
    );
    let post_show = current_native_staging_presentation(
        fixture,
        request,
        crate::presentation_observation::NativeStagingPresentationPhase::PostShow,
    );
    let transferred = present_background_native_staging(fixture, post_show);
    assert!(visible.platform_effects().iter().all(|effect| {
        !matches!(
            effect.effect(),
            PlatformEffect::RequestFocus { binding, .. } if *binding == request.binding()
        )
    }));
    transferred
}

fn advance_pending_native_root_to_first_live(
    fixture: &mut BackgroundFixture,
    request: crate::frame::NativeCreateRequest,
) -> EngineTransition {
    let _ = advance_pending_native_root_to_ownership_transfer(fixture, request);
    let surface = request.binding().surface();
    let bounds =
        LogicalRect::new(0.0, 0.0, 300.0, 220.0).expect("native target test bounds must be valid");
    let measurements = surface_measurements(&fixture.engine, surface, bounds);
    let (_, output, transition) = publish_surface_projection_with_output(
        &mut fixture.engine,
        fixture.presentation_host,
        surface,
        measurements,
    );
    assert_eq!(
        output.continuation(),
        crate::presentation_observation::HostPresentationContinuation::Presented,
        "the exact first-live output must wake only after successful presentation",
    );
    transition
}

#[test]
fn auto_focused_native_window_waits_for_first_live_before_pane_focus_is_admitted() {
    const NATIVE_SURFACE: SurfaceId = SurfaceId::new(93);
    let mut policy = DockPolicy::default();
    policy.set_allow_native_surfaces(true);
    let mut fixture = background_fixture(policy);
    let request = install_pending_native_root_reservation(&mut fixture, RootId::new(94));
    let prepared = fixture
        .engine
        .viewport()
        .native_create_saga(request.saga())
        .expect("pending native create must retain its release-time focus claim")
        .prepared()
        .clone();
    assert!(fixture.engine.viewport_focus.reserve_activation_causal(
        request.saga(),
        prepared.focus_causal(),
        ViewportActivationRequest::tear_off_committed(
            request.binding(),
            PaneFocusDisposition::Clear,
        ),
    ));

    let _ = fixture.engine.viewport.take_new_effects();
    advance_pending_native_root_to_awaiting_visible(&mut fixture, request);
    let snapshot = background_native_window_snapshot_with_focus(
        &mut fixture,
        request,
        WindowPresentationState::Visible,
        4,
        true,
        GlobalFocusedWindow::Dock(request.binding()),
    );
    let provider = test_platform_provider(&mut fixture.engine);
    let expected_epoch = fixture.engine.version().epoch();
    let visible = submit_test_input(
        &mut fixture.engine,
        fixture.presentation_host,
        EngineInput::PublishPlatformSnapshot {
            provider,
            expected_epoch,
            snapshot,
        },
    )
    .expect("an exact auto-focused staging window must not deadlock admission");
    assert!(fixture.engine.workspace().surface(NATIVE_SURFACE).is_none());
    assert_eq!(
        fixture.engine.viewport_focus().published_pane_intent(),
        None,
        "staging focus must not publish pane focus before first-live admission"
    );
    let post_show = current_native_staging_presentation(
        &fixture,
        request,
        crate::presentation_observation::NativeStagingPresentationPhase::PostShow,
    );
    let _ = present_background_native_staging(&mut fixture, post_show);
    assert_eq!(
        fixture
            .engine
            .viewport()
            .viewport(NATIVE_SURFACE)
            .map(crate::viewport_registry::ViewportRecord::admission),
        Some(crate::viewport_registry::ViewportAdmission::Pending)
    );
    let bounds =
        LogicalRect::new(0.0, 0.0, 300.0, 220.0).expect("native target test bounds must be valid");
    let measurements = surface_measurements(&fixture.engine, NATIVE_SURFACE, bounds);
    let (_, transition) = publish_surface_projection_with_transition(
        &mut fixture.engine,
        fixture.presentation_host,
        NATIVE_SURFACE,
        measurements,
    );

    assert!(fixture.engine.workspace().surface(NATIVE_SURFACE).is_some());
    assert_eq!(
        fixture
            .engine
            .viewport()
            .viewport(NATIVE_SURFACE)
            .map(crate::viewport_registry::ViewportRecord::admission),
        Some(crate::viewport_registry::ViewportAdmission::Admitted)
    );
    let intent = fixture
        .engine
        .viewport_focus()
        .pending_pane_intent()
        .expect("first-live admission must install the release-time pane claim");
    assert_eq!(intent.surface(), request.binding().surface());
    assert_eq!(intent.native_guard(), Some(request.binding()));
    assert_eq!(intent.causal(), prepared.focus_causal());
    assert_eq!(intent.focus(), PanelFocus::None);
    assert_eq!(
        fixture.engine.viewport_focus().published_pane_intent(),
        Some(intent)
    );
    assert!(visible.platform_effects().iter().all(|effect| {
        !matches!(
            effect.effect(),
            PlatformEffect::RequestFocus { binding, .. } if *binding == request.binding()
        )
    }));
    assert!(transition.platform_effects().iter().all(|effect| {
        !matches!(
            effect.effect(),
            PlatformEffect::RequestFocus { binding, .. } if *binding == request.binding()
        )
    }));
}

#[test]
fn native_focus_reservation_replays_after_activation_control_becomes_available() {
    let mut policy = DockPolicy::default();
    policy.set_allow_native_surfaces(true);
    let mut fixture = background_fixture(policy);
    let request = install_pending_native_root_reservation(&mut fixture, RootId::new(97));
    let prepared = fixture
        .engine
        .viewport()
        .native_create_saga(request.saga())
        .expect("pending native create must retain its release-time focus claim")
        .prepared()
        .clone();
    assert!(fixture.engine.viewport_focus.reserve_activation_causal(
        request.saga(),
        prepared.focus_causal(),
        ViewportActivationRequest::tear_off_committed(
            request.binding(),
            PaneFocusDisposition::Clear,
        ),
    ));

    let _ = fixture.engine.viewport.take_new_effects();
    advance_pending_native_root_to_awaiting_visible(&mut fixture, request);
    let visible = publish_background_native_window(
        &mut fixture,
        request,
        WindowPresentationState::Visible,
        4,
    );
    assert!(visible.platform_effects().iter().all(|effect| {
        !matches!(
            effect.effect(),
            PlatformEffect::RequestFocus { binding, .. } if *binding == request.binding()
        )
    }));
    let post_show = current_native_staging_presentation(
        &fixture,
        request,
        crate::presentation_observation::NativeStagingPresentationPhase::PostShow,
    );
    let transferred = present_background_native_staging(&mut fixture, post_show);
    assert!(transferred.platform_effects().iter().all(|effect| {
        !matches!(
            effect.effect(),
            PlatformEffect::RequestFocus { binding, .. } if *binding == request.binding()
        )
    }));
    let bounds =
        LogicalRect::new(0.0, 0.0, 300.0, 220.0).expect("native target test bounds must be valid");
    let measurements = surface_measurements(&fixture.engine, request.binding().surface(), bounds);
    let (_, first_live) = publish_surface_projection_with_transition(
        &mut fixture.engine,
        fixture.presentation_host,
        request.binding().surface(),
        measurements,
    );
    assert!(first_live.platform_effects().iter().all(|effect| {
        !matches!(
            effect.effect(),
            PlatformEffect::RequestFocus { binding, .. } if *binding == request.binding()
        )
    }));
    assert_eq!(
        fixture
            .engine
            .viewport_focus
            .winning_activation_reservation()
            .map(|(owner, _, _)| owner),
        Some(request.saga())
    );

    let snapshot = background_native_window_snapshot(
        &mut fixture,
        request,
        WindowPresentationState::Visible,
        5,
        true,
    );
    let provider = test_platform_provider(&mut fixture.engine);
    let expected_epoch = fixture.engine.version().epoch();
    let replayed = submit_test_input(
        &mut fixture.engine,
        fixture.presentation_host,
        EngineInput::PublishPlatformSnapshot {
            provider,
            expected_epoch,
            snapshot,
        },
    )
    .expect("the authoritative capability update must replay the reservation");
    assert!(replayed.platform_effects().iter().any(|effect| {
        matches!(
            effect.effect(),
            PlatformEffect::RequestFocus { binding, .. } if *binding == request.binding()
        )
    }));
    assert!(
        fixture
            .engine
            .viewport_focus
            .winning_activation_reservation()
            .is_none()
    );
}

#[test]
fn delayed_auto_focus_from_a_superseded_native_create_reasserts_the_current_winner() {
    let mut policy = DockPolicy::default();
    policy.set_allow_native_surfaces(true);
    let mut fixture = background_fixture(policy);
    let request = install_pending_native_root_reservation(&mut fixture, RootId::new(95));
    let prepared = fixture
        .engine
        .viewport()
        .native_create_saga(request.saga())
        .expect("pending native create must retain its release-time focus claim")
        .prepared()
        .clone();
    assert!(fixture.engine.viewport_focus.reserve_activation_causal(
        request.saga(),
        prepared.focus_causal(),
        ViewportActivationRequest::tear_off_committed(
            request.binding(),
            PaneFocusDisposition::Clear,
        ),
    ));

    let _ = fixture.engine.viewport.take_new_effects();
    advance_pending_native_root_to_awaiting_visible(&mut fixture, request);
    let source_binding = background_source_binding(&fixture);
    let winner_causal = mint_test_focus_causal(&mut fixture.engine);
    let winner = fixture
        .engine
        .start_viewport_activation(
            ViewportActivationRequest::pointer_tab_gesture(
                source_binding,
                PanelFocus::Item(ItemId::new(2)),
            ),
            winner_causal,
            &mut Vec::new(),
        )
        .expect("the newer source-window gesture must become the focus winner");
    assert!(
        matches!(
            winner.outcome(),
            ActivationStartOutcome::ObserveOnlyRecorded { .. }
        ),
        "unexpected winner activation: {:?}",
        winner.outcome()
    );
    let visible = background_native_window_snapshot_with_focus(
        &mut fixture,
        request,
        WindowPresentationState::Visible,
        4,
        true,
        GlobalFocusedWindow::Foreign,
    );
    let provider = test_platform_provider(&mut fixture.engine);
    let expected_epoch = fixture.engine.version().epoch();
    let visible = submit_test_input(
        &mut fixture.engine,
        fixture.presentation_host,
        EngineInput::PublishPlatformSnapshot {
            provider,
            expected_epoch,
            snapshot: visible,
        },
    )
    .expect("the superseded native create must still commit its topology");
    assert!(
        visible
            .platform_effects()
            .iter()
            .all(|effect| { !matches!(effect.effect(), PlatformEffect::RequestFocus { .. }) })
    );
    let post_show = current_native_staging_presentation(
        &fixture,
        request,
        crate::presentation_observation::NativeStagingPresentationPhase::PostShow,
    );
    let _ = present_background_native_staging(&mut fixture, post_show);
    let bounds =
        LogicalRect::new(0.0, 0.0, 300.0, 220.0).expect("native target test bounds must be valid");
    let measurements = surface_measurements(&fixture.engine, request.binding().surface(), bounds);
    let (_, first_live) = publish_surface_projection_with_transition(
        &mut fixture.engine,
        fixture.presentation_host,
        request.binding().surface(),
        measurements,
    );
    assert!(first_live.platform_effects().iter().any(|effect| {
        matches!(
            effect.effect(),
            PlatformEffect::RequestFocus { binding, .. } if *binding == source_binding
        )
    }));
    assert!(
        fixture
            .engine
            .viewport_focus
            .suppressed_tear_off_bindings()
            .contains(&request.binding())
    );

    let delayed_auto_focus = background_native_window_snapshot_with_focus(
        &mut fixture,
        request,
        WindowPresentationState::Visible,
        5,
        true,
        GlobalFocusedWindow::Dock(request.binding()),
    );
    let provider = test_platform_provider(&mut fixture.engine);
    let expected_epoch = fixture.engine.version().epoch();
    let delayed = submit_test_input(
        &mut fixture.engine,
        fixture.presentation_host,
        EngineInput::PublishPlatformSnapshot {
            provider,
            expected_epoch,
            snapshot: delayed_auto_focus,
        },
    )
    .expect("the delayed auto-focus edge must reduce atomically");
    assert!(
        delayed
            .platform_effects()
            .iter()
            .all(|effect| { !matches!(effect.effect(), PlatformEffect::RequestFocus { .. }) })
    );
    assert_eq!(
        fixture
            .engine
            .viewport_focus()
            .pending_activation()
            .map(crate::viewport_focus::PendingViewportActivation::request),
        Some(ViewportActivationRequest::pointer_tab_gesture(
            source_binding,
            PanelFocus::Item(ItemId::new(2)),
        ))
    );
    assert!(
        fixture
            .engine
            .viewport_focus
            .suppressed_tear_off_bindings()
            .contains(&request.binding()),
        "the old binding must remain isolated until the exact focus barrier settles"
    );

    let winner_observed = background_native_window_snapshot_with_focus(
        &mut fixture,
        request,
        WindowPresentationState::Visible,
        6,
        true,
        GlobalFocusedWindow::Dock(source_binding),
    );
    let provider = test_platform_provider(&mut fixture.engine);
    let expected_epoch = fixture.engine.version().epoch();
    submit_test_input(
        &mut fixture.engine,
        fixture.presentation_host,
        EngineInput::PublishPlatformSnapshot {
            provider,
            expected_epoch,
            snapshot: winner_observed,
        },
    )
    .expect("the compensating focus observation must settle the winning activation");
    assert!(
        fixture
            .engine
            .viewport_focus
            .suppressed_tear_off_bindings()
            .is_empty(),
        "the exact winner observation must retire the lifecycle quarantine"
    );

    let later_user_focus = background_native_window_snapshot_with_focus(
        &mut fixture,
        request,
        WindowPresentationState::Visible,
        7,
        true,
        GlobalFocusedWindow::Dock(request.binding()),
    );
    let provider = test_platform_provider(&mut fixture.engine);
    let expected_epoch = fixture.engine.version().epoch();
    submit_test_input(
        &mut fixture.engine,
        fixture.presentation_host,
        EngineInput::PublishPlatformSnapshot {
            provider,
            expected_epoch,
            snapshot: later_user_focus,
        },
    )
    .expect("a later focus edge must not remain permanently quarantined");
    assert_eq!(
        fixture.engine.viewport_focus().winning_focus_target(),
        Some(request.binding())
    );
}

#[test]
fn tick_final_vacancy_suppresses_focus_for_a_newly_visible_native_binding() {
    const NATIVE_SURFACE: SurfaceId = SurfaceId::new(93);
    const DESTINATION_FLOATING: FloatingPresentationId = FloatingPresentationId::new(94);
    const COMMAND_SOURCE: StableInputSourceId = StableInputSourceId::new(97);

    let mut policy = DockPolicy::default();
    policy.set_allow_native_surfaces(true);
    let mut fixture = background_fixture(policy);
    let reserved_root = RootId::new(94);
    let request = install_pending_native_root_reservation(&mut fixture, reserved_root);
    let prepared = fixture
        .engine
        .viewport()
        .native_create_saga(request.saga())
        .expect("pending native create must retain its prepared command")
        .prepared()
        .clone();
    let mut after_native_commit = fixture.engine.workspace().clone();
    WorkspaceTransaction::from_commands([prepared.command().clone()])
        .apply(&mut after_native_commit, fixture.engine.policy_snapshot())
        .expect("prepared native command must remain applicable");
    let source = after_native_commit
        .root(reserved_root)
        .and_then(|root| {
            after_native_commit
                .capture_node_source(reserved_root, root.node)
                .ok()
        })
        .expect("prepared native root must remain rehomeable");
    let expected_after_native_commit = WorkspaceVersion::new(
        fixture.engine.version().epoch(),
        fixture
            .engine
            .version()
            .revision()
            .checked_next()
            .expect("fixture revision can advance once"),
    );

    let _ = fixture.engine.viewport.take_new_effects();
    advance_pending_native_root_to_awaiting_visible(&mut fixture, request);
    let snapshot = background_native_window_snapshot(
        &mut fixture,
        request,
        WindowPresentationState::Visible,
        4,
        true,
    );
    let provider = test_platform_provider(&mut fixture.engine);
    let expected_epoch = fixture.engine.version().epoch();
    let visible = submit_test_input(
        &mut fixture.engine,
        fixture.presentation_host,
        EngineInput::PublishPlatformSnapshot {
            provider,
            expected_epoch,
            snapshot,
        },
    )
    .expect("visible native observation must reduce before post-show presentation");
    assert!(
        visible
            .platform_effects()
            .iter()
            .all(|effect| { !matches!(effect.effect(), PlatformEffect::RequestFocus { .. }) })
    );
    let post_show = current_native_staging_presentation(
        &fixture,
        request,
        crate::presentation_observation::NativeStagingPresentationPhase::PostShow,
    );
    let post_show_output = emit_background_native_staging(&mut fixture, post_show);

    let mut prelude = fixture
        .engine
        .begin_host_frame(fixture.presentation_host)
        .expect("post-show settlement frame must begin");
    prelude
        .submit_presentation_observation(HostPresentationObservation::Batch(vec![
            HostPresentationObservationEntry::new(
                post_show_output.stream(),
                HostPresentationStreamObservation::Captured {
                    generation: HostPresentationCaptureGeneration::new(
                        post_show_output.key().ordinal_for_test(),
                    ),
                    progress: HostPresentationProgress::Retired {
                        settled_through: post_show_output.key(),
                        presented: Authority::Known(Some(post_show_output.key())),
                    },
                },
            ),
        ]))
        .expect("post-show presentation proof must submit");
    let mut frame = prelude
        .seal(&fixture.engine)
        .expect("post-show proof must transfer before semantic input");
    let mut semantic_writer = TestInputStream::resume(&fixture.engine, COMMAND_SOURCE);
    semantic_writer
        .append(
            &mut frame,
            EngineInput::WorkspaceCommand {
                expected: expected_after_native_commit,
                command: WorkspaceCommand::RehomeRoot {
                    source,
                    target: RootPresentationTarget::Contained {
                        surface: TARGET_SURFACE,
                        floating: DESTINATION_FLOATING,
                        rect: background_contained_rect(),
                        position: ContainedPosition::Front,
                    },
                },
            },
        )
        .expect("vacating command belongs to the semantic phase");
    complete_host_frame_with_explicit_surface_roster(&fixture.engine, &mut frame);
    let transition = frame
        .finish(&mut fixture.engine)
        .expect("visible admission and final vacancy must commit atomically");

    assert!(fixture.engine.workspace().surface(NATIVE_SURFACE).is_none());
    assert!(fixture.engine.viewport().viewport(NATIVE_SURFACE).is_none());
    assert!(transition.platform_effects().iter().any(|effect| {
        matches!(
            effect.effect(),
            PlatformEffect::ReleaseChild { binding } if *binding == request.binding()
        )
    }));
    assert!(transition.platform_effects().iter().all(|effect| {
        !matches!(
            effect.effect(),
            PlatformEffect::RequestFocus { binding, .. } if *binding == request.binding()
        )
    }));
    assert!(
        fixture
            .engine
            .viewport_focus()
            .pending_activation()
            .is_none(),
        "the final vacancy must clear the native activation before focus effect emission"
    );
}

#[test]
fn tick_final_runtime_child_vacancy_emits_one_release_before_effect_extraction() {
    const NATIVE_SURFACE: SurfaceId = SurfaceId::new(93);
    const DESTINATION_FLOATING: FloatingPresentationId = FloatingPresentationId::new(94);
    const INPUT_SOURCE: StableInputSourceId = StableInputSourceId::new(94);

    let mut policy = DockPolicy::default();
    policy.set_allow_native_surfaces(true);
    let mut fixture = background_fixture(policy);
    let reserved_root = RootId::new(94);
    let request = install_pending_native_root_reservation(&mut fixture, reserved_root);
    let create_effects = fixture.engine.viewport.take_new_effects();
    assert!(create_effects.iter().any(|effect| {
        effect.id() == request.effect()
            && matches!(
                effect.effect(),
                crate::effect::PlatformEffect::CreateWindow { binding, .. }
                    if *binding == request.binding()
            )
    }));

    let _ = advance_pending_native_root_to_first_live(&mut fixture, request);
    assert!(fixture.engine.workspace().surface(NATIVE_SURFACE).is_some());
    assert!(
        fixture
            .engine
            .viewport()
            .native_create_saga(request.saga())
            .is_none(),
        "commit must consume the active saga"
    );
    assert!(!fixture.engine.native_create_reserves_root(reserved_root));
    assert!(
        !fixture
            .engine
            .native_create_reserves_floating(FloatingPresentationId::new(93))
    );
    assert!(
        fixture
            .engine
            .bound_surface_recoveries
            .get(&NATIVE_SURFACE)
            .is_some_and(|bound| bound.binding == request.binding())
    );

    let root = fixture
        .engine
        .workspace()
        .root(reserved_root)
        .expect("reserved root was installed");
    let source = fixture
        .engine
        .workspace()
        .capture_node_source(reserved_root, root.node)
        .expect("reserved root remains capturable");
    let expected = fixture.engine.version();
    let mut frame = begin_test_host_frame(&fixture.engine, fixture.presentation_host);
    let mut semantic_writer = TestInputStream::resume(&fixture.engine, INPUT_SOURCE);
    semantic_writer
        .append(
            &mut frame,
            EngineInput::WorkspaceCommand {
                expected,
                command: WorkspaceCommand::RehomeRoot {
                    source,
                    target: RootPresentationTarget::Contained {
                        surface: TARGET_SURFACE,
                        floating: DESTINATION_FLOATING,
                        rect: background_contained_rect(),
                        position: ContainedPosition::Front,
                    },
                },
            },
        )
        .expect("test input must fit the semantic host-frame phase");
    complete_host_frame_with_explicit_surface_roster(&fixture.engine, &mut frame);
    let vacated = frame
        .finish(&mut fixture.engine)
        .expect("runtime-owned surface vacancy commits");

    assert!(fixture.engine.workspace().surface(NATIVE_SURFACE).is_none());
    let releases = vacated
        .platform_effects()
        .iter()
        .filter(|effect| {
            matches!(
                effect.effect(),
                crate::effect::PlatformEffect::ReleaseChild { binding }
                    if *binding == request.binding()
            )
        })
        .count();
    assert_eq!(releases, 1);
    assert!(fixture.engine.viewport().viewport(NATIVE_SURFACE).is_none());
    assert!(
        fixture
            .engine
            .viewport()
            .binding_retirements()
            .any(|(binding, retirement)| binding == request.binding()
                && matches!(
                    retirement.status(),
                    crate::frame::BindingRetirementStatus::CleanupRequested { .. }
                ))
    );

    let mut next_frame = begin_test_host_frame(&fixture.engine, fixture.presentation_host);
    complete_host_frame_with_explicit_surface_roster(&fixture.engine, &mut next_frame);
    let next = next_frame
        .finish(&mut fixture.engine)
        .expect("the next empty tick commits");
    assert!(next.platform_effects().iter().all(|effect| {
        !matches!(
            effect.effect(),
            crate::effect::PlatformEffect::ReleaseChild { binding }
                if *binding == request.binding()
        )
    }));
}

#[test]
fn product_native_root_to_contained_waits_for_presented_target_before_transfer() {
    const NATIVE_SURFACE: SurfaceId = SurfaceId::new(93);

    let mut policy = DockPolicy::default();
    policy.set_allow_native_surfaces(true);
    let mut fixture = background_fixture(policy);
    let root = RootId::new(94);
    let request = install_pending_native_root_reservation(&mut fixture, root);
    let _ = fixture.engine.viewport.take_new_effects();
    let _ = advance_pending_native_root_to_first_live(&mut fixture, request);
    let target_measurements =
        surface_measurements(&fixture.engine, TARGET_SURFACE, background_bounds());
    let _ = publish_surface_projection_with_output(
        &mut fixture.engine,
        fixture.presentation_host,
        TARGET_SURFACE,
        target_measurements,
    );
    let _ = fixture.engine.viewport.take_new_effects();
    let target_projection = fixture
        .engine
        .interaction_projection(TARGET_SURFACE)
        .expect("the target surface has exact local response authority");
    let target_scene = target_projection.plan_stamp();
    let target_tab = *target_projection
        .plan()
        .tab_records()
        .iter()
        .find(|record| record.id().item == ItemId::new(3))
        .expect("the background contained tab remains available")
        .id();
    let target_region = target_projection
        .hit_manifest()
        .regions()
        .iter()
        .find(|region| {
            matches!(
                region.id().kind(),
                PresentationHitRegionKind::TabBody(tab) if tab.item == ItemId::new(3)
            )
        })
        .expect("the target tab has an exact pointer receiver")
        .id();
    let target_point = region_center(
        target_projection
            .hit_manifest()
            .region(target_region)
            .expect("the target tab receiver remains present"),
    );
    let source_version = fixture.engine.version();

    let staged = submit_test_batch(
        &mut fixture.engine,
        fixture.presentation_host,
        [
            EngineInput::FloatRoot {
                expected: source_version,
                root,
                surface: TARGET_SURFACE,
                rect: None,
            },
            EngineInput::LocalTabGesture {
                expected: source_version,
                surface: TARGET_SURFACE,
                source: TabGestureSource::Item(target_tab),
                phase: LocalTabGesturePhase::Begin {
                    scene: target_scene,
                    initial: target_point,
                    current: target_point,
                },
            },
            EngineInput::LocalTabGesture {
                expected: source_version,
                surface: TARGET_SURFACE,
                source: TabGestureSource::Item(target_tab),
                phase: LocalTabGesturePhase::Release {
                    scene: target_scene,
                    current: target_point,
                },
            },
        ],
    )
    .expect("native-to-contained product action stages");

    assert!(
        matches!(
            staged.reduced_inputs()[0].outcome(),
            InputOutcome::ProductActionProcessed {
                outcome: DockspaceActionOutcome::RootFloatRequested {
                    root: actual_root,
                    source_surface: NATIVE_SURFACE,
                    target_surface: TARGET_SURFACE,
                    items,
                },
                version,
            } if *actual_root == root && items == &[ItemId::new(1)] && *version == source_version
        ),
        "actual staged outcome: {:?}",
        staged.reduced_inputs()[0].outcome()
    );
    assert_eq!(fixture.engine.version(), source_version);
    assert!(matches!(
        staged.reduced_inputs()[1].outcome(),
        InputOutcome::InteractionProcessed {
            outcome: InteractionOutcome::Rejected(
                InteractionRejection::PresentationTransitionPending
            ),
            ..
        }
    ));
    assert!(matches!(
        staged.reduced_inputs()[2].outcome(),
        InputOutcome::InteractionProcessed {
            outcome: InteractionOutcome::Rejected(InteractionRejection::NoActiveGesture),
            ..
        }
    ));
    assert_eq!(
        fixture.engine.workspace().presentation_for_root(root),
        Some(crate::RootPresentationOwner::Main {
            surface: NATIVE_SURFACE,
        }),
        "staging must preserve native source ownership"
    );
    assert_eq!(
        fixture
            .engine
            .presentation_preview()
            .map(|preview| preview.visual().surface()),
        Some(TARGET_SURFACE),
        "the target surface must receive one exact contained staging preview"
    );
    assert!(staged.platform_effects().iter().all(|effect| {
        !matches!(
            effect.effect(),
            PlatformEffect::ReleaseChild { binding } if *binding == request.binding()
        )
    }));

    let pending = fixture
        .engine
        .pending_presentation_rehome
        .as_ref()
        .expect("the target presentation remains pending");
    assert_eq!(pending.source_binding, request.binding());
    assert_eq!(
        pending.source_presentation.binding(),
        Some(request.binding())
    );
    assert_ne!(
        fixture
            .engine
            .prepare_presentation_floating_identity()
            .expect("a later floating identity remains available"),
        pending
            .floating
            .expect("contained rehome retains its floating identity"),
        "a pending presentation rehome reserves its floating identity"
    );
    assert!(
        fixture
            .engine
            .retained_presentation_emissions()
            .contains(&pending.source_presentation.emission()),
        "the exact source presentation remains retained until target settlement"
    );
    let pending_before = pending.clone();
    let pending_cause = pending.cause;
    let source_owner_before = fixture.engine.workspace().presentation_for_root(root);
    let version_before_press = fixture.engine.version();
    let provider = fixture
        .engine
        .create_surface_local_pointer_provider(
            SurfaceLocalPointerScope::new(
                fixture.presentation_host,
                SurfaceLocalPointerEndpoint::Logical(TARGET_SURFACE),
            ),
            PointerEdgeSequence::new(0),
        )
        .expect("the target surface pointer provider mints");
    let mut press = begin_test_host_frame(&fixture.engine, fixture.presentation_host);
    press
        .submit_surface_pointer_journal(
            &provider,
            local_pointer_journal(
                0,
                [(
                    PointerEdgeKind::ButtonPressed(PointerButton::Primary),
                    target_point,
                )],
            ),
        )
        .expect("the unrelated primary press stages");
    let projection = press
        .view()
        .interaction_projection(TARGET_SURFACE)
        .expect("the sealed frame retains target receiver authority");
    let candidate = press
        .pointer_receiver_candidates()
        .expect("the primary press freezes a receiver candidate")
        .candidates()[0]
        .clone();
    let delivery = PointerReceiverDelivery::new(
        projection,
        PointerReceiverDeliveryDisposition::Dock(target_region),
    )
    .expect("the target tab receiver accepts drag delivery");
    press
        .submit_pointer_receiver_receipts(
            PointerReceiverReceiptBatch::new([candidate.receipt(
                PointerReceiverObservation::Presented(
                    PresentedPointerReceiverObservation::new([
                        PointerReceiverProbeReceipt::Delivery(delivery),
                    ])
                    .expect("the press receipt answers its delivery probe"),
                ),
            )])
            .expect("the primary press receipt batch is exact"),
        )
        .expect("the primary press receipt stages");
    complete_host_frame_with_explicit_surface_roster(&fixture.engine, &mut press);
    let press_transition = press
        .finish(&mut fixture.engine)
        .expect("the unrelated primary press commits");
    assert!(matches!(
        press_transition.reduced_pointer_edges()[0].interaction_outcomes(),
        [InteractionOutcome::Rejected(
            InteractionRejection::PresentationTransitionPending
        )]
    ));
    assert_eq!(
        fixture.engine.pending_presentation_rehome.as_ref(),
        Some(&pending_before)
    );
    assert_eq!(fixture.engine.version(), version_before_press);
    assert_eq!(
        fixture.engine.workspace().presentation_for_root(root),
        source_owner_before
    );
    assert!(press_transition.platform_effects().iter().all(|effect| {
        !matches!(
            effect.effect(),
            PlatformEffect::ReleaseChild { binding } if *binding == request.binding()
        )
    }));
    assert!(press_transition.interaction_events().iter().all(|event| {
        !matches!(
            event.kind(),
            InteractionEventKind::Cancelled {
                reason: InteractionCancelReason::ReplacedByNewGesture,
                ..
            }
        )
    }));
    let mut drain = provider
        .drain()
        .expect("the committed surface-local producer drains");
    let retirement = fixture
        .engine
        .retire_quiesced_surface_local_pointer_provider(&mut drain)
        .expect("the drained surface-local producer retires");
    assert!(!retirement.interaction_changed());
    assert!(retirement.repaint_required());
    assert_eq!(
        fixture.engine.pending_presentation_rehome.as_ref(),
        Some(&pending_before)
    );
    let mut unrelated_interaction_events = Vec::new();
    assert!(
        fixture
            .engine
            .cancel_pending_release_obligations_caused(
                pending_cause,
                InteractionCancelReason::ReplacedByNewGesture,
                &mut unrelated_interaction_events,
            )
            .is_empty(),
        "a programmatic rehome is not a pointer release obligation"
    );
    assert!(fixture.engine.pending_presentation_rehome.is_some());
    assert!(unrelated_interaction_events.is_empty());

    let measurements = surface_measurements(&fixture.engine, TARGET_SURFACE, background_bounds());
    let (_, _, transferred) = publish_surface_projection_with_output(
        &mut fixture.engine,
        fixture.presentation_host,
        TARGET_SURFACE,
        measurements,
    );

    assert!(matches!(
        fixture.engine.workspace().presentation_for_root(root),
        Some(crate::RootPresentationOwner::Contained {
            surface: TARGET_SURFACE,
            ..
        })
    ));
    assert!(fixture.engine.workspace().surface(NATIVE_SURFACE).is_none());
    assert_eq!(
        fixture.engine.version().revision(),
        source_version
            .revision()
            .checked_next()
            .expect("one ownership transfer advances exactly one revision")
    );
    assert_eq!(
        transferred
            .platform_effects()
            .iter()
            .filter(|effect| matches!(
                effect.effect(),
                PlatformEffect::ReleaseChild { binding } if *binding == request.binding()
            ))
            .count(),
        1,
        "presented target admission retires the native child exactly once"
    );
    assert!(transferred.events().iter().any(|event| matches!(
        event.kind(),
        WorkspaceEventKind::PresentationRehomeSettled {
            root: actual_root,
            source_surface: NATIVE_SURFACE,
            target_surface: TARGET_SURFACE,
            result: crate::event::PresentationRehomeResult::Applied,
            outcome: Some(DockspaceActionOutcome::RootFloated { .. }),
        } if *actual_root == root
    )));
}

#[test]
fn product_single_item_native_child_dock_waits_for_target_presentation() {
    const NATIVE_SURFACE: SurfaceId = SurfaceId::new(93);

    let mut policy = DockPolicy::default();
    policy.set_allow_native_surfaces(true);
    let mut fixture = background_fixture(policy);
    let root = RootId::new(94);
    let request = install_pending_native_root_reservation(&mut fixture, root);
    let _ = fixture.engine.viewport.take_new_effects();
    let _ = advance_pending_native_root_to_first_live(&mut fixture, request);
    let target_measurements =
        surface_measurements(&fixture.engine, TARGET_SURFACE, background_bounds());
    let _ = publish_surface_projection_with_output(
        &mut fixture.engine,
        fixture.presentation_host,
        TARGET_SURFACE,
        target_measurements,
    );
    let _ = fixture.engine.viewport.take_new_effects();
    let source_version = fixture.engine.version();

    let staged = submit_test_input(
        &mut fixture.engine,
        fixture.presentation_host,
        EngineInput::DockItem {
            expected: source_version,
            item: ItemId::new(1),
            placement: DockPlacement::Before(ItemId::new(3)),
        },
    )
    .expect("single-item native root dock must stage");

    assert!(
        matches!(
            staged.reduced_inputs()[0].outcome(),
            InputOutcome::ProductActionProcessed {
                outcome: DockspaceActionOutcome::RootDockRequested {
                    root: actual_root,
                    source_surface: NATIVE_SURFACE,
                    target_root: TARGET_ROOT,
                    items,
                },
                version,
            } if *actual_root == root && items == &[ItemId::new(1)] && *version == source_version
        ),
        "actual outcome: {:?}",
        staged.reduced_inputs()[0].outcome()
    );
    assert_eq!(fixture.engine.version(), source_version);
    assert_eq!(
        fixture.engine.workspace().presentation_for_root(root),
        Some(crate::RootPresentationOwner::Main {
            surface: NATIVE_SURFACE,
        })
    );
    assert!(fixture.engine.pending_presentation_rehome.is_some());
    assert!(staged.platform_effects().iter().all(|effect| {
        !matches!(
            effect.effect(),
            PlatformEffect::ReleaseChild { binding } if *binding == request.binding()
        )
    }));
    let pending_before = fixture
        .engine
        .pending_presentation_rehome
        .clone()
        .expect("the dock transition owns its source reservation");
    let PreviewProof::PresentationRehome { command } = pending_before.preview.proof() else {
        panic!("presentation rehome retains its exact frozen command");
    };
    assert!(matches!(
        fixture
            .engine
            .stage_journal_workspace_command(
                tab_strip_test_cause(),
                command,
                fixture.engine.policy_snapshot(),
            )
            .expect("pointer/local command staging reduces"),
        Err(CommandError::SurfaceLifecycleFrozen {
            surface: NATIVE_SURFACE,
        })
    ));
    let public_rejection = submit_test_input(
        &mut fixture.engine,
        fixture.presentation_host,
        EngineInput::WorkspaceCommand {
            expected: source_version,
            command: command.clone(),
        },
    )
    .expect("public command reduces as a typed reservation rejection");
    assert!(matches!(
        public_rejection.reduced_inputs()[0].outcome(),
        InputOutcome::CommandRejected {
            error: CommandError::SurfaceLifecycleFrozen {
                surface: NATIVE_SURFACE,
            },
            version,
        } if *version == source_version
    ));
    assert_eq!(
        fixture.engine.pending_presentation_rehome.as_ref(),
        Some(&pending_before)
    );

    let measurements = surface_measurements(&fixture.engine, TARGET_SURFACE, background_bounds());
    let (_, _, transferred) = publish_surface_projection_with_output(
        &mut fixture.engine,
        fixture.presentation_host,
        TARGET_SURFACE,
        measurements,
    );

    assert!(fixture.engine.workspace().root(root).is_none());
    assert_eq!(
        fixture
            .engine
            .workspace()
            .presentation_for_root(TARGET_ROOT),
        Some(crate::RootPresentationOwner::Contained {
            surface: TARGET_SURFACE,
            floating: FloatingPresentationId::new(2),
        })
    );
    assert_eq!(
        transferred
            .platform_effects()
            .iter()
            .filter(|effect| matches!(
                effect.effect(),
                PlatformEffect::ReleaseChild { binding } if *binding == request.binding()
            ))
            .count(),
        1
    );
}

#[test]
fn product_native_main_dock_back_uses_bound_recovery_after_target_presentation() {
    const NATIVE_SURFACE: SurfaceId = SurfaceId::new(93);

    let mut policy = DockPolicy::default();
    policy.set_allow_native_surfaces(true);
    let mut fixture = background_fixture(policy);
    let root = RootId::new(94);
    let request = install_pending_native_root_reservation(&mut fixture, root);
    let _ = fixture.engine.viewport.take_new_effects();
    let _ = advance_pending_native_root_to_first_live(&mut fixture, request);
    let bound = fixture
        .engine
        .bound_surface_recoveries
        .get(&NATIVE_SURFACE)
        .expect("first-live native surface retains its recovery obligation");
    let target = bound.obligation.target();
    let target_surface = target.host_surface();
    let expected_floating = target
        .converted_main()
        .expect("a native main root reserves converted recovery")
        .floating();
    let target_measurements =
        surface_measurements(&fixture.engine, target_surface, background_bounds());
    let _ = publish_surface_projection_with_output(
        &mut fixture.engine,
        fixture.presentation_host,
        target_surface,
        target_measurements,
    );
    let _ = fixture.engine.viewport.take_new_effects();
    let source_version = fixture.engine.version();

    let staged = submit_test_input(
        &mut fixture.engine,
        fixture.presentation_host,
        EngineInput::DockBackRoot {
            expected: source_version,
            root,
        },
    )
    .expect("native main dock-back must stage against its bound recovery");

    assert!(matches!(
        staged.reduced_inputs()[0].outcome(),
        InputOutcome::ProductActionProcessed {
            outcome: DockspaceActionOutcome::RootFloatRequested {
                root: actual_root,
                source_surface: NATIVE_SURFACE,
                target_surface: actual_target,
                items,
            },
            version,
        } if *actual_root == root
            && *actual_target == target_surface
            && items == &[ItemId::new(1)]
            && *version == source_version
    ));
    assert_eq!(fixture.engine.version(), source_version);
    assert_eq!(
        fixture.engine.workspace().presentation_for_root(root),
        Some(crate::RootPresentationOwner::Main {
            surface: NATIVE_SURFACE,
        })
    );
    let pending = fixture
        .engine
        .pending_presentation_rehome
        .as_ref()
        .expect("dock-back waits for the exact recovery-host presentation");
    assert_eq!(pending.target_surface, target_surface);
    assert_eq!(pending.floating, Some(expected_floating));
    assert_eq!(
        pending.commit_authority,
        PresentationRehomeCommitAuthority::BoundConvertedMain
    );
    assert!(staged.platform_effects().iter().all(|effect| {
        !matches!(
            effect.effect(),
            PlatformEffect::ReleaseChild { binding } if *binding == request.binding()
        )
    }));

    let measurements = surface_measurements(&fixture.engine, target_surface, background_bounds());
    let (_, _, transferred) = publish_surface_projection_with_output(
        &mut fixture.engine,
        fixture.presentation_host,
        target_surface,
        measurements,
    );

    assert_eq!(
        fixture.engine.workspace().presentation_for_root(root),
        Some(crate::RootPresentationOwner::Contained {
            surface: target_surface,
            floating: expected_floating,
        })
    );
    assert!(fixture.engine.workspace().surface(NATIVE_SURFACE).is_none());
    assert_eq!(
        transferred
            .platform_effects()
            .iter()
            .filter(|effect| matches!(
                effect.effect(),
                PlatformEffect::ReleaseChild { binding } if *binding == request.binding()
            ))
            .count(),
        1
    );
    assert!(transferred.events().iter().any(|event| matches!(
        event.kind(),
        WorkspaceEventKind::PresentationRehomeSettled {
            root: actual_root,
            source_surface: NATIVE_SURFACE,
            target_surface: actual_target,
            result: crate::event::PresentationRehomeResult::Applied,
            outcome: Some(DockspaceActionOutcome::RootFloated { .. }),
        } if *actual_root == root && *actual_target == target_surface
    )));
}

#[test]
fn runtime_owned_native_main_dock_back_requires_window_lifecycle_capability() {
    const NATIVE_SURFACE: SurfaceId = SurfaceId::new(93);

    let mut policy = DockPolicy::default();
    policy.set_allow_native_surfaces(true);
    let mut fixture = background_fixture(policy);
    let root = RootId::new(94);
    let request = install_pending_native_root_reservation(&mut fixture, root);
    let _ = fixture.engine.viewport.take_new_effects();
    let _ = advance_pending_native_root_to_first_live(&mut fixture, request);
    let target_surface = fixture
        .engine
        .bound_surface_recoveries
        .get(&NATIVE_SURFACE)
        .expect("first-live native surface retains its recovery obligation")
        .obligation
        .target()
        .host_surface();
    let target_measurements =
        surface_measurements(&fixture.engine, target_surface, background_bounds());
    let _ = publish_surface_projection_with_output(
        &mut fixture.engine,
        fixture.presentation_host,
        target_surface,
        target_measurements,
    );
    assert_eq!(
        fixture
            .engine
            .viewport()
            .viewport(NATIVE_SURFACE)
            .map(crate::viewport_registry::ViewportRecord::ownership),
        Some(ViewportOwnership::RuntimeOwned)
    );

    let mut capabilities = PlatformCapabilities::default();
    capabilities.set_native_window_lifecycle(PlatformCapability::unsupported(
        crate::platform::PlatformRequirement::NativeWindowLifecycle,
        crate::platform::PlatformCapabilityReason::BackendUnsupported,
    ));
    capabilities.set_authoritative_inventory(PlatformCapability::Supported);
    capabilities.set_global_window_placement(PlatformCapability::Supported);
    capabilities.set_work_area(PlatformCapability::Supported);
    capabilities.set_global_focus_observation(PlatformCapability::Supported);
    let snapshot = background_native_window_snapshot_with_capabilities(
        &mut fixture,
        request,
        WindowPresentationState::Visible,
        5,
        GlobalFocusedWindow::Foreign,
        capabilities,
    );
    let provider = test_platform_provider(&mut fixture.engine);
    let expected_epoch = fixture.engine.version().epoch();
    submit_test_input(
        &mut fixture.engine,
        fixture.presentation_host,
        EngineInput::PublishPlatformSnapshot {
            provider,
            expected_epoch,
            snapshot,
        },
    )
    .expect("lifecycle capability downgrade must reduce nonfatally");
    let version = fixture.engine.version();

    let rejected = submit_test_input(
        &mut fixture.engine,
        fixture.presentation_host,
        EngineInput::DockBackRoot {
            expected: version,
            root,
        },
    )
    .expect("runtime-owned dock-back must return a typed rejection");

    assert!(matches!(
        rejected.reduced_inputs()[0].outcome(),
        InputOutcome::ProductActionRejected {
            reason: DockspaceActionRejection::NativeUnavailable,
            version: actual,
        } if *actual == version
    ));
    assert_eq!(fixture.engine.version(), version);
    assert!(fixture.engine.pending_presentation_rehome.is_none());
    assert_eq!(
        fixture.engine.workspace().presentation_for_root(root),
        Some(crate::RootPresentationOwner::Main {
            surface: NATIVE_SURFACE,
        })
    );
}

#[test]
fn root_presentation_transitions_share_one_pending_reservation() {
    let mut policy = DockPolicy::default();
    policy.set_allow_native_surfaces(true);
    let mut creating = background_fixture(policy.clone());
    let root = RootId::new(94);
    let request = install_pending_native_root_reservation(&mut creating, root);
    let _ = creating.engine.viewport.take_new_effects();
    let version = creating.engine.version();

    let conflict = submit_test_input(
        &mut creating.engine,
        creating.presentation_host,
        EngineInput::FloatRoot {
            expected: version,
            root,
            surface: TARGET_SURFACE,
            rect: None,
        },
    )
    .expect("a competing contained transition must reduce as a typed conflict");
    assert!(matches!(
        conflict.reduced_inputs()[0].outcome(),
        InputOutcome::ProductActionRejected {
            reason: DockspaceActionRejection::Conflict,
            version: actual,
        } if *actual == version
    ));
    assert!(
        creating
            .engine
            .viewport()
            .native_create_saga(request.saga())
            .is_some()
    );
    assert!(creating.engine.pending_presentation_rehome.is_none());
    assert!(conflict.platform_effects().is_empty());

    let mut rehoming = background_fixture(policy);
    let request = install_pending_native_root_reservation(&mut rehoming, root);
    let _ = rehoming.engine.viewport.take_new_effects();
    let _ = advance_pending_native_root_to_first_live(&mut rehoming, request);
    let target_measurements =
        surface_measurements(&rehoming.engine, TARGET_SURFACE, background_bounds());
    let _ = publish_surface_projection_with_output(
        &mut rehoming.engine,
        rehoming.presentation_host,
        TARGET_SURFACE,
        target_measurements,
    );
    let _ = rehoming.engine.viewport.take_new_effects();
    let version = rehoming.engine.version();
    let staged = submit_test_input(
        &mut rehoming.engine,
        rehoming.presentation_host,
        EngineInput::FloatRoot {
            expected: version,
            root,
            surface: TARGET_SURFACE,
            rect: None,
        },
    )
    .expect("the first presentation transition stages");
    assert!(matches!(
        staged.reduced_inputs()[0].outcome(),
        InputOutcome::ProductActionProcessed {
            outcome: DockspaceActionOutcome::RootFloatRequested { .. },
            ..
        }
    ));
    let pending = rehoming
        .engine
        .pending_presentation_rehome
        .clone()
        .expect("the first presentation transition owns the root reservation");
    let tear_off = submit_test_input(
        &mut rehoming.engine,
        rehoming.presentation_host,
        EngineInput::TearOffRoot {
            expected: version,
            root,
            placement: crate::model::NativeWindowPlacement::new(
                PhysicalRect::new(800.0, 120.0, 360.0, 260.0)
                    .expect("native placement must validate"),
            ),
        },
    )
    .expect("a competing native transition must reduce as a typed conflict");
    assert!(matches!(
        tear_off.reduced_inputs()[0].outcome(),
        InputOutcome::ProductActionRejected {
            reason: DockspaceActionRejection::Conflict,
            ..
        }
    ));
    let dock = submit_test_input(
        &mut rehoming.engine,
        rehoming.presentation_host,
        EngineInput::DockRoot {
            expected: version,
            root,
            placement: crate::model::DockPlacement::Main(SOURCE_SURFACE),
        },
    )
    .expect("a competing dock transition must reduce as a typed conflict");
    assert!(matches!(
        dock.reduced_inputs()[0].outcome(),
        InputOutcome::ProductActionRejected {
            reason: DockspaceActionRejection::Conflict,
            ..
        }
    ));
    assert_eq!(
        rehoming.engine.pending_presentation_rehome.as_ref(),
        Some(&pending)
    );
    let different_root = submit_test_input(
        &mut rehoming.engine,
        rehoming.presentation_host,
        EngineInput::FloatRoot {
            expected: version,
            root: SOURCE_ROOT,
            surface: TARGET_SURFACE,
            rect: None,
        },
    )
    .expect("the single rehome coordinator rejects a different root");
    assert!(matches!(
        different_root.reduced_inputs()[0].outcome(),
        InputOutcome::ProductActionRejected {
            reason: DockspaceActionRejection::Conflict,
            ..
        }
    ));
    assert_eq!(
        rehoming.engine.pending_presentation_rehome.as_ref(),
        Some(&pending)
    );
    assert!(tear_off.platform_effects().is_empty());
    assert!(dock.platform_effects().is_empty());
    assert!(different_root.platform_effects().is_empty());
}

#[test]
fn active_local_drag_rejects_programmatic_native_tear_off_atomically() {
    let mut policy = DockPolicy::default();
    policy.set_allow_native_surfaces(true);
    let mut fixture = background_fixture(policy);
    let _ = publish_native_background_platform(&mut fixture);
    let projection = fixture
        .engine
        .interaction_projection(SOURCE_SURFACE)
        .expect("the source surface has exact local response authority");
    let scene = projection.plan_stamp();
    let tab = *projection
        .plan()
        .tab_records()
        .iter()
        .find(|record| record.id().item == ItemId::new(1))
        .expect("the source item has one exact tab")
        .id();
    let point = region_center(
        projection
            .hit_manifest()
            .regions()
            .iter()
            .find(|region| {
                matches!(
                    region.id().kind(),
                    PresentationHitRegionKind::TabBody(id) if id == tab
                )
            })
            .expect("the source tab has one exact hit region"),
    );
    let version = fixture.engine.version();

    let drag = submit_test_input(
        &mut fixture.engine,
        fixture.presentation_host,
        EngineInput::LocalTabGesture {
            expected: version,
            surface: SOURCE_SURFACE,
            source: TabGestureSource::Item(tab),
            phase: LocalTabGesturePhase::Begin {
                scene,
                initial: point,
                current: point,
            },
        },
    )
    .expect("the local drag begins from exact source authority");
    assert!(matches!(
        drag.reduced_inputs()[0].outcome(),
        InputOutcome::InteractionProcessed {
            outcome: InteractionOutcome::DragBegan { .. },
            ..
        }
    ));

    let rejected = submit_test_input(
        &mut fixture.engine,
        fixture.presentation_host,
        EngineInput::TearOffRoot {
            expected: version,
            root: SOURCE_ROOT,
            placement: crate::model::NativeWindowPlacement::new(
                PhysicalRect::new(700.0, 100.0, 420.0, 320.0)
                    .expect("native placement must validate"),
            ),
        },
    )
    .expect("the competing tear-off reduces as a typed rejection");
    assert!(matches!(
        rejected.reduced_inputs()[0].outcome(),
        InputOutcome::ProductActionRejected {
            reason: DockspaceActionRejection::Conflict,
            version: actual,
        } if *actual == version
    ));
    assert!(rejected.platform_effects().is_empty());
    assert!(
        fixture
            .engine
            .viewport()
            .native_create_sagas()
            .next()
            .is_none()
    );
    assert!(matches!(
        fixture.engine.interaction().status(),
        InteractionStatus::Dragging { .. }
    ));
}

#[test]
fn pending_native_create_reserves_complete_source_root_for_shared_and_public_commands() {
    let mut policy = DockPolicy::default();
    policy.set_allow_native_surfaces(true);
    let mut fixture = background_fixture(policy);
    let _ = publish_native_background_platform(&mut fixture);
    fixture
        .engine
        .issue_root_recovery_anchors(InputSequence::new(94), [TARGET_SURFACE])
        .expect("surviving target recovery anchor must issue");
    let source_record = fixture
        .engine
        .workspace()
        .root(SOURCE_ROOT)
        .expect("source root remains live");
    let source = fixture
        .engine
        .workspace()
        .capture_node_source(SOURCE_ROOT, source_record.node)
        .expect("complete source remains capturable");
    let payload = MovePayload::Tabs(source.clone());
    let native_surface = SurfaceId::new(93);
    let command = WorkspaceCommand::RehomeRoot {
        source,
        target: RootPresentationTarget::NewSurface {
            surface: native_surface,
        },
    };
    let focus_causal = mint_test_focus_causal(&mut fixture.engine);
    let source_presentation = fixture
        .engine
        .interaction_authority(SOURCE_SURFACE)
        .expect("source presentation remains authoritative");
    let converted = crate::surface_recovery::ConvertedMainRecovery::new(
        SOURCE_ROOT,
        FloatingPresentationId::new(93),
        LogicalSize::new(0.0, 0.0).expect("minimum size validates"),
    );
    let proposal = crate::frame::NativeCreateProposal::new(
        native_surface,
        PhysicalRect::new(700.0, 100.0, 420.0, 320.0).expect("native placement must validate"),
        converted,
    );
    let recovery_anchor = fixture
        .engine
        .root_recovery_anchor(TARGET_SURFACE)
        .expect("surviving target surface owns a recovery anchor");
    let recovery_target = SurfaceRecoveryTarget::with_converted_main(recovery_anchor, converted);
    let mut future = fixture.engine.workspace().clone();
    WorkspaceTransaction::from_commands([command.clone()])
        .apply(&mut future, fixture.engine.policy_snapshot())
        .expect("complete root native future validates");
    let obligation_id = fixture
        .engine
        .next_surface_recovery_obligation_id(InputSequence::new(94))
        .expect("recovery identity advances");
    let obligation = fixture
        .engine
        .authorize_surface_recovery_obligation(
            InputSequence::new(94),
            obligation_id,
            &future,
            native_surface,
            recovery_target,
            fixture.engine.policy_snapshot(),
        )
        .expect("complete root recovery validates");
    let prepared = crate::frame::PreparedNativeCreate::new(
        source_presentation,
        payload,
        fixture.engine.version().epoch(),
        command,
        proposal,
        obligation,
        focus_causal,
    );
    let request = fixture
        .engine
        .viewport
        .start_native_create(prepared)
        .expect("complete source root native create must start");
    assert!(
        fixture
            .engine
            .viewport()
            .native_create_saga(request.saga())
            .is_some()
    );
    let (saga_id, saga_before) = fixture
        .engine
        .viewport()
        .native_create_sagas()
        .next()
        .map(|(id, saga)| (id, saga.clone()))
        .expect("native create saga remains pending");
    let projection = fixture
        .engine
        .interaction_projection(SOURCE_SURFACE)
        .expect("the source surface has exact local response authority");
    let scene = projection.plan_stamp();
    let tab = *projection
        .plan()
        .tab_records()
        .iter()
        .find(|record| record.id().item == ItemId::new(1))
        .expect("the source item has one exact tab")
        .id();
    let point = region_center(
        projection
            .hit_manifest()
            .regions()
            .iter()
            .find(|region| {
                matches!(
                    region.id().kind(),
                    PresentationHitRegionKind::TabBody(id) if id == tab
                )
            })
            .expect("the source tab has one exact hit region"),
    );
    let gesture_version = fixture.engine.version();
    let gesture = submit_test_input(
        &mut fixture.engine,
        fixture.presentation_host,
        EngineInput::LocalTabGesture {
            expected: gesture_version,
            surface: SOURCE_SURFACE,
            source: TabGestureSource::Item(tab),
            phase: LocalTabGesturePhase::Begin {
                scene,
                initial: point,
                current: point,
            },
        },
    )
    .expect("the competing local gesture reduces as a typed rejection");
    assert!(matches!(
        gesture.reduced_inputs()[0].outcome(),
        InputOutcome::InteractionProcessed {
            outcome: InteractionOutcome::Rejected(
                InteractionRejection::PresentationTransitionPending
            ),
            ..
        }
    ));
    assert_eq!(
        fixture.engine.viewport().native_create_saga(saga_id),
        Some(&saga_before)
    );
    assert_eq!(
        fixture.engine.interaction().status(),
        InteractionStatus::Idle
    );
    let source_record = fixture
        .engine
        .workspace()
        .root(SOURCE_ROOT)
        .expect("source root remains live while native create is pending");
    let source = fixture
        .engine
        .workspace()
        .capture_node_source(SOURCE_ROOT, source_record.node)
        .expect("complete source remains capturable");
    let target_record = fixture
        .engine
        .workspace()
        .root(TARGET_ROOT)
        .expect("target root remains live");
    let target = fixture
        .engine
        .workspace()
        .capture_tab_target(TARGET_ROOT, target_record.node)
        .expect("target tabs remain capturable");
    let command = WorkspaceCommand::Move {
        payload: MovePayload::Tabs(source),
        target: crate::command::DockTarget::Center(target),
    };
    let expected = fixture.engine.version();

    assert!(matches!(
        fixture
            .engine
            .stage_journal_workspace_command(
                tab_strip_test_cause(),
                &command,
                fixture.engine.policy_snapshot(),
            )
            .expect("shared pointer/local staging gate reduces"),
        Err(CommandError::SurfaceLifecycleFrozen {
            surface: SOURCE_SURFACE,
        })
    ));
    assert_eq!(
        fixture.engine.viewport().native_create_saga(saga_id),
        Some(&saga_before)
    );

    let rejected = submit_test_input(
        &mut fixture.engine,
        fixture.presentation_host,
        EngineInput::WorkspaceCommand { expected, command },
    )
    .expect("public workspace command reduces as an atomic rejection");
    assert!(matches!(
        rejected.reduced_inputs()[0].outcome(),
        InputOutcome::CommandRejected {
            error: CommandError::SurfaceLifecycleFrozen {
                surface: SOURCE_SURFACE,
            },
            version,
        } if *version == expected
    ));
    assert_eq!(fixture.engine.version(), expected);
    assert_eq!(
        fixture.engine.viewport().native_create_saga(saga_id),
        Some(&saga_before)
    );
}

#[test]
fn product_native_root_to_contained_settles_when_the_source_host_retires() {
    const NATIVE_SURFACE: SurfaceId = SurfaceId::new(93);

    let mut policy = DockPolicy::default();
    policy.set_allow_native_surfaces(true);
    let mut fixture = background_fixture(policy);
    let root = RootId::new(96);
    let request = install_pending_native_root_reservation(&mut fixture, root);
    let _ = fixture.engine.viewport.take_new_effects();
    let _ = advance_pending_native_root_to_first_live(&mut fixture, request);
    let target_measurements =
        surface_measurements(&fixture.engine, TARGET_SURFACE, background_bounds());
    let _ = publish_surface_projection_with_output(
        &mut fixture.engine,
        fixture.presentation_host,
        TARGET_SURFACE,
        target_measurements,
    );
    let _ = fixture.engine.viewport.take_new_effects();
    let source_version = fixture.engine.version();

    let staged = submit_test_input(
        &mut fixture.engine,
        fixture.presentation_host,
        EngineInput::FloatRoot {
            expected: source_version,
            root,
            surface: TARGET_SURFACE,
            rect: None,
        },
    )
    .expect("native-to-contained product action stages");
    assert!(matches!(
        staged.reduced_inputs()[0].outcome(),
        InputOutcome::ProductActionProcessed {
            outcome: DockspaceActionOutcome::RootFloatRequested { .. },
            ..
        }
    ));
    let source_emission = fixture
        .engine
        .pending_presentation_rehome
        .as_ref()
        .expect("the target presentation remains pending")
        .source_presentation
        .emission();
    assert!(
        fixture
            .engine
            .retained_presentation_emissions()
            .contains(&source_emission)
    );

    let retirement = fixture
        .engine
        .retire_presentation_host(
            fixture.presentation_host,
            PresentationHostRetirementReason::RuntimeDestroyed,
        )
        .expect("source presentation host retirement is atomic");
    let transition = retirement
        .transition()
        .expect("the first source host retirement publishes a transition");

    assert!(fixture.engine.pending_presentation_rehome.is_none());
    assert_eq!(
        fixture.engine.workspace().presentation_for_root(root),
        Some(crate::RootPresentationOwner::Main {
            surface: NATIVE_SURFACE,
        })
    );
    assert!(transition.events().iter().any(|event| matches!(
        event.kind(),
        WorkspaceEventKind::PresentationRehomeSettled {
            root: actual_root,
            source_surface: NATIVE_SURFACE,
            target_surface: TARGET_SURFACE,
            result: crate::event::PresentationRehomeResult::SourceUnavailable,
            outcome: None,
        } if *actual_root == root
    )));
    assert!(transition.platform_effects().iter().all(|effect| {
        !matches!(
            effect.effect(),
            PlatformEffect::ReleaseChild { binding } if *binding == request.binding()
        )
    }));
    assert!(
        !fixture
            .engine
            .retained_presentation_emissions()
            .contains(&source_emission),
        "terminal settlement releases the exact retired source emission"
    );
}

#[test]
fn product_native_root_rehome_terminates_on_exact_backend_failure() {
    const NATIVE_SURFACE: SurfaceId = SurfaceId::new(93);

    let mut policy = DockPolicy::default();
    policy.set_allow_native_surfaces(true);
    let mut fixture = background_fixture(policy);
    let root = RootId::new(94);
    let request = install_pending_native_root_reservation(&mut fixture, root);
    let _ = fixture.engine.viewport.take_new_effects();
    let _ = advance_pending_native_root_to_first_live(&mut fixture, request);
    let target_measurements =
        surface_measurements(&fixture.engine, TARGET_SURFACE, background_bounds());
    let _ = publish_surface_projection_with_output(
        &mut fixture.engine,
        fixture.presentation_host,
        TARGET_SURFACE,
        target_measurements,
    );
    let _ = fixture.engine.viewport.take_new_effects();
    let source_version = fixture.engine.version();
    let _ = submit_test_input(
        &mut fixture.engine,
        fixture.presentation_host,
        EngineInput::FloatRoot {
            expected: source_version,
            root,
            surface: TARGET_SURFACE,
            rect: None,
        },
    )
    .expect("native-to-contained transition must stage");
    assert!(fixture.engine.pending_presentation_rehome.is_some());

    let mut frame = begin_test_host_frame(&fixture.engine, fixture.presentation_host);
    for obligation in frame
        .issue_presentation_obligations()
        .expect("the failure frame issues its exact output roster")
    {
        let disposition = if obligation.slot().surface() == TARGET_SURFACE {
            HostPresentationDisposition::Unavailable(
                HostPresentationUnavailableReason::BackendFailure,
            )
        } else {
            HostPresentationDisposition::Unavailable(
                HostPresentationUnavailableReason::OutputNotProduced,
            )
        };
        frame
            .settle_presentation_obligation(obligation, disposition)
            .expect("each exact output slot accepts one disposition");
    }
    complete_surface_contribution_roster(&mut frame);
    let transition = frame
        .finish(&mut fixture.engine)
        .expect("an explicit backend failure settles without a core fatal error");

    assert!(fixture.engine.pending_presentation_rehome.is_none());
    assert_eq!(
        fixture.engine.workspace().presentation_for_root(root),
        Some(crate::RootPresentationOwner::Main {
            surface: NATIVE_SURFACE,
        })
    );
    assert!(transition.events().iter().any(|event| matches!(
        event.kind(),
        WorkspaceEventKind::PresentationRehomeSettled {
            root: actual_root,
            source_surface: NATIVE_SURFACE,
            target_surface: TARGET_SURFACE,
            result: crate::event::PresentationRehomeResult::TargetNotPresented,
            outcome: None,
        } if *actual_root == root
    )));
    assert!(transition.platform_effects().iter().all(|effect| {
        !matches!(
            effect.effect(),
            PlatformEffect::ReleaseChild { binding } if *binding == request.binding()
        )
    }));
}

#[test]
fn product_native_root_rehome_ignores_target_failure_without_preview_token() {
    const NATIVE_SURFACE: SurfaceId = SurfaceId::new(93);

    let mut policy = DockPolicy::default();
    policy.set_allow_native_surfaces(true);
    let mut fixture = background_fixture(policy);
    let root = RootId::new(94);
    let request = install_pending_native_root_reservation(&mut fixture, root);
    let _ = fixture.engine.viewport.take_new_effects();
    let _ = advance_pending_native_root_to_first_live(&mut fixture, request);
    let target_measurements =
        surface_measurements(&fixture.engine, TARGET_SURFACE, background_bounds());
    let _ = publish_surface_projection_with_output(
        &mut fixture.engine,
        fixture.presentation_host,
        TARGET_SURFACE,
        target_measurements,
    );
    let source_version = fixture.engine.version();
    let _ = submit_test_input(
        &mut fixture.engine,
        fixture.presentation_host,
        EngineInput::FloatRoot {
            expected: source_version,
            root,
            surface: TARGET_SURFACE,
            rect: None,
        },
    )
    .expect("native-to-contained transition must stage");

    assert!(
        !fixture
            .engine
            .observe_pending_presentation_rehome_dispositions(&[
                HostPresentationDispositionOutcome::new(
                    HostPresentationSlot::Surface {
                        surface: TARGET_SURFACE,
                    },
                    HostInteractionPresentation::default(),
                    HostPresentationDisposition::Unavailable(
                        HostPresentationUnavailableReason::BackendFailure,
                    ),
                ),
            ])
    );
    let mut events = Vec::new();
    assert!(
        !fixture
            .engine
            .settle_presented_pending_presentation_rehome(&mut events)
            .expect("an unrelated target failure remains inert")
    );
    assert!(fixture.engine.pending_presentation_rehome.is_some());
    assert_eq!(
        fixture.engine.workspace().presentation_for_root(root),
        Some(crate::RootPresentationOwner::Main {
            surface: NATIVE_SURFACE,
        })
    );
    assert!(events.is_empty());
}

#[test]
fn native_presentation_close_compiles_its_bound_recovery_obligation() {
    let mut policy = DockPolicy::default();
    policy.set_allow_native_surfaces(true);
    let mut fixture = background_fixture(policy);
    let request = install_pending_native_root_reservation(&mut fixture, RootId::new(95));
    let _ = fixture.engine.viewport.take_new_effects();
    let _ = advance_pending_native_root_to_first_live(&mut fixture, request);
    let recovery_host = fixture
        .engine
        .bound_surface_recoveries
        .get(&request.binding().surface())
        .expect("first-live native surface retains recovery authority")
        .obligation
        .target()
        .host_surface();
    let target_measurements =
        surface_measurements(&fixture.engine, recovery_host, background_bounds());
    let _ = publish_surface_projection_with_output(
        &mut fixture.engine,
        fixture.presentation_host,
        recovery_host,
        target_measurements,
    );
    let edge = NativeCloseEdge::from_authoritative_requested(
        fixture.engine.authority_domain,
        request.binding(),
        CloseObservationGeneration::new(1),
        fixture.engine.viewport().registry().inventory_generation(),
    );

    let capture = fixture
        .engine
        .capture_surface_close(
            edge,
            &SurfaceCloseRequest::RecoverPresentation,
            fixture.engine.policy_snapshot(),
        )
        .expect("bound presentation recovery must compile");

    assert!(capture.requirements.is_empty());
    assert!(matches!(
        capture.prepared,
        PreparedCloseOperation::SurfaceRehome { transaction, .. }
            if transaction.target_surface() == recovery_host
    ));
    assert_eq!(
        fixture
            .engine
            .workspace()
            .presentation_for_root(RootId::new(95)),
        Some(crate::RootPresentationOwner::Main {
            surface: SurfaceId::new(93),
        }),
        "preparing close recovery must not move the live source"
    );
}

#[test]
fn indeterminate_create_survives_inventory_absence_and_its_origin_survives_restore() {
    let mut policy = DockPolicy::default();
    policy.set_allow_native_surfaces(true);
    let mut fixture = background_fixture(policy);
    let reserved_root = RootId::new(94);
    let request = install_pending_native_root_reservation(&mut fixture, reserved_root);
    assert!(
        fixture
            .engine
            .viewport
            .take_new_effects()
            .iter()
            .any(|effect| {
                effect.id() == request.effect()
                    && matches!(
                        effect.effect(),
                        PlatformEffect::CreateWindow { binding, .. }
                            if *binding == request.binding()
                    )
            })
    );
    assert_eq!(
        fixture
            .engine
            .viewport
            .report_effect(
                fixture.engine.version().epoch(),
                EffectResult::new(
                    request.effect(),
                    request.binding().epoch(),
                    EffectDispatchResult::Indeterminate(
                        crate::effect::EffectIndeterminateReason::AcknowledgementLost,
                    ),
                ),
            )
            .expect("indeterminate create report must reduce"),
        EffectTransition::Applied
    );

    let expected_epoch = fixture.engine.version().epoch();
    let source_binding = background_source_binding(&fixture);
    let provider = test_platform_provider(&mut fixture.engine);
    submit_test_input(
        &mut fixture.engine,
        fixture.presentation_host,
        EngineInput::PublishPlatformSnapshot {
            provider,
            expected_epoch,
            snapshot: background_platform_snapshot(
                source_binding,
                Some(
                    PhysicalRect::new(0.0, 0.0, 400.0, 300.0).expect("source bounds must be valid"),
                ),
                2,
                false,
            ),
        },
    )
    .expect("authoritative absence must not terminate an active create");
    assert!(matches!(
        fixture
            .engine
            .viewport()
            .native_create_saga(request.saga())
            .map(crate::frame::NativeCreateSaga::phase),
        Some(crate::frame::NativeCreatePhase::AwaitingHidden { create })
            if create == request.effect()
    ));

    assert_eq!(
        fixture
            .engine
            .viewport
            .cancel_native_create(request.saga())
            .expect("indeterminate create cancellation must reduce"),
        None
    );
    let retirement = fixture
        .engine
        .viewport()
        .binding_retirements()
        .find_map(|(binding, retirement)| (binding == request.binding()).then_some(retirement))
        .expect("an emitted create must retain exact compensation ownership");
    let origin = retirement.origin();
    assert_eq!(
        origin,
        crate::frame::BindingRetirementOrigin::NativeCreateAborted {
            create: request.effect(),
        }
    );
    assert!(retirement.may_reappear());

    fixture
        .engine
        .viewport
        .reconcile_workspace_epoch(WorkspaceEpoch::new(1), &BTreeSet::new())
        .expect("workspace restore must preserve terminal create compensation ownership");
    let restored = fixture
        .engine
        .viewport()
        .binding_retirements()
        .find_map(|(binding, retirement)| (binding == request.binding()).then_some(retirement))
        .expect("restore must retain exact compensation origin");
    assert_eq!(restored.origin(), origin);
    assert_eq!(
        restored.status(),
        crate::frame::BindingRetirementStatus::AwaitingAppearance
    );
}

#[test]
fn definitive_create_failure_releases_the_reserved_surface_immediately() {
    let mut policy = DockPolicy::default();
    policy.set_allow_native_surfaces(true);
    let mut fixture = background_fixture(policy);
    let reserved_root = RootId::new(94);
    let request = install_pending_native_root_reservation(&mut fixture, reserved_root);
    let _ = fixture.engine.viewport.take_new_effects();

    assert_eq!(
        fixture
            .engine
            .viewport
            .report_effect(
                fixture.engine.version().epoch(),
                EffectResult::new(
                    request.effect(),
                    request.binding().epoch(),
                    EffectDispatchResult::DispatchFailed(
                        crate::effect::DispatchFailureReason::WindowUnavailable,
                    ),
                ),
            )
            .expect("definitive create failure must reduce"),
        EffectTransition::Applied
    );
    assert!(
        fixture
            .engine
            .viewport()
            .native_create_saga(request.saga())
            .is_none()
    );
    assert!(
        fixture
            .engine
            .viewport()
            .viewport(request.binding().surface())
            .is_none()
    );
    assert!(
        fixture
            .engine
            .viewport()
            .binding_retirements()
            .all(|(binding, _)| binding != request.binding())
    );
    assert!(!fixture.engine.native_create_reserves_root(reserved_root));
    assert!(fixture.engine.viewport.take_new_effects().is_empty());
}

#[test]
fn explicit_tick_fatal_reduction_rolls_back_provenance_and_complete_state() {
    let root = RootId::new(1);
    let surface = SurfaceId::new(1);
    let mut builder = Workspace::builder();
    let tabs = builder.insert_node(Node::tabs([ItemId::new(1), ItemId::new(2)]));
    builder.set_root(root, RootRecord::new(tabs));
    builder.set_surface(surface, SurfacePresentation::with_main(root));
    let workspace = builder.build().expect("test workspace must be valid");
    let command = WorkspaceCommand::Select {
        source: workspace
            .capture_item_source(root, tabs, ItemId::new(2))
            .expect("source must be capturable"),
    };
    let mut engine =
        DockEngine::new(workspace, DockPolicy::default()).expect("engine must be valid");
    let presentation_host = engine
        .create_presentation_host()
        .expect("fatal reduction presentation host must mint");
    engine.version = WorkspaceVersion::new(
        WorkspaceEpoch::default(),
        WorkspaceRevision::new(u64::MAX - 1),
    );
    engine
        .try_rebuild_presentation_requirements(None)
        .expect("the exhausted-revision fixture must retain a current manifest");
    let expected = engine.version;
    let mut policy = engine.policy.to_policy();
    policy.set_allow_native_surfaces(true);
    let source = StableInputSourceId::new(41);
    let mut frame = begin_test_host_frame(&engine, presentation_host);
    frame
        .append_input(
            source,
            SourceSequence::new(1),
            EngineInput::WorkspaceCommand { expected, command },
        )
        .expect("workspace command belongs to the semantic phase");
    frame
        .append_configuration(
            source,
            SourceSequence::new(2),
            EngineInput::ReplacePolicy { expected, policy },
        )
        .expect("policy replacement belongs to the terminal configuration phase");
    complete_host_frame_with_explicit_surface_roster(&engine, &mut frame);
    let before = engine.candidate();

    assert!(matches!(
        frame.finish(&mut engine),
        Err(EngineError::WorkspaceRevisionExhausted { .. })
    ));
    assert_eq!(engine, before);
    assert_eq!(engine.last_reducer_tick(), ReducerTickId::default());
    assert_eq!(engine.last_input_sequence(), InputSequence::default());
    assert_eq!(engine.semantic_input_watermark(), None);
}

#[test]
fn explicit_tick_counter_exhaustion_is_atomic() {
    let mut fixture = counter_fixture();
    fixture.engine.last_reducer_tick = ReducerTickId::new(u64::MAX);
    let before_tick_exhaustion = fixture.engine.candidate();

    let mut prelude = fixture
        .engine
        .begin_host_frame(fixture.presentation_host)
        .expect("tick-exhaustion prelude must begin");
    prelude
        .submit_presentation_observation(HostPresentationObservation::NoUpdate)
        .expect("tick-exhaustion observation must stage");
    assert!(matches!(
        prelude.seal(&fixture.engine),
        Err(EngineError::ReducerTickExhausted)
    ));
    assert_eq!(fixture.engine, before_tick_exhaustion);

    fixture.engine.last_reducer_tick = ReducerTickId::default();
    fixture.engine.last_input = InputSequence::new(u64::MAX);
    let source = StableInputSourceId::new(42);
    let before_input_exhaustion = fixture.engine.candidate();
    let mut frame = begin_test_host_frame(&fixture.engine, fixture.presentation_host);
    assert_eq!(
        frame.append_input(
            source,
            SourceSequence::new(1),
            EngineInput::ValidateWorkspace,
        ),
        Err(CoreHostFrameError::InputPrefixReductionFailed),
        "input sequence exhaustion fails while reducing the input prefix",
    );
    assert_eq!(
        frame.finish(&mut fixture.engine),
        Err(EngineError::InputSequenceExhausted)
    );
    assert_eq!(fixture.engine, before_input_exhaustion);
    assert_eq!(fixture.engine.semantic_input_watermark(), None);
}

#[test]
fn surface_contribution_keeps_its_exact_authority_across_delayed_submission() {
    let mut fixture = counter_fixture();
    publish_counter_scene(&mut fixture);
    let stale_contribution =
        begin_surface_measurement(&fixture.engine, TARGET_SURFACE, test_rect());
    let submitted_base = stale_contribution.token().base();

    let expected = fixture.engine.version();
    let mut policy = fixture.engine.policy().clone();
    policy.set_allow_native_surfaces(true);
    submit_test_input(
        &mut fixture.engine,
        fixture.presentation_host,
        EngineInput::ReplacePolicy { expected, policy },
    )
    .expect("policy change must advance the workspace version");
    publish_counter_scene(&mut fixture);

    let before_workspace = fixture.engine.workspace().clone();
    let before_target_scene = fixture
        .engine
        .scene()
        .surface(TARGET_SURFACE)
        .cloned()
        .expect("target surface remains rostered before delayed submission");
    let before_interaction = fixture.engine.interaction().clone();

    let transition = submit_surface_measurement(
        &mut fixture.engine,
        fixture.presentation_host,
        stale_contribution,
    );

    assert!(matches!(
        transition.surface_contributions().iter().find(|outcome| outcome.surface() == TARGET_SURFACE),
        Some(SurfaceContributionOutcome::Rejected {
            surface: TARGET_SURFACE,
            reason: SurfaceContributionRejection::StaleBase {
                submitted,
                current: Some(current),
            },
        }) if *submitted == submitted_base && *current != submitted_base
    ));
    assert!(transition.events().is_empty());
    assert!(transition.interaction_events().is_empty());
    assert_eq!(fixture.engine.workspace(), &before_workspace);
    assert_eq!(
        fixture.engine.scene().surface(TARGET_SURFACE),
        Some(&before_target_scene),
        "the rejected delayed contribution cannot mutate its own target surface"
    );
    assert_eq!(fixture.engine.interaction(), &before_interaction);
}

#[test]
fn rootless_contained_surface_remains_focusable_routeable_and_registerable() {
    let rootless_surface = SurfaceId::new(20);
    let target_surface = SurfaceId::new(21);
    let contained_root = RootId::new(20);
    let target_root = RootId::new(21);
    let floating = FloatingPresentationId::new(20);
    let item = ItemId::new(20);
    let mut builder = Workspace::builder();
    let contained_tabs = builder.insert_node(Node::tabs([item]));
    let target_tabs = builder.insert_node(Node::tabs([ItemId::new(21)]));
    builder.set_root(contained_root, RootRecord::new(contained_tabs));
    builder.set_root(target_root, RootRecord::new(target_tabs));
    builder.set_surface(rootless_surface, SurfacePresentation::rootless());
    builder.set_surface(target_surface, SurfacePresentation::with_main(target_root));
    builder.set_contained_floating(
        floating,
        ContainedFloating::new(contained_root, test_rect()),
    );
    builder
        .attach_contained(rootless_surface, floating)
        .expect("rootless surface draft must exist");
    let workspace = builder.build().expect("rootless workspace must be valid");
    let mut engine =
        DockEngine::new(workspace, DockPolicy::default()).expect("rootless engine must be valid");
    let presentation_host = engine
        .create_presentation_host()
        .expect("rootless presentation host must mint");
    let provider = test_platform_provider(&mut engine);

    assert!(engine.surface_items(rootless_surface).contains(&item));
    assert!(
        DockEngine::capture_surface_item_source(engine.workspace(), rootless_surface, item,)
            .is_some()
    );
    assert_eq!(
        engine.payload_surface(&MovePayload::Item(
            engine
                .workspace()
                .capture_item_source(contained_root, contained_tabs, item)
                .expect("contained item must be current"),
        )),
        Some(rootless_surface)
    );

    let expected = engine.version();
    submit_test_input(
        &mut engine,
        presentation_host,
        EngineInput::RegisterViewport {
            provider,
            expected,
            surface: target_surface,
            token: WindowToken::new(21),
            role: ViewportRole::Root,
            recovery_target: None,
        },
    )
    .expect("target root anchor registration must reduce");
    let anchor = engine
        .root_recovery_anchor(target_surface)
        .expect("target root must own a recovery anchor");
    let expected = engine.version();
    let registered = submit_test_input(
        &mut engine,
        presentation_host,
        EngineInput::RegisterViewport {
            provider,
            expected,
            surface: rootless_surface,
            token: WindowToken::new(20),
            role: ViewportRole::Child,
            recovery_target: Some(SurfaceRecoveryTarget::forest_only(anchor)),
        },
    )
    .expect("rootless child registration must reduce");
    assert!(matches!(
        registered.reduced_inputs()[0].outcome(),
        InputOutcome::ViewportRegistered { binding }
            if binding.surface() == rootless_surface
    ));
}

#[test]
fn rootless_native_child_rehome_waits_for_target_presentation_before_release() {
    let child_surface = SurfaceId::new(20);
    let host_surface = SurfaceId::new(21);
    let child_root = RootId::new(20);
    let host_root = RootId::new(21);
    let child_floating = FloatingPresentationId::new(20);
    let child_item = ItemId::new(20);
    let mut builder = Workspace::builder();
    let child_tabs = builder.insert_node(Node::tabs([child_item]));
    let host_tabs = builder.insert_node(Node::tabs([ItemId::new(21)]));
    builder.set_root(child_root, RootRecord::new(child_tabs));
    builder.set_root(host_root, RootRecord::new(host_tabs));
    builder.set_surface(child_surface, SurfacePresentation::rootless());
    builder.set_surface(host_surface, SurfacePresentation::with_main(host_root));
    builder.set_contained_floating(
        child_floating,
        ContainedFloating::new(child_root, test_rect()),
    );
    builder
        .attach_contained(child_surface, child_floating)
        .expect("rootless child surface must exist");
    let workspace = builder.build().expect("rootless workspace must be valid");
    let mut engine =
        DockEngine::new(workspace, DockPolicy::default()).expect("rootless engine must be valid");
    let presentation_host = engine
        .create_presentation_host()
        .expect("rootless presentation host must mint");
    let provider = test_platform_provider(&mut engine);

    let expected = engine.version();
    let host_registration = submit_test_input(
        &mut engine,
        presentation_host,
        EngineInput::RegisterViewport {
            provider,
            expected,
            surface: host_surface,
            token: WindowToken::new(21),
            role: ViewportRole::Root,
            recovery_target: None,
        },
    )
    .expect("recovery host registration must reduce");
    let host_binding = match host_registration.reduced_inputs()[0].outcome() {
        InputOutcome::ViewportRegistered { binding } => *binding,
        outcome => panic!("expected host registration, got {outcome:?}"),
    };
    let expected = engine.version();
    let child_registration = submit_test_input(
        &mut engine,
        presentation_host,
        EngineInput::BootstrapChildViewport {
            provider,
            expected,
            surface: child_surface,
            token: WindowToken::new(20),
            recovery: SurfaceRecoveryBootstrap::new(host_surface),
        },
    )
    .expect("rootless child registration must reduce");
    let child_binding = match child_registration.reduced_inputs()[0].outcome() {
        InputOutcome::ViewportRegistered { binding } => *binding,
        outcome => panic!("expected child registration, got {outcome:?}"),
    };

    let mut capabilities = PlatformCapabilities::default();
    capabilities.set_native_window_lifecycle(PlatformCapability::Supported);
    capabilities.set_authoritative_inventory(PlatformCapability::Supported);
    capabilities.set_global_window_placement(PlatformCapability::Supported);
    capabilities.set_work_area(PlatformCapability::Supported);
    capabilities.set_global_focus_observation(PlatformCapability::Supported);
    let observed = |binding: ViewportBinding, x: f64| {
        ObservedWindow::new(binding)
            .with_coordinate_observation(WindowCoordinateObservation::new(
                binding,
                CoordinateObservationGeneration::new(1),
                Authority::Known(
                    PhysicalRect::new(x, 0.0, 400.0, 300.0)
                        .expect("native content bounds must be valid"),
                ),
                Authority::Known(
                    PhysicalRect::new(x - 8.0, -30.0, 416.0, 338.0)
                        .expect("native outer bounds must be valid"),
                ),
                Authority::Known(ScaleFactor::new(1.0).expect("content scale must be valid")),
                Authority::Known(ScaleFactor::new(1.0).expect("presentation scale must be valid")),
            ))
            .with_presentation_observation(WindowPresentationObservation::new(
                binding,
                crate::viewport::PresentationObservationGeneration::new(1),
                Authority::Known(WindowPresentationState::Visible),
                PresentationEffectAcknowledgement::known(None),
            ))
    };
    let snapshot = test_platform_snapshot(
        capabilities,
        FocusObservationEnvelope::new(
            FocusObservationGeneration::new(1),
            Authority::Known(GlobalFocusedWindow::Foreign),
            Authority::Known(None),
        ),
        vec![observed(host_binding, 0.0), observed(child_binding, 500.0)],
        Vec::new(),
        known_work_area_observation(
            1,
            vec![ObservedWorkArea::new(
                WorkAreaToken::new(20),
                PhysicalRect::new(0.0, 0.0, 1920.0, 1080.0).expect("work area must be valid"),
                ScaleFactor::new(1.0).expect("work-area scale must be valid"),
            )],
        ),
    )
    .expect("the exact two-window snapshot must validate");
    let expected_epoch = engine.version().epoch();
    submit_test_input(
        &mut engine,
        presentation_host,
        EngineInput::PublishPlatformSnapshot {
            provider,
            expected_epoch,
            snapshot,
        },
    )
    .expect("the exact two-window snapshot must reduce");
    for surface in [host_surface, child_surface] {
        let measurements = surface_measurements(&engine, surface, background_bounds());
        let _ = publish_surface_projection_with_output(
            &mut engine,
            presentation_host,
            surface,
            measurements,
        );
    }
    assert_eq!(
        engine
            .viewport()
            .viewport(child_surface)
            .map(crate::viewport_registry::ViewportRecord::admission),
        Some(crate::viewport_registry::ViewportAdmission::Admitted)
    );
    let _ = engine.viewport.take_new_effects();
    let source_version = engine.version();

    let staged = submit_test_input(
        &mut engine,
        presentation_host,
        EngineInput::FloatRoot {
            expected: source_version,
            root: child_root,
            surface: host_surface,
            rect: None,
        },
    )
    .expect("rootless child rehome must stage");
    assert!(matches!(
        staged.reduced_inputs()[0].outcome(),
        InputOutcome::ProductActionProcessed {
            outcome: DockspaceActionOutcome::RootFloatRequested {
                root,
                source_surface,
                target_surface,
                ..
            },
            ..
        } if *root == child_root
            && *source_surface == child_surface
            && *target_surface == host_surface
    ));
    assert_eq!(
        engine.workspace().presentation_for_root(child_root),
        Some(crate::RootPresentationOwner::Contained {
            surface: child_surface,
            floating: child_floating,
        })
    );
    assert!(engine.pending_presentation_rehome.is_some());
    assert!(staged.platform_effects().iter().all(|effect| {
        !matches!(
            effect.effect(),
            PlatformEffect::ReleaseChild { binding } if *binding == child_binding
        )
    }));

    let measurements = surface_measurements(&engine, host_surface, background_bounds());
    let (_, _, transferred) = publish_surface_projection_with_output(
        &mut engine,
        presentation_host,
        host_surface,
        measurements,
    );
    let final_owner = engine.workspace().presentation_for_root(child_root);
    assert!(
        matches!(
            final_owner,
        Some(crate::RootPresentationOwner::Contained {
            surface,
            floating,
        }) if surface == host_surface && floating == child_floating
        ),
        "unexpected final owner {final_owner:?}, pending {:?}, events {:?}",
        engine.pending_presentation_rehome,
        transferred.events()
    );
    assert!(engine.workspace().surface(child_surface).is_none());
    assert_eq!(
        transferred
            .platform_effects()
            .iter()
            .filter(|effect| matches!(
                effect.effect(),
                PlatformEffect::ReleaseChild { binding } if *binding == child_binding
            ))
            .count(),
        1
    );
}

#[test]
fn child_viewport_bootstrap_mints_its_recovery_identity_in_core() {
    let host_surface = SurfaceId::new(22);
    let child_surface = SurfaceId::new(23);
    let host_root = RootId::new(22);
    let child_root = RootId::new(23);
    let mut builder = Workspace::builder();
    let host_tabs = builder.insert_node(Node::tabs([ItemId::new(22)]));
    let child_tabs = builder.insert_node(Node::tabs([ItemId::new(23)]));
    builder.set_root(host_root, RootRecord::new(host_tabs));
    builder.set_root(child_root, RootRecord::new(child_tabs));
    builder.set_surface(host_surface, SurfacePresentation::with_main(host_root));
    builder.set_surface(child_surface, SurfacePresentation::with_main(child_root));
    let workspace = builder.build().expect("bootstrap workspace must be valid");
    let mut engine =
        DockEngine::new(workspace, DockPolicy::default()).expect("bootstrap engine must be valid");
    let presentation_host = engine
        .create_presentation_host()
        .expect("bootstrap presentation host must mint");
    let provider = test_platform_provider(&mut engine);

    let expected = engine.version();
    submit_test_input(
        &mut engine,
        presentation_host,
        EngineInput::RegisterViewport {
            provider,
            expected,
            surface: host_surface,
            token: WindowToken::new(22),
            role: ViewportRole::Root,
            recovery_target: None,
        },
    )
    .expect("recovery host registration must reduce");
    let frontier_before = engine.presentation_identity_frontier();
    let minimum_size = engine.presentation_config().minimum_floating_size();
    let expected = engine.version();
    let registered = submit_test_input(
        &mut engine,
        presentation_host,
        EngineInput::BootstrapChildViewport {
            provider,
            expected,
            surface: child_surface,
            token: WindowToken::new(23),
            recovery: SurfaceRecoveryBootstrap::new(host_surface),
        },
    )
    .expect("child bootstrap registration must reduce");

    assert!(matches!(
        registered.reduced_inputs()[0].outcome(),
        InputOutcome::ViewportRegistered { binding }
            if binding.surface() == child_surface
    ));
    let target = engine
        .bound_surface_recoveries
        .get(&child_surface)
        .expect("registered child must retain a recovery obligation")
        .obligation
        .target();
    assert_eq!(target.host_surface(), host_surface);
    let converted = target
        .converted_main()
        .expect("rooted child must reserve a converted-main presentation");
    assert_eq!(converted.source_root(), child_root);
    assert_eq!(converted.minimum_size(), minimum_size);
    assert!(converted.floating().get() > frontier_before.last_floating());
    assert_eq!(
        engine.presentation_identity_frontier().last_floating(),
        converted.floating().get()
    );
}

#[test]
fn rejected_child_viewport_bootstrap_does_not_consume_a_floating_identity() {
    let host_surface = SurfaceId::new(24);
    let child_surface = SurfaceId::new(25);
    let host_root = RootId::new(24);
    let child_root = RootId::new(25);
    let mut builder = Workspace::builder();
    let host_tabs = builder.insert_node(Node::tabs([ItemId::new(24)]));
    let child_tabs = builder.insert_node(Node::tabs([ItemId::new(25)]));
    builder.set_root(host_root, RootRecord::new(host_tabs));
    builder.set_root(child_root, RootRecord::new(child_tabs));
    builder.set_surface(host_surface, SurfacePresentation::with_main(host_root));
    builder.set_surface(child_surface, SurfacePresentation::with_main(child_root));
    let workspace = builder.build().expect("bootstrap workspace must be valid");
    let mut policy = DockPolicy::default();
    policy.set_allow_contained_floating(false);
    let mut engine = DockEngine::new(workspace, policy).expect("bootstrap engine must be valid");
    let presentation_host = engine
        .create_presentation_host()
        .expect("bootstrap presentation host must mint");
    let provider = test_platform_provider(&mut engine);

    let expected = engine.version();
    submit_test_input(
        &mut engine,
        presentation_host,
        EngineInput::RegisterViewport {
            provider,
            expected,
            surface: host_surface,
            token: WindowToken::new(24),
            role: ViewportRole::Root,
            recovery_target: None,
        },
    )
    .expect("recovery host registration must reduce");
    let frontier_before = engine.presentation_identity_frontier();
    let expected = engine.version();
    let rejected = submit_test_input(
        &mut engine,
        presentation_host,
        EngineInput::BootstrapChildViewport {
            provider,
            expected,
            surface: child_surface,
            token: WindowToken::new(25),
            recovery: SurfaceRecoveryBootstrap::new(host_surface),
        },
    )
    .expect("policy rejection must still reduce the input");

    assert!(matches!(
        rejected.reduced_inputs()[0].outcome(),
        InputOutcome::ViewportRegistrationRejected { surface } if *surface == child_surface
    ));
    assert_eq!(engine.presentation_identity_frontier(), frontier_before);
    assert!(engine.viewport().viewport(child_surface).is_none());
}

#[test]
fn main_departure_is_not_vacancy_until_the_last_contained_root_leaves() {
    let source_surface = SurfaceId::new(30);
    let target_surface = SurfaceId::new(31);
    let main_root = RootId::new(30);
    let sibling_root = RootId::new(31);
    let target_root = RootId::new(32);
    let sibling = FloatingPresentationId::new(30);
    let moved_main = FloatingPresentationId::new(31);
    let mut builder = Workspace::builder();
    let main_tabs = builder.insert_node(Node::tabs([ItemId::new(30)]));
    let sibling_tabs = builder.insert_node(Node::tabs([ItemId::new(31)]));
    let target_tabs = builder.insert_node(Node::tabs([ItemId::new(32)]));
    builder.set_root(main_root, RootRecord::new(main_tabs));
    builder.set_root(sibling_root, RootRecord::new(sibling_tabs));
    builder.set_root(target_root, RootRecord::new(target_tabs));
    builder.set_surface(source_surface, SurfacePresentation::with_main(main_root));
    builder.set_surface(target_surface, SurfacePresentation::with_main(target_root));
    builder.set_contained_floating(sibling, ContainedFloating::new(sibling_root, test_rect()));
    builder
        .attach_contained(source_surface, sibling)
        .expect("source surface must exist");
    let workspace = builder.build().expect("workspace must be valid");
    let mut engine =
        DockEngine::new(workspace, DockPolicy::default()).expect("engine must be valid");
    let presentation_host = engine
        .create_presentation_host()
        .expect("vacancy presentation host must mint");
    let provider = test_platform_provider(&mut engine);
    let expected = engine.version();
    submit_test_input(
        &mut engine,
        presentation_host,
        EngineInput::RegisterViewport {
            provider,
            expected,
            surface: source_surface,
            token: WindowToken::new(30),
            role: ViewportRole::Root,
            recovery_target: None,
        },
    )
    .expect("source viewport registration must reduce");
    let main_source = engine
        .workspace()
        .capture_node_source(main_root, main_tabs)
        .expect("main root must be current");
    let mut events = Vec::new();
    let application = engine
        .apply_interaction_command(
            InputSequence::new(30),
            &WorkspaceCommand::RehomeRoot {
                source: main_source,
                target: RootPresentationTarget::Contained {
                    surface: target_surface,
                    floating: moved_main,
                    rect: test_rect(),
                    position: ContainedPosition::Front,
                },
            },
            &mut events,
        )
        .expect("main root rehome must reduce");
    assert!(matches!(
        application,
        CommandApplication::Applied { changed: true, .. }
    ));
    let source = engine
        .workspace()
        .surface(source_surface)
        .expect("contained sibling must keep the source surface alive");
    assert_eq!(source.main_root, None);
    assert_eq!(source.contained, vec![sibling]);
    assert_no_pointer_passthrough_effects(&engine);

    let sibling_source = engine
        .workspace()
        .capture_node_source(sibling_root, sibling_tabs)
        .expect("contained sibling must remain current");
    engine
        .apply_interaction_command(
            InputSequence::new(31),
            &WorkspaceCommand::RehomeRoot {
                source: sibling_source,
                target: RootPresentationTarget::Contained {
                    surface: target_surface,
                    floating: sibling,
                    rect: test_rect(),
                    position: ContainedPosition::Front,
                },
            },
            &mut events,
        )
        .expect("last contained rehome must reduce");
    assert!(engine.workspace().surface(source_surface).is_none());
    assert_eq!(
        engine
            .workspace()
            .surface(target_surface)
            .expect("target surface must remain")
            .contained,
        vec![moved_main, sibling]
    );
}

#[test]
fn stale_roster_precondition_is_a_nonpublishing_recovery_rejection() {
    let source_surface = SurfaceId::new(40);
    let target_surface = SurfaceId::new(41);
    let source_root = RootId::new(40);
    let target_root = RootId::new(41);
    let floating = FloatingPresentationId::new(40);
    let mut builder = Workspace::builder();
    let source_tabs = builder.insert_node(Node::tabs([ItemId::new(40), ItemId::new(42)]));
    let target_tabs = builder.insert_node(Node::tabs([ItemId::new(41)]));
    builder.set_root(source_root, RootRecord::new(source_tabs));
    builder.set_root(target_root, RootRecord::new(target_tabs));
    builder.set_surface(source_surface, SurfacePresentation::with_main(source_root));
    builder.set_surface(target_surface, SurfacePresentation::with_main(target_root));
    let workspace = builder.build().expect("workspace must be valid");
    let mut engine =
        DockEngine::new(workspace, DockPolicy::default()).expect("engine must be valid");
    let roster = SurfaceRosterDisposition::capture(engine.workspace(), source_surface, None)
        .expect("source roster must freeze");
    let placement = SurfaceRehomePlacement::new(
        target_surface,
        Some(SurfaceMainRehome::Present(
            RootPresentationTarget::Contained {
                surface: target_surface,
                floating,
                rect: test_rect(),
                position: ContainedPosition::Front,
            },
        )),
        Vec::new(),
    );
    let transaction = roster
        .compile_rehome_transaction(engine.workspace(), &placement)
        .expect("current roster must compile");
    let Some(Node::Tabs { selected, .. }) = engine.workspace.nodes.get_mut(source_tabs) else {
        panic!("source root must remain tabs");
    };
    *selected = Some(ItemId::new(42));
    engine
        .workspace
        .tab_mru
        .insert(source_tabs, vec![ItemId::new(42), ItemId::new(40)]);
    engine
        .workspace
        .validate()
        .expect("stale source mutation must remain valid");
    let before = engine.workspace().clone();
    let version = engine.version();
    let mut events = Vec::new();

    let applied = engine
        .apply_surface_roster_transaction(
            InputSequence::new(40),
            &roster,
            &transaction,
            None,
            &BTreeMap::new(),
            &mut events,
            WorkspacePublicationAuthority::Ordinary,
        )
        .expect("stale roster is an expected rejection");

    assert!(!applied);
    assert_eq!(engine.workspace(), &before);
    assert_eq!(engine.version(), version);
    assert!(events.is_empty());
}
