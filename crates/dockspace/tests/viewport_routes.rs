use dockspace::command::MovePayload;
use dockspace::effect::PlatformEffect;
use dockspace::engine::DockEngine;
use dockspace::geometry::{LogicalRect, LogicalSize, PhysicalPoint, PhysicalRect, ScaleFactor};
use dockspace::graph::{Node, RootRecord, SurfacePresentation, Workspace};
use dockspace::ids::{FloatingPresentationId, ItemId, NodeId, RootId, SurfaceId};
use dockspace::intent::{
    Authority, AuthorityUnavailableReason, ContainedTearOffProposal, PointerButton,
    PointerButtonState, PointerId, RendererIntent,
};
use dockspace::interaction::{DragSessionId, InteractionOutcome, InteractionStatus};
use dockspace::platform::{
    ButtonObservation, ObservedWindow, PlatformCapabilities, PlatformCapability,
    PlatformCapabilityReason, PlatformRequirement, PlatformSnapshot, PointerObservation,
    PointerWindow, WindowInputState,
};
use dockspace::policy::DockPolicy;
use dockspace::scene::{BuildingScene, ReadySurfaceScene};
use dockspace::transition::InputOutcome;
use dockspace::viewport::{ViewportRole, WindowToken};

const ROOT_SOURCE: RootId = RootId::new(1);
const ROOT_TARGET: RootId = RootId::new(2);
const SURFACE_SOURCE: SurfaceId = SurfaceId::new(1);
const SURFACE_TARGET: SurfaceId = SurfaceId::new(2);
const SOURCE_TOKEN: WindowToken = WindowToken::new(10);
const TARGET_TOKEN: WindowToken = WindowToken::new(20);
const UNKNOWN_TOKEN: WindowToken = WindowToken::new(30);
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

fn fixture() -> Fixture {
    let mut builder = Workspace::builder();
    let source_tabs = builder.insert_node(Node::tabs([ItemId::new(1)]));
    let target_tabs = builder.insert_node(Node::tabs([ItemId::new(2)]));
    builder.set_root(ROOT_SOURCE, RootRecord::new(source_tabs));
    builder.set_root(ROOT_TARGET, RootRecord::new(target_tabs));
    builder.set_surface(SURFACE_SOURCE, SurfacePresentation::new(ROOT_SOURCE));
    builder.set_surface(SURFACE_TARGET, SurfacePresentation::new(ROOT_TARGET));
    let workspace = builder.build().expect("test workspace must be valid");
    let mut engine =
        DockEngine::new(workspace, DockPolicy::default()).expect("test engine must be valid");

    let mut scene = BuildingScene::new([SURFACE_SOURCE, SURFACE_TARGET])
        .expect("surface roster must be unique");
    for surface in [SURFACE_SOURCE, SURFACE_TARGET] {
        scene
            .insert_ready(ReadySurfaceScene::new(
                surface,
                logical_rect(0.0, 0.0, 600.0, 400.0),
            ))
            .expect("initial surface facts must be unique");
    }
    engine.enqueue_scene(scene).expect("scene must enqueue");
    engine.reduce_pending().expect("scene must publish");
    let recovery_placement = engine
        .contained_placement(
            SURFACE_SOURCE,
            logical_rect(20.0, 20.0, 400.0, 300.0),
            LogicalSize::new(0.0, 0.0).expect("minimum size must be valid"),
        )
        .expect("ready source scene must authorize recovery placement");

    engine
        .enqueue_viewport_registration(SURFACE_SOURCE, SOURCE_TOKEN, ViewportRole::Root, None)
        .expect("source registration must enqueue");
    engine
        .enqueue_viewport_registration(
            SURFACE_TARGET,
            TARGET_TOKEN,
            ViewportRole::Child,
            Some(ContainedTearOffProposal::new(
                ROOT_TARGET,
                FloatingPresentationId::new(20),
                recovery_placement,
                1,
            )),
        )
        .expect("target registration must enqueue");
    let transition = engine
        .reduce_pending()
        .expect("viewport registrations must reduce");
    assert_eq!(transition.reduced_inputs().len(), 2);
    assert!(
        transition
            .reduced_inputs()
            .iter()
            .all(|reduced| matches!(reduced.outcome(), InputOutcome::ViewportRegistered { .. }))
    );

    Fixture {
        engine,
        source_tabs,
    }
}

fn routing_capabilities() -> PlatformCapabilities {
    let mut capabilities = PlatformCapabilities::default();
    capabilities.set_authoritative_inventory(PlatformCapability::Supported);
    capabilities.set_hovered_window(PlatformCapability::Supported);
    capabilities.set_desktop_pointer_position(PlatformCapability::Supported);
    capabilities.set_authoritative_button_state(PlatformCapability::Supported);
    capabilities.set_pointer_passthrough(PlatformCapability::Supported);
    capabilities
}

fn observed_window(token: WindowToken, x: f64, input_state: WindowInputState) -> ObservedWindow {
    ObservedWindow::new(token)
        .with_content_bounds(Authority::Known(physical_rect(x, 0.0, 600.0, 400.0)))
        .with_outer_bounds(Authority::Known(physical_rect(
            x - 8.0,
            -30.0,
            616.0,
            438.0,
        )))
        .with_scale_factor(Authority::Known(
            ScaleFactor::new(1.0).expect("test scale factor must be valid"),
        ))
        .with_input_state(Authority::Known(input_state))
        .with_close_requested(Authority::Known(false))
}

fn pointer_observation(hovered: Authority<PointerWindow>) -> PointerObservation {
    PointerObservation::new(
        POINTER,
        hovered,
        Authority::Known(
            PhysicalPoint::new(750.0, 150.0).expect("test desktop point must be valid"),
        ),
        Authority::Known(vec![ButtonObservation::new(
            PointerButton::Primary,
            PointerButtonState::Pressed,
        )]),
    )
    .expect("test pointer observation must be unambiguous")
}

fn snapshot(hovered: Authority<PointerWindow>, source_input: WindowInputState) -> PlatformSnapshot {
    PlatformSnapshot::new(
        routing_capabilities(),
        vec![
            observed_window(SOURCE_TOKEN, 0.0, source_input),
            observed_window(TARGET_TOKEN, 600.0, WindowInputState::ReceivesInput),
        ],
        vec![pointer_observation(hovered)],
        Vec::new(),
    )
    .expect("test platform snapshot must be canonical")
}

fn publish_snapshot(
    fixture: &mut Fixture,
    hovered: Authority<PointerWindow>,
    source_input: WindowInputState,
) {
    fixture
        .engine
        .enqueue_platform_snapshot(snapshot(hovered, source_input))
        .expect("platform snapshot must enqueue");
    let transition = fixture
        .engine
        .reduce_pending()
        .expect("platform snapshot must reduce");
    assert!(matches!(
        transition.reduced_inputs()[0].outcome(),
        InputOutcome::PlatformSnapshotPublished { .. }
    ));
}

fn publish_scene(fixture: &mut Fixture) {
    let mut scene = BuildingScene::new([SURFACE_SOURCE, SURFACE_TARGET])
        .expect("surface roster must be unique");
    scene
        .insert_ready(ReadySurfaceScene::new(
            SURFACE_SOURCE,
            logical_rect(0.0, 0.0, 600.0, 400.0),
        ))
        .expect("source scene must be ready");
    scene
        .insert_ready(ReadySurfaceScene::new(
            SURFACE_TARGET,
            logical_rect(0.0, 0.0, 600.0, 400.0),
        ))
        .expect("target scene must be ready");
    fixture
        .engine
        .enqueue_scene(scene)
        .expect("scene must enqueue");
    fixture.engine.reduce_pending().expect("scene must publish");
}

fn arm_and_begin(
    fixture: &mut Fixture,
) -> (DragSessionId, dockspace::transition::EngineTransition) {
    let payload = MovePayload::Item(
        fixture
            .engine
            .workspace()
            .capture_item_source(ROOT_SOURCE, fixture.source_tabs, ItemId::new(1))
            .expect("source item must be current"),
    );
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
    (session, transition)
}

#[test]
fn no_hovered_window_is_authoritative_known_none() {
    let mut fixture = fixture();

    publish_snapshot(
        &mut fixture,
        Authority::Known(PointerWindow::None),
        WindowInputState::ReceivesInput,
    );

    let proof = fixture
        .engine
        .viewport()
        .route(POINTER)
        .expect("complete pointer facts must publish a route");
    assert_eq!(proof.target(), &Authority::Known(None));
    assert_eq!(proof.target_binding(), None);
    assert!(fixture.engine.viewport().route_is_current(proof));
}

#[test]
fn unknown_hover_authority_stays_unknown() {
    let mut fixture = fixture();

    publish_snapshot(
        &mut fixture,
        Authority::Unknown(AuthorityUnavailableReason::NotReported),
        WindowInputState::ReceivesInput,
    );

    let proof = fixture
        .engine
        .viewport()
        .route(POINTER)
        .expect("the route set must preserve an unavailable observation");
    assert_eq!(
        proof.target(),
        &Authority::Unknown(AuthorityUnavailableReason::NotReported)
    );
    assert_eq!(proof.target_binding(), None);
}

#[test]
fn foreign_window_is_authoritative_known_none() {
    let mut fixture = fixture();

    publish_snapshot(
        &mut fixture,
        Authority::Known(PointerWindow::Foreign),
        WindowInputState::ReceivesInput,
    );

    let proof = fixture
        .engine
        .viewport()
        .route(POINTER)
        .expect("foreign classification must still publish a route proof");
    assert_eq!(proof.target(), &Authority::Known(None));
    assert_eq!(proof.target_binding(), None);
}

#[test]
fn unregistered_dock_window_token_is_not_treated_as_foreign() {
    let mut fixture = fixture();

    publish_snapshot(
        &mut fixture,
        Authority::Known(PointerWindow::Dock(UNKNOWN_TOKEN)),
        WindowInputState::ReceivesInput,
    );

    let proof = fixture
        .engine
        .viewport()
        .route(POINTER)
        .expect("unknown tokens must publish an unavailable proof");
    assert_eq!(
        proof.target(),
        &Authority::Unknown(AuthorityUnavailableReason::SurfaceUnavailable)
    );
    assert_eq!(proof.target_binding(), None);
}

#[test]
fn a_route_proof_becomes_stale_after_the_next_complete_snapshot() {
    let mut fixture = fixture();
    publish_snapshot(
        &mut fixture,
        Authority::Known(PointerWindow::None),
        WindowInputState::ReceivesInput,
    );
    let stale = fixture
        .engine
        .viewport()
        .route(POINTER)
        .expect("first snapshot must publish a route")
        .clone();

    publish_snapshot(
        &mut fixture,
        Authority::Known(PointerWindow::None),
        WindowInputState::ReceivesInput,
    );

    let current = fixture
        .engine
        .viewport()
        .route(POINTER)
        .expect("second snapshot must replace the route");
    assert_ne!(stale.stamp(), current.stamp());
    assert!(!fixture.engine.viewport().route_is_current(&stale));
    assert!(fixture.engine.viewport().route_is_current(current));
}

#[test]
fn requested_but_unobserved_source_passthrough_blocks_cross_window_routing() {
    let mut fixture = fixture();
    publish_snapshot(
        &mut fixture,
        Authority::Known(PointerWindow::Dock(TARGET_TOKEN)),
        WindowInputState::ReceivesInput,
    );
    publish_scene(&mut fixture);

    let (_session, begin) = arm_and_begin(&mut fixture);
    let source_binding = fixture
        .engine
        .viewport()
        .viewport(SURFACE_SOURCE)
        .expect("source viewport must remain registered")
        .binding();
    assert!(begin.platform_effects().iter().any(|request| matches!(
        request.effect(),
        PlatformEffect::SetPointerPassthrough {
            binding,
            enabled: true,
        } if *binding == source_binding
    )));

    publish_snapshot(
        &mut fixture,
        Authority::Known(PointerWindow::Dock(TARGET_TOKEN)),
        WindowInputState::ReceivesInput,
    );

    let proof = fixture
        .engine
        .viewport()
        .route(POINTER)
        .expect("the pending pass-through observation must publish an unavailable proof");
    assert_eq!(
        proof.target(),
        &Authority::Unknown(AuthorityUnavailableReason::SurfaceUnavailable)
    );
    assert_eq!(proof.target_binding(), None);
}

#[test]
fn routing_capability_revocation_cancels_the_drag_and_requests_input_restoration() {
    let mut fixture = fixture();
    publish_snapshot(
        &mut fixture,
        Authority::Known(PointerWindow::Dock(TARGET_TOKEN)),
        WindowInputState::ReceivesInput,
    );
    publish_scene(&mut fixture);
    let (session, _) = arm_and_begin(&mut fixture);
    publish_snapshot(
        &mut fixture,
        Authority::Known(PointerWindow::Dock(TARGET_TOKEN)),
        WindowInputState::PassThrough,
    );
    assert_eq!(
        fixture.engine.interaction().status(),
        InteractionStatus::Dragging { session }
    );

    let mut capabilities = routing_capabilities();
    capabilities.set_pointer_passthrough(PlatformCapability::unsupported(
        PlatformRequirement::PointerPassthrough,
        PlatformCapabilityReason::BackendUnsupported,
    ));
    let degraded = PlatformSnapshot::new(
        capabilities,
        vec![
            observed_window(SOURCE_TOKEN, 0.0, WindowInputState::PassThrough),
            observed_window(TARGET_TOKEN, 600.0, WindowInputState::ReceivesInput),
        ],
        vec![pointer_observation(Authority::Known(PointerWindow::Dock(
            TARGET_TOKEN,
        )))],
        Vec::new(),
    )
    .expect("degraded snapshot must remain canonical");
    fixture
        .engine
        .enqueue_platform_snapshot(degraded)
        .expect("capability revocation must enqueue");
    let transition = fixture
        .engine
        .reduce_pending()
        .expect("capability revocation must reduce");

    assert_eq!(
        fixture.engine.interaction().status(),
        InteractionStatus::Idle
    );
    let source_binding = fixture
        .engine
        .viewport()
        .viewport(SURFACE_SOURCE)
        .expect("source viewport must remain registered")
        .binding();
    assert!(transition.platform_effects().iter().any(|request| matches!(
        request.effect(),
        PlatformEffect::SetPointerPassthrough {
            binding,
            enabled: false,
        } if *binding == source_binding
    )));
}
