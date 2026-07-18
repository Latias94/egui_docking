use dockspace::command::{MovePayload, RootContent, WorkspaceCommand};
use dockspace::effect::{
    DispatchFailureReason, EffectDispatchResult, EffectIndeterminateReason, EffectPhase,
    EffectRequest, EffectResult, EffectTransition, PlatformEffect,
};
use dockspace::engine::{DockEngine, EngineError, EngineInput};
use dockspace::frame::{
    NativeCreateRequest, NativeCreateStatus, RestoreReplacementStatus, RetiredViewportStatus,
    ViewportCloseDecision, ViewportClosePlan, ViewportCloseRequestId, ViewportCloseStatus,
};
use dockspace::geometry::{LogicalRect, PhysicalPoint, PhysicalRect, ScaleFactor};
use dockspace::graph::{Axis, Node, RootRecord, SurfacePresentation, Workspace};
use dockspace::ids::{FloatingPresentationId, ItemId, NodeId, RootId, SurfaceId};
use dockspace::intent::{
    Authority, ContainedTearOffProposal, NativeTearOffProposal, PointerButton, PointerButtonState,
    PointerId, RendererIntent, TargetAuthority, TearOffRequest,
};
use dockspace::interaction::{
    DragSessionId, InteractionDelivery, InteractionOutcome, InteractionStatus,
    PreviewResolutionStatus,
};
use dockspace::platform::{
    ButtonObservation, ObservedWindow, ObservedWorkArea, PlatformCapabilities, PlatformCapability,
    PlatformSnapshot, PointerObservation, PointerWindow, WindowInputState,
};
use dockspace::policy::DockPolicy;
use dockspace::scene::{BuildingScene, ReadySurfaceScene};
use dockspace::transition::{EngineTransition, InputOutcome};
use dockspace::viewport::{ViewportBinding, ViewportRole, WindowToken, WorkAreaToken};
use dockspace::viewport_registry::ViewportLifecycle;

const ROOT_SOURCE: RootId = RootId::new(1);
const ROOT_HOST: RootId = RootId::new(2);
const ROOT_NATIVE: RootId = RootId::new(10);
const SURFACE_SOURCE: SurfaceId = SurfaceId::new(1);
const SURFACE_HOST: SurfaceId = SurfaceId::new(2);
const SURFACE_NATIVE: SurfaceId = SurfaceId::new(10);
const FLOATING_RECOVERY: FloatingPresentationId = FloatingPresentationId::new(10);
const SOURCE_TOKEN: WindowToken = WindowToken::new(41);
const HOST_TOKEN: WindowToken = WindowToken::new(42);
const WORK_AREA: WorkAreaToken = WorkAreaToken::new(51);
const POINTER: PointerId = PointerId::new(1);

struct Fixture {
    engine: DockEngine,
    source_tabs: NodeId,
}

fn logical_rect(x: f64, y: f64, width: f64, height: f64) -> LogicalRect {
    LogicalRect::new(x, y, width, height).expect("test logical rectangle must be valid")
}

fn physical_rect(x: f64, y: f64, width: f64, height: f64) -> PhysicalRect {
    PhysicalRect::new(x, y, width, height).expect("test physical rectangle must be valid")
}

fn base_workspace_with_items(source_items: &[u64], host_items: &[u64]) -> (Workspace, NodeId) {
    let mut builder = Workspace::builder();
    let source_tabs =
        builder.insert_node(Node::tabs(source_items.iter().copied().map(ItemId::new)));
    let host_tabs = builder.insert_node(Node::tabs(host_items.iter().copied().map(ItemId::new)));
    builder.set_root(ROOT_SOURCE, RootRecord::new(source_tabs));
    builder.set_root(ROOT_HOST, RootRecord::new(host_tabs));
    builder.set_surface(SURFACE_SOURCE, SurfacePresentation::new(ROOT_SOURCE));
    builder.set_surface(SURFACE_HOST, SurfacePresentation::new(ROOT_HOST));
    (
        builder.build().expect("test workspace must be valid"),
        source_tabs,
    )
}

fn base_workspace_with_source_items(source_items: &[u64]) -> (Workspace, NodeId) {
    base_workspace_with_items(source_items, &[3])
}

fn base_workspace() -> (Workspace, NodeId) {
    base_workspace_with_source_items(&[1, 2])
}

fn source_only_workspace() -> Workspace {
    let mut builder = Workspace::builder();
    let source_tabs = builder.insert_node(Node::tabs([ItemId::new(1), ItemId::new(2)]));
    builder.set_root(ROOT_SOURCE, RootRecord::new(source_tabs));
    builder.set_surface(SURFACE_SOURCE, SurfacePresentation::new(ROOT_SOURCE));
    builder
        .build()
        .expect("replacement workspace must be valid")
}

fn source_and_new_surface_workspace() -> Workspace {
    let mut builder = Workspace::builder();
    let source_tabs = builder.insert_node(Node::tabs([ItemId::new(1), ItemId::new(2)]));
    let new_tabs = builder.insert_node(Node::tabs([ItemId::new(5)]));
    builder.set_root(ROOT_SOURCE, RootRecord::new(source_tabs));
    builder.set_root(ROOT_NATIVE, RootRecord::new(new_tabs));
    builder.set_surface(SURFACE_SOURCE, SurfacePresentation::new(ROOT_SOURCE));
    builder.set_surface(SURFACE_NATIVE, SurfacePresentation::new(ROOT_NATIVE));
    builder
        .build()
        .expect("replacement workspace must be valid")
}

fn fixture() -> Fixture {
    let (workspace, source_tabs) = base_workspace();
    let mut policy = DockPolicy::default();
    policy.set_allow_native_surfaces(true);
    let engine = DockEngine::new(workspace, policy).expect("test engine must be valid");
    Fixture {
        engine,
        source_tabs,
    }
}

fn fixture_with_source_items(source_items: &[u64]) -> Fixture {
    let (workspace, source_tabs) = base_workspace_with_source_items(source_items);
    let mut policy = DockPolicy::default();
    policy.set_allow_native_surfaces(true);
    let engine = DockEngine::new(workspace, policy).expect("test engine must be valid");
    Fixture {
        engine,
        source_tabs,
    }
}

fn fixture_with_host_items(host_items: &[u64]) -> Fixture {
    let (workspace, source_tabs) = base_workspace_with_items(&[1, 2], host_items);
    let mut policy = DockPolicy::default();
    policy.set_allow_native_surfaces(true);
    let engine = DockEngine::new(workspace, policy).expect("test engine must be valid");
    Fixture {
        engine,
        source_tabs,
    }
}

fn fixture_with_split_source() -> (Fixture, NodeId) {
    let mut builder = Workspace::builder();
    let source_tabs = builder.insert_node(Node::tabs([ItemId::new(1)]));
    let source_peer = builder.insert_node(Node::tabs([ItemId::new(2)]));
    let source_split = builder.insert_node(
        Node::equal_split(Axis::Horizontal, [source_tabs, source_peer])
            .expect("test split must be valid"),
    );
    let host_tabs = builder.insert_node(Node::tabs([ItemId::new(3)]));
    builder.set_root(ROOT_SOURCE, RootRecord::new(source_split));
    builder.set_root(ROOT_HOST, RootRecord::new(host_tabs));
    builder.set_surface(SURFACE_SOURCE, SurfacePresentation::new(ROOT_SOURCE));
    builder.set_surface(SURFACE_HOST, SurfacePresentation::new(ROOT_HOST));
    let workspace = builder.build().expect("test workspace must be valid");
    let mut policy = DockPolicy::default();
    policy.set_allow_native_surfaces(true);
    let engine = DockEngine::new(workspace, policy).expect("test engine must be valid");
    (
        Fixture {
            engine,
            source_tabs,
        },
        source_split,
    )
}

fn platform_capabilities() -> PlatformCapabilities {
    let mut capabilities = PlatformCapabilities::default();
    capabilities.set_native_window_lifecycle(PlatformCapability::Supported);
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

fn observed_window(
    token: WindowToken,
    x: f64,
    close_requested: bool,
    input_state: WindowInputState,
) -> ObservedWindow {
    ObservedWindow::new(token)
        .with_content_bounds(Authority::Known(physical_rect(x, 0.0, 900.0, 700.0)))
        .with_outer_bounds(Authority::Known(physical_rect(
            x - 8.0,
            -30.0,
            916.0,
            738.0,
        )))
        .with_scale_factor(Authority::Known(
            ScaleFactor::new(1.0).expect("test scale factor must be valid"),
        ))
        .with_input_state(Authority::Known(input_state))
        .with_close_requested(Authority::Known(close_requested))
}

fn source_window(close_requested: bool) -> ObservedWindow {
    observed_window(
        SOURCE_TOKEN,
        0.0,
        close_requested,
        WindowInputState::PassThrough,
    )
}

fn host_window(close_requested: bool) -> ObservedWindow {
    observed_window(
        HOST_TOKEN,
        900.0,
        close_requested,
        WindowInputState::ReceivesInput,
    )
}

fn unavailable_host_window() -> ObservedWindow {
    ObservedWindow::new(HOST_TOKEN).with_close_requested(Authority::Known(false))
}

fn pointer_observation() -> PointerObservation {
    PointerObservation::new(
        POINTER,
        Authority::Known(PointerWindow::None),
        Authority::Known(
            PhysicalPoint::new(150.0, 160.0).expect("test desktop point must be valid"),
        ),
        Authority::Known(vec![ButtonObservation::new(
            PointerButton::Primary,
            PointerButtonState::Released,
        )]),
    )
    .expect("test pointer observation must be canonical")
}

fn platform_snapshot(windows: Vec<ObservedWindow>) -> PlatformSnapshot {
    PlatformSnapshot::new(
        platform_capabilities(),
        windows,
        vec![pointer_observation()],
        vec![ObservedWorkArea::new(
            WORK_AREA,
            physical_rect(-1920.0, -200.0, 3840.0, 1400.0),
            ScaleFactor::new(1.0).expect("test work-area scale factor must be valid"),
        )],
    )
    .expect("test platform snapshot must be canonical")
}

fn publish_windows(fixture: &mut Fixture, windows: Vec<ObservedWindow>) -> EngineTransition {
    fixture
        .engine
        .enqueue_platform_snapshot(platform_snapshot(windows))
        .expect("platform snapshot must enqueue");
    let transition = fixture
        .engine
        .reduce_pending()
        .expect("platform snapshot must reduce");
    assert!(matches!(
        transition.reduced_inputs()[0].outcome(),
        InputOutcome::PlatformSnapshotPublished { .. }
    ));
    transition
}

fn register_base_viewports(
    fixture: &mut Fixture,
    host_role: ViewportRole,
) -> (ViewportBinding, ViewportBinding) {
    fixture
        .engine
        .enqueue_viewport_registration(SURFACE_SOURCE, SOURCE_TOKEN, ViewportRole::Root, None)
        .expect("source viewport registration must enqueue");
    fixture
        .engine
        .enqueue_viewport_registration(
            SURFACE_HOST,
            HOST_TOKEN,
            host_role,
            Some(close_recovery(ROOT_HOST, FloatingPresentationId::new(70))),
        )
        .expect("host viewport registration must enqueue");
    let transition = fixture
        .engine
        .reduce_pending()
        .expect("viewport registrations must reduce");
    let bindings: Vec<_> = transition
        .reduced_inputs()
        .iter()
        .map(|reduced| match reduced.outcome() {
            InputOutcome::ViewportRegistered { binding } => *binding,
            outcome => panic!("unexpected registration outcome: {outcome:?}"),
        })
        .collect();
    (bindings[0], bindings[1])
}

fn prepare_base_platform(fixture: &mut Fixture, host_role: ViewportRole) {
    register_base_viewports(fixture, host_role);
    publish_windows(fixture, vec![source_window(false), host_window(false)]);
}

fn publish_scene(fixture: &mut Fixture) {
    let surfaces: Vec<_> = fixture
        .engine
        .workspace()
        .surfaces()
        .map(|(surface, _)| surface)
        .collect();
    let mut scene =
        BuildingScene::new(surfaces.iter().copied()).expect("test surface roster must be unique");
    let mut x = 0.0;
    for surface in surfaces {
        scene
            .insert_ready(ReadySurfaceScene::new(
                surface,
                logical_rect(x, 0.0, 900.0, 700.0),
            ))
            .expect("surface scene must be complete");
        x += 900.0;
    }
    fixture
        .engine
        .enqueue_scene(scene)
        .expect("scene must enqueue");
    fixture.engine.reduce_pending().expect("scene must publish");
}

fn arm_and_begin_with_payload(fixture: &mut Fixture, payload: MovePayload) -> DragSessionId {
    fixture
        .engine
        .enqueue_renderer_intent(RendererIntent::ArmDrag {
            pointer: POINTER,
            button: PointerButton::Primary,
            payload,
        })
        .expect("arm drag must enqueue");
    let transition = fixture
        .engine
        .reduce_pending()
        .expect("arm drag must reduce");
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
        .expect("begin drag must enqueue");
    let transition = fixture
        .engine
        .reduce_pending()
        .expect("begin drag must reduce");
    assert!(matches!(
        transition.reduced_inputs()[0].outcome(),
        InputOutcome::InteractionProcessed {
            outcome: InteractionOutcome::DragBegan { .. },
            ..
        }
    ));
    session
}

fn recovery(root: RootId, floating: FloatingPresentationId) -> ContainedTearOffProposal {
    ContainedTearOffProposal::new(
        SURFACE_HOST,
        root,
        floating,
        logical_rect(40.0, 50.0, 480.0, 360.0),
        7,
    )
}

fn close_recovery(root: RootId, floating: FloatingPresentationId) -> ContainedTearOffProposal {
    ContainedTearOffProposal::new(
        SURFACE_SOURCE,
        root,
        floating,
        logical_rect(30.0, 40.0, 420.0, 320.0),
        9,
    )
}

fn current_route(fixture: &Fixture) -> TargetAuthority {
    TargetAuthority::routed(
        fixture
            .engine
            .viewport()
            .route(POINTER)
            .expect("current platform snapshot must publish a route proof")
            .clone(),
    )
}

fn start_native_create(fixture: &mut Fixture) -> NativeCreateRequest {
    let payload = MovePayload::Item(
        fixture
            .engine
            .workspace()
            .capture_item_source(ROOT_SOURCE, fixture.source_tabs, ItemId::new(1))
            .expect("source item must be current"),
    );
    start_native_create_with_payload(fixture, payload, ROOT_NATIVE)
}

fn start_native_create_with_payload(
    fixture: &mut Fixture,
    payload: MovePayload,
    destination_root: RootId,
) -> NativeCreateRequest {
    publish_scene(fixture);
    let session = arm_and_begin_with_payload(fixture, payload);
    publish_windows(fixture, vec![source_window(false), host_window(false)]);
    let placement = fixture
        .engine
        .viewport_placement(
            SURFACE_SOURCE,
            logical_rect(100.0, 120.0, 640.0, 480.0),
            WORK_AREA,
        )
        .expect("current source facts must produce a placement proof");
    let request = TearOffRequest::native(
        NativeTearOffProposal::new(
            SURFACE_NATIVE,
            destination_root,
            placement,
            recovery(destination_root, FLOATING_RECOVERY),
        ),
        None,
    );
    let preview_target = current_route(fixture);
    fixture
        .engine
        .enqueue_renderer_intent(RendererIntent::UpdateDrag {
            session,
            target: preview_target,
            tear_off: Some(request.clone()),
        })
        .expect("native preview must enqueue");
    let preview = fixture
        .engine
        .reduce_pending()
        .expect("native preview must reduce");
    assert!(matches!(
        preview.reduced_inputs()[0].outcome(),
        InputOutcome::InteractionProcessed {
            outcome: InteractionOutcome::PreviewUpdated {
                status: PreviewResolutionStatus::Resolved,
                preview: Some(_),
                ..
            },
            ..
        }
    ));
    let acknowledgement = fixture
        .engine
        .interaction()
        .preview()
        .expect("native preview must exist")
        .acknowledgement();
    let release_target = current_route(fixture);
    fixture
        .engine
        .enqueue_renderer_intent(RendererIntent::ReleaseDrag {
            session,
            pointer: POINTER,
            button: PointerButton::Primary,
            button_state: Authority::Known(PointerButtonState::Released),
            target: release_target,
            tear_off: Some(request),
        })
        .expect("native release must enqueue");
    fixture
        .engine
        .enqueue_renderer_intent(RendererIntent::AcknowledgePreview(acknowledgement))
        .expect("preview acknowledgement must enqueue");
    let transition = fixture
        .engine
        .reduce_pending()
        .expect("native release must reduce");
    let requested = transition
        .reduced_inputs()
        .iter()
        .find_map(|reduced| match reduced.outcome() {
            InputOutcome::InteractionProcessed {
                outcome:
                    InteractionOutcome::DragDelivered {
                        delivery: InteractionDelivery::NativeRequested(request),
                        ..
                    },
                ..
            } => Some(*request),
            _ => None,
        })
        .expect("native release must create one saga");
    assert!(transition.platform_effects().iter().any(|effect| {
        effect.id() == requested.effect()
            && matches!(
                effect.effect(),
                PlatformEffect::CreateWindow { binding, .. }
                    if *binding == requested.binding()
            )
    }));
    requested
}

fn report_create_result(
    fixture: &mut Fixture,
    request: NativeCreateRequest,
    result: EffectDispatchResult,
) -> EngineTransition {
    fixture
        .engine
        .enqueue_platform_effect_result(EffectResult::new(
            request.effect(),
            request.binding().epoch(),
            result,
        ))
        .expect("effect result must enqueue");
    fixture
        .engine
        .reduce_pending()
        .expect("effect result must reduce")
}

fn native_window(request: NativeCreateRequest, close_requested: bool) -> ObservedWindow {
    observed_window(
        request.binding().token(),
        1800.0,
        close_requested,
        WindowInputState::ReceivesInput,
    )
}

fn close_request_from(transition: &EngineTransition) -> ViewportCloseRequestId {
    transition
        .reduced_inputs()
        .iter()
        .find_map(|reduced| match reduced.outcome() {
            InputOutcome::PlatformSnapshotPublished { transition } => {
                transition.close_requests().first().copied()
            }
            _ => None,
        })
        .expect("snapshot must publish one close-request edge")
}

fn decide_close(
    fixture: &mut Fixture,
    request: ViewportCloseRequestId,
    decision: ViewportCloseDecision,
) -> EngineTransition {
    fixture
        .engine
        .enqueue_viewport_close_decision(request, decision)
        .expect("close decision must enqueue");
    fixture
        .engine
        .reduce_pending()
        .expect("close decision must reduce")
}

fn effect_of_kind(
    effects: &[EffectRequest],
    predicate: impl Fn(&PlatformEffect) -> bool,
) -> &EffectRequest {
    let matching: Vec<_> = effects
        .iter()
        .filter(|request| predicate(request.effect()))
        .collect();
    assert_eq!(matching.len(), 1, "expected exactly one matching effect");
    matching[0]
}

fn observe_ready_and_assert_compensation(fixture: &mut Fixture, request: NativeCreateRequest) {
    let ready = publish_windows(
        fixture,
        vec![
            source_window(false),
            host_window(false),
            native_window(request, false),
        ],
    );
    let compensation = effect_of_kind(ready.platform_effects(), |effect| {
        matches!(
            effect,
            PlatformEffect::CompensatingClose {
                binding,
                compensates,
            } if *binding == request.binding() && *compensates == request.effect()
        )
    });
    assert!(matches!(
        fixture
            .engine
            .viewport()
            .native_create_saga(request.saga())
            .expect("rejected create saga must remain auditable")
            .status(),
        NativeCreateStatus::Compensating { effect } if effect == compensation.id()
    ));
    assert!(fixture.engine.workspace().surface(SURFACE_NATIVE).is_none());
}

fn assert_old_platform_inputs_are_stale(
    fixture: &mut Fixture,
    request: NativeCreateRequest,
    old_snapshot: PlatformSnapshot,
) {
    let old_epoch = request.binding().epoch();
    fixture
        .engine
        .enqueue(EngineInput::PublishPlatformSnapshot {
            expected_epoch: old_epoch,
            snapshot: old_snapshot,
        })
        .expect("old snapshot must enqueue");
    fixture
        .engine
        .enqueue_platform_effect_result(EffectResult::new(
            request.effect(),
            old_epoch,
            EffectDispatchResult::DispatchFailed(DispatchFailureReason::ProviderStopped),
        ))
        .expect("old effect result must enqueue");
    let stale = fixture
        .engine
        .reduce_pending()
        .expect("stale platform inputs must reduce harmlessly");
    assert!(stale.reduced_inputs().iter().any(|reduced| matches!(
        reduced.outcome(),
        InputOutcome::PlatformSnapshotStale {
            expected_epoch,
            current_epoch,
        } if *expected_epoch == old_epoch && *current_epoch != old_epoch
    )));
    assert!(stale.reduced_inputs().iter().any(|reduced| matches!(
        reduced.outcome(),
        InputOutcome::PlatformEffectReported {
            effect,
            transition: EffectTransition::StaleEpoch,
        } if *effect == request.effect()
    )));
}

#[test]
fn create_commits_only_after_the_reserved_binding_is_observed_ready() {
    let mut fixture = fixture();
    prepare_base_platform(&mut fixture, ViewportRole::Child);
    let before = fixture.engine.workspace().clone();
    let request = start_native_create(&mut fixture);
    assert_eq!(fixture.engine.workspace(), &before);
    assert_eq!(
        fixture
            .engine
            .viewport()
            .native_create_saga(request.saga())
            .expect("create saga must be queryable")
            .status(),
        NativeCreateStatus::Requested
    );

    publish_windows(
        &mut fixture,
        vec![
            source_window(false),
            host_window(false),
            native_window(request, false),
        ],
    );

    assert_eq!(
        fixture
            .engine
            .viewport()
            .native_create_saga(request.saga())
            .expect("committed saga must remain queryable")
            .status(),
        NativeCreateStatus::Committed
    );
    assert!(fixture.engine.workspace().surface(SURFACE_NATIVE).is_some());
    assert!(fixture.engine.workspace().root(ROOT_NATIVE).is_some());
    assert_eq!(
        fixture.engine.workspace().item_multiset(),
        before.item_multiset()
    );
    assert!(matches!(
        fixture
            .engine
            .viewport()
            .effects()
            .record(request.effect())
            .expect("create effect must remain queryable")
            .phase(),
        EffectPhase::ObservedApplied { .. }
    ));
}

#[test]
fn unrelated_viewport_fact_loss_does_not_cancel_a_resize() {
    let (mut fixture, source_split) = fixture_with_split_source();
    prepare_base_platform(&mut fixture, ViewportRole::Child);
    let split = fixture
        .engine
        .workspace()
        .capture_node_source(ROOT_SOURCE, source_split)
        .expect("source split must be current");
    fixture
        .engine
        .enqueue_renderer_intent(RendererIntent::BeginResize {
            pointer: POINTER,
            button: PointerButton::Primary,
            split,
        })
        .expect("resize begin must enqueue");
    let begun = fixture
        .engine
        .reduce_pending()
        .expect("resize begin must reduce");
    let session = match begun.reduced_inputs()[0].outcome() {
        InputOutcome::InteractionProcessed {
            outcome: InteractionOutcome::ResizeBegan { session, .. },
            ..
        } => *session,
        outcome => panic!("unexpected resize outcome: {outcome:?}"),
    };

    publish_windows(
        &mut fixture,
        vec![source_window(false), unavailable_host_window()],
    );

    assert_eq!(
        fixture.engine.interaction().status(),
        InteractionStatus::Resizing { session }
    );
}

#[test]
fn unrelated_viewport_fact_loss_does_not_cancel_a_local_contained_drag() {
    let mut fixture = fixture();
    prepare_base_platform(&mut fixture, ViewportRole::Child);
    publish_scene(&mut fixture);
    let payload = MovePayload::Item(
        fixture
            .engine
            .workspace()
            .capture_item_source(ROOT_SOURCE, fixture.source_tabs, ItemId::new(1))
            .expect("source item must be current"),
    );
    let session = arm_and_begin_with_payload(&mut fixture, payload);
    let request = TearOffRequest::Contained(ContainedTearOffProposal::new(
        SURFACE_SOURCE,
        ROOT_NATIVE,
        FloatingPresentationId::new(71),
        logical_rect(40.0, 50.0, 480.0, 360.0),
        7,
    ));
    fixture
        .engine
        .enqueue_renderer_intent(RendererIntent::UpdateDrag {
            session,
            target: TargetAuthority::local(SURFACE_SOURCE, Authority::Known(None)),
            tear_off: Some(request),
        })
        .expect("contained preview must enqueue");
    fixture
        .engine
        .reduce_pending()
        .expect("contained preview must reduce");
    let preview = fixture
        .engine
        .interaction()
        .preview()
        .expect("contained preview must exist")
        .token();

    publish_windows(
        &mut fixture,
        vec![source_window(false), unavailable_host_window()],
    );

    assert_eq!(
        fixture.engine.interaction().status(),
        InteractionStatus::Dragging { session }
    );
    assert_eq!(
        fixture
            .engine
            .interaction()
            .preview()
            .expect("unrelated facts must preserve the preview")
            .token(),
        preview
    );
}

#[test]
fn create_ready_with_a_stale_source_is_compensated_without_moving_content() {
    let mut fixture = fixture();
    prepare_base_platform(&mut fixture, ViewportRole::Child);
    let request = start_native_create(&mut fixture);
    let initial_items = fixture.engine.workspace().item_multiset();
    let changing_source = fixture
        .engine
        .workspace()
        .capture_item_source(ROOT_SOURCE, fixture.source_tabs, ItemId::new(2))
        .expect("second source item must be current");
    fixture
        .engine
        .enqueue_command(WorkspaceCommand::Select {
            source: changing_source,
        })
        .expect("source mutation must enqueue");
    fixture
        .engine
        .reduce_pending()
        .expect("source mutation must commit");

    let ready = publish_windows(
        &mut fixture,
        vec![
            source_window(false),
            host_window(false),
            native_window(request, false),
        ],
    );
    let compensation = effect_of_kind(ready.platform_effects(), |effect| {
        matches!(
            effect,
            PlatformEffect::CompensatingClose {
                binding,
                compensates,
            } if *binding == request.binding() && *compensates == request.effect()
        )
    });
    assert_eq!(fixture.engine.workspace().item_multiset(), initial_items);
    assert!(fixture.engine.workspace().surface(SURFACE_NATIVE).is_none());
    assert!(matches!(
        fixture
            .engine
            .viewport()
            .native_create_saga(request.saga())
            .expect("failed create commit must remain auditable")
            .status(),
        NativeCreateStatus::Compensating { effect }
            if effect == compensation.id()
    ));
}

#[test]
fn complete_root_create_rejects_frozen_fingerprint_changes() {
    let mut fixture = fixture();
    prepare_base_platform(&mut fixture, ViewportRole::Child);
    let payload = MovePayload::Tabs(
        fixture
            .engine
            .workspace()
            .capture_node_source(ROOT_SOURCE, fixture.source_tabs)
            .expect("complete tabs root must be current"),
    );
    let request = start_native_create_with_payload(&mut fixture, payload, ROOT_SOURCE);
    let changing_source = fixture
        .engine
        .workspace()
        .capture_item_source(ROOT_SOURCE, fixture.source_tabs, ItemId::new(2))
        .expect("second source item must be current");
    fixture
        .engine
        .enqueue_command(WorkspaceCommand::Select {
            source: changing_source,
        })
        .expect("source mutation must enqueue");
    fixture
        .engine
        .reduce_pending()
        .expect("source mutation must commit");

    observe_ready_and_assert_compensation(&mut fixture, request);
    assert_eq!(
        fixture
            .engine
            .workspace()
            .root(ROOT_SOURCE)
            .expect("source root must remain")
            .node,
        fixture.source_tabs
    );
}

#[test]
fn complete_single_item_root_create_rejects_replacement_content() {
    let mut fixture = fixture_with_source_items(&[1]);
    prepare_base_platform(&mut fixture, ViewportRole::Child);
    let payload = MovePayload::Item(
        fixture
            .engine
            .workspace()
            .capture_item_source(ROOT_SOURCE, fixture.source_tabs, ItemId::new(1))
            .expect("single source item must be current"),
    );
    let request = start_native_create_with_payload(&mut fixture, payload, ROOT_SOURCE);
    let source = fixture
        .engine
        .workspace()
        .capture_item_source(ROOT_SOURCE, fixture.source_tabs, ItemId::new(1))
        .expect("single source item must still be current");
    fixture
        .engine
        .enqueue_command(WorkspaceCommand::Close { source })
        .expect("source removal must enqueue");
    fixture
        .engine
        .reduce_pending()
        .expect("source removal must commit");
    fixture
        .engine
        .enqueue_command(WorkspaceCommand::CreateSurfaceRoot {
            surface: SURFACE_SOURCE,
            root: ROOT_SOURCE,
            content: RootContent::OpenItem(ItemId::new(99)),
        })
        .expect("replacement root must enqueue");
    fixture
        .engine
        .reduce_pending()
        .expect("replacement root must commit");

    observe_ready_and_assert_compensation(&mut fixture, request);
    let items = fixture.engine.workspace().item_multiset();
    assert!(items.contains_key(&ItemId::new(99)));
    assert!(!items.contains_key(&ItemId::new(1)));
}

#[test]
fn complete_root_disappearance_rejects_only_the_create_saga() {
    let mut fixture = fixture_with_source_items(&[1]);
    prepare_base_platform(&mut fixture, ViewportRole::Child);
    let payload = MovePayload::Item(
        fixture
            .engine
            .workspace()
            .capture_item_source(ROOT_SOURCE, fixture.source_tabs, ItemId::new(1))
            .expect("single source item must be current"),
    );
    let request = start_native_create_with_payload(&mut fixture, payload, ROOT_SOURCE);
    let source = fixture
        .engine
        .workspace()
        .capture_item_source(ROOT_SOURCE, fixture.source_tabs, ItemId::new(1))
        .expect("single source item must still be current");
    fixture
        .engine
        .enqueue_command(WorkspaceCommand::Close { source })
        .expect("source removal must enqueue");
    fixture
        .engine
        .reduce_pending()
        .expect("source removal must commit");
    assert!(fixture.engine.workspace().root(ROOT_SOURCE).is_none());

    observe_ready_and_assert_compensation(&mut fixture, request);
    assert!(fixture.engine.workspace().root(ROOT_SOURCE).is_none());
}

#[test]
fn dispatch_failure_plus_authoritative_absence_releases_the_reservation_for_retry() {
    let mut fixture = fixture();
    prepare_base_platform(&mut fixture, ViewportRole::Child);
    let first = start_native_create(&mut fixture);

    let reported = report_create_result(
        &mut fixture,
        first,
        EffectDispatchResult::DispatchFailed(DispatchFailureReason::AdapterRejected),
    );
    assert!(matches!(
        reported.reduced_inputs()[0].outcome(),
        InputOutcome::PlatformEffectReported {
            transition: EffectTransition::Applied,
            ..
        }
    ));
    assert_eq!(
        fixture
            .engine
            .viewport()
            .native_create_saga(first.saga())
            .expect("cancelled saga must remain until inventory resolves it")
            .status(),
        NativeCreateStatus::Cancelled
    );

    let absence = publish_windows(&mut fixture, vec![source_window(false), host_window(false)]);
    assert!(absence.platform_effects().is_empty());
    assert!(
        fixture
            .engine
            .viewport()
            .native_create_saga(first.saga())
            .is_none()
    );
    assert!(fixture.engine.viewport().viewport(SURFACE_NATIVE).is_none());

    let retry = start_native_create(&mut fixture);
    assert_ne!(retry.saga(), first.saga());
    assert_ne!(retry.effect(), first.effect());
    assert_eq!(retry.binding().surface(), first.binding().surface());
    assert_ne!(retry.binding().token(), first.binding().token());
    assert_ne!(retry.binding().incarnation(), first.binding().incarnation());
}

#[test]
fn indeterminate_create_absence_releases_the_surface_but_tracks_a_late_window() {
    let mut fixture = fixture();
    prepare_base_platform(&mut fixture, ViewportRole::Child);
    let request = start_native_create(&mut fixture);
    report_create_result(
        &mut fixture,
        request,
        EffectDispatchResult::Indeterminate(EffectIndeterminateReason::AcknowledgementLost),
    );
    assert_eq!(
        fixture
            .engine
            .viewport()
            .native_create_saga(request.saga())
            .expect("indeterminate saga must remain queryable")
            .status(),
        NativeCreateStatus::Indeterminate
    );

    let absence = publish_windows(&mut fixture, vec![source_window(false), host_window(false)]);
    assert!(absence.platform_effects().is_empty());
    assert!(
        fixture
            .engine
            .viewport()
            .native_create_saga(request.saga())
            .is_none()
    );
    assert!(fixture.engine.viewport().viewport(SURFACE_NATIVE).is_none());
    assert!(matches!(
        fixture
            .engine
            .viewport()
            .effects()
            .record(request.effect())
            .expect("indeterminate effect must remain auditable")
            .phase(),
        EffectPhase::Indeterminate(EffectIndeterminateReason::AcknowledgementLost)
    ));
    let tombstone = fixture
        .engine
        .viewport()
        .retired_viewports()
        .find_map(|(token, retired)| (token == request.binding().token()).then_some(retired))
        .expect("indeterminate create must retain a late-appearance tombstone");
    assert!(!tombstone.observed());
    assert!(tombstone.may_appear_late());

    let late = publish_windows(
        &mut fixture,
        vec![
            source_window(false),
            host_window(false),
            native_window(request, false),
        ],
    );
    effect_of_kind(late.platform_effects(), |effect| {
        matches!(
            effect,
            PlatformEffect::CompensatingClose {
                binding,
                compensates,
            } if *binding == request.binding() && *compensates == request.effect()
        )
    });
}

#[test]
fn indeterminate_create_can_be_cancelled_explicitly_without_a_timeout() {
    let mut fixture = fixture();
    prepare_base_platform(&mut fixture, ViewportRole::Child);
    let request = start_native_create(&mut fixture);
    report_create_result(
        &mut fixture,
        request,
        EffectDispatchResult::Indeterminate(EffectIndeterminateReason::AcknowledgementLost),
    );

    fixture
        .engine
        .enqueue_native_create_cancellation(request.saga())
        .expect("explicit create cancellation must enqueue");
    let cancelled = fixture
        .engine
        .reduce_pending()
        .expect("explicit create cancellation must reduce");
    assert!(matches!(
        cancelled.reduced_inputs()[0].outcome(),
        InputOutcome::NativeCreateCancelled {
            saga,
            compensation: None,
        } if *saga == request.saga()
    ));
    assert_eq!(
        fixture
            .engine
            .viewport()
            .native_create_saga(request.saga())
            .expect("cancelled saga must remain until inventory resolves it")
            .status(),
        NativeCreateStatus::Cancelled
    );

    let absence = publish_windows(&mut fixture, vec![source_window(false), host_window(false)]);
    assert!(absence.platform_effects().is_empty());
    assert!(
        fixture
            .engine
            .viewport()
            .native_create_saga(request.saga())
            .is_none()
    );
}

#[test]
fn a_cancelled_create_which_appears_late_is_compensated_exactly_once() {
    let mut fixture = fixture();
    prepare_base_platform(&mut fixture, ViewportRole::Child);
    let request = start_native_create(&mut fixture);
    report_create_result(
        &mut fixture,
        request,
        EffectDispatchResult::DispatchFailed(DispatchFailureReason::WindowUnavailable),
    );

    let late = publish_windows(
        &mut fixture,
        vec![
            source_window(false),
            host_window(false),
            native_window(request, false),
        ],
    );
    let compensation = effect_of_kind(late.platform_effects(), |effect| {
        matches!(
            effect,
            PlatformEffect::CompensatingClose {
                binding,
                compensates,
            } if *binding == request.binding() && *compensates == request.effect()
        )
    });
    assert!(matches!(
        fixture
            .engine
            .viewport()
            .native_create_saga(request.saga())
            .expect("compensating saga must remain queryable")
            .status(),
        NativeCreateStatus::Compensating { effect }
            if effect == compensation.id()
    ));

    let repeated = publish_windows(
        &mut fixture,
        vec![
            source_window(false),
            host_window(false),
            native_window(request, false),
        ],
    );
    assert!(repeated.platform_effects().is_empty());
    assert_eq!(
        fixture
            .engine
            .viewport()
            .effects()
            .records()
            .filter(|(_, record)| matches!(
                record.request().effect(),
                PlatformEffect::CompensatingClose { binding, .. }
                    if *binding == request.binding()
            ))
            .count(),
        1
    );

    publish_windows(&mut fixture, vec![source_window(false), host_window(false)]);
    assert!(matches!(
        fixture
            .engine
            .viewport()
            .effects()
            .record(compensation.id())
            .expect("compensation must remain auditable")
            .phase(),
        EffectPhase::Destroyed { .. }
    ));
    assert!(fixture.engine.viewport().viewport(SURFACE_NATIVE).is_none());
}

#[test]
fn failed_create_compensation_retries_only_after_explicit_input() {
    let mut fixture = fixture();
    prepare_base_platform(&mut fixture, ViewportRole::Child);
    let request = start_native_create(&mut fixture);
    let changing_source = fixture
        .engine
        .workspace()
        .capture_item_source(ROOT_SOURCE, fixture.source_tabs, ItemId::new(2))
        .expect("second source item must be current");
    fixture
        .engine
        .enqueue_command(WorkspaceCommand::Select {
            source: changing_source,
        })
        .expect("source mutation must enqueue");
    fixture
        .engine
        .reduce_pending()
        .expect("source mutation must commit");
    let ready = publish_windows(
        &mut fixture,
        vec![
            source_window(false),
            host_window(false),
            native_window(request, false),
        ],
    );
    let failed_cleanup = effect_of_kind(ready.platform_effects(), |effect| {
        matches!(
            effect,
            PlatformEffect::CompensatingClose { binding, .. }
                if *binding == request.binding()
        )
    })
    .id();
    fixture
        .engine
        .enqueue_platform_effect_result(EffectResult::new(
            failed_cleanup,
            fixture.engine.version().epoch(),
            EffectDispatchResult::DispatchFailed(DispatchFailureReason::WindowUnavailable),
        ))
        .expect("cleanup failure must enqueue");
    let failed = fixture
        .engine
        .reduce_pending()
        .expect("cleanup failure must reduce");
    assert!(failed.platform_effects().is_empty());

    fixture
        .engine
        .enqueue_viewport_cleanup_retry(failed_cleanup)
        .expect("cleanup retry must enqueue");
    let retried = fixture
        .engine
        .reduce_pending()
        .expect("cleanup retry must reduce");
    let retry = effect_of_kind(retried.platform_effects(), |effect| {
        matches!(
            effect,
            PlatformEffect::CompensatingClose { binding, compensates }
                if *binding == request.binding() && *compensates == request.effect()
        )
    });
    assert_ne!(retry.id(), failed_cleanup);
    assert!(matches!(
        retried.reduced_inputs()[0].outcome(),
        InputOutcome::ViewportCleanupRetried {
            failed_effect,
            retry: actual,
        } if *failed_effect == failed_cleanup && *actual == retry.id()
    ));
    assert!(matches!(
        fixture
            .engine
            .viewport()
            .native_create_saga(request.saga())
            .expect("compensating create must remain queryable")
            .status(),
        NativeCreateStatus::Compensating { effect } if effect == retry.id()
    ));

    let repeated = publish_windows(
        &mut fixture,
        vec![
            source_window(false),
            host_window(false),
            native_window(request, false),
        ],
    );
    assert!(repeated.platform_effects().is_empty());
}

#[test]
fn root_and_child_close_veto_emit_role_specific_effects_without_mutating_workspace() {
    for role in [ViewportRole::Root, ViewportRole::Child] {
        let mut fixture = fixture();
        prepare_base_platform(&mut fixture, role);
        let before = fixture.engine.workspace().clone();
        let close = publish_windows(&mut fixture, vec![source_window(false), host_window(true)]);
        let request = close_request_from(&close);

        let decided = decide_close(&mut fixture, request, ViewportCloseDecision::Veto);
        assert_eq!(fixture.engine.workspace(), &before);
        let expected_binding = fixture
            .engine
            .viewport()
            .viewport(SURFACE_HOST)
            .expect("host viewport must remain registered")
            .binding();
        let effect = effect_of_kind(decided.platform_effects(), |effect| match effect {
            PlatformEffect::CancelRootClose { binding } => {
                role == ViewportRole::Root && *binding == expected_binding
            }
            PlatformEffect::RetainChild { binding } => {
                role == ViewportRole::Child && *binding == expected_binding
            }
            _ => false,
        });
        assert_eq!(
            fixture
                .engine
                .viewport()
                .viewport_close_request(request)
                .expect("close history must remain queryable")
                .status(),
            ViewportCloseStatus::Vetoed {
                effect: effect.id()
            }
        );

        let repeated = publish_windows(&mut fixture, vec![source_window(false), host_window(true)]);
        assert!(repeated.platform_effects().is_empty());
        let InputOutcome::PlatformSnapshotPublished { transition } =
            repeated.reduced_inputs()[0].outcome()
        else {
            panic!("repeated close fact must publish a platform transition");
        };
        assert!(transition.close_requests().is_empty());
    }
}

#[test]
fn root_and_child_close_accept_commit_recovery_only_after_authoritative_destruction() {
    for (index, role) in [ViewportRole::Root, ViewportRole::Child]
        .into_iter()
        .enumerate()
    {
        let mut fixture = fixture();
        prepare_base_platform(&mut fixture, role);
        let before = fixture.engine.workspace().clone();
        let close = publish_windows(&mut fixture, vec![source_window(false), host_window(true)]);
        let request = close_request_from(&close);
        let floating = FloatingPresentationId::new(80 + index as u64);
        let plan = ViewportClosePlan::new(None, close_recovery(ROOT_HOST, floating));

        let decided = decide_close(
            &mut fixture,
            request,
            ViewportCloseDecision::Accept(plan.clone()),
        );
        assert_eq!(fixture.engine.workspace(), &before);
        let binding = fixture
            .engine
            .viewport()
            .viewport(SURFACE_HOST)
            .expect("closing viewport must remain until destruction")
            .binding();
        let effect = effect_of_kind(decided.platform_effects(), |effect| match effect {
            PlatformEffect::RequestRootClose { binding: actual } => {
                role == ViewportRole::Root && *actual == binding
            }
            PlatformEffect::ReleaseChild { binding: actual } => {
                role == ViewportRole::Child && *actual == binding
            }
            _ => false,
        });
        assert_eq!(
            fixture
                .engine
                .viewport()
                .viewport_close_request(request)
                .expect("accepted close must remain queryable")
                .status(),
            ViewportCloseStatus::AwaitingDestroyed {
                effect: Some(effect.id())
            }
        );

        publish_windows(&mut fixture, vec![source_window(false), host_window(true)]);
        assert_eq!(fixture.engine.workspace(), &before);

        publish_windows(&mut fixture, vec![source_window(false)]);
        assert!(fixture.engine.workspace().surface(SURFACE_HOST).is_none());
        let recovered = fixture
            .engine
            .workspace()
            .contained_floating(floating)
            .expect("destroyed root must be recovered whole");
        assert_eq!(recovered.root, ROOT_HOST);
        assert_eq!(recovered.surface, SURFACE_SOURCE);
        assert_eq!(
            fixture
                .engine
                .viewport()
                .viewport_close_request(request)
                .expect("destroyed close history must remain queryable")
                .status(),
            ViewportCloseStatus::Destroyed
        );
        assert!(matches!(
            fixture
                .engine
                .viewport()
                .effects()
                .record(effect.id())
                .expect("close effect must remain auditable")
                .phase(),
            EffectPhase::Destroyed { .. }
        ));
    }
}

#[test]
fn partial_close_primary_is_discarded_before_whole_root_recovery() {
    let mut fixture = fixture_with_host_items(&[3, 4]);
    prepare_base_platform(&mut fixture, ViewportRole::Child);
    let requested = publish_windows(&mut fixture, vec![source_window(false), host_window(true)]);
    let request = close_request_from(&requested);
    let host_node = fixture
        .engine
        .workspace()
        .root(ROOT_HOST)
        .expect("host root must exist")
        .node;
    let partial = fixture
        .engine
        .workspace()
        .capture_item_source(ROOT_HOST, host_node, ItemId::new(3))
        .expect("partial close source must be current");
    let floating = FloatingPresentationId::new(90);
    decide_close(
        &mut fixture,
        request,
        ViewportCloseDecision::Accept(ViewportClosePlan::new(
            Some(WorkspaceCommand::Close { source: partial }),
            close_recovery(ROOT_HOST, floating),
        )),
    );

    publish_windows(&mut fixture, vec![source_window(false)]);

    let items = fixture.engine.workspace().item_multiset();
    assert!(items.contains_key(&ItemId::new(3)));
    assert!(items.contains_key(&ItemId::new(4)));
    assert!(fixture.engine.workspace().surface(SURFACE_HOST).is_none());
    assert_eq!(
        fixture
            .engine
            .workspace()
            .contained_floating(floating)
            .expect("complete host root must recover")
            .root,
        ROOT_HOST
    );
}

#[test]
fn unrelated_close_primary_is_discarded_before_whole_root_recovery() {
    let mut fixture = fixture();
    prepare_base_platform(&mut fixture, ViewportRole::Child);
    let requested = publish_windows(&mut fixture, vec![source_window(false), host_window(true)]);
    let request = close_request_from(&requested);
    let unrelated = fixture
        .engine
        .workspace()
        .capture_item_source(ROOT_SOURCE, fixture.source_tabs, ItemId::new(2))
        .expect("unrelated source selection must be current");
    let floating = FloatingPresentationId::new(91);
    decide_close(
        &mut fixture,
        request,
        ViewportCloseDecision::Accept(ViewportClosePlan::new(
            Some(WorkspaceCommand::Select { source: unrelated }),
            close_recovery(ROOT_HOST, floating),
        )),
    );

    publish_windows(&mut fixture, vec![source_window(false)]);

    assert!(matches!(
        fixture.engine.workspace().node(fixture.source_tabs),
        Some(Node::Tabs {
            selected: Some(item),
            ..
        }) if *item == ItemId::new(1)
    ));
    assert_eq!(
        fixture
            .engine
            .workspace()
            .contained_floating(floating)
            .expect("complete host root must recover")
            .root,
        ROOT_HOST
    );
}

#[test]
fn close_accept_rejects_a_recovery_root_from_another_surface() {
    let mut fixture = fixture();
    prepare_base_platform(&mut fixture, ViewportRole::Child);
    let requested = publish_windows(&mut fixture, vec![source_window(false), host_window(true)]);
    let request = close_request_from(&requested);
    fixture
        .engine
        .enqueue_viewport_close_decision(
            request,
            ViewportCloseDecision::Accept(ViewportClosePlan::new(
                None,
                close_recovery(ROOT_SOURCE, FloatingPresentationId::new(92)),
            )),
        )
        .expect("invalid close decision must enqueue");

    let error = fixture
        .engine
        .reduce_pending()
        .expect_err("a recovery root from another surface must be rejected atomically");
    assert!(matches!(
        error,
        EngineError::Viewport {
            source: dockspace::frame::ViewportCoordinatorError::CloseRecoveryRootMismatch {
                request: actual,
            },
            ..
        } if actual == request
    ));
    assert_eq!(
        fixture
            .engine
            .viewport()
            .viewport_close_request(request)
            .expect("close request must remain undecided")
            .status(),
        ViewportCloseStatus::AwaitingDecision
    );
    assert!(fixture.engine.workspace().surface(SURFACE_HOST).is_some());
}

#[test]
fn accepted_close_dispatch_failure_keeps_the_root_and_allows_an_explicit_retry() {
    let mut fixture = fixture();
    prepare_base_platform(&mut fixture, ViewportRole::Child);
    let before = fixture.engine.workspace().clone();
    let requested = publish_windows(&mut fixture, vec![source_window(false), host_window(true)]);
    let request = close_request_from(&requested);
    let plan = ViewportClosePlan::new(
        None,
        close_recovery(ROOT_HOST, FloatingPresentationId::new(31)),
    );
    let first = decide_close(
        &mut fixture,
        request,
        ViewportCloseDecision::Accept(plan.clone()),
    );
    let first_effect = effect_of_kind(first.platform_effects(), |effect| {
        matches!(effect, PlatformEffect::ReleaseChild { .. })
    });
    let epoch = fixture.engine.version().epoch();
    fixture
        .engine
        .enqueue_platform_effect_result(EffectResult::new(
            first_effect.id(),
            epoch,
            EffectDispatchResult::DispatchFailed(DispatchFailureReason::ProviderStopped),
        ))
        .expect("close dispatch failure must enqueue");
    fixture
        .engine
        .reduce_pending()
        .expect("close dispatch failure must reduce");
    assert_eq!(fixture.engine.workspace(), &before);
    assert_eq!(
        fixture
            .engine
            .viewport()
            .viewport_close_request(request)
            .expect("failed close must remain queryable")
            .status(),
        ViewportCloseStatus::EffectFailed {
            effect: first_effect.id()
        }
    );
    assert_eq!(
        fixture
            .engine
            .viewport()
            .viewport(SURFACE_HOST)
            .expect("failed close must retain the viewport")
            .lifecycle(),
        ViewportLifecycle::CloseRequested
    );

    let retried = decide_close(&mut fixture, request, ViewportCloseDecision::Accept(plan));
    let retry_effect = effect_of_kind(retried.platform_effects(), |effect| {
        matches!(effect, PlatformEffect::ReleaseChild { .. })
    });
    assert_ne!(retry_effect.id(), first_effect.id());
    publish_windows(&mut fixture, vec![source_window(false)]);
    assert!(fixture.engine.workspace().surface(SURFACE_HOST).is_none());
    assert!(
        fixture
            .engine
            .workspace()
            .contained_floating(FloatingPresentationId::new(31))
            .is_some()
    );
}

#[test]
fn accepted_close_result_is_independent_of_failure_and_destruction_order() {
    fn accepted_close(
        fixture: &mut Fixture,
        floating: FloatingPresentationId,
    ) -> (ViewportCloseRequestId, dockspace::effect::EffectId) {
        prepare_base_platform(fixture, ViewportRole::Child);
        let requested = publish_windows(fixture, vec![source_window(false), host_window(true)]);
        let request = close_request_from(&requested);
        let decided = decide_close(
            fixture,
            request,
            ViewportCloseDecision::Accept(ViewportClosePlan::new(
                None,
                close_recovery(ROOT_HOST, floating),
            )),
        );
        let effect = effect_of_kind(decided.platform_effects(), |effect| {
            matches!(effect, PlatformEffect::ReleaseChild { .. })
        });
        (request, effect.id())
    }

    let floating = FloatingPresentationId::new(32);
    let mut failure_first = fixture();
    let (failure_first_request, failure_first_effect) =
        accepted_close(&mut failure_first, floating);
    failure_first
        .engine
        .enqueue_platform_effect_result(EffectResult::new(
            failure_first_effect,
            failure_first.engine.version().epoch(),
            EffectDispatchResult::DispatchFailed(DispatchFailureReason::ProviderStopped),
        ))
        .expect("close failure must enqueue");
    failure_first
        .engine
        .reduce_pending()
        .expect("close failure must reduce");
    assert!(
        failure_first
            .engine
            .viewport()
            .viewport_close_request(failure_first_request)
            .expect("failed close request must remain queryable")
            .plan()
            .is_some()
    );
    publish_windows(&mut failure_first, vec![source_window(false)]);

    let mut destruction_first = fixture();
    let (destruction_first_request, destruction_first_effect) =
        accepted_close(&mut destruction_first, floating);
    publish_windows(&mut destruction_first, vec![source_window(false)]);
    destruction_first
        .engine
        .enqueue_platform_effect_result(EffectResult::new(
            destruction_first_effect,
            destruction_first.engine.version().epoch(),
            EffectDispatchResult::DispatchFailed(DispatchFailureReason::ProviderStopped),
        ))
        .expect("late close failure must enqueue");
    destruction_first
        .engine
        .reduce_pending()
        .expect("late close failure must be harmless");

    assert_eq!(
        failure_first.engine.workspace(),
        destruction_first.engine.workspace()
    );
    assert_eq!(
        failure_first
            .engine
            .viewport()
            .viewport_close_request(failure_first_request)
            .expect("failure-first close must remain queryable")
            .status(),
        ViewportCloseStatus::Destroyed
    );
    assert_eq!(
        destruction_first
            .engine
            .viewport()
            .viewport_close_request(destruction_first_request)
            .expect("destruction-first close must remain queryable")
            .status(),
        ViewportCloseStatus::Destroyed
    );
}

#[test]
fn direct_native_destruction_uses_the_committed_whole_root_recovery() {
    let mut fixture = fixture();
    prepare_base_platform(&mut fixture, ViewportRole::Child);
    let request = start_native_create(&mut fixture);
    publish_windows(
        &mut fixture,
        vec![
            source_window(false),
            host_window(false),
            native_window(request, false),
        ],
    );
    assert!(fixture.engine.workspace().surface(SURFACE_NATIVE).is_some());

    publish_windows(&mut fixture, vec![source_window(false), host_window(false)]);

    assert!(fixture.engine.workspace().surface(SURFACE_NATIVE).is_none());
    assert!(fixture.engine.viewport().viewport(SURFACE_NATIVE).is_none());
    let recovered = fixture
        .engine
        .workspace()
        .contained_floating(FLOATING_RECOVERY)
        .expect("direct destruction must recover the complete root");
    assert_eq!(recovered.root, ROOT_NATIVE);
    assert_eq!(recovered.surface, SURFACE_HOST);
}

#[test]
fn restore_rebinds_retained_tokens_to_the_new_epoch_and_incarnation() {
    let mut fixture = fixture();
    let (source_before, host_before) = register_base_viewports(&mut fixture, ViewportRole::Child);
    publish_windows(&mut fixture, vec![source_window(false), host_window(false)]);
    let workspace = fixture.engine.workspace().clone();
    let version_before = fixture.engine.version();

    fixture
        .engine
        .enqueue_workspace_replacement(workspace)
        .expect("workspace replacement must enqueue");
    let transition = fixture
        .engine
        .reduce_pending()
        .expect("workspace replacement must reduce");
    let reconciliation = match transition.reduced_inputs()[0].outcome() {
        InputOutcome::WorkspaceReplaced {
            before,
            after,
            reconciliation,
        } => {
            assert_eq!(*before, version_before);
            assert_ne!(after.epoch(), before.epoch());
            reconciliation
        }
        outcome => panic!("unexpected restore outcome: {outcome:?}"),
    };
    assert_eq!(reconciliation.rebound().len(), 2);
    assert!(reconciliation.retired().is_empty());

    for before in [source_before, host_before] {
        let after = fixture
            .engine
            .viewport()
            .viewport(before.surface())
            .expect("retained surface must remain registered")
            .binding();
        assert_eq!(after.surface(), before.surface());
        assert_eq!(after.token(), before.token());
        assert_ne!(after.epoch(), before.epoch());
        assert_ne!(after.incarnation(), before.incarnation());
        assert_eq!(
            fixture
                .engine
                .viewport()
                .viewport(before.surface())
                .expect("retained viewport must be queryable")
                .lifecycle(),
            ViewportLifecycle::AwaitingObservation
        );
        assert!(
            reconciliation
                .rebound()
                .iter()
                .any(|(old, new)| *old == before && *new == after)
        );
    }
}

#[test]
fn a_ready_restore_replacement_is_rebound_instead_of_recreated_on_the_next_restore() {
    let mut fixture = fixture();
    prepare_base_platform(&mut fixture, ViewportRole::Child);
    let close = publish_windows(&mut fixture, vec![source_window(false), host_window(true)]);
    let close_request = close_request_from(&close);
    decide_close(
        &mut fixture,
        close_request,
        ViewportCloseDecision::Accept(ViewportClosePlan::new(
            None,
            close_recovery(ROOT_HOST, FloatingPresentationId::new(30)),
        )),
    );

    let workspace = fixture.engine.workspace().clone();
    fixture
        .engine
        .enqueue_workspace_replacement(workspace.clone())
        .expect("first workspace replacement must enqueue");
    let first = fixture
        .engine
        .reduce_pending()
        .expect("first workspace replacement must reduce");
    let first_reconciliation = match first.reduced_inputs()[0].outcome() {
        InputOutcome::WorkspaceReplaced { reconciliation, .. } => reconciliation,
        outcome => panic!("unexpected first restore outcome: {outcome:?}"),
    };
    let [replacement_before] = first_reconciliation.replacements() else {
        panic!("destructive pending close must request one replacement");
    };
    assert!(first.platform_effects().iter().any(|request| {
        matches!(
            request.effect(),
            PlatformEffect::RequestReplacement { binding, .. }
                if binding == replacement_before
        )
    }));

    publish_windows(
        &mut fixture,
        vec![
            source_window(false),
            observed_window(
                replacement_before.token(),
                900.0,
                false,
                WindowInputState::ReceivesInput,
            ),
        ],
    );
    assert_eq!(
        fixture
            .engine
            .viewport()
            .restore_replacement(SURFACE_HOST)
            .expect("ready replacement must remain queryable")
            .status(),
        RestoreReplacementStatus::Ready
    );

    fixture
        .engine
        .enqueue_workspace_replacement(workspace)
        .expect("second workspace replacement must enqueue");
    let second = fixture
        .engine
        .reduce_pending()
        .expect("second workspace replacement must reduce");
    let second_reconciliation = match second.reduced_inputs()[0].outcome() {
        InputOutcome::WorkspaceReplaced { reconciliation, .. } => reconciliation,
        outcome => panic!("unexpected second restore outcome: {outcome:?}"),
    };
    assert!(second_reconciliation.replacements().is_empty());
    assert!(
        second
            .platform_effects()
            .iter()
            .all(|request| !matches!(request.effect(), PlatformEffect::RequestReplacement { .. }))
    );
    let replacement_after = fixture
        .engine
        .viewport()
        .viewport(SURFACE_HOST)
        .expect("ready replacement must remain bound")
        .binding();
    assert_eq!(replacement_after.token(), replacement_before.token());
    assert_ne!(
        replacement_after.incarnation(),
        replacement_before.incarnation()
    );
    assert!(
        second_reconciliation
            .rebound()
            .contains(&(*replacement_before, replacement_after))
    );
}

#[test]
fn restore_removes_an_observed_binding_and_requests_exact_cleanup() {
    let mut fixture = fixture();
    let (_, host_binding) = register_base_viewports(&mut fixture, ViewportRole::Child);
    publish_windows(&mut fixture, vec![source_window(false), host_window(false)]);

    fixture
        .engine
        .enqueue_workspace_replacement(source_only_workspace())
        .expect("workspace replacement must enqueue");
    let transition = fixture
        .engine
        .reduce_pending()
        .expect("workspace replacement must reduce");
    let reconciliation = match transition.reduced_inputs()[0].outcome() {
        InputOutcome::WorkspaceReplaced { reconciliation, .. } => reconciliation,
        outcome => panic!("unexpected restore outcome: {outcome:?}"),
    };
    assert_eq!(reconciliation.retired(), &[host_binding]);
    assert!(fixture.engine.viewport().viewport(SURFACE_HOST).is_none());
    let cleanup = effect_of_kind(transition.platform_effects(), |effect| {
        matches!(
            effect,
            PlatformEffect::ReleaseChild { binding } if *binding == host_binding
        )
    });
    assert_eq!(reconciliation.cleanup_effects(), &[cleanup.id()]);
    let retired = fixture
        .engine
        .viewport()
        .retired_viewports()
        .find_map(|(token, retired)| (token == HOST_TOKEN).then_some(retired))
        .expect("old observed binding must remain as a cleanup tombstone");
    assert!(retired.observed());
    assert_eq!(
        retired.status(),
        RetiredViewportStatus::CleanupRequested {
            effect: cleanup.id()
        }
    );
}

#[test]
fn failed_retired_cleanup_retries_only_after_explicit_input() {
    let mut fixture = fixture();
    let (_, host_binding) = register_base_viewports(&mut fixture, ViewportRole::Child);
    publish_windows(&mut fixture, vec![source_window(false), host_window(false)]);
    fixture
        .engine
        .enqueue_workspace_replacement(source_only_workspace())
        .expect("workspace replacement must enqueue");
    let restored = fixture
        .engine
        .reduce_pending()
        .expect("workspace replacement must reduce");
    let failed_cleanup = effect_of_kind(restored.platform_effects(), |effect| {
        matches!(effect, PlatformEffect::ReleaseChild { binding } if *binding == host_binding)
    })
    .id();
    fixture
        .engine
        .enqueue_platform_effect_result(EffectResult::new(
            failed_cleanup,
            fixture.engine.version().epoch(),
            EffectDispatchResult::DispatchFailed(DispatchFailureReason::WindowUnavailable),
        ))
        .expect("retired cleanup failure must enqueue");
    fixture
        .engine
        .reduce_pending()
        .expect("retired cleanup failure must reduce");

    fixture
        .engine
        .enqueue_viewport_cleanup_retry(failed_cleanup)
        .expect("retired cleanup retry must enqueue");
    let retried = fixture
        .engine
        .reduce_pending()
        .expect("retired cleanup retry must reduce");
    let retry = effect_of_kind(
        retried.platform_effects(),
        |effect| matches!(effect, PlatformEffect::ReleaseChild { binding } if *binding == host_binding),
    );
    assert_ne!(retry.id(), failed_cleanup);
    assert_eq!(retry.epoch(), fixture.engine.version().epoch());
    let retired = fixture
        .engine
        .viewport()
        .retired_viewports()
        .find_map(|(token, retired)| (token == HOST_TOKEN).then_some(retired))
        .expect("retired binding must retain cleanup ownership");
    assert!(retired.observed());
    assert_eq!(
        retired.status(),
        RetiredViewportStatus::CleanupRequested { effect: retry.id() }
    );
}

#[test]
fn retired_tombstone_reserves_its_token_until_authoritative_absence() {
    let mut fixture = fixture();
    register_base_viewports(&mut fixture, ViewportRole::Child);
    publish_windows(&mut fixture, vec![source_window(false), host_window(false)]);
    fixture
        .engine
        .enqueue_workspace_replacement(source_only_workspace())
        .expect("first workspace replacement must enqueue");
    fixture
        .engine
        .reduce_pending()
        .expect("first workspace replacement must reduce");
    fixture
        .engine
        .enqueue_workspace_replacement(source_and_new_surface_workspace())
        .expect("second workspace replacement must enqueue");
    fixture
        .engine
        .reduce_pending()
        .expect("second workspace replacement must reduce");
    assert!(
        fixture
            .engine
            .viewport()
            .retired_viewports()
            .any(|(token, _)| token == HOST_TOKEN)
    );

    fixture
        .engine
        .enqueue_viewport_registration(SURFACE_NATIVE, HOST_TOKEN, ViewportRole::Root, None)
        .expect("conflicting registration must enqueue");
    let error = fixture
        .engine
        .reduce_pending()
        .expect_err("retired token reuse must be rejected atomically");
    assert!(matches!(
        error,
        EngineError::Viewport {
            source: dockspace::frame::ViewportCoordinatorError::RetiredTokenReserved {
                token,
            },
            ..
        } if token == HOST_TOKEN
    ));
    assert!(fixture.engine.viewport().viewport(SURFACE_NATIVE).is_none());
}

#[test]
fn repeated_restore_in_one_boundary_reissues_only_the_current_tombstone_cleanup() {
    let mut fixture = fixture();
    let (_, host_binding) = register_base_viewports(&mut fixture, ViewportRole::Child);
    publish_windows(&mut fixture, vec![source_window(false), host_window(false)]);
    let replacement = source_only_workspace();
    fixture
        .engine
        .enqueue_workspace_replacement(replacement.clone())
        .expect("first workspace replacement must enqueue");
    fixture
        .engine
        .enqueue_workspace_replacement(replacement)
        .expect("second workspace replacement must enqueue");

    let transition = fixture
        .engine
        .reduce_pending()
        .expect("both workspace replacements must reduce atomically");
    let cleanup_ids: Vec<_> = transition
        .reduced_inputs()
        .iter()
        .map(|reduced| match reduced.outcome() {
            InputOutcome::WorkspaceReplaced { reconciliation, .. } => {
                let [effect] = reconciliation.cleanup_effects() else {
                    panic!("each restore must name its exact cleanup effect");
                };
                *effect
            }
            outcome => panic!("unexpected restore outcome: {outcome:?}"),
        })
        .collect();
    assert_ne!(cleanup_ids[0], cleanup_ids[1]);
    assert!(matches!(
        fixture
            .engine
            .viewport()
            .effects()
            .record(cleanup_ids[0])
            .expect("superseded cleanup must remain auditable")
            .phase(),
        EffectPhase::InvalidatedByRestore { .. }
    ));
    let emitted = effect_of_kind(transition.platform_effects(), |effect| {
        matches!(
            effect,
            PlatformEffect::ReleaseChild { binding } if *binding == host_binding
        )
    });
    assert_eq!(emitted.id(), cleanup_ids[1]);
    assert_eq!(transition.platform_effects().len(), 1);
    let retired = fixture
        .engine
        .viewport()
        .retired_viewports()
        .find_map(|(token, retired)| (token == HOST_TOKEN).then_some(retired))
        .expect("old binding must keep one current cleanup tombstone");
    assert_eq!(
        retired.status(),
        RetiredViewportStatus::CleanupRequested {
            effect: cleanup_ids[1]
        }
    );
}

#[test]
fn restore_tombstones_a_pending_create_and_compensates_one_late_appearance() {
    let mut fixture = fixture();
    prepare_base_platform(&mut fixture, ViewportRole::Child);
    let request = start_native_create(&mut fixture);
    let old_snapshot = platform_snapshot(vec![source_window(false), host_window(false)]);
    let replacement = fixture.engine.workspace().clone();

    fixture
        .engine
        .enqueue_workspace_replacement(replacement)
        .expect("workspace replacement must enqueue");
    let restored = fixture
        .engine
        .reduce_pending()
        .expect("workspace replacement must reduce");
    let reconciliation = match restored.reduced_inputs()[0].outcome() {
        InputOutcome::WorkspaceReplaced { reconciliation, .. } => reconciliation,
        outcome => panic!("unexpected restore outcome: {outcome:?}"),
    };
    assert!(reconciliation.retired().contains(&request.binding()));
    assert!(reconciliation.cleanup_effects().is_empty());
    let tombstone = fixture
        .engine
        .viewport()
        .retired_viewports()
        .find_map(|(token, retired)| (token == request.binding().token()).then_some(retired))
        .expect("pending create must retain a late-appearance tombstone");
    assert!(!tombstone.observed());
    assert!(tombstone.may_appear_late());
    assert_eq!(
        tombstone.status(),
        RetiredViewportStatus::AwaitingAppearance
    );

    assert_old_platform_inputs_are_stale(&mut fixture, request, old_snapshot);

    let late = publish_windows(
        &mut fixture,
        vec![
            source_window(false),
            host_window(false),
            native_window(request, false),
        ],
    );
    let compensation = effect_of_kind(late.platform_effects(), |effect| {
        matches!(
            effect,
            PlatformEffect::CompensatingClose {
                binding,
                compensates,
            } if *binding == request.binding() && *compensates == request.effect()
        )
    });
    let retired = fixture
        .engine
        .viewport()
        .retired_viewports()
        .find_map(|(token, retired)| (token == request.binding().token()).then_some(retired))
        .expect("late window must remain isolated as retired");
    assert!(retired.observed());
    assert_eq!(
        retired.status(),
        RetiredViewportStatus::CleanupRequested {
            effect: compensation.id()
        }
    );

    let repeated = publish_windows(
        &mut fixture,
        vec![
            source_window(false),
            host_window(false),
            native_window(request, false),
        ],
    );
    assert!(repeated.platform_effects().is_empty());
    assert_eq!(
        fixture
            .engine
            .viewport()
            .effects()
            .records()
            .filter(|(_, record)| matches!(
                record.request().effect(),
                PlatformEffect::CompensatingClose { binding, .. }
                    if *binding == request.binding()
            ))
            .count(),
        1
    );
}
