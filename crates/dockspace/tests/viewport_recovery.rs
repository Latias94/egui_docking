#[path = "viewport_recovery/cleanup_ownership.rs"]
mod cleanup_ownership;
mod support;

use support::{
    TestPresentationHost, complete_host_frame_with_current_outputs,
    complete_host_frame_with_retained_or_unavailable, complete_host_frame_with_unavailable,
};

use dockspace::NativeCloseEdge;
use dockspace::RootPresentationOwner;
use dockspace::command::WorkspaceCommand;
use dockspace::effect::{
    DispatchFailureReason, EffectDispatchResult, EffectId, EffectInvalidation, EffectPhase,
    EffectRecordLookup, EffectResult, EffectUnsupportedReason, PlatformEffect,
    PlatformEffectEmission,
};
use dockspace::engine::{
    CoreHostFrame, DockEngine, EngineError, EngineInput, SurfaceContributionBeginError,
};
use dockspace::frame::{BindingRetirementOrigin, BindingRetirementStatus, RecoveryPendingStatus};
use dockspace::geometry::{LogicalRect, LogicalSize, PhysicalRect, ScaleFactor};
use dockspace::graph::{Axis, Node, RootRecord, SurfacePresentation, Workspace};
use dockspace::ids::{
    FloatingPresentationId, ItemId, NodeId, RootId, StableInputSourceId, SurfaceId,
};
use dockspace::intent::{Authority, AuthorityUnavailableReason};
use dockspace::interaction::{InteractionOutcome, InteractionRejection};
use dockspace::platform::{
    CloseEffectAcknowledgement, ObservedWindow, ObservedWorkArea, PlatformCapabilities,
    PlatformCapability, PlatformSnapshot, PresentationEffectAcknowledgement,
    WindowCloseObservation, WindowCloseState, WindowCoordinateObservation, WindowInputState,
    WindowPresentationObservation, WindowPresentationState,
};
use dockspace::policy::DockPolicy;
use dockspace::presentation_hit::PresentationHitRegionKind;
use dockspace::presentation_observation::NativeStagingPresentationPhase;
use dockspace::scene::SurfaceScene;
use dockspace::semantic_input::{
    SemanticDelivery, SemanticKey, SemanticReceiverAction, SemanticReceiverEvent,
};
use dockspace::surface_recovery::{
    ConvertedMainRecovery, RootRecoveryAnchor, SurfaceRecoveryBlockedReason, SurfaceRecoveryError,
    SurfaceRecoveryTarget,
};
use dockspace::transition::{EngineTransition, InputOutcome, SurfaceContributionOutcome};
use dockspace::viewport::{
    CloseObservationGeneration, CoordinateObservationGeneration, PresentationObservationGeneration,
    ViewportBinding, ViewportRole, WindowToken, WorkAreaToken,
};
use dockspace::viewport_focus::{FocusObservationGeneration, unknown_focus_observation};
use dockspace::viewport_registry::ViewportAdmission;

const ROOT_HOST: RootId = RootId::new(1);
const ROOT_CHILD: RootId = RootId::new(2);
const SURFACE_HOST: SurfaceId = SurfaceId::new(1);
const SURFACE_CHILD: SurfaceId = SurfaceId::new(2);
const RECOVERY_FLOATING: FloatingPresentationId = FloatingPresentationId::new(20);
const HOST_TOKEN: WindowToken = WindowToken::new(10);
const CHILD_TOKEN: WindowToken = WindowToken::new(20);
const WORK_AREA: WorkAreaToken = WorkAreaToken::new(30);
const TEST_INPUT_SOURCE: StableInputSourceId = StableInputSourceId::new(0xd0c5_0000_0000_0201);

fn submit_test_input(
    engine: &mut DockEngine,
    presentation_host: &mut TestPresentationHost,
    input: EngineInput,
) -> Result<EngineTransition, EngineError> {
    support::submit_input(engine, presentation_host, TEST_INPUT_SOURCE, input)
}

struct Fixture {
    engine: DockEngine,
    presentation_host: TestPresentationHost,
    host_binding: ViewportBinding,
    child_binding: ViewportBinding,
    child_root_node: NodeId,
    initial_items: std::collections::BTreeMap<ItemId, usize>,
    recovery_target: SurfaceRecoveryTarget,
    expected_recovered_rect: LogicalRect,
}

struct PendingFixture {
    fixture: Fixture,
    replacement_binding: ViewportBinding,
    replacement_effect: EffectId,
}

fn logical_rect(x: f64, y: f64, width: f64, height: f64) -> LogicalRect {
    LogicalRect::new(x, y, width, height).expect("test logical rectangle must be valid")
}

fn physical_rect(x: f64, y: f64, width: f64, height: f64) -> PhysicalRect {
    PhysicalRect::new(x, y, width, height).expect("test physical rectangle must be valid")
}

fn child_outer_bounds() -> PhysicalRect {
    physical_rect(1192.0, -30.0, 916.0, 738.0)
}

fn workspace() -> (Workspace, NodeId) {
    let mut builder = Workspace::builder();
    let host_tabs = builder.insert_node(Node::tabs([ItemId::new(1)]));
    let child_left = builder.insert_node(Node::tabs([ItemId::new(2), ItemId::new(3)]));
    let child_right = builder.insert_node(Node::tabs([ItemId::new(4)]));
    let child_root = builder.insert_node(
        Node::equal_split(Axis::Horizontal, [child_left, child_right])
            .expect("test split must be valid"),
    );
    builder.set_root(ROOT_HOST, RootRecord::new(host_tabs));
    builder.set_root(ROOT_CHILD, RootRecord::new(child_root));
    builder.set_surface(SURFACE_HOST, SurfacePresentation::with_main(ROOT_HOST));
    builder.set_surface(SURFACE_CHILD, SurfacePresentation::with_main(ROOT_CHILD));
    (
        builder.build().expect("test workspace must be valid"),
        child_root,
    )
}

fn host_only_workspace() -> Workspace {
    let mut builder = Workspace::builder();
    let host_tabs = builder.insert_node(Node::tabs([ItemId::new(1)]));
    builder.set_root(ROOT_HOST, RootRecord::new(host_tabs));
    builder.set_surface(SURFACE_HOST, SurfacePresentation::with_main(ROOT_HOST));
    builder.build().expect("host-only workspace must be valid")
}

fn platform_capabilities() -> PlatformCapabilities {
    let mut capabilities = PlatformCapabilities::default();
    capabilities.set_native_window_lifecycle(PlatformCapability::Supported);
    capabilities.set_authoritative_inventory(PlatformCapability::Supported);
    capabilities.set_global_window_placement(PlatformCapability::Supported);
    capabilities.set_work_area(PlatformCapability::Supported);
    capabilities.set_close_cancellation(PlatformCapability::Supported);
    capabilities
}

fn ready_window(binding: ViewportBinding, x: f64) -> ObservedWindow {
    ready_window_at_generation(binding, 1, x, 1.0)
}

fn ready_window_with_scale(binding: ViewportBinding, x: f64, scale: f64) -> ObservedWindow {
    ready_window_at_generation(binding, 2, x, scale)
}

fn ready_window_at_generation(
    binding: ViewportBinding,
    generation: u64,
    x: f64,
    scale: f64,
) -> ObservedWindow {
    ObservedWindow::new(binding)
        .with_coordinate_observation(WindowCoordinateObservation::new(
            binding,
            CoordinateObservationGeneration::new(generation),
            Authority::Known(physical_rect(x, 0.0, 900.0, 700.0)),
            Authority::Known(physical_rect(x - 8.0, -30.0, 916.0, 738.0)),
            Authority::Known(ScaleFactor::new(scale).expect("test scale factor must be valid")),
            Authority::Known(ScaleFactor::new(scale).expect("test scale factor must be valid")),
        ))
        .with_input_state(Authority::Known(WindowInputState::ReceivesInput))
}

fn ready_window_without_outer_at_generation(
    binding: ViewportBinding,
    generation: u64,
    x: f64,
    scale: f64,
) -> ObservedWindow {
    ObservedWindow::new(binding)
        .with_coordinate_observation(WindowCoordinateObservation::new(
            binding,
            CoordinateObservationGeneration::new(generation),
            Authority::Known(physical_rect(x, 0.0, 900.0, 700.0)),
            Authority::Unknown(AuthorityUnavailableReason::NotReported),
            Authority::Known(ScaleFactor::new(scale).expect("test scale factor must be valid")),
            Authority::Known(ScaleFactor::new(scale).expect("test scale factor must be valid")),
        ))
        .with_input_state(Authority::Known(WindowInputState::ReceivesInput))
}

fn host_window(binding: ViewportBinding) -> ObservedWindow {
    ready_window(binding, 0.0)
}

fn child_window(binding: ViewportBinding) -> ObservedWindow {
    ready_window(binding, 1200.0)
}

fn unavailable_host_window(binding: ViewportBinding) -> ObservedWindow {
    ObservedWindow::new(binding)
}

fn replacement_window(binding: ViewportBinding) -> ObservedWindow {
    ready_window(binding, 1200.0)
}

fn partially_observed_replacement_window(binding: ViewportBinding) -> ObservedWindow {
    ObservedWindow::new(binding)
}

fn publish_windows(
    engine: &mut DockEngine,
    presentation_host: &mut TestPresentationHost,
    windows: Vec<ObservedWindow>,
) -> EngineTransition {
    publish_windows_with_presentation_and_close(
        engine,
        presentation_host,
        windows,
        Authority::Known(WindowPresentationState::Visible),
        &[],
    )
}

fn publish_windows_with_close(
    engine: &mut DockEngine,
    presentation_host: &mut TestPresentationHost,
    windows: Vec<ObservedWindow>,
    states: &[(ViewportBinding, WindowCloseState, Option<EffectId>)],
) -> EngineTransition {
    publish_windows_with_presentation_and_close(
        engine,
        presentation_host,
        windows,
        Authority::Known(WindowPresentationState::Visible),
        states,
    )
}

fn publish_destroyed_child(
    engine: &mut DockEngine,
    presentation_host: &mut TestPresentationHost,
    child_binding: ViewportBinding,
    windows: Vec<ObservedWindow>,
) -> EngineTransition {
    publish_windows_with_close(
        engine,
        presentation_host,
        windows,
        &[(child_binding, WindowCloseState::Destroyed, None)],
    )
}

fn publish_windows_with_presentation(
    engine: &mut DockEngine,
    presentation_host: &mut TestPresentationHost,
    windows: Vec<ObservedWindow>,
    presentation: Authority<WindowPresentationState>,
) -> EngineTransition {
    publish_windows_with_presentation_and_close(
        engine,
        presentation_host,
        windows,
        presentation,
        &[],
    )
}

fn publish_windows_with_presentation_ack(
    engine: &mut DockEngine,
    presentation_host: &mut TestPresentationHost,
    windows: Vec<ObservedWindow>,
    presentation: WindowPresentationState,
    binding: ViewportBinding,
    effect: EffectId,
) -> EngineTransition {
    let observation_generation = presentation_host.next_platform_observation_generation();
    let snapshot = platform_snapshot_with_facts_at_generation(
        observation_generation,
        windows,
        Authority::Known(presentation),
        &[],
        &[(binding, effect)],
        platform_capabilities(),
    );
    let expected_epoch = engine.version().epoch();
    let provider = presentation_host.platform_provider();
    submit_test_input(
        engine,
        presentation_host,
        EngineInput::PublishPlatformSnapshot {
            provider,
            expected_epoch,
            snapshot,
        },
    )
    .expect("correlated platform snapshot must publish")
}

fn publish_windows_with_presentation_and_close(
    engine: &mut DockEngine,
    presentation_host: &mut TestPresentationHost,
    windows: Vec<ObservedWindow>,
    presentation: Authority<WindowPresentationState>,
    states: &[(ViewportBinding, WindowCloseState, Option<EffectId>)],
) -> EngineTransition {
    publish_windows_with_facts(
        engine,
        presentation_host,
        windows,
        presentation,
        states,
        platform_capabilities(),
    )
}

fn publish_windows_with_facts(
    engine: &mut DockEngine,
    presentation_host: &mut TestPresentationHost,
    windows: Vec<ObservedWindow>,
    presentation: Authority<WindowPresentationState>,
    states: &[(ViewportBinding, WindowCloseState, Option<EffectId>)],
    capabilities: PlatformCapabilities,
) -> EngineTransition {
    let observation_generation = presentation_host.next_platform_observation_generation();
    let snapshot = platform_snapshot_with_facts_at_generation(
        observation_generation,
        windows,
        presentation,
        states,
        &[],
        capabilities,
    );
    let expected_epoch = engine.version().epoch();
    let provider = presentation_host.platform_provider();
    submit_test_input(
        engine,
        presentation_host,
        EngineInput::PublishPlatformSnapshot {
            provider,
            expected_epoch,
            snapshot,
        },
    )
    .expect("platform snapshot must publish")
}

fn platform_snapshot_with_facts_at_generation(
    observation_generation: u64,
    mut windows: Vec<ObservedWindow>,
    presentation: Authority<WindowPresentationState>,
    states: &[(ViewportBinding, WindowCloseState, Option<EffectId>)],
    presentation_acknowledgements: &[(ViewportBinding, EffectId)],
    capabilities: PlatformCapabilities,
) -> PlatformSnapshot {
    for window in &mut windows {
        let binding = window.binding();
        let coordinate_observation = match window.coordinate_observation() {
            Some(observation) => WindowCoordinateObservation::new(
                binding,
                CoordinateObservationGeneration::new(observation_generation),
                *observation.content_bounds(),
                *observation.outer_bounds(),
                *observation.native_scale_factor(),
                *observation.presentation_scale_factor(),
            ),
            None => WindowCoordinateObservation::new(
                binding,
                CoordinateObservationGeneration::new(observation_generation),
                Authority::Unknown(AuthorityUnavailableReason::NotReported),
                Authority::Unknown(AuthorityUnavailableReason::NotReported),
                Authority::Unknown(AuthorityUnavailableReason::NotReported),
                Authority::Unknown(AuthorityUnavailableReason::NotReported),
            ),
        };
        *window = window
            .clone()
            .with_coordinate_observation(coordinate_observation)
            .with_presentation_observation(WindowPresentationObservation::new(
                binding,
                PresentationObservationGeneration::new(observation_generation),
                presentation,
                PresentationEffectAcknowledgement::known(
                    presentation_acknowledgements.iter().find_map(
                        |(acknowledged_binding, effect)| {
                            (*acknowledged_binding == binding).then_some(*effect)
                        },
                    ),
                ),
            ));
    }
    let inventory_observation =
        support::known_inventory_observation(observation_generation, &windows);
    PlatformSnapshot::new(
        dockspace::viewport::PlatformSnapshotGeneration::new(observation_generation),
        support::known_capability_observation(observation_generation, capabilities),
        unknown_focus_observation(
            FocusObservationGeneration::new(observation_generation),
            AuthorityUnavailableReason::NotReported,
        ),
        inventory_observation,
        windows,
        states
            .iter()
            .map(|(binding, state, acknowledged_effect)| {
                WindowCloseObservation::new(
                    *binding,
                    CloseObservationGeneration::new(observation_generation),
                    Authority::Known(*state),
                    CloseEffectAcknowledgement::known(*acknowledged_effect),
                )
            })
            .collect(),
        support::known_work_area_observation(
            observation_generation,
            vec![ObservedWorkArea::new(
                WORK_AREA,
                physical_rect(-1920.0, -200.0, 3840.0, 1400.0),
                ScaleFactor::new(1.0).expect("test work-area scale factor must be valid"),
            )],
        ),
    )
    .expect("test platform snapshot must be canonical")
}

fn child_next_tab_event(
    frame: &CoreHostFrame,
    binding: ViewportBinding,
) -> (dockspace::scene::TabSceneId, SemanticReceiverEvent) {
    let projection = frame
        .view()
        .semantic_projection(SURFACE_CHILD)
        .expect("sealed child must have presented semantic authority");
    let tab = projection
        .plan()
        .tab_records()
        .iter()
        .find(|tab| tab.selected() && tab.id().item == ItemId::new(2))
        .map(|tab| *tab.id())
        .expect("child left leaf must select item 2");
    let event = SemanticReceiverEvent::new(
        projection.output_ticket(),
        projection.authority().emission(),
        SemanticDelivery::Native(binding),
        PresentationHitRegionKind::TabBody(tab),
        SemanticReceiverAction::Key(SemanticKey::ArrowRight),
    );
    (tab, event)
}

fn next_destroyed_child_snapshot(fixture: &mut Fixture) -> PlatformSnapshot {
    let generation = fixture
        .presentation_host
        .next_platform_observation_generation();
    platform_snapshot_with_facts_at_generation(
        generation,
        vec![unavailable_host_window(fixture.host_binding)],
        Authority::Known(WindowPresentationState::Visible),
        &[(fixture.child_binding, WindowCloseState::Destroyed, None)],
        &[],
        platform_capabilities(),
    )
}

fn publish_scene(engine: &mut DockEngine, host: &mut TestPresentationHost) {
    let mut frame = host.begin(engine);
    let surfaces = frame.surfaces().collect::<Vec<_>>();
    for surface in surfaces {
        let viewport = engine.viewport().viewport(surface);
        if viewport.is_some_and(|record| !record.is_ready()) {
            continue;
        }
        let ticket = frame
            .view()
            .begin_surface_contribution(surface)
            .expect("ready fixture surface must have a current ticket");
        let contribution = frame
            .view()
            .prepare_surface_contribution(
                ticket,
                support::measurements(
                    engine,
                    surface,
                    logical_rect(0.0, 0.0, 900.0, 700.0),
                    support::MeasurementProfile::default(),
                ),
            )
            .expect("ready fixture surface must prepare measurements");
        frame
            .push_surface_contribution(contribution)
            .expect("one fixture contribution per surface");
    }
    complete_host_frame_with_unavailable(engine, &mut frame);
    let transition = host.finish(frame, engine);
    let outputs: Vec<_> = transition
        .surface_contributions()
        .iter()
        .filter_map(|outcome| match outcome {
            SurfaceContributionOutcome::Ready { surface, .. } => Some(*surface),
            SurfaceContributionOutcome::Retained { .. }
            | SurfaceContributionOutcome::Unavailable { .. }
            | SurfaceContributionOutcome::Rejected { .. } => None,
        })
        .collect();
    if outputs.is_empty() {
        return;
    }
    let emit_frame = host.begin(engine);
    let emit_frame = complete_host_frame_with_current_outputs(engine, emit_frame);
    host.finish_presentation(emit_frame, engine);

    let observation_frame = host.begin(engine);
    let observation_frame = complete_host_frame_with_current_outputs(engine, observation_frame);
    let transition = host.finish_presentation(observation_frame, engine);
    assert!(
        transition
            .presentation_observations()
            .iter()
            .all(|outcome| matches!(
                outcome,
                dockspace::presentation_observation::HostPresentationObservationOutcome::Retired { .. }
            ))
    );
}

fn make_host_current_and_retry_recovery(
    engine: &mut DockEngine,
    presentation_host: &mut TestPresentationHost,
) -> EngineTransition {
    let host_binding = engine
        .viewport()
        .viewport(SURFACE_HOST)
        .expect("host viewport must remain registered")
        .binding();
    publish_windows(engine, presentation_host, vec![host_window(host_binding)]);
    publish_scene(engine, presentation_host);
    publish_windows(engine, presentation_host, vec![host_window(host_binding)])
}

fn fixture() -> Fixture {
    fixture_with_child_outer(true)
}

fn fixture_with_child_outer(child_outer_available: bool) -> Fixture {
    fixture_with_provider(child_outer_available, false)
}

fn joined_fixture() -> Fixture {
    fixture_with_provider(true, true)
}

fn fixture_with_provider(child_outer_available: bool, joined: bool) -> Fixture {
    let (workspace, child_root_node) = workspace();
    let initial_items = workspace.item_multiset();
    let mut engine =
        DockEngine::new(workspace, DockPolicy::default()).expect("test engine must be valid");
    let mut presentation_host = if joined {
        TestPresentationHost::new_joined(&mut engine)
    } else {
        TestPresentationHost::new(&mut engine)
    };
    let provider = presentation_host.platform_provider();
    publish_scene(&mut engine, &mut presentation_host);
    let expected = engine.version();
    let registered_host = submit_test_input(
        &mut engine,
        &mut presentation_host,
        EngineInput::RegisterViewport {
            provider,
            expected,
            surface: SURFACE_HOST,
            token: HOST_TOKEN,
            role: ViewportRole::Root,
            recovery_target: None,
        },
    )
    .expect("host root viewport registration must reduce");
    assert!(matches!(
        registered_host.reduced_inputs()[0].outcome(),
        InputOutcome::ViewportRegistered { binding }
            if binding.surface() == SURFACE_HOST && binding.token() == HOST_TOKEN
    ));
    let anchor = engine
        .root_recovery_anchor(SURFACE_HOST)
        .expect("registered host root must own a recovery anchor");
    let recovery_target = recovery_target(anchor);
    let expected = engine.version();
    let registered_child = submit_test_input(
        &mut engine,
        &mut presentation_host,
        EngineInput::RegisterViewport {
            provider,
            expected,
            surface: SURFACE_CHILD,
            token: CHILD_TOKEN,
            role: ViewportRole::Child,
            recovery_target: Some(recovery_target),
        },
    )
    .expect("child viewport registration must reduce");
    assert!(matches!(
        registered_child.reduced_inputs()[0].outcome(),
        InputOutcome::ViewportRegistered { binding }
            if binding.surface() == SURFACE_CHILD && binding.token() == CHILD_TOKEN
    ));
    let child_binding = engine
        .viewport()
        .viewport(SURFACE_CHILD)
        .expect("child viewport must be registered")
        .binding();
    let host_binding = engine
        .viewport()
        .viewport(SURFACE_HOST)
        .expect("host viewport must be registered")
        .binding();
    let child_window = if child_outer_available {
        child_window(child_binding)
    } else {
        ready_window_without_outer_at_generation(child_binding, 1, 1200.0, 1.0)
    };
    publish_windows(
        &mut engine,
        &mut presentation_host,
        vec![host_window(host_binding), child_window],
    );
    publish_scene(&mut engine, &mut presentation_host);
    let expected_recovered_rect = engine
        .contained_placement(
            SURFACE_HOST,
            logical_rect(1192.0, -30.0, 916.0, 738.0),
            LogicalSize::new(0.0, 0.0).expect("minimum size must be valid"),
        )
        .expect("latest native geometry must be recoverable in the host scene")
        .clamped_rect();
    Fixture {
        engine,
        presentation_host,
        host_binding,
        child_binding,
        child_root_node,
        initial_items,
        recovery_target,
        expected_recovered_rect,
    }
}

fn recovery_target(anchor: RootRecoveryAnchor) -> SurfaceRecoveryTarget {
    SurfaceRecoveryTarget::with_converted_main(
        anchor,
        ConvertedMainRecovery::new(
            ROOT_CHILD,
            RECOVERY_FLOATING,
            LogicalSize::new(0.0, 0.0).expect("minimum size must be valid"),
        ),
    )
}

fn native_close_edge_from(transition: &EngineTransition) -> NativeCloseEdge {
    transition
        .reduced_inputs()
        .iter()
        .find_map(|reduced| match reduced.outcome() {
            InputOutcome::PlatformSnapshotPublished {
                native_close_edges, ..
            } => native_close_edges.first().copied(),
            _ => None,
        })
        .expect("snapshot must publish one authoritative native close edge")
}

fn request_replacement(transition: &EngineTransition) -> (EffectId, ViewportBinding) {
    let matching: Vec<_> = transition
        .platform_effects()
        .iter()
        .filter_map(|request| match request.effect() {
            PlatformEffect::RequestReplacement {
                binding,
                placement,
                role,
            } => Some((request.id(), *binding, *placement, *role)),
            _ => None,
        })
        .collect();
    assert_eq!(
        matching.len(),
        1,
        "expected exactly one replacement request"
    );
    let (effect, binding, placement, role) = matching[0];
    assert_eq!(binding.surface(), SURFACE_CHILD);
    assert_eq!(placement, child_outer_bounds());
    assert_eq!(role, ViewportRole::Child);
    (effect, binding)
}

fn pending_fixture() -> PendingFixture {
    pending_fixture_from(fixture())
}

fn pending_joined_fixture() -> PendingFixture {
    pending_fixture_from(joined_fixture())
}

fn pending_fixture_from(mut fixture: Fixture) -> PendingFixture {
    let child_binding = fixture.child_binding;
    let destroyed = publish_destroyed_child(
        &mut fixture.engine,
        &mut fixture.presentation_host,
        child_binding,
        vec![unavailable_host_window(fixture.host_binding)],
    );
    let (replacement_effect, replacement_binding) = request_replacement(&destroyed);
    let pending = fixture
        .engine
        .viewport()
        .recovery_pending(SURFACE_CHILD)
        .expect("destroyed child must retain a queryable recovery");
    assert_eq!(pending.destroyed_binding(), fixture.child_binding);
    assert_eq!(
        fixture.engine.surface_recovery_target(SURFACE_CHILD),
        Some(fixture.recovery_target)
    );
    assert_eq!(pending.replacement_binding(), Some(replacement_binding));
    assert_eq!(pending.replacement_effect(), Some(replacement_effect));
    assert_eq!(
        pending.status(),
        RecoveryPendingStatus::ReplacementRequested {
            replacement: replacement_effect
        }
    );
    assert!(fixture.engine.workspace().surface(SURFACE_CHILD).is_some());
    assert!(
        fixture
            .engine
            .workspace()
            .contained_floating(RECOVERY_FLOATING)
            .is_none()
    );
    PendingFixture {
        fixture,
        replacement_binding,
        replacement_effect,
    }
}

fn replace_joined_backend(fixture: &mut Fixture) -> EngineTransition {
    let predecessor = fixture.presentation_host.take_backend_ingress();
    let mut drained = predecessor.drain();
    let replacement = fixture
        .engine
        .begin_backend_ingress_provider_replacement(&mut drained)
        .expect("joined provider replacement must preserve lifecycle authority");
    assert!(drained.is_consumed());
    let (mut ticket, transition) = replacement.into_parts();
    let successor = fixture
        .engine
        .finish_backend_ingress_provider_replacement(&mut ticket, fixture.presentation_host.lease())
        .expect("joined successor lanes must activate atomically");
    assert!(ticket.is_consumed());
    fixture.presentation_host.adopt_backend_ingress(
        &fixture.engine,
        successor,
        drained.pointer_through(),
    );
    transition
}

fn assert_recovery_presentation_scope(
    fixture: &mut Fixture,
    expected_staging: Option<NativeStagingPresentationPhase>,
    live_surface_expected: bool,
) {
    let schedule = fixture
        .engine
        .host_presentation_schedule()
        .expect("recovery state must derive one physical output schedule");
    assert_eq!(
        schedule.surfaces().any(|surface| surface == SURFACE_CHILD),
        live_surface_expected
    );
    assert_eq!(
        schedule
            .native_staging_presentations()
            .filter(|presentation| presentation.binding().surface() == SURFACE_CHILD)
            .map(|presentation| presentation.phase())
            .collect::<Vec<_>>(),
        expected_staging.into_iter().collect::<Vec<_>>()
    );

    let frame = fixture.presentation_host.begin(&fixture.engine);
    let live_surface_present = frame.surfaces().any(|surface| surface == SURFACE_CHILD);
    assert_eq!(live_surface_present, live_surface_expected);
    let staging = frame
        .view()
        .native_staging_presentations()
        .filter(|presentation| presentation.binding().surface() == SURFACE_CHILD)
        .map(|presentation| presentation.phase())
        .collect::<Vec<_>>();
    assert_eq!(staging, expected_staging.into_iter().collect::<Vec<_>>());

    if !live_surface_expected {
        assert!(frame.view().interaction_projection(SURFACE_CHILD).is_none());
        assert!(frame.view().semantic_projection(SURFACE_CHILD).is_none());
        assert!(matches!(
            frame.view().begin_surface_contribution(SURFACE_CHILD),
            Err(SurfaceContributionBeginError::SurfaceOutsideRoster {
                surface: SURFACE_CHILD,
            })
        ));
    }
}

fn advance_replacement_to_pre_show(pending: &mut PendingFixture) {
    let binding = pending.replacement_binding;
    let host = pending.fixture.host_binding;
    let already_awaiting_pre_show = matches!(
        pending
            .fixture
            .engine
            .viewport()
            .recovery_pending(SURFACE_CHILD)
            .expect("replacement recovery remains pending")
            .status(),
        RecoveryPendingStatus::AwaitingPreShowPresentation { replacement }
            if replacement == pending.replacement_effect
    );
    if !already_awaiting_pre_show {
        assert_recovery_presentation_scope(&mut pending.fixture, None, false);
        let hidden = publish_windows_with_presentation_ack(
            &mut pending.fixture.engine,
            &mut pending.fixture.presentation_host,
            vec![unavailable_host_window(host), replacement_window(binding)],
            WindowPresentationState::Hidden,
            binding,
            pending.replacement_effect,
        );
        assert_no_new_effects(&hidden);
        assert_eq!(
            pending
                .fixture
                .engine
                .viewport()
                .recovery_pending(SURFACE_CHILD)
                .expect("hidden replacement recovery remains pending")
                .status(),
            RecoveryPendingStatus::AwaitingPreShowPresentation {
                replacement: pending.replacement_effect,
            }
        );
    }
    assert_recovery_presentation_scope(
        &mut pending.fixture,
        Some(NativeStagingPresentationPhase::PreShow),
        false,
    );
}

fn present_replacement_pre_show(pending: &mut PendingFixture) -> EffectId {
    advance_replacement_to_pre_show(pending);
    let binding = pending.replacement_binding;
    let pre_show = support::present_requested_native_staging(
        &mut pending.fixture.engine,
        &mut pending.fixture.presentation_host,
        binding,
        NativeStagingPresentationPhase::PreShow,
    );
    let show = pre_show
        .platform_effects()
        .iter()
        .find_map(|emission| match emission.effect() {
            PlatformEffect::ShowWindow {
                binding: actual, ..
            } if *actual == binding => Some(emission.id()),
            _ => None,
        })
        .expect("pre-show presentation must emit one correlated ShowWindow");
    assert!(matches!(
        pending
            .fixture
            .engine
            .viewport()
            .recovery_pending(SURFACE_CHILD)
            .expect("pre-show presentation keeps recovery pending")
            .status(),
        RecoveryPendingStatus::AwaitingShowAcknowledgement {
            replacement,
            show: actual_show,
        } if replacement == pending.replacement_effect && actual_show == show
    ));
    assert_recovery_presentation_scope(&mut pending.fixture, None, false);
    show
}

fn acknowledge_replacement_show(pending: &mut PendingFixture, show: EffectId) {
    let binding = pending.replacement_binding;
    let host = pending.fixture.host_binding;
    let acknowledged = publish_windows_with_presentation_ack(
        &mut pending.fixture.engine,
        &mut pending.fixture.presentation_host,
        vec![unavailable_host_window(host), replacement_window(binding)],
        WindowPresentationState::Hidden,
        binding,
        show,
    );
    assert_no_new_effects(&acknowledged);
    assert!(matches!(
        pending
            .fixture
            .engine
            .viewport()
            .recovery_pending(SURFACE_CHILD)
            .expect("show acknowledgement keeps recovery pending")
            .status(),
        RecoveryPendingStatus::AwaitingVisible {
            replacement,
            show: actual_show,
        } if replacement == pending.replacement_effect && actual_show == show
    ));
    assert_recovery_presentation_scope(&mut pending.fixture, None, false);
}

fn advance_replacement_to_post_show(pending: &mut PendingFixture, show: EffectId) {
    let binding = pending.replacement_binding;
    let host = pending.fixture.host_binding;
    let visible = publish_windows(
        &mut pending.fixture.engine,
        &mut pending.fixture.presentation_host,
        vec![unavailable_host_window(host), replacement_window(binding)],
    );
    assert_no_new_effects(&visible);
    assert!(matches!(
        pending
            .fixture
            .engine
            .viewport()
            .recovery_pending(SURFACE_CHILD)
            .expect("visible replacement keeps recovery pending through post-show")
            .status(),
        RecoveryPendingStatus::AwaitingPostShowPresentation {
            replacement,
            show: actual_show,
        } if replacement == pending.replacement_effect && actual_show == show
    ));
    assert_recovery_presentation_scope(
        &mut pending.fixture,
        Some(NativeStagingPresentationPhase::PostShow),
        false,
    );
}

fn advance_replacement_to_first_live(pending: &mut PendingFixture) -> EngineTransition {
    let binding = pending.replacement_binding;
    let show = present_replacement_pre_show(pending);
    acknowledge_replacement_show(pending, show);
    advance_replacement_to_post_show(pending, show);
    let post_show = support::present_requested_native_staging(
        &mut pending.fixture.engine,
        &mut pending.fixture.presentation_host,
        binding,
        NativeStagingPresentationPhase::PostShow,
    );
    assert_awaiting_first_live_presentation(&pending.fixture, binding);
    assert_recovery_presentation_scope(&mut pending.fixture, None, true);
    post_show
}

#[derive(Debug, Clone, Copy)]
enum ProviderReplacementCheckpoint {
    AwaitingHidden,
    PreShowPresentation,
    ShowAcknowledgement,
    Visible,
    PostShowPresentation,
}

fn pending_at_provider_replacement_checkpoint(
    checkpoint: ProviderReplacementCheckpoint,
) -> (PendingFixture, EffectId) {
    let mut pending = pending_joined_fixture();
    match checkpoint {
        ProviderReplacementCheckpoint::AwaitingHidden => {
            let last_effect = pending.replacement_effect;
            (pending, last_effect)
        }
        ProviderReplacementCheckpoint::PreShowPresentation => {
            advance_replacement_to_pre_show(&mut pending);
            let last_effect = pending.replacement_effect;
            (pending, last_effect)
        }
        ProviderReplacementCheckpoint::ShowAcknowledgement => {
            let show = present_replacement_pre_show(&mut pending);
            (pending, show)
        }
        ProviderReplacementCheckpoint::Visible => {
            let show = present_replacement_pre_show(&mut pending);
            acknowledge_replacement_show(&mut pending, show);
            (pending, show)
        }
        ProviderReplacementCheckpoint::PostShowPresentation => {
            let show = present_replacement_pre_show(&mut pending);
            acknowledge_replacement_show(&mut pending, show);
            advance_replacement_to_post_show(&mut pending, show);
            (pending, show)
        }
    }
}

fn assert_whole_root_recovered(fixture: &Fixture) {
    assert!(fixture.engine.workspace().surface(SURFACE_CHILD).is_none());
    let recovered = fixture
        .engine
        .workspace()
        .contained_floating(RECOVERY_FLOATING)
        .expect("the complete child root must be recovered as one contained presentation");
    assert_eq!(recovered.root, ROOT_CHILD);
    assert_eq!(recovered.rect, fixture.expected_recovered_rect);
    assert_eq!(
        fixture.engine.workspace().presentation_for_root(ROOT_CHILD),
        Some(RootPresentationOwner::Contained {
            surface: SURFACE_HOST,
            floating: RECOVERY_FLOATING,
        })
    );
    assert_eq!(
        fixture
            .engine
            .workspace()
            .root(ROOT_CHILD)
            .expect("recovered root must remain durable")
            .node,
        fixture.child_root_node
    );
    assert_eq!(
        fixture.engine.workspace().item_multiset(),
        fixture.initial_items
    );
}

fn effect_count(engine: &DockEngine, predicate: impl Fn(&PlatformEffect) -> bool) -> usize {
    engine
        .viewport()
        .effects()
        .records()
        .filter(|(_, record)| predicate(record.request().effect()))
        .count()
}

fn assert_no_new_effects(transition: &EngineTransition) {
    assert!(transition.platform_effects().is_empty());
}

fn assert_awaiting_first_live_presentation(fixture: &Fixture, binding: ViewportBinding) {
    let recovery = fixture
        .engine
        .viewport()
        .recovery_pending(SURFACE_CHILD)
        .expect("visible replacement must await its first live presentation");
    assert_eq!(recovery.replacement_binding(), Some(binding));
    assert_eq!(
        recovery.status(),
        RecoveryPendingStatus::AwaitingFirstLivePresentation
    );

    let record = fixture
        .engine
        .viewport()
        .viewport(SURFACE_CHILD)
        .expect("visible replacement must remain registered while awaiting presentation");
    assert_eq!(record.binding(), binding);
    assert!(record.is_ready());
    assert_eq!(record.admission(), ViewportAdmission::Pending);
}

fn assert_replacement_admitted(fixture: &Fixture, binding: ViewportBinding) {
    assert!(
        fixture
            .engine
            .viewport()
            .recovery_pending(SURFACE_CHILD)
            .is_none(),
        "first live presentation must consume the recovery obligation"
    );

    let record = fixture
        .engine
        .viewport()
        .viewport(SURFACE_CHILD)
        .expect("presented replacement must remain registered after admission");
    assert_eq!(record.binding(), binding);
    assert!(record.is_ready());
    assert_eq!(record.admission(), ViewportAdmission::Admitted);
}

#[test]
fn destroyed_child_recovers_the_whole_root_when_the_host_is_ready() {
    let mut fixture = fixture();
    let child_binding = fixture.child_binding;

    let destroyed = publish_destroyed_child(
        &mut fixture.engine,
        &mut fixture.presentation_host,
        child_binding,
        vec![host_window(fixture.host_binding)],
    );

    assert_no_new_effects(&destroyed);
    assert_whole_root_recovered(&fixture);
    assert!(
        fixture
            .engine
            .viewport()
            .recovery_pending(SURFACE_CHILD)
            .is_none()
    );
    assert_eq!(
        effect_count(&fixture.engine, |effect| matches!(
            effect,
            PlatformEffect::RequestReplacement { .. }
        )),
        0
    );
}

#[test]
fn minimized_host_requires_fresh_visible_scene_authority_before_recovery() {
    let mut fixture = fixture();
    let before = fixture.engine.workspace().clone();

    let destroyed = publish_windows_with_presentation_and_close(
        &mut fixture.engine,
        &mut fixture.presentation_host,
        vec![host_window(fixture.host_binding)],
        Authority::Known(WindowPresentationState::Minimized),
        &[(fixture.child_binding, WindowCloseState::Destroyed, None)],
    );
    let (replacement_effect, replacement_binding) = request_replacement(&destroyed);

    assert!(
        fixture
            .engine
            .viewport()
            .viewport(SURFACE_HOST)
            .is_some_and(|record| record.is_ready()),
        "minimizing the host must not destroy its structurally observed record"
    );
    assert!(
        matches!(
            fixture.engine.scene().surface(SURFACE_HOST),
            Some(SurfaceScene::Stale(_))
        ),
        "minimizing the host must invalidate its placement-bound ready scene"
    );
    assert_eq!(
        fixture.engine.workspace(),
        &before,
        "a minimized host must not be selected as a recovery destination"
    );
    assert_eq!(
        fixture
            .engine
            .viewport()
            .recovery_pending(SURFACE_CHILD)
            .expect("destroyed child recovery must remain pending")
            .status(),
        RecoveryPendingStatus::ReplacementRequested {
            replacement: replacement_effect
        }
    );

    let visible_without_scene = publish_windows(
        &mut fixture.engine,
        &mut fixture.presentation_host,
        vec![host_window(fixture.host_binding)],
    );
    assert_no_new_effects(&visible_without_scene);
    assert_eq!(
        fixture.engine.workspace(),
        &before,
        "a visible observation cannot resurrect the stale placement proof"
    );

    publish_scene(&mut fixture.engine, &mut fixture.presentation_host);
    let visible = publish_windows(
        &mut fixture.engine,
        &mut fixture.presentation_host,
        vec![host_window(fixture.host_binding)],
    );
    let compensation = visible
        .platform_effects()
        .iter()
        .find_map(|request| {
            matches!(
                request.effect(),
                PlatformEffect::CompensatingClose {
                    binding,
                    compensates,
                } if *binding == replacement_binding && *compensates == replacement_effect
            )
            .then_some(request.id())
        })
        .expect("a newer visible observation must recover and retire the replacement");

    assert_whole_root_recovered(&fixture);
    assert_eq!(
        fixture
            .engine
            .viewport()
            .recovery_pending(SURFACE_CHILD)
            .expect("replacement compensation must remain queryable")
            .status(),
        RecoveryPendingStatus::CompensatingReplacement {
            replacement: replacement_effect,
            cleanup: compensation,
        }
    );
}

#[test]
fn destroyed_child_recovers_after_a_transient_coordinate_gap() {
    let mut fixture = fixture();
    let child_binding = fixture.child_binding;

    publish_windows(
        &mut fixture.engine,
        &mut fixture.presentation_host,
        vec![
            host_window(fixture.host_binding),
            ObservedWindow::new(child_binding),
        ],
    );
    let child = fixture
        .engine
        .viewport()
        .viewport(SURFACE_CHILD)
        .expect("child binding must remain registered through a coordinate gap");
    assert!(
        !child.is_ready(),
        "historical recovery geometry must not restore current scene authority"
    );

    publish_destroyed_child(
        &mut fixture.engine,
        &mut fixture.presentation_host,
        child_binding,
        vec![host_window(fixture.host_binding)],
    );

    assert_whole_root_recovered(&fixture);
    assert!(
        fixture
            .engine
            .pending_surface_recovery(SURFACE_CHILD)
            .is_none()
    );
}

#[test]
fn recovery_keeps_the_last_exact_outer_anchor_when_the_latest_fact_is_unknown() {
    let mut fixture = fixture();
    let child_binding = fixture.child_binding;

    publish_windows(
        &mut fixture.engine,
        &mut fixture.presentation_host,
        vec![
            host_window(fixture.host_binding),
            ready_window_without_outer_at_generation(child_binding, 2, 1400.0, 1.0),
        ],
    );
    publish_scene(&mut fixture.engine, &mut fixture.presentation_host);

    publish_destroyed_child(
        &mut fixture.engine,
        &mut fixture.presentation_host,
        child_binding,
        vec![host_window(fixture.host_binding)],
    );

    assert_whole_root_recovered(&fixture);
}

#[test]
fn rooted_recovery_without_any_exact_outer_anchor_stays_pending() {
    let mut fixture = fixture_with_child_outer(false);
    let child_binding = fixture.child_binding;

    publish_destroyed_child(
        &mut fixture.engine,
        &mut fixture.presentation_host,
        child_binding,
        vec![host_window(fixture.host_binding)],
    );

    assert!(fixture.engine.workspace().surface(SURFACE_CHILD).is_some());
    assert!(
        fixture
            .engine
            .workspace()
            .contained_floating(RECOVERY_FLOATING)
            .is_none()
    );
    assert!(
        fixture
            .engine
            .pending_surface_recovery(SURFACE_CHILD)
            .is_some()
    );
    assert!(matches!(
        fixture.engine.blocked_surface_recovery(SURFACE_CHILD),
        Some(SurfaceRecoveryBlockedReason::Compile(
            SurfaceRecoveryError::SourceOuterBoundsUnavailable { surface }
        )) if *surface == SURFACE_CHILD
    ));
}

#[test]
fn recovery_uses_the_latest_native_outer_bounds_in_the_current_host_scale() {
    let mut fixture = fixture();
    let child_binding = fixture.child_binding;
    publish_windows(
        &mut fixture.engine,
        &mut fixture.presentation_host,
        vec![
            ready_window_with_scale(fixture.host_binding, 100.0, 2.0),
            ready_window_at_generation(fixture.child_binding, 2, 300.0, 1.0),
        ],
    );
    publish_scene(&mut fixture.engine, &mut fixture.presentation_host);

    publish_destroyed_child(
        &mut fixture.engine,
        &mut fixture.presentation_host,
        child_binding,
        vec![ready_window_with_scale(fixture.host_binding, 100.0, 2.0)],
    );

    let recovered = fixture
        .engine
        .workspace()
        .contained_floating(RECOVERY_FLOATING)
        .expect("destroyed child must recover into the current host");
    assert_eq!(recovered.root, ROOT_CHILD);
    assert_eq!(
        recovered.rect,
        logical_rect(96.0, 0.0, 458.0, 369.0),
        "desktop-physical child geometry must be converted with the host scale exactly once"
    );
    assert_eq!(
        fixture.engine.workspace().item_multiset(),
        fixture.initial_items
    );
}

#[test]
fn unavailable_host_keeps_recovery_pending_and_requests_one_last_outer_placement() {
    let mut pending = pending_fixture();

    let repeated = publish_windows(
        &mut pending.fixture.engine,
        &mut pending.fixture.presentation_host,
        vec![unavailable_host_window(pending.fixture.host_binding)],
    );

    assert_no_new_effects(&repeated);
    assert_eq!(
        effect_count(&pending.fixture.engine, |effect| matches!(
            effect,
            PlatformEffect::RequestReplacement { binding, .. }
                if *binding == pending.replacement_binding
        )),
        1
    );
    assert_eq!(
        pending
            .fixture
            .engine
            .viewport()
            .recovery_pending(SURFACE_CHILD)
            .expect("recovery must remain pending")
            .status(),
        RecoveryPendingStatus::ReplacementRequested {
            replacement: pending.replacement_effect
        }
    );
}

#[test]
fn staging_recovery_replacement_close_keeps_the_original_root_out_of_close_plans() {
    let mut pending = pending_fixture();
    let before = pending.fixture.engine.workspace().clone();

    let requested = publish_windows_with_close(
        &mut pending.fixture.engine,
        &mut pending.fixture.presentation_host,
        vec![
            unavailable_host_window(pending.fixture.host_binding),
            replacement_window(pending.replacement_binding),
        ],
        &[(
            pending.replacement_binding,
            WindowCloseState::LiveRequested,
            None,
        )],
    );
    assert!(requested.reduced_inputs().iter().any(|input| matches!(
        input.outcome(),
        InputOutcome::PlatformSnapshotPublished {
            native_close_edges,
            ..
        } if native_close_edges.is_empty()
    )));
    assert_eq!(pending.fixture.engine.active_close_plans().count(), 0);
    assert_eq!(pending.fixture.engine.workspace(), &before);
    let record = pending
        .fixture
        .engine
        .viewport()
        .viewport(SURFACE_CHILD)
        .expect("staging replacement must remain registered until exact destruction");
    assert_eq!(record.binding(), pending.replacement_binding);
    assert_eq!(record.admission(), ViewportAdmission::Retiring);
    let cleanup = requested
        .platform_effects()
        .iter()
        .find_map(|request| {
            matches!(
                request.effect(),
                PlatformEffect::CompensatingClose {
                    binding,
                    compensates,
                } if *binding == pending.replacement_binding
                    && *compensates == pending.replacement_effect
            )
            .then_some(request.id())
        })
        .expect("staging replacement close must issue one private compensation");

    let expected_epoch = pending.fixture.engine.version().epoch();
    let provider = pending.fixture.presentation_host.platform_provider();
    let failed = submit_test_input(
        &mut pending.fixture.engine,
        &mut pending.fixture.presentation_host,
        EngineInput::ReportPlatformEffect {
            provider,
            expected_epoch,
            result: EffectResult::new(
                cleanup,
                expected_epoch,
                EffectDispatchResult::DispatchFailed(DispatchFailureReason::WindowUnavailable),
            ),
        },
    )
    .expect("staging cleanup failure must not create a regular close plan");
    assert_no_new_effects(&failed);
    assert_eq!(pending.fixture.engine.active_close_plans().count(), 0);
    assert_eq!(pending.fixture.engine.workspace(), &before);
    assert_eq!(
        pending
            .fixture
            .engine
            .viewport()
            .viewport(SURFACE_CHILD)
            .expect("failed staging cleanup must retain a non-admitted binding")
            .admission(),
        ViewportAdmission::Retiring
    );

    let destroyed = publish_destroyed_child(
        &mut pending.fixture.engine,
        &mut pending.fixture.presentation_host,
        pending.replacement_binding,
        vec![unavailable_host_window(pending.fixture.host_binding)],
    );
    assert_no_new_effects(&destroyed);
    assert_eq!(pending.fixture.engine.workspace(), &before);
    assert_eq!(pending.fixture.engine.active_close_plans().count(), 0);
    let recovery = pending
        .fixture
        .engine
        .viewport()
        .recovery_pending(SURFACE_CHILD)
        .expect("exact staging destruction must preserve the original recovery");
    assert_eq!(recovery.replacement_binding(), None);
    assert_eq!(
        recovery.status(),
        RecoveryPendingStatus::AwaitingRecoveryHost
    );
    assert_recovery_presentation_scope(&mut pending.fixture, None, false);
}

#[test]
fn staging_recovery_close_owns_one_cleanup_until_the_exact_terminal_destroyed_edge() {
    let mut pending = pending_fixture();
    let before = pending.fixture.engine.workspace().clone();
    let replacement = pending.replacement_binding;
    let host = pending.fixture.host_binding;

    // The host is deliberately ready here. Without staging-close ownership,
    // the same snapshot can enqueue `RetryRecovery`, recover the graph, and
    // request a second compensating close for this still-pending replacement.
    let requested = publish_windows_with_close(
        &mut pending.fixture.engine,
        &mut pending.fixture.presentation_host,
        vec![host_window(host), replacement_window(replacement)],
        &[(replacement, WindowCloseState::LiveRequested, None)],
    );
    let cleanup = requested
        .platform_effects()
        .iter()
        .find_map(|request| {
            matches!(
                request.effect(),
                PlatformEffect::CompensatingClose {
                    binding,
                    compensates,
                } if *binding == replacement && *compensates == pending.replacement_effect
            )
            .then_some(request.id())
        })
        .expect("staging close must issue its sole private cleanup");
    assert_eq!(
        effect_count(&pending.fixture.engine, |effect| matches!(
            effect,
            PlatformEffect::CompensatingClose { binding, .. } if *binding == replacement
        )),
        1,
        "the staging abort and recovery retry must not each own a close"
    );
    assert_eq!(pending.fixture.engine.workspace(), &before);
    let recovery = pending
        .fixture
        .engine
        .viewport()
        .recovery_pending(SURFACE_CHILD)
        .expect("the original recovery remains authoritative during staging cleanup");
    assert_eq!(recovery.replacement_binding(), Some(replacement));
    assert_eq!(
        recovery.status(),
        RecoveryPendingStatus::ReplacementRequested {
            replacement: pending.replacement_effect,
        },
        "a suppressed retry cannot take ownership from the staging abort"
    );

    let destroyed = publish_destroyed_child(
        &mut pending.fixture.engine,
        &mut pending.fixture.presentation_host,
        replacement,
        vec![host_window(host)],
    );
    assert!(
        destroyed.platform_effects().iter().all(|request| !matches!(
            request.effect(),
            PlatformEffect::CompensatingClose { binding, .. } if *binding == replacement
        )),
        "the exact terminal edge must finish the existing cleanup instead of creating another one"
    );
    assert_eq!(pending.fixture.engine.workspace(), &before);
    let recovery = pending
        .fixture
        .engine
        .viewport()
        .recovery_pending(SURFACE_CHILD)
        .expect("destroyed staging replacement returns to the original retained recovery");
    assert_eq!(recovery.replacement_binding(), None);
    assert_eq!(recovery.replacement_effect(), None);
    assert_eq!(
        recovery.status(),
        RecoveryPendingStatus::AwaitingRecoveryHost
    );
    assert_recovery_presentation_scope(&mut pending.fixture, None, false);

    // The staging terminal deliberately removes the child viewport while the
    // source roster is still retained. Publish only the host contribution and
    // explicitly defer the now-headless child, then let the next host snapshot
    // perform the ordinary retained-recovery retry.
    let waiting = publish_windows(
        &mut pending.fixture.engine,
        &mut pending.fixture.presentation_host,
        vec![host_window(host)],
    );
    assert!(
        waiting.platform_effects().iter().all(|request| !matches!(
            request.effect(),
            PlatformEffect::CompensatingClose { binding, .. } if *binding == replacement
        )),
        "making the host observable cannot resurrect a staging cleanup"
    );
    publish_scene(
        &mut pending.fixture.engine,
        &mut pending.fixture.presentation_host,
    );
    let recovered = publish_windows(
        &mut pending.fixture.engine,
        &mut pending.fixture.presentation_host,
        vec![host_window(host)],
    );
    assert!(
        recovered.platform_effects().iter().all(|request| !matches!(
            request.effect(),
            PlatformEffect::CompensatingClose { binding, .. } if *binding == replacement
        )),
        "the later graph recovery has no replacement cleanup left to dispatch"
    );
    assert_whole_root_recovered(&pending.fixture);
    assert!(
        pending
            .fixture
            .engine
            .viewport()
            .recovery_pending(SURFACE_CHILD)
            .is_none(),
        "the only recovery obligation must be completed after the terminal retry"
    );
    assert!(
        pending
            .fixture
            .engine
            .pending_surface_recovery(SURFACE_CHILD)
            .is_none(),
        "the retained roster must not outlive its completed recovery"
    );
    assert_eq!(
        effect_count(&pending.fixture.engine, |effect| matches!(
            effect,
            PlatformEffect::CompensatingClose { binding, .. } if *binding == replacement
        )),
        0,
        "a terminal cleanup may compact after its publication boundary"
    );
    assert!(matches!(
        pending.fixture.engine.viewport().effects().lookup(cleanup),
        EffectRecordLookup::RetiredTerminal
    ));
}

#[test]
fn failed_staging_recovery_close_requires_exact_clear_and_first_live_presentation() {
    let mut pending = pending_fixture();
    let replacement = pending.replacement_binding;
    let host = pending.fixture.host_binding;
    let requested = publish_windows_with_close(
        &mut pending.fixture.engine,
        &mut pending.fixture.presentation_host,
        vec![
            unavailable_host_window(host),
            replacement_window(replacement),
        ],
        &[(replacement, WindowCloseState::LiveRequested, None)],
    );
    let cleanup = requested
        .platform_effects()
        .iter()
        .find_map(|request| {
            matches!(
                request.effect(),
                PlatformEffect::CompensatingClose {
                    binding,
                    compensates,
                } if *binding == replacement && *compensates == pending.replacement_effect
            )
            .then_some(request.id())
        })
        .expect("staging close must issue its private cleanup");

    let expected_epoch = pending.fixture.engine.version().epoch();
    let provider = pending.fixture.presentation_host.platform_provider();
    submit_test_input(
        &mut pending.fixture.engine,
        &mut pending.fixture.presentation_host,
        EngineInput::ReportPlatformEffect {
            provider,
            expected_epoch,
            result: EffectResult::new(
                cleanup,
                expected_epoch,
                EffectDispatchResult::DispatchFailed(DispatchFailureReason::WindowUnavailable),
            ),
        },
    )
    .expect("failed private cleanup must reduce");

    let cleared = publish_windows_with_facts(
        &mut pending.fixture.engine,
        &mut pending.fixture.presentation_host,
        vec![
            unavailable_host_window(host),
            replacement_window(replacement),
        ],
        Authority::Known(WindowPresentationState::Visible),
        &[(replacement, WindowCloseState::LiveClear, None)],
        platform_capabilities(),
    );
    assert_no_new_effects(&cleared);
    let record = pending
        .fixture
        .engine
        .viewport()
        .viewport(SURFACE_CHILD)
        .expect("exact clear must retain the staging replacement");
    assert_eq!(record.binding(), replacement);
    assert_eq!(record.admission(), ViewportAdmission::Pending);
    assert_eq!(
        pending
            .fixture
            .engine
            .viewport()
            .recovery_pending(SURFACE_CHILD)
            .expect("the original replacement must remain pending after clear")
            .status(),
        RecoveryPendingStatus::ReplacementRequested {
            replacement: pending.replacement_effect
        }
    );
    assert!(matches!(
        pending
            .fixture
            .engine
            .viewport()
            .effects()
            .record(pending.replacement_effect)
            .expect("original replacement request remains auditable")
            .phase(),
        EffectPhase::Requested
    ));

    let visible = advance_replacement_to_first_live(&mut pending);
    assert_no_new_effects(&visible);

    publish_scene(
        &mut pending.fixture.engine,
        &mut pending.fixture.presentation_host,
    );
    assert_replacement_admitted(&pending.fixture, replacement);
    assert!(matches!(
        pending
            .fixture
            .engine
            .viewport()
            .effects()
            .record(pending.replacement_effect)
            .expect("original replacement request must settle by visible proof")
            .phase(),
        EffectPhase::ObservedApplied { .. }
    ));
}

#[test]
fn same_host_frame_staging_close_clear_invalidates_unemitted_cleanup_before_admission() {
    let mut pending = pending_fixture();
    let replacement = pending.replacement_binding;
    let host = pending.fixture.host_binding;
    let first_generation = pending
        .fixture
        .presentation_host
        .next_platform_observation_generation();
    let second_generation = pending
        .fixture
        .presentation_host
        .next_platform_observation_generation();
    let expected_epoch = pending.fixture.engine.version().epoch();
    let provider = pending.fixture.presentation_host.platform_provider();
    let requested = platform_snapshot_with_facts_at_generation(
        first_generation,
        vec![
            unavailable_host_window(host),
            replacement_window(replacement),
        ],
        Authority::Known(WindowPresentationState::Hidden),
        &[(replacement, WindowCloseState::LiveRequested, None)],
        &[(replacement, pending.replacement_effect)],
        platform_capabilities(),
    );
    let cleared = platform_snapshot_with_facts_at_generation(
        second_generation,
        vec![
            unavailable_host_window(host),
            replacement_window(replacement),
        ],
        Authority::Known(WindowPresentationState::Hidden),
        &[(replacement, WindowCloseState::LiveClear, None)],
        &[(replacement, pending.replacement_effect)],
        platform_capabilities(),
    );

    let transition = support::submit_inputs(
        &mut pending.fixture.engine,
        &mut pending.fixture.presentation_host,
        TEST_INPUT_SOURCE,
        [
            EngineInput::PublishPlatformSnapshot {
                provider,
                expected_epoch,
                snapshot: requested,
            },
            EngineInput::PublishPlatformSnapshot {
                provider,
                expected_epoch,
                snapshot: cleared,
            },
        ],
    )
    .expect("the exact requested-to-clear sequence must reduce in one host frame");

    assert!(transition.platform_effects().iter().all(|request| {
        !matches!(
            request.effect(),
            PlatformEffect::CompensatingClose { binding, .. } if *binding == replacement
        )
    }));
    let cleanup = pending
        .fixture
        .engine
        .viewport()
        .effects()
        .records()
        .find_map(|(effect, record)| {
            matches!(
                record.request().effect(),
                PlatformEffect::CompensatingClose { binding, compensates }
                    if *binding == replacement && *compensates == pending.replacement_effect
            )
            .then_some(effect)
        })
        .expect("the requested staging cleanup remains auditable after its cancellation");
    let cleanup_record = pending
        .fixture
        .engine
        .viewport()
        .effects()
        .record(cleanup)
        .expect("the cancelled staging cleanup record remains queryable");
    assert!(!cleanup_record.was_emitted());
    assert!(matches!(
        cleanup_record.phase(),
        EffectPhase::Invalidated {
            cause: EffectInvalidation::StagingCloseCleared
        }
    ));
    assert_eq!(
        pending
            .fixture
            .engine
            .viewport()
            .viewport(SURFACE_CHILD)
            .expect("exact clear retains the replacement binding")
            .admission(),
        ViewportAdmission::Pending
    );

    let visible = advance_replacement_to_first_live(&mut pending);
    assert!(visible.platform_effects().iter().all(|request| {
        !matches!(
            request.effect(),
            PlatformEffect::CompensatingClose { binding, .. } if *binding == replacement
        )
    }));

    publish_scene(
        &mut pending.fixture.engine,
        &mut pending.fixture.presentation_host,
    );
    assert_replacement_admitted(&pending.fixture, replacement);
    assert!(matches!(
        pending
            .fixture
            .engine
            .viewport()
            .effects()
            .record(cleanup)
            .expect("the cancelled cleanup remains invalidated after admission")
            .phase(),
        EffectPhase::Invalidated {
            cause: EffectInvalidation::StagingCloseCleared
        }
    ));
}

#[test]
fn replacement_first_live_presentation_resolves_pending_without_moving_topology() {
    let mut pending = pending_fixture();
    let before = pending.fixture.engine.workspace().clone();

    let ready = advance_replacement_to_first_live(&mut pending);

    assert_no_new_effects(&ready);
    assert_eq!(pending.fixture.engine.workspace(), &before);
    assert!(
        pending
            .fixture
            .engine
            .workspace()
            .surface(SURFACE_CHILD)
            .is_some()
    );
    assert!(
        pending
            .fixture
            .engine
            .workspace()
            .contained_floating(RECOVERY_FLOATING)
            .is_none()
    );

    publish_scene(
        &mut pending.fixture.engine,
        &mut pending.fixture.presentation_host,
    );
    assert_replacement_admitted(&pending.fixture, pending.replacement_binding);
    assert!(matches!(
        pending
            .fixture
            .engine
            .viewport()
            .effects()
            .record(pending.replacement_effect)
            .expect("replacement effect must remain auditable")
            .phase(),
        EffectPhase::ObservedApplied { .. }
    ));

    let repeated = publish_windows(
        &mut pending.fixture.engine,
        &mut pending.fixture.presentation_host,
        vec![
            unavailable_host_window(pending.fixture.host_binding),
            replacement_window(pending.replacement_binding),
        ],
    );
    assert_no_new_effects(&repeated);
}

#[test]
fn joined_provider_replacement_terminates_every_pre_admission_recovery_bringup() {
    for checkpoint in [
        ProviderReplacementCheckpoint::AwaitingHidden,
        ProviderReplacementCheckpoint::PreShowPresentation,
        ProviderReplacementCheckpoint::ShowAcknowledgement,
        ProviderReplacementCheckpoint::Visible,
        ProviderReplacementCheckpoint::PostShowPresentation,
    ] {
        let (mut pending, last_effect) = pending_at_provider_replacement_checkpoint(checkpoint);
        let replacement = pending.replacement_binding;
        let transition = replace_joined_backend(&mut pending.fixture);
        assert!(transition.platform_effects().is_empty());
        assert_eq!(
            pending
                .fixture
                .engine
                .viewport()
                .recovery_pending(SURFACE_CHILD)
                .expect("provider replacement must retain recovery ownership")
                .status(),
            RecoveryPendingStatus::ReplacementProviderLost {
                replacement: pending.replacement_effect,
                last_effect,
            }
        );
        assert_recovery_presentation_scope(&mut pending.fixture, None, false);

        assert!(
            pending
                .fixture
                .engine
                .viewport()
                .effects()
                .record(pending.replacement_effect)
                .is_some(),
            "the replacement lineage must survive compaction at {checkpoint:?}"
        );
        assert!(
            pending
                .fixture
                .engine
                .viewport()
                .effects()
                .record(last_effect)
                .is_some(),
            "the latest bring-up effect must survive compaction at {checkpoint:?}"
        );

        let cleanup_transition = publish_windows(
            &mut pending.fixture.engine,
            &mut pending.fixture.presentation_host,
            vec![
                unavailable_host_window(pending.fixture.host_binding),
                replacement_window(replacement),
            ],
        );
        let cleanup = cleanup_transition
            .platform_effects()
            .iter()
            .find_map(|emission| {
                matches!(
                    emission.effect(),
                    PlatformEffect::CompensatingClose {
                        binding,
                        compensates,
                    } if *binding == replacement && *compensates == pending.replacement_effect
                )
                .then_some(emission.id())
            })
            .unwrap_or_else(|| {
                panic!(
                    "successor observation must retire the provider-lost replacement at {checkpoint:?}"
                )
            });
        let provider_lost = pending
            .fixture
            .engine
            .viewport()
            .recovery_pending(SURFACE_CHILD)
            .expect("content recovery must remain pending independently of window cleanup");
        assert_eq!(provider_lost.replacement_binding(), None);
        assert_eq!(
            provider_lost.status(),
            RecoveryPendingStatus::ReplacementProviderLost {
                replacement: pending.replacement_effect,
                last_effect,
            }
        );
        let retirement = pending
            .fixture
            .engine
            .viewport()
            .binding_retirements()
            .find_map(|(binding, retirement)| (binding == replacement).then_some(retirement))
            .expect("the provider-lost binding must have one independent cleanup owner");
        assert_eq!(
            retirement.origin(),
            BindingRetirementOrigin::RecoveryReplacementProviderLost {
                replacement: pending.replacement_effect,
                last_effect,
            }
        );
        assert_eq!(
            retirement.status(),
            BindingRetirementStatus::CleanupRequested { effect: cleanup }
        );
        assert_recovery_presentation_scope(&mut pending.fixture, None, false);

        let _ = publish_windows(
            &mut pending.fixture.engine,
            &mut pending.fixture.presentation_host,
            vec![
                host_window(pending.fixture.host_binding),
                replacement_window(replacement),
            ],
        );
        publish_scene(
            &mut pending.fixture.engine,
            &mut pending.fixture.presentation_host,
        );
        let recovered = publish_windows(
            &mut pending.fixture.engine,
            &mut pending.fixture.presentation_host,
            vec![
                host_window(pending.fixture.host_binding),
                replacement_window(replacement),
            ],
        );

        assert_whole_root_recovered(&pending.fixture);
        assert_no_new_effects(&recovered);
        assert!(
            pending
                .fixture
                .engine
                .viewport()
                .recovery_pending(SURFACE_CHILD)
                .is_none(),
            "content recovery must complete without taking ownership back from retirement"
        );
    }
}

#[test]
fn destroyed_child_rejects_its_old_semantic_output_later_in_the_same_frame() {
    let mut fixture = fixture();
    let expected = fixture.engine.version();
    let provider = fixture.presentation_host.platform_provider();
    let snapshot = next_destroyed_child_snapshot(&mut fixture);
    let mut writer = support::TestInputStream::resume(&fixture.engine, TEST_INPUT_SOURCE);
    let mut frame = fixture.presentation_host.begin(&fixture.engine);
    let (tab, event) = child_next_tab_event(&frame, fixture.child_binding);

    writer
        .append(
            &mut frame,
            EngineInput::PublishPlatformSnapshot {
                provider,
                expected_epoch: expected.epoch(),
                snapshot,
            },
        )
        .expect("destroyed observation must reduce first");
    assert!(!frame.surfaces().any(|surface| surface == SURFACE_CHILD));
    assert!(frame.view().interaction_projection(SURFACE_CHILD).is_none());
    assert!(frame.view().semantic_projection(SURFACE_CHILD).is_none());

    writer
        .append(
            &mut frame,
            EngineInput::ActivateSemanticReceiver { expected, event },
        )
        .expect("stale semantic delivery must reduce to a typed rejection");
    complete_host_frame_with_retained_or_unavailable(&fixture.engine, &mut frame);
    let transition = fixture.presentation_host.finish(frame, &mut fixture.engine);

    assert!(matches!(
        transition.reduced_inputs()[1].outcome(),
        InputOutcome::InteractionProcessed {
            outcome: InteractionOutcome::Rejected(
                InteractionRejection::SemanticPresentationUnavailable {
                    surface: SURFACE_CHILD,
                },
            ),
            ..
        }
    ));
    assert!(matches!(
        fixture.engine.workspace().node(tab.tabs),
        Some(Node::Tabs {
            selected: Some(item),
            ..
        }) if *item == ItemId::new(2)
    ));
    assert!(
        fixture
            .engine
            .viewport()
            .recovery_pending(SURFACE_CHILD)
            .is_some()
    );
}

#[test]
fn semantic_input_before_child_destruction_keeps_actual_same_frame_order() {
    let mut fixture = fixture();
    let expected = fixture.engine.version();
    let provider = fixture.presentation_host.platform_provider();
    let snapshot = next_destroyed_child_snapshot(&mut fixture);
    let mut writer = support::TestInputStream::resume(&fixture.engine, TEST_INPUT_SOURCE);
    let mut frame = fixture.presentation_host.begin(&fixture.engine);
    let (tab, event) = child_next_tab_event(&frame, fixture.child_binding);

    writer
        .append(
            &mut frame,
            EngineInput::ActivateSemanticReceiver { expected, event },
        )
        .expect("current semantic delivery must reduce first");
    writer
        .append(
            &mut frame,
            EngineInput::PublishPlatformSnapshot {
                provider,
                expected_epoch: expected.epoch(),
                snapshot,
            },
        )
        .expect("later destroyed observation must preserve arrival order");
    complete_host_frame_with_retained_or_unavailable(&fixture.engine, &mut frame);
    let transition = fixture.presentation_host.finish(frame, &mut fixture.engine);

    let semantic_outcome = transition.reduced_inputs()[0].outcome();
    assert!(
        matches!(
            semantic_outcome,
            InputOutcome::CommandProcessed { changed: true, .. }
        ),
        "unexpected semantic outcome before destruction: {semantic_outcome:?}"
    );
    assert!(matches!(
        transition.reduced_inputs()[1].outcome(),
        InputOutcome::PlatformSnapshotPublished { .. }
    ));
    assert!(matches!(
        fixture.engine.workspace().node(tab.tabs),
        Some(Node::Tabs {
            selected: Some(item),
            ..
        }) if *item == ItemId::new(3)
    ));
    assert!(
        fixture
            .engine
            .viewport()
            .recovery_pending(SURFACE_CHILD)
            .is_some()
    );
}

#[test]
fn adopted_replacement_retains_recovery_for_a_second_destruction() {
    let mut pending = pending_fixture();
    let _ = advance_replacement_to_first_live(&mut pending);

    publish_scene(
        &mut pending.fixture.engine,
        &mut pending.fixture.presentation_host,
    );
    assert_replacement_admitted(&pending.fixture, pending.replacement_binding);

    publish_destroyed_child(
        &mut pending.fixture.engine,
        &mut pending.fixture.presentation_host,
        pending.replacement_binding,
        vec![host_window(pending.fixture.host_binding)],
    );
    publish_scene(
        &mut pending.fixture.engine,
        &mut pending.fixture.presentation_host,
    );
    publish_windows(
        &mut pending.fixture.engine,
        &mut pending.fixture.presentation_host,
        vec![host_window(pending.fixture.host_binding)],
    );

    assert_whole_root_recovered(&pending.fixture);
}

#[test]
fn cancelled_child_close_still_recovers_after_unexpected_destruction() {
    let mut fixture = fixture();
    let close = publish_windows_with_close(
        &mut fixture.engine,
        &mut fixture.presentation_host,
        vec![
            host_window(fixture.host_binding),
            child_window(fixture.child_binding),
        ],
        &[(fixture.child_binding, WindowCloseState::LiveRequested, None)],
    );
    let edge = native_close_edge_from(&close);
    let expected = fixture.engine.version();
    let cancellation = submit_test_input(
        &mut fixture.engine,
        &mut fixture.presentation_host,
        EngineInput::CancelSurfaceClose { expected, edge },
    )
    .expect("close cancellation must reduce");
    assert!(matches!(
        cancellation.reduced_inputs(),
        [input]
            if matches!(
                input.outcome(),
                InputOutcome::SurfaceCloseCancellationRequested { edge: actual, .. }
                    if *actual == edge
            )
    ));
    assert!(cancellation.platform_effects().iter().any(|effect| {
        matches!(
            effect.effect(),
            PlatformEffect::ResolveNativeClose { edge, resolution, .. }
                if edge.binding() == fixture.child_binding
                    && *resolution == dockspace::effect::NativeCloseResolution::Cancel
        )
    }));

    publish_windows_with_close(
        &mut fixture.engine,
        &mut fixture.presentation_host,
        vec![host_window(fixture.host_binding)],
        &[(fixture.child_binding, WindowCloseState::Destroyed, None)],
    );
    publish_scene(&mut fixture.engine, &mut fixture.presentation_host);
    publish_windows(
        &mut fixture.engine,
        &mut fixture.presentation_host,
        vec![host_window(fixture.host_binding)],
    );

    assert_whole_root_recovered(&fixture);
}

#[test]
fn host_ready_before_replacement_rehomes_and_compensates_exactly_once() {
    let mut pending = pending_fixture();

    let recovered = make_host_current_and_retry_recovery(
        &mut pending.fixture.engine,
        &mut pending.fixture.presentation_host,
    );
    let compensations: Vec<&PlatformEffectEmission> = recovered
        .platform_effects()
        .iter()
        .filter(|request| {
            matches!(
                request.effect(),
                PlatformEffect::CompensatingClose {
                    binding,
                    compensates,
                } if *binding == pending.replacement_binding
                    && *compensates == pending.replacement_effect
            )
        })
        .collect();

    assert_eq!(compensations.len(), 1);
    assert_eq!(
        compensations[0].provider(),
        pending.fixture.presentation_host.platform_provider()
    );
    let compensation = compensations[0].id();
    assert_whole_root_recovered(&pending.fixture);
    assert_eq!(
        pending
            .fixture
            .engine
            .viewport()
            .recovery_pending(SURFACE_CHILD)
            .expect("replacement compensation must remain queryable")
            .status(),
        RecoveryPendingStatus::CompensatingReplacement {
            replacement: pending.replacement_effect,
            cleanup: compensation,
        }
    );

    let repeated = publish_windows(
        &mut pending.fixture.engine,
        &mut pending.fixture.presentation_host,
        vec![host_window(pending.fixture.host_binding)],
    );
    assert_no_new_effects(&repeated);
    assert_eq!(
        effect_count(&pending.fixture.engine, |effect| matches!(
            effect,
            PlatformEffect::CompensatingClose {
                binding,
                compensates,
            } if *binding == pending.replacement_binding
                && *compensates == pending.replacement_effect
        )),
        1
    );
}

#[test]
fn terminal_replacement_cleanup_consumes_the_recovery_obligation() {
    let mut pending = pending_fixture();
    let recovered = make_host_current_and_retry_recovery(
        &mut pending.fixture.engine,
        &mut pending.fixture.presentation_host,
    );
    assert!(recovered.platform_effects().iter().any(|request| {
        matches!(
            request.effect(),
            PlatformEffect::CompensatingClose { binding, .. }
                if *binding == pending.replacement_binding
        )
    }));
    assert_whole_root_recovered(&pending.fixture);

    let terminal = publish_destroyed_child(
        &mut pending.fixture.engine,
        &mut pending.fixture.presentation_host,
        pending.replacement_binding,
        vec![host_window(pending.fixture.host_binding)],
    );
    assert_no_new_effects(&terminal);
    assert!(
        pending
            .fixture
            .engine
            .viewport()
            .recovery_pending(SURFACE_CHILD)
            .is_none()
    );

    let host_tabs = pending
        .fixture
        .engine
        .workspace()
        .root(ROOT_HOST)
        .expect("recovery host root remains available")
        .node;
    let selection = pending
        .fixture
        .engine
        .workspace()
        .capture_item_source(ROOT_HOST, host_tabs, ItemId::new(1))
        .expect("ordinary host selection source remains current");
    let expected = pending.fixture.engine.version();
    let command = submit_test_input(
        &mut pending.fixture.engine,
        &mut pending.fixture.presentation_host,
        EngineInput::WorkspaceCommand {
            expected,
            command: WorkspaceCommand::Select { source: selection },
        },
    )
    .expect("a consumed recovery obligation must not freeze ordinary commands");
    assert!(matches!(
        command.reduced_inputs()[0].outcome(),
        InputOutcome::CommandProcessed { .. }
    ));
}

#[test]
fn failed_replacement_compensation_retries_only_after_explicit_input() {
    let mut pending = pending_fixture();
    let recovered = make_host_current_and_retry_recovery(
        &mut pending.fixture.engine,
        &mut pending.fixture.presentation_host,
    );
    let failed_cleanup = recovered
        .platform_effects()
        .iter()
        .find_map(|request| {
            matches!(
                request.effect(),
                PlatformEffect::CompensatingClose {
                    binding,
                    compensates,
                } if *binding == pending.replacement_binding
                    && *compensates == pending.replacement_effect
            )
            .then_some(request.id())
        })
        .expect("recovery must request one replacement cleanup");
    let expected_epoch = pending.fixture.engine.version().epoch();
    let provider = pending.fixture.presentation_host.platform_provider();
    let failed = submit_test_input(
        &mut pending.fixture.engine,
        &mut pending.fixture.presentation_host,
        EngineInput::ReportPlatformEffect {
            provider,
            expected_epoch,
            result: EffectResult::new(
                failed_cleanup,
                expected_epoch,
                EffectDispatchResult::DispatchFailed(DispatchFailureReason::WindowUnavailable),
            ),
        },
    )
    .expect("replacement cleanup failure must reduce");
    assert_no_new_effects(&failed);

    let expected = pending.fixture.engine.version();
    let retried = submit_test_input(
        &mut pending.fixture.engine,
        &mut pending.fixture.presentation_host,
        EngineInput::RetryViewportCleanup {
            expected,
            failed_effect: failed_cleanup,
        },
    )
    .expect("replacement cleanup retry must reduce");
    let retry = retried
        .platform_effects()
        .iter()
        .find_map(|request| {
            matches!(
                request.effect(),
                PlatformEffect::CompensatingClose {
                    binding,
                    compensates,
                } if *binding == pending.replacement_binding
                    && *compensates == pending.replacement_effect
            )
            .then_some(request.id())
        })
        .expect("explicit retry must issue one new replacement cleanup");
    assert_ne!(retry, failed_cleanup);
    assert_eq!(
        pending
            .fixture
            .engine
            .viewport()
            .recovery_pending(SURFACE_CHILD)
            .expect("replacement cleanup must remain queryable")
            .status(),
        RecoveryPendingStatus::CompensatingReplacement {
            replacement: pending.replacement_effect,
            cleanup: retry,
        }
    );

    let late = publish_windows(
        &mut pending.fixture.engine,
        &mut pending.fixture.presentation_host,
        vec![
            host_window(pending.fixture.host_binding),
            replacement_window(pending.replacement_binding),
        ],
    );
    assert_no_new_effects(&late);
}

#[test]
fn replacement_dispatch_failure_remains_recoverable_without_redispatch() {
    let mut pending = pending_fixture();
    let expected_epoch = pending.fixture.engine.version().epoch();
    let provider = pending.fixture.presentation_host.platform_provider();
    let failed = submit_test_input(
        &mut pending.fixture.engine,
        &mut pending.fixture.presentation_host,
        EngineInput::ReportPlatformEffect {
            provider,
            expected_epoch,
            result: EffectResult::new(
                pending.replacement_effect,
                expected_epoch,
                EffectDispatchResult::DispatchFailed(DispatchFailureReason::AdapterRejected),
            ),
        },
    )
    .expect("replacement failure must reduce");

    assert_no_new_effects(&failed);
    let recovery_pending = pending
        .fixture
        .engine
        .viewport()
        .recovery_pending(SURFACE_CHILD)
        .expect("dispatch failure must retain the recovery");
    assert_eq!(recovery_pending.replacement_binding(), None);
    assert_eq!(
        recovery_pending.status(),
        RecoveryPendingStatus::ReplacementFailed {
            replacement: pending.replacement_effect,
            failed: pending.replacement_effect,
        }
    );
    assert_recovery_presentation_scope(&mut pending.fixture, None, false);
    assert!(matches!(
        pending
            .fixture
            .engine
            .viewport()
            .effects()
            .record(pending.replacement_effect)
            .expect("failed replacement effect must remain auditable")
            .phase(),
        EffectPhase::DispatchFailed(DispatchFailureReason::AdapterRejected)
    ));

    let repeated = publish_windows(
        &mut pending.fixture.engine,
        &mut pending.fixture.presentation_host,
        vec![unavailable_host_window(pending.fixture.host_binding)],
    );
    assert_no_new_effects(&repeated);
    assert_eq!(
        effect_count(&pending.fixture.engine, |effect| matches!(
            effect,
            PlatformEffect::RequestReplacement { .. }
        )),
        1
    );

    let recovered = make_host_current_and_retry_recovery(
        &mut pending.fixture.engine,
        &mut pending.fixture.presentation_host,
    );
    assert_no_new_effects(&recovered);
    assert_whole_root_recovered(&pending.fixture);
    assert!(
        pending
            .fixture
            .engine
            .viewport()
            .recovery_pending(SURFACE_CHILD)
            .is_none()
    );
    assert_eq!(
        effect_count(&pending.fixture.engine, |effect| matches!(
            effect,
            PlatformEffect::CompensatingClose { .. }
        )),
        0
    );
}

#[test]
fn external_replacement_cannot_bypass_correlated_native_bringup() {
    let mut pending = pending_fixture();
    let expected_epoch = pending.fixture.engine.version().epoch();
    let provider = pending.fixture.presentation_host.platform_provider();
    submit_test_input(
        &mut pending.fixture.engine,
        &mut pending.fixture.presentation_host,
        EngineInput::ReportPlatformEffect {
            provider,
            expected_epoch,
            result: EffectResult::new(
                pending.replacement_effect,
                expected_epoch,
                EffectDispatchResult::DispatchFailed(DispatchFailureReason::AdapterRejected),
            ),
        },
    )
    .expect("replacement failure must release the reserved binding");
    assert_eq!(
        pending
            .fixture
            .engine
            .viewport()
            .recovery_pending(SURFACE_CHILD)
            .expect("failed replacement must remain pending")
            .replacement_binding(),
        None
    );

    let mismatched = SurfaceRecoveryTarget::with_converted_main(
        pending.fixture.recovery_target.anchor(),
        ConvertedMainRecovery::new(
            ROOT_CHILD,
            RECOVERY_FLOATING,
            LogicalSize::new(1.0, 0.0).expect("different minimum size must be valid"),
        ),
    );
    let expected = pending.fixture.engine.version();
    let rejected = submit_test_input(
        &mut pending.fixture.engine,
        &mut pending.fixture.presentation_host,
        EngineInput::RegisterViewport {
            provider,
            expected,
            surface: SURFACE_CHILD,
            token: WindowToken::new(21),
            role: ViewportRole::Child,
            recovery_target: Some(mismatched),
        },
    )
    .expect("mismatched replacement registration must reduce");
    let outcome = rejected.reduced_inputs()[0].outcome();
    assert!(
        matches!(
            outcome,
            InputOutcome::ViewportRegistrationRejected {
                surface: SURFACE_CHILD
            }
        ),
        "unexpected mismatched-target registration outcome: {outcome:?}"
    );
    assert!(
        pending
            .fixture
            .engine
            .viewport()
            .viewport(SURFACE_CHILD)
            .is_none()
    );

    let rejected = submit_test_input(
        &mut pending.fixture.engine,
        &mut pending.fixture.presentation_host,
        EngineInput::RegisterViewport {
            provider,
            expected,
            surface: SURFACE_CHILD,
            token: WindowToken::new(22),
            role: ViewportRole::Child,
            recovery_target: Some(pending.fixture.recovery_target),
        },
    )
    .expect("uncorrelated replacement registration must reduce fail-closed");
    assert!(matches!(
        rejected.reduced_inputs()[0].outcome(),
        InputOutcome::ViewportRegistrationRejected {
            surface: SURFACE_CHILD
        }
    ));
    let recovery = pending
        .fixture
        .engine
        .viewport()
        .recovery_pending(SURFACE_CHILD)
        .expect("failed replacement recovery must remain pending");
    assert_eq!(recovery.replacement_binding(), None);
    assert_eq!(
        pending
            .fixture
            .engine
            .surface_recovery_target(SURFACE_CHILD),
        Some(pending.fixture.recovery_target)
    );
    assert!(matches!(
        recovery.status(),
        RecoveryPendingStatus::ReplacementFailed { .. }
    ));
    assert_recovery_presentation_scope(&mut pending.fixture, None, false);
}

#[test]
fn dispatch_failed_replacement_cannot_bypass_bringup_with_visible_facts() {
    let mut pending = pending_fixture();
    let observed = publish_windows(
        &mut pending.fixture.engine,
        &mut pending.fixture.presentation_host,
        vec![
            unavailable_host_window(pending.fixture.host_binding),
            partially_observed_replacement_window(pending.replacement_binding),
        ],
    );
    assert_no_new_effects(&observed);
    assert_eq!(
        pending
            .fixture
            .engine
            .viewport()
            .recovery_pending(SURFACE_CHILD)
            .expect("geometryless replacement must remain pending")
            .status(),
        RecoveryPendingStatus::ReplacementRequested {
            replacement: pending.replacement_effect
        }
    );
    assert!(matches!(
        pending
            .fixture
            .engine
            .viewport()
            .effects()
            .record(pending.replacement_effect)
            .expect("replacement effect remains auditable")
            .phase(),
        EffectPhase::Requested
    ));
    let expected_epoch = pending.fixture.engine.version().epoch();
    let provider = pending.fixture.presentation_host.platform_provider();
    let rejected = submit_test_input(
        &mut pending.fixture.engine,
        &mut pending.fixture.presentation_host,
        EngineInput::ReportPlatformEffect {
            provider,
            expected_epoch,
            result: EffectResult::new(
                pending.replacement_effect,
                expected_epoch,
                EffectDispatchResult::DispatchFailed(DispatchFailureReason::AdapterRejected),
            ),
        },
    )
    .expect("observed replacement failure must reduce");
    let cleanup = rejected
        .platform_effects()
        .iter()
        .find_map(|emission| {
            matches!(
                emission.effect(),
                PlatformEffect::CompensatingClose {
                    binding,
                    compensates,
                } if *binding == pending.replacement_binding
                    && *compensates == pending.replacement_effect
            )
            .then_some(emission.id())
        })
        .expect("an observed failed replacement must be retired immediately");

    let failed = pending
        .fixture
        .engine
        .viewport()
        .recovery_pending(SURFACE_CHILD)
        .unwrap_or_else(|| {
            panic!(
                "observed replacement must retain recovery association; pending={:?}, replacement={:?}",
                pending
                    .fixture
                    .engine
                    .viewport()
                    .pending_recoveries()
                    .collect::<Vec<_>>(),
                pending
                    .fixture
                    .engine
                    .viewport()
                    .registry()
                    .record(SURFACE_CHILD),
            )
        });
    assert_eq!(failed.replacement_binding(), None);
    assert_eq!(
        failed.status(),
        RecoveryPendingStatus::ReplacementFailed {
            replacement: pending.replacement_effect,
            failed: pending.replacement_effect,
        }
    );
    let retirement = pending
        .fixture
        .engine
        .viewport()
        .binding_retirements()
        .find_map(|(binding, retirement)| {
            (binding == pending.replacement_binding).then_some(retirement)
        })
        .expect("the failed replacement binding must have one cleanup owner");
    assert_eq!(
        retirement.origin(),
        BindingRetirementOrigin::RecoveryReplacementFailed {
            replacement: pending.replacement_effect,
            failed: pending.replacement_effect,
        }
    );
    assert_eq!(
        retirement.status(),
        BindingRetirementStatus::CleanupRequested { effect: cleanup }
    );
    assert_recovery_presentation_scope(&mut pending.fixture, None, false);

    let ready = publish_windows(
        &mut pending.fixture.engine,
        &mut pending.fixture.presentation_host,
        vec![
            unavailable_host_window(pending.fixture.host_binding),
            replacement_window(pending.replacement_binding),
        ],
    );
    assert_no_new_effects(&ready);
    assert_eq!(
        pending
            .fixture
            .engine
            .viewport()
            .recovery_pending(SURFACE_CHILD)
            .expect("uncorrelated visible facts cannot consume recovery")
            .status(),
        RecoveryPendingStatus::ReplacementFailed {
            replacement: pending.replacement_effect,
            failed: pending.replacement_effect,
        }
    );
    assert!(
        pending
            .fixture
            .engine
            .workspace()
            .contained_floating(RECOVERY_FLOATING)
            .is_none()
    );

    let recovered = make_host_current_and_retry_recovery(
        &mut pending.fixture.engine,
        &mut pending.fixture.presentation_host,
    );
    assert_no_new_effects(&recovered);
    assert_whole_root_recovered(&pending.fixture);
}

#[test]
fn failed_recovery_show_retires_the_window_without_waiting_for_host_recovery() {
    for failure in [
        EffectDispatchResult::DispatchFailed(DispatchFailureReason::AdapterRejected),
        EffectDispatchResult::Unsupported(EffectUnsupportedReason::BackendUnsupported),
    ] {
        let mut pending = pending_fixture();
        let show = present_replacement_pre_show(&mut pending);
        let expected_epoch = pending.fixture.engine.version().epoch();
        let provider = pending.fixture.presentation_host.platform_provider();
        let failed = submit_test_input(
            &mut pending.fixture.engine,
            &mut pending.fixture.presentation_host,
            EngineInput::ReportPlatformEffect {
                provider,
                expected_epoch,
                result: EffectResult::new(show, expected_epoch, failure),
            },
        )
        .expect("deterministic show failure must reduce");

        let cleanup = failed
            .platform_effects()
            .iter()
            .find_map(|emission| {
                matches!(
                    emission.effect(),
                    PlatformEffect::CompensatingClose {
                        binding,
                        compensates,
                    } if *binding == pending.replacement_binding
                        && *compensates == pending.replacement_effect
                )
                .then_some(emission.id())
            })
            .expect("the failed show must immediately schedule exact binding cleanup");
        let recovery = pending
            .fixture
            .engine
            .viewport()
            .recovery_pending(SURFACE_CHILD)
            .expect("content recovery must remain pending independently of window cleanup");
        assert_eq!(recovery.replacement_binding(), None);
        assert_eq!(
            recovery.status(),
            RecoveryPendingStatus::ReplacementFailed {
                replacement: pending.replacement_effect,
                failed: show,
            }
        );
        assert!(
            pending
                .fixture
                .engine
                .viewport()
                .viewport(SURFACE_CHILD)
                .is_none()
        );
        let retirement = pending
            .fixture
            .engine
            .viewport()
            .binding_retirements()
            .find_map(|(binding, retirement)| {
                (binding == pending.replacement_binding).then_some(retirement)
            })
            .expect("failed show must transfer its exact binding to retirement");
        assert_eq!(
            retirement.origin(),
            BindingRetirementOrigin::RecoveryReplacementFailed {
                replacement: pending.replacement_effect,
                failed: show,
            }
        );
        assert_eq!(
            retirement.status(),
            BindingRetirementStatus::CleanupRequested { effect: cleanup }
        );
        assert_recovery_presentation_scope(&mut pending.fixture, None, false);

        let unavailable = publish_windows(
            &mut pending.fixture.engine,
            &mut pending.fixture.presentation_host,
            vec![unavailable_host_window(pending.fixture.host_binding)],
        );
        assert_no_new_effects(&unavailable);
        assert_eq!(
            pending
                .fixture
                .engine
                .viewport()
                .recovery_pending(SURFACE_CHILD)
                .expect("unavailable host must retain content recovery")
                .status(),
            RecoveryPendingStatus::ReplacementFailed {
                replacement: pending.replacement_effect,
                failed: show,
            }
        );
        assert_eq!(
            pending
                .fixture
                .engine
                .viewport()
                .binding_retirements()
                .find_map(|(binding, retirement)| {
                    (binding == pending.replacement_binding).then_some(retirement.status())
                }),
            Some(BindingRetirementStatus::CleanupRequested { effect: cleanup })
        );
    }
}

#[test]
fn recovery_replacement_requires_exact_hidden_acknowledgement() {
    for presentation in [
        Authority::Known(WindowPresentationState::Hidden),
        Authority::Known(WindowPresentationState::Visible),
        Authority::Unknown(AuthorityUnavailableReason::NotReported),
    ] {
        let mut pending = pending_fixture();
        let transition = publish_windows_with_presentation(
            &mut pending.fixture.engine,
            &mut pending.fixture.presentation_host,
            vec![
                unavailable_host_window(pending.fixture.host_binding),
                replacement_window(pending.replacement_binding),
            ],
            presentation,
        );
        assert_no_new_effects(&transition);

        let recovery = pending
            .fixture
            .engine
            .viewport()
            .recovery_pending(SURFACE_CHILD)
            .expect("uncorrelated replacement facts must remain pending");
        assert_eq!(
            recovery.status(),
            RecoveryPendingStatus::ReplacementRequested {
                replacement: pending.replacement_effect
            }
        );
        assert_eq!(
            pending
                .fixture
                .engine
                .viewport()
                .registry()
                .record(SURFACE_CHILD)
                .expect("replacement binding remains registered")
                .admission(),
            ViewportAdmission::Pending
        );
        assert!(matches!(
            pending
                .fixture
                .engine
                .viewport()
                .effects()
                .record(pending.replacement_effect)
                .expect("replacement effect remains auditable")
                .phase(),
            EffectPhase::Requested
        ));
    }
}

#[test]
fn recovery_replacement_rejects_the_wrong_hidden_acknowledgement() {
    let mut pending = pending_fixture();
    let transition = publish_windows_with_presentation_ack(
        &mut pending.fixture.engine,
        &mut pending.fixture.presentation_host,
        vec![
            unavailable_host_window(pending.fixture.host_binding),
            replacement_window(pending.replacement_binding),
        ],
        WindowPresentationState::Hidden,
        pending.replacement_binding,
        EffectId::new(u64::MAX - 1),
    );
    assert_no_new_effects(&transition);
    assert_eq!(
        pending
            .fixture
            .engine
            .viewport()
            .recovery_pending(SURFACE_CHILD)
            .expect("wrong acknowledgement cannot consume recovery")
            .status(),
        RecoveryPendingStatus::ReplacementRequested {
            replacement: pending.replacement_effect,
        }
    );
    assert!(matches!(
        pending
            .fixture
            .engine
            .viewport()
            .effects()
            .record(pending.replacement_effect)
            .expect("replacement effect remains pending")
            .phase(),
        EffectPhase::Requested
    ));
}
