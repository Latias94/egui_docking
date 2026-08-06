//! Native lifecycle, recovery, contained geometry, and vacancy tests.

use super::*;

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
    let prepared = PreparedNativeTearOff::new(
        DragSessionId::new(fixture.engine.version().epoch(), DragGeneration::new(93)),
        source_presentation,
        payload,
        fixture.engine.version(),
        command,
        proposal,
        obligation,
        focus_causal,
        PaneFocusDisposition::Clear,
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
    const WORK_AREA: WorkAreaToken = WorkAreaToken::new(93);
    let mut capabilities = PlatformCapabilities::default();
    capabilities.set_native_window_lifecycle(PlatformCapability::Supported);
    capabilities.set_authoritative_inventory(PlatformCapability::Supported);
    capabilities.set_global_window_placement(PlatformCapability::Supported);
    capabilities.set_work_area(PlatformCapability::Supported);
    capabilities.set_global_focus_observation(PlatformCapability::Supported);
    if focus_control {
        capabilities.set_window_activation_control(PlatformCapability::Supported);
    }
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
        ViewportActivationRequest::tear_off_committed(request.binding(), prepared.pane_focus(),),
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
    assert_eq!(intent.target(), request.binding());
    assert_eq!(intent.causal(), prepared.focus_causal());
    assert_eq!(intent.focus(), PanelFocus::None);
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
        ViewportActivationRequest::tear_off_committed(request.binding(), prepared.pane_focus(),),
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
        ViewportActivationRequest::tear_off_committed(request.binding(), prepared.pane_focus(),),
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
