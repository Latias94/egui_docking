use dockspace::command::MovePayload;
use dockspace::effect::{EffectPhase, EffectRequest, PlatformEffect};
use dockspace::engine::DockEngine;
use dockspace::geometry::{LogicalRect, PhysicalPoint, PhysicalRect, ScaleFactor};
use dockspace::graph::{Node, RootRecord, SurfacePresentation, Workspace};
use dockspace::ids::{FloatingPresentationId, ItemId, NodeId, RootId, SurfaceId};
use dockspace::intent::{
    Authority, ContainedTearOffProposal, NativeTearOffProposal, PointerButton, PointerButtonState,
    PointerId, RendererIntent, TargetAuthority, TearOffRequest,
};
use dockspace::interaction::{
    DragSessionId, InteractionCancelReason, InteractionDelivery, InteractionOutcome,
    InteractionRejection, InteractionStatus, PreviewResolutionStatus, PreviewVisual,
    WorkspaceDeliveryKind,
};
use dockspace::platform::{
    ButtonObservation, ObservedWindow, PlatformCapabilities, PlatformCapability,
    PlatformCapabilityReason, PlatformRequirement, PlatformSnapshot, PointerObservation,
    PointerWindow, WindowInputState,
};
use dockspace::policy::{ContainedFallback, DockPolicy};
use dockspace::scene::{BuildingScene, ReadySurfaceScene};
use dockspace::transition::InputOutcome;
use dockspace::viewport::{ViewportRole, WindowToken};

const ROOT_A: RootId = RootId::new(1);
const ROOT_B: RootId = RootId::new(2);
const ROOT_NEW: RootId = RootId::new(10);
const SURFACE_A: SurfaceId = SurfaceId::new(1);
const SURFACE_B: SurfaceId = SurfaceId::new(2);
const SURFACE_NEW: SurfaceId = SurfaceId::new(10);
const FLOATING_NEW: FloatingPresentationId = FloatingPresentationId::new(10);
const POINTER: PointerId = PointerId::new(1);
const SOURCE_WINDOW: WindowToken = WindowToken::new(41);

struct Fixture {
    engine: DockEngine,
    tabs_a: NodeId,
}

fn logical_rect(x: f64, y: f64, width: f64, height: f64) -> LogicalRect {
    LogicalRect::new(x, y, width, height).expect("test logical rectangle must be valid")
}

fn physical_rect(x: f64, y: f64, width: f64, height: f64) -> PhysicalRect {
    PhysicalRect::new(x, y, width, height).expect("test physical rectangle must be valid")
}

fn physical_point(x: f64, y: f64) -> PhysicalPoint {
    PhysicalPoint::new(x, y).expect("test physical point must be valid")
}

fn platform_capabilities(native_lifecycle: PlatformCapability) -> PlatformCapabilities {
    let mut capabilities = PlatformCapabilities::default();
    capabilities.set_native_window_lifecycle(native_lifecycle);
    capabilities.set_authoritative_inventory(PlatformCapability::Supported);
    capabilities.set_hovered_window(PlatformCapability::Supported);
    capabilities.set_desktop_pointer_position(PlatformCapability::Supported);
    capabilities.set_authoritative_button_state(PlatformCapability::Supported);
    capabilities.set_global_window_placement(PlatformCapability::Supported);
    capabilities.set_work_area(PlatformCapability::Supported);
    capabilities.set_pointer_passthrough(PlatformCapability::Supported);
    capabilities.set_window_focus(PlatformCapability::Supported);
    capabilities.set_close_cancellation(PlatformCapability::Supported);
    capabilities
}

fn platform_snapshot(native_lifecycle: PlatformCapability) -> PlatformSnapshot {
    let source = ObservedWindow::new(SOURCE_WINDOW)
        .with_content_bounds(Authority::Known(physical_rect(0.0, 0.0, 1200.0, 900.0)))
        .with_outer_bounds(Authority::Known(physical_rect(-8.0, -30.0, 1216.0, 938.0)))
        .with_scale_factor(Authority::Known(
            ScaleFactor::new(1.0).expect("test scale factor must be valid"),
        ))
        .with_work_area(Authority::Known(Some(physical_rect(
            -1920.0, -200.0, 3840.0, 1400.0,
        ))))
        .with_input_state(Authority::Known(WindowInputState::PassThrough))
        .with_close_requested(Authority::Known(false));
    let pointer = PointerObservation::new(
        POINTER,
        Authority::Known(PointerWindow::None),
        Authority::Known(physical_point(150.0, 160.0)),
        Authority::Known(vec![ButtonObservation::new(
            PointerButton::Primary,
            PointerButtonState::Released,
        )]),
    )
    .expect("test pointer roster must be unambiguous");
    PlatformSnapshot::new(
        platform_capabilities(native_lifecycle),
        vec![source],
        vec![pointer],
    )
    .expect("test platform snapshot must be canonical")
}

fn fixture(policy: DockPolicy, source_items: &[u64]) -> Fixture {
    let mut builder = Workspace::builder();
    let tabs_a = builder.insert_node(Node::tabs(source_items.iter().copied().map(ItemId::new)));
    let tabs_b = builder.insert_node(Node::tabs([ItemId::new(3)]));
    builder.set_root(ROOT_A, RootRecord::new(tabs_a));
    builder.set_root(ROOT_B, RootRecord::new(tabs_b));
    builder.set_surface(SURFACE_A, SurfacePresentation::new(ROOT_A));
    builder.set_surface(SURFACE_B, SurfacePresentation::new(ROOT_B));
    let workspace = builder.build().expect("test workspace must be valid");
    let engine = DockEngine::new(workspace, policy).expect("test engine must be valid");
    Fixture { engine, tabs_a }
}

fn publish_platform(fixture: &mut Fixture, native_lifecycle: PlatformCapability) {
    fixture
        .engine
        .enqueue_platform_snapshot(platform_snapshot(native_lifecycle))
        .expect("platform snapshot sequence must be available");
    let transition = fixture
        .engine
        .reduce_pending()
        .expect("platform snapshot must publish");
    assert!(matches!(
        transition.reduced_inputs()[0].outcome(),
        InputOutcome::PlatformSnapshotPublished { .. }
    ));
}

fn register_native_source(fixture: &mut Fixture, native_lifecycle: PlatformCapability) {
    fixture
        .engine
        .enqueue_viewport_registration(SURFACE_A, SOURCE_WINDOW, ViewportRole::Root, None)
        .expect("viewport registration sequence must be available");
    let transition = fixture
        .engine
        .reduce_pending()
        .expect("viewport registration must reduce");
    assert!(matches!(
        transition.reduced_inputs()[0].outcome(),
        InputOutcome::ViewportRegistered { binding }
            if binding.surface() == SURFACE_A && binding.token() == SOURCE_WINDOW
    ));
    publish_platform(fixture, native_lifecycle);
}

fn unsupported_native_lifecycle() -> PlatformCapability {
    PlatformCapability::unsupported(
        PlatformRequirement::NativeWindowLifecycle,
        PlatformCapabilityReason::BackendUnsupported,
    )
}

fn unknown_native_lifecycle() -> PlatformCapability {
    PlatformCapability::unknown(
        PlatformRequirement::NativeWindowLifecycle,
        PlatformCapabilityReason::EnvironmentUnavailable,
    )
}

fn publish_scene(fixture: &mut Fixture) {
    let mut scene = BuildingScene::new([SURFACE_A, SURFACE_B]).expect("test roster must be unique");
    scene
        .insert_ready(ReadySurfaceScene::new(
            SURFACE_A,
            logical_rect(0.0, 0.0, 400.0, 300.0),
        ))
        .expect("source facts must be complete");
    scene
        .insert_ready(ReadySurfaceScene::new(
            SURFACE_B,
            logical_rect(0.0, 0.0, 400.0, 300.0),
        ))
        .expect("target facts must be complete");
    fixture
        .engine
        .enqueue_scene(scene)
        .expect("scene sequence must be available");
    fixture.engine.reduce_pending().expect("scene must publish");
}

fn arm_and_begin(fixture: &mut Fixture) -> DragSessionId {
    let payload = MovePayload::Item(
        fixture
            .engine
            .workspace()
            .capture_item_source(ROOT_A, fixture.tabs_a, ItemId::new(1))
            .expect("source item must be current"),
    );
    fixture
        .engine
        .enqueue_renderer_intent(RendererIntent::ArmDrag {
            pointer: POINTER,
            button: PointerButton::Primary,
            payload,
        })
        .expect("arm sequence must be available");
    let transition = fixture.engine.reduce_pending().expect("arm must reduce");
    let session = match transition.reduced_inputs()[0].outcome() {
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
        .expect("begin sequence must be available");
    fixture.engine.reduce_pending().expect("begin must reduce");
    session
}

fn preview_tear_off(
    fixture: &mut Fixture,
    session: DragSessionId,
    request: TearOffRequest,
) -> InteractionOutcome {
    let target = tear_off_target(fixture, &request);
    fixture
        .engine
        .enqueue_renderer_intent(RendererIntent::UpdateDrag {
            session,
            target,
            tear_off: Some(request),
        })
        .expect("update sequence must be available");
    let transition = fixture
        .engine
        .reduce_pending()
        .expect("tear-off preview must reduce");
    match transition.reduced_inputs()[0].outcome() {
        InputOutcome::InteractionProcessed { outcome, .. } => outcome.clone(),
        outcome => panic!("unexpected preview outcome: {outcome:?}"),
    }
}

fn release_tear_off(
    fixture: &mut Fixture,
    session: DragSessionId,
    request: TearOffRequest,
) -> InteractionOutcome {
    release_tear_off_with_effects(fixture, session, request).0
}

fn release_tear_off_with_effects(
    fixture: &mut Fixture,
    session: DragSessionId,
    request: TearOffRequest,
) -> (InteractionOutcome, Vec<EffectRequest>) {
    let acknowledgement = fixture
        .engine
        .interaction()
        .preview()
        .expect("preview must exist before release")
        .acknowledgement();
    let target = tear_off_target(fixture, &request);
    fixture
        .engine
        .enqueue_renderer_intent(RendererIntent::ReleaseDrag {
            session,
            pointer: POINTER,
            button: PointerButton::Primary,
            button_state: Authority::Known(PointerButtonState::Released),
            target,
            tear_off: Some(request),
        })
        .expect("release sequence must be available");
    fixture
        .engine
        .enqueue_renderer_intent(RendererIntent::AcknowledgePreview(acknowledgement))
        .expect("ack sequence must be available");
    let transition = fixture
        .engine
        .reduce_pending()
        .expect("tear-off release must reduce");
    let outcome = match transition.reduced_inputs()[1].outcome() {
        InputOutcome::InteractionProcessed { outcome, .. } => outcome.clone(),
        outcome => panic!("unexpected release outcome: {outcome:?}"),
    };
    (outcome, transition.platform_effects().to_vec())
}

fn contained_proposal(root: RootId, x: f64) -> ContainedTearOffProposal {
    ContainedTearOffProposal::new(
        SURFACE_B,
        root,
        FLOATING_NEW,
        logical_rect(x, 20.0, 240.0, 180.0),
        7,
    )
}

fn native_request(
    fixture: &Fixture,
    surface: SurfaceId,
    root: RootId,
    placement: LogicalRect,
    recovery: ContainedTearOffProposal,
    contained_fallback: Option<ContainedTearOffProposal>,
) -> TearOffRequest {
    let placement = fixture
        .engine
        .viewport_placement(SURFACE_A, placement)
        .expect("current source viewport facts must produce a placement proof");
    TearOffRequest::Native {
        proposal: NativeTearOffProposal::new(surface, root, placement, recovery),
        contained_fallback,
    }
}

fn tear_off_target(fixture: &Fixture, request: &TearOffRequest) -> TargetAuthority {
    match request {
        TearOffRequest::Contained(_) => TargetAuthority::local(SURFACE_A, Authority::Known(None)),
        TearOffRequest::Native { .. } => TargetAuthority::routed(
            fixture
                .engine
                .viewport()
                .route(POINTER)
                .expect("current platform snapshot must publish a route proof")
                .clone(),
        ),
    }
}

#[test]
fn contained_preview_and_delivery_use_the_same_exact_command() {
    let mut fixture = fixture(DockPolicy::default(), &[1, 2]);
    publish_scene(&mut fixture);
    let session = arm_and_begin(&mut fixture);
    let request = TearOffRequest::Contained(contained_proposal(ROOT_NEW, 10.0));

    let preview = preview_tear_off(&mut fixture, session, request.clone());
    assert!(matches!(
        preview,
        InteractionOutcome::PreviewUpdated {
            status: PreviewResolutionStatus::Resolved,
            preview: Some(ref preview),
            ..
        } if matches!(preview.visual(), PreviewVisual::Contained { fallback: false, .. })
    ));
    let delivery = release_tear_off(&mut fixture, session, request);
    assert!(matches!(
        delivery,
        InteractionOutcome::DragDelivered {
            delivery: InteractionDelivery::Workspace {
                kind: WorkspaceDeliveryKind::Contained,
                changed: true,
                ..
            },
            ..
        }
    ));
    assert!(fixture.engine.workspace().root(ROOT_NEW).is_some());
    let floating = fixture
        .engine
        .workspace()
        .contained_floating(FLOATING_NEW)
        .expect("contained presentation must exist");
    assert_eq!(floating.root, ROOT_NEW);
    assert_eq!(floating.surface, SURFACE_B);
}

#[test]
fn native_release_requests_a_create_saga_without_moving_content() {
    let mut policy = DockPolicy::default();
    policy.set_allow_native_surfaces(true);
    let mut fixture = fixture(policy, &[1, 2]);
    register_native_source(&mut fixture, PlatformCapability::Supported);
    publish_scene(&mut fixture);
    let session = arm_and_begin(&mut fixture);
    publish_platform(&mut fixture, PlatformCapability::Supported);
    let request = native_request(
        &fixture,
        SURFACE_NEW,
        ROOT_NEW,
        logical_rect(100.0, 120.0, 640.0, 480.0),
        contained_proposal(ROOT_NEW, 10.0),
        None,
    );
    let before = fixture.engine.workspace().clone();
    let version = fixture.engine.version();

    let preview = preview_tear_off(&mut fixture, session, request.clone());
    assert!(matches!(
        preview,
        InteractionOutcome::PreviewUpdated {
            preview: Some(ref preview),
            ..
        } if matches!(preview.visual(), PreviewVisual::Native { .. })
    ));
    let (delivery, effects) = release_tear_off_with_effects(&mut fixture, session, request);
    let requested = match delivery {
        InteractionOutcome::DragDelivered {
            delivery: InteractionDelivery::NativeRequested(requested),
            ..
        } => requested,
        outcome => panic!("unexpected native delivery: {outcome:?}"),
    };
    let saga = fixture
        .engine
        .viewport()
        .native_create_saga(requested.saga())
        .expect("native create saga must remain queryable");
    assert_eq!(saga.prepared().source_version(), version);
    assert_eq!(saga.prepared().proposal().surface(), SURFACE_NEW);
    assert_eq!(
        saga.prepared().command(),
        &dockspace::command::WorkspaceCommand::CreateSurfaceRoot {
            surface: SURFACE_NEW,
            root: ROOT_NEW,
            content: dockspace::command::RootContent::Move(MovePayload::Item(
                before
                    .capture_item_source(ROOT_A, fixture.tabs_a, ItemId::new(1))
                    .expect("original source must be capturable")
            )),
        }
    );
    assert_eq!(requested.binding(), saga.binding());
    assert_eq!(requested.effect(), saga.effect());
    let create_effects: Vec<_> = effects
        .iter()
        .filter(|effect| matches!(effect.effect(), PlatformEffect::CreateWindow { .. }))
        .collect();
    assert_eq!(create_effects.len(), 1);
    assert_eq!(create_effects[0].id(), requested.effect());
    assert!(matches!(
        create_effects[0].effect(),
        PlatformEffect::CreateWindow {
            binding,
            placement,
            role: ViewportRole::Child,
        } if *binding == requested.binding()
            && *placement == physical_rect(100.0, 120.0, 640.0, 480.0)
    ));
    assert_eq!(
        fixture
            .engine
            .viewport()
            .effects()
            .record(requested.effect())
            .expect("create effect must remain queryable")
            .phase(),
        EffectPhase::Requested
    );
    assert_eq!(fixture.engine.workspace(), &before);
    assert_eq!(fixture.engine.version(), version);
    assert_eq!(
        fixture.engine.interaction().status(),
        InteractionStatus::Idle
    );
    assert!(fixture.engine.scene().is_some());
}

#[test]
fn identical_platform_facts_keep_a_native_placement_valid_through_delivery() {
    let mut policy = DockPolicy::default();
    policy.set_allow_native_surfaces(true);
    let mut fixture = fixture(policy, &[1, 2]);
    register_native_source(&mut fixture, PlatformCapability::Supported);
    publish_scene(&mut fixture);
    let session = arm_and_begin(&mut fixture);
    publish_platform(&mut fixture, PlatformCapability::Supported);
    let request = native_request(
        &fixture,
        SURFACE_NEW,
        ROOT_NEW,
        logical_rect(100.0, 120.0, 640.0, 480.0),
        contained_proposal(ROOT_NEW, 10.0),
        None,
    );

    let preview = preview_tear_off(&mut fixture, session, request.clone());
    assert!(matches!(
        preview,
        InteractionOutcome::PreviewUpdated {
            preview: Some(ref preview),
            ..
        } if matches!(preview.visual(), PreviewVisual::Native { .. })
    ));
    publish_platform(&mut fixture, PlatformCapability::Supported);

    let delivery = release_tear_off(&mut fixture, session, request);
    assert!(matches!(
        delivery,
        InteractionOutcome::DragDelivered {
            delivery: InteractionDelivery::NativeRequested(_),
            ..
        }
    ));
}

#[test]
fn native_tear_off_rejects_an_existing_surface_even_for_a_complete_root() {
    let mut policy = DockPolicy::default();
    policy.set_allow_native_surfaces(true);
    let mut fixture = fixture(policy, &[1]);
    register_native_source(&mut fixture, PlatformCapability::Supported);
    publish_scene(&mut fixture);
    let session = arm_and_begin(&mut fixture);
    publish_platform(&mut fixture, PlatformCapability::Supported);
    let request = native_request(
        &fixture,
        SURFACE_A,
        ROOT_A,
        logical_rect(100.0, 120.0, 640.0, 480.0),
        contained_proposal(ROOT_A, 10.0),
        None,
    );

    let outcome = preview_tear_off(&mut fixture, session, request);
    assert!(matches!(
        outcome,
        InteractionOutcome::PreviewUpdated {
            preview: None,
            status: PreviewResolutionStatus::Rejected,
            ..
        }
    ));
    assert!(fixture.engine.workspace().surface(SURFACE_A).is_some());
    assert!(fixture.engine.interaction().preview().is_none());
}

#[test]
fn native_unavailable_uses_contained_only_when_fallback_is_explicitly_enabled() {
    let mut disabled_policy = DockPolicy::default();
    disabled_policy.set_allow_native_surfaces(true);
    let mut disabled = fixture(disabled_policy, &[1, 2]);
    register_native_source(&mut disabled, unsupported_native_lifecycle());
    publish_scene(&mut disabled);
    let disabled_session = arm_and_begin(&mut disabled);
    publish_platform(&mut disabled, unsupported_native_lifecycle());
    let disabled_request = native_request(
        &disabled,
        SURFACE_NEW,
        ROOT_NEW,
        logical_rect(10.0, 10.0, 500.0, 400.0),
        contained_proposal(ROOT_NEW, 10.0),
        Some(contained_proposal(ROOT_NEW, 10.0)),
    );
    let outcome = preview_tear_off(&mut disabled, disabled_session, disabled_request);
    assert!(matches!(
        outcome,
        InteractionOutcome::PreviewUpdated {
            preview: None,
            status: PreviewResolutionStatus::Rejected,
            ..
        }
    ));

    let mut enabled_policy = DockPolicy::default();
    enabled_policy.set_allow_native_surfaces(true);
    enabled_policy.set_contained_fallback(ContainedFallback::Enabled);
    let mut enabled = fixture(enabled_policy, &[1, 2]);
    register_native_source(&mut enabled, unsupported_native_lifecycle());
    publish_scene(&mut enabled);
    let enabled_session = arm_and_begin(&mut enabled);
    publish_platform(&mut enabled, unsupported_native_lifecycle());
    let enabled_request = native_request(
        &enabled,
        SURFACE_NEW,
        ROOT_NEW,
        logical_rect(10.0, 10.0, 500.0, 400.0),
        contained_proposal(ROOT_NEW, 10.0),
        Some(contained_proposal(ROOT_NEW, 10.0)),
    );
    let preview = preview_tear_off(&mut enabled, enabled_session, enabled_request.clone());
    assert!(matches!(
        preview,
        InteractionOutcome::PreviewUpdated {
            preview: Some(ref preview),
            ..
        } if matches!(preview.visual(), PreviewVisual::Contained { fallback: true, .. })
    ));
    let delivery = release_tear_off(&mut enabled, enabled_session, enabled_request);
    assert!(matches!(
        delivery,
        InteractionOutcome::DragDelivered {
            delivery: InteractionDelivery::Workspace {
                kind: WorkspaceDeliveryKind::ContainedFallback,
                changed: true,
                ..
            },
            ..
        }
    ));
}

#[test]
fn unknown_native_capability_cancels_instead_of_falling_back() {
    let mut policy = DockPolicy::default();
    policy.set_allow_native_surfaces(true);
    policy.set_contained_fallback(ContainedFallback::Enabled);
    let mut fixture = fixture(policy, &[1, 2]);
    register_native_source(&mut fixture, unknown_native_lifecycle());
    publish_scene(&mut fixture);
    let session = arm_and_begin(&mut fixture);
    publish_platform(&mut fixture, unknown_native_lifecycle());
    let request = native_request(
        &fixture,
        SURFACE_NEW,
        ROOT_NEW,
        logical_rect(10.0, 10.0, 500.0, 400.0),
        contained_proposal(ROOT_NEW, 10.0),
        Some(contained_proposal(ROOT_NEW, 10.0)),
    );

    let outcome = preview_tear_off(&mut fixture, session, request);
    assert!(matches!(
        outcome,
        InteractionOutcome::Cancelled {
            reason: InteractionCancelReason::NativeCapabilityUnknown,
            ..
        }
    ));
    assert_eq!(
        fixture.engine.interaction().status(),
        InteractionStatus::Idle
    );
}

#[test]
fn release_rejects_a_changed_tear_off_placement_even_after_ack() {
    let mut fixture = fixture(DockPolicy::default(), &[1, 2]);
    publish_scene(&mut fixture);
    let session = arm_and_begin(&mut fixture);
    let painted = TearOffRequest::Contained(contained_proposal(ROOT_NEW, 10.0));
    let changed = TearOffRequest::Contained(contained_proposal(ROOT_NEW, 30.0));
    preview_tear_off(&mut fixture, session, painted);
    let before = fixture.engine.workspace().clone();

    let outcome = release_tear_off(&mut fixture, session, changed);
    assert!(matches!(
        outcome,
        InteractionOutcome::Rejected(InteractionRejection::TargetChanged)
    ));
    assert_eq!(fixture.engine.workspace(), &before);
}

#[test]
fn complete_root_tear_off_preserves_root_identity_and_rejects_replacement_identity() {
    let mut mismatch = fixture(DockPolicy::default(), &[1]);
    publish_scene(&mut mismatch);
    let mismatch_session = arm_and_begin(&mut mismatch);
    let outcome = preview_tear_off(
        &mut mismatch,
        mismatch_session,
        TearOffRequest::Contained(contained_proposal(ROOT_NEW, 10.0)),
    );
    assert!(matches!(
        outcome,
        InteractionOutcome::PreviewUpdated {
            preview: None,
            status: PreviewResolutionStatus::Rejected,
            ..
        }
    ));

    let mut preserved = fixture(DockPolicy::default(), &[1]);
    publish_scene(&mut preserved);
    let session = arm_and_begin(&mut preserved);
    let request = TearOffRequest::Contained(contained_proposal(ROOT_A, 10.0));
    preview_tear_off(&mut preserved, session, request.clone());
    let delivery = release_tear_off(&mut preserved, session, request);
    assert!(matches!(delivery, InteractionOutcome::DragDelivered { .. }));
    assert!(preserved.engine.workspace().root(ROOT_A).is_some());
    assert!(preserved.engine.workspace().root(ROOT_NEW).is_none());
    assert!(preserved.engine.workspace().surface(SURFACE_A).is_none());
    assert_eq!(
        preserved
            .engine
            .workspace()
            .contained_floating(FLOATING_NEW)
            .expect("root must be rehomed")
            .root,
        ROOT_A
    );
}
