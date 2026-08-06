mod support;

use std::collections::BTreeMap;

use dockspace::RootPresentationOwner;
use dockspace::command::{
    CloseCommitOutcome, ContainedPosition, ContentCloseTarget, MovePayload, RootContent,
    RootPresentationTarget, WorkspaceCommand,
};
use dockspace::effect::{
    CleanupObservationToken, DispatchFailureReason, EffectDispatchResult, EffectId,
    EffectIndeterminateReason, EffectPhase, EffectRequest, EffectResult, EffectTransition,
    EffectUnsupportedReason, PlatformEffect, PlatformEffectEmission,
};
use dockspace::engine::{
    BackendIngressProgress, CoreHostFrame, CoreHostFrameError, DockEngine, EngineError, EngineInput,
};
use dockspace::frame::{
    BindingRetirementOrigin, BindingRetirementStatus, NativeCreatePhase, NativeCreateRequest,
    RecoveryPendingStatus,
};
use dockspace::geometry::{
    LogicalPoint, LogicalRect, LogicalSize, PhysicalPoint, PhysicalRect, ScaleFactor,
};
use dockspace::graph::{Node, RootRecord, SurfacePresentation, Workspace};
use dockspace::ids::{
    FloatingPresentationId, ItemId, NodeId, RootId, StableInputSourceId, SurfaceId, WorkspaceEpoch,
};
use dockspace::intent::{Authority, AuthorityUnavailableReason, PointerButton, PointerId};
use dockspace::interaction::{
    DragSessionId, InteractionDelivery, InteractionOutcome, InteractionRejection,
    InteractionStatus, PreviewResolutionStatus,
};
use dockspace::platform::{
    CloseEffectAcknowledgement, InputEffectAcknowledgement, ObservedWindow, ObservedWorkArea,
    PlatformCapabilities, PlatformCapability, PlatformSnapshot, PresentationEffectAcknowledgement,
    WindowCloseObservation, WindowCloseState, WindowCoordinateObservation, WindowInputObservation,
    WindowInputState, WindowPresentationObservation, WindowPresentationState,
};
use dockspace::pointer_journal::{
    DesktopDockRoute, DesktopRouteFact, DesktopWorkAreaRoute, PointerCaptureOwner, PointerEdge,
    PointerEdgeJournal, PointerEdgeKind, PointerEdgeLocation, PointerEdgeSequence,
    PointerEventDeliveryOwner, PointerInputLease, PointerProviderScope,
};
use dockspace::pointer_receiver::{
    PointerReceiverDelivery, PointerReceiverDeliveryDisposition, PointerReceiverHoverHit,
    PointerReceiverHoverHitDisposition, PointerReceiverObservation, PointerReceiverProbeReceipt,
    PointerReceiverReceiptBatch, PointerReceiverUnknownReason, PresentedPointerReceiverObservation,
};
use dockspace::policy::DockPolicy;
use dockspace::presentation_hit::PresentationHitRegionKind;
use dockspace::presentation_observation::NativeStagingPresentationPhase;
use dockspace::scene::SurfaceScene;
use dockspace::surface_recovery::{
    ConvertedMainRecovery, RootRecoveryAnchor, SurfaceRecoveryTarget,
};
use dockspace::transition::{EngineTransition, InputOutcome, SurfaceContributionOutcome};
use dockspace::viewport::{
    CloseObservationGeneration, CoordinateObservationGeneration, InputObservationGeneration,
    PresentationObservationGeneration, ViewportBinding, ViewportRole, WindowToken, WorkAreaToken,
};
use dockspace::viewport_focus::{
    FocusObservationEnvelope, FocusObservationGeneration, GlobalFocusedWindow,
    unknown_focus_observation,
};
use dockspace::viewport_registry::{ViewportAdmission, ViewportLifecycle, ViewportOwnership};
use dockspace::{NativeCloseEdge, SurfaceCloseRequest};

const ROOT_SOURCE: RootId = RootId::new(1);
const ROOT_HOST: RootId = RootId::new(2);
const ROOT_NATIVE: RootId = RootId::new(3);
const ROOT_REPLACEMENT: RootId = RootId::new(11);
const ROOT_NATIVE_RETRY: RootId = RootId::new(4);
const SURFACE_SOURCE: SurfaceId = SurfaceId::new(1);
const SURFACE_HOST: SurfaceId = SurfaceId::new(2);
const SURFACE_NATIVE: SurfaceId = SurfaceId::new(3);
const SURFACE_REPLACEMENT: SurfaceId = SurfaceId::new(11);
const SURFACE_NATIVE_RETRY: SurfaceId = SurfaceId::new(4);
const FLOATING_RECOVERY: FloatingPresentationId = FloatingPresentationId::new(1);
const FLOATING_RECOVERY_RETRY: FloatingPresentationId = FloatingPresentationId::new(2);
const SOURCE_TOKEN: WindowToken = WindowToken::new(41);
const HOST_TOKEN: WindowToken = WindowToken::new(42);
const WORK_AREA: WorkAreaToken = WorkAreaToken::new(51);
const POINTER: PointerId = PointerId::new(1);
const TEST_INPUT_SOURCE: StableInputSourceId = StableInputSourceId::new(0xd0c5_0000_0000_0202);

fn submit_test_input(
    fixture: &mut Fixture,
    input: EngineInput,
) -> Result<EngineTransition, EngineError> {
    if fixture.presentation_host.has_joined_backend() {
        return support::submit_input(
            &mut fixture.engine,
            &mut fixture.presentation_host,
            TEST_INPUT_SOURCE,
            input,
        );
    }
    let mut frame = fixture.presentation_host.begin(&fixture.engine);
    support::TestInputStream::resume(&fixture.engine, TEST_INPUT_SOURCE)
        .append(&mut frame, input)
        .expect("test host-frame input must be structurally valid");
    stage_pointer_checkpoint(fixture, &mut frame);
    support::complete_host_frame_with_retained_or_unavailable(&fixture.engine, &mut frame);
    Ok(fixture.presentation_host.finish(frame, &mut fixture.engine))
}

fn submit_test_inputs(
    fixture: &mut Fixture,
    inputs: impl IntoIterator<Item = EngineInput>,
) -> Result<EngineTransition, EngineError> {
    if fixture.presentation_host.has_joined_backend() {
        return support::submit_inputs(
            &mut fixture.engine,
            &mut fixture.presentation_host,
            TEST_INPUT_SOURCE,
            inputs,
        );
    }
    let mut stream = support::TestInputStream::resume(&fixture.engine, TEST_INPUT_SOURCE);
    let mut frame = fixture.presentation_host.begin(&fixture.engine);
    for input in inputs {
        stream
            .append(&mut frame, input)
            .expect("test host-frame input must be structurally valid");
    }
    stage_pointer_checkpoint(fixture, &mut frame);
    support::complete_host_frame_with_retained_or_unavailable(&fixture.engine, &mut frame);
    Ok(fixture.presentation_host.finish(frame, &mut fixture.engine))
}

fn submit_test_input_expect_error(fixture: &mut Fixture, input: EngineInput) -> EngineError {
    let mut frame = fixture.presentation_host.begin(&fixture.engine);
    let mut input_stream = support::TestInputStream::resume(&fixture.engine, TEST_INPUT_SOURCE);
    assert_eq!(
        input_stream.append(&mut frame, input),
        Err(CoreHostFrameError::InputPrefixReductionFailed),
        "rejected test input must poison the host frame at the input prefix"
    );
    frame
        .finish(&mut fixture.engine)
        .expect_err("test input must fail atomically")
}

fn retained_native_staging_resource_is_visible(
    fixture: &mut Fixture,
    resource: dockspace::presentation_observation::NativeStagingResourceId,
) -> bool {
    let mut frame = fixture.presentation_host.begin(&fixture.engine);
    let retained = frame
        .view()
        .retained_native_staging_resources()
        .any(|descriptor| descriptor.id() == resource);
    stage_pointer_checkpoint(fixture, &mut frame);
    support::complete_host_frame_with_retained_or_unavailable(&fixture.engine, &mut frame);
    let _ = fixture.presentation_host.finish(frame, &mut fixture.engine);
    retained
}

fn command_input(engine: &DockEngine, command: WorkspaceCommand) -> EngineInput {
    EngineInput::WorkspaceCommand {
        expected: engine.version(),
        command,
    }
}

fn platform_snapshot_input(engine: &DockEngine, snapshot: PlatformSnapshot) -> EngineInput {
    EngineInput::PublishPlatformSnapshot {
        provider: engine
            .platform_provider()
            .expect("test fixture must retain its platform provider"),
        expected_epoch: engine.version().epoch(),
        snapshot,
    }
}

fn platform_effect_input(engine: &DockEngine, result: EffectResult) -> EngineInput {
    EngineInput::ReportPlatformEffect {
        provider: engine
            .platform_provider()
            .expect("test fixture must retain its platform provider"),
        expected_epoch: engine.version().epoch(),
        result,
    }
}

struct Fixture {
    engine: DockEngine,
    presentation_host: support::TestPresentationHost,
    source_tabs: NodeId,
    source_binding: Option<ViewportBinding>,
    host_binding: Option<ViewportBinding>,
    input_generation: u64,
    pointer_sequence: u64,
    close_generations: BTreeMap<ViewportBinding, u64>,
}

fn stage_pointer_checkpoint(fixture: &Fixture, frame: &mut CoreHostFrame) {
    if fixture.presentation_host.has_joined_backend() {
        return;
    }
    let Some(provider) = fixture.engine.pointer_provider() else {
        return;
    };
    let sequence = PointerEdgeSequence::new(fixture.pointer_sequence);
    frame
        .submit_pointer_journal(
            provider,
            PointerEdgeJournal::new(sequence, sequence, Vec::new())
                .expect("pointer checkpoint must preserve the committed watermark"),
        )
        .expect("active pointer provider must checkpoint every host frame");
    frame
        .submit_pointer_receiver_receipts(
            PointerReceiverReceiptBatch::new(Vec::new())
                .expect("an empty pointer checkpoint has no receiver probes"),
        )
        .expect("empty pointer checkpoint receipts must stage");
}

fn platform_effects<'a>(
    fixture: &Fixture,
    transition: &'a EngineTransition,
) -> &'a [PlatformEffectEmission] {
    let provider = fixture.presentation_host.platform_provider();
    let effects = transition.platform_effects();
    assert!(
        effects
            .iter()
            .all(|emission| emission.provider() == provider),
        "every platform effect must retain the fixture provider provenance"
    );
    effects
}

macro_rules! publish_windows {
    ($fixture:expr, $windows:expr $(,)?) => {{
        let windows = $windows;
        publish_windows_impl($fixture, windows)
    }};
}

macro_rules! publish_windows_with_close_observations {
    ($fixture:expr, $windows:expr, $states:expr $(,)?) => {{
        let windows = $windows;
        let states = $states;
        publish_windows_with_close_observations_impl($fixture, windows, states)
    }};
}

macro_rules! publish_windows_with_global_focus {
    ($fixture:expr, $windows:expr, $focused:expr, $acknowledged_effect:expr $(,)?) => {{
        let windows = $windows;
        let focused = $focused;
        let acknowledged_effect = $acknowledged_effect;
        publish_windows_with_global_focus_impl($fixture, windows, focused, acknowledged_effect)
    }};
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
    builder.set_surface(SURFACE_SOURCE, SurfacePresentation::with_main(ROOT_SOURCE));
    builder.set_surface(SURFACE_HOST, SurfacePresentation::with_main(ROOT_HOST));
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
    builder.set_surface(SURFACE_SOURCE, SurfacePresentation::with_main(ROOT_SOURCE));
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
    builder.set_surface(SURFACE_SOURCE, SurfacePresentation::with_main(ROOT_SOURCE));
    builder.set_surface(SURFACE_NATIVE, SurfacePresentation::with_main(ROOT_NATIVE));
    builder
        .build()
        .expect("replacement workspace must be valid")
}

fn fixture() -> Fixture {
    let (workspace, source_tabs) = base_workspace();
    let mut policy = DockPolicy::default();
    policy.set_allow_native_surfaces(true);
    let mut engine = DockEngine::new(workspace, policy).expect("test engine must be valid");
    let presentation_host = support::TestPresentationHost::new(&mut engine);
    Fixture {
        engine,
        presentation_host,
        source_tabs,
        source_binding: None,
        host_binding: None,
        input_generation: 0,
        pointer_sequence: 0,
        close_generations: BTreeMap::new(),
    }
}

fn joined_fixture() -> Fixture {
    let (workspace, source_tabs) = base_workspace();
    let mut policy = DockPolicy::default();
    policy.set_allow_native_surfaces(true);
    let mut engine = DockEngine::new(workspace, policy).expect("test engine must be valid");
    let presentation_host = support::TestPresentationHost::new_joined(&mut engine);
    Fixture {
        engine,
        presentation_host,
        source_tabs,
        source_binding: None,
        host_binding: None,
        input_generation: 0,
        pointer_sequence: 0,
        close_generations: BTreeMap::new(),
    }
}

fn fixture_with_source_items(source_items: &[u64]) -> Fixture {
    let (workspace, source_tabs) = base_workspace_with_source_items(source_items);
    let mut policy = DockPolicy::default();
    policy.set_allow_native_surfaces(true);
    let mut engine = DockEngine::new(workspace, policy).expect("test engine must be valid");
    let presentation_host = support::TestPresentationHost::new(&mut engine);
    Fixture {
        engine,
        presentation_host,
        source_tabs,
        source_binding: None,
        host_binding: None,
        input_generation: 0,
        pointer_sequence: 0,
        close_generations: BTreeMap::new(),
    }
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
    capabilities.set_pointer_hit_test_observation(PlatformCapability::Supported);
    capabilities.set_pointer_hit_test_control(PlatformCapability::Supported);
    capabilities.set_global_focus_observation(PlatformCapability::Supported);
    capabilities.set_window_activation_control(PlatformCapability::Supported);
    capabilities.set_close_cancellation(PlatformCapability::Supported);
    capabilities
}

fn observed_window(
    binding: ViewportBinding,
    generation: CoordinateObservationGeneration,
    x: f64,
    input_state: WindowInputState,
) -> ObservedWindow {
    ObservedWindow::new(binding)
        .with_coordinate_observation(WindowCoordinateObservation::new(
            binding,
            generation,
            Authority::Known(physical_rect(x, 0.0, 900.0, 700.0)),
            Authority::Known(physical_rect(x - 8.0, -30.0, 916.0, 738.0)),
            Authority::Known(ScaleFactor::new(1.0).expect("test scale factor must be valid")),
            Authority::Known(ScaleFactor::new(1.0).expect("test scale factor must be valid")),
        ))
        .with_input_state(Authority::Known(input_state))
}

fn known_source_binding(fixture: &Fixture) -> ViewportBinding {
    fixture
        .engine
        .viewport()
        .viewport(SURFACE_SOURCE)
        .map(dockspace::viewport_registry::ViewportRecord::binding)
        .or(fixture.source_binding)
        .expect("source binding must have been registered before observation")
}

fn known_host_binding(fixture: &Fixture) -> ViewportBinding {
    fixture
        .engine
        .viewport()
        .viewport(SURFACE_HOST)
        .map(dockspace::viewport_registry::ViewportRecord::binding)
        .or(fixture.host_binding)
        .expect("host binding must have been registered before observation")
}

fn source_window(fixture: &Fixture) -> ObservedWindow {
    observed_window(
        known_source_binding(fixture),
        CoordinateObservationGeneration::new(1),
        0.0,
        WindowInputState::PassThrough,
    )
}

fn host_window(fixture: &Fixture) -> ObservedWindow {
    host_window_at_coordinate_generation(fixture, CoordinateObservationGeneration::new(1))
}

fn host_window_at_coordinate_generation(
    fixture: &Fixture,
    generation: CoordinateObservationGeneration,
) -> ObservedWindow {
    observed_window(
        known_host_binding(fixture),
        generation,
        900.0,
        WindowInputState::ReceivesInput,
    )
}

fn resized_host_window(fixture: &Fixture) -> ObservedWindow {
    let binding = known_host_binding(fixture);
    ObservedWindow::new(binding)
        .with_coordinate_observation(WindowCoordinateObservation::new(
            binding,
            CoordinateObservationGeneration::new(2),
            Authority::Known(physical_rect(900.0, 0.0, 720.0, 560.0)),
            Authority::Known(physical_rect(892.0, -30.0, 736.0, 598.0)),
            Authority::Known(ScaleFactor::new(1.25).expect("test scale factor must be valid")),
            Authority::Known(ScaleFactor::new(1.25).expect("test scale factor must be valid")),
        ))
        .with_input_state(Authority::Known(WindowInputState::ReceivesInput))
}

fn resized_source_window(fixture: &Fixture) -> ObservedWindow {
    let binding = known_source_binding(fixture);
    ObservedWindow::new(binding)
        .with_coordinate_observation(WindowCoordinateObservation::new(
            binding,
            CoordinateObservationGeneration::new(2),
            Authority::Known(physical_rect(0.0, 0.0, 720.0, 560.0)),
            Authority::Known(physical_rect(-8.0, -30.0, 736.0, 598.0)),
            Authority::Known(ScaleFactor::new(1.25).expect("test scale factor must be valid")),
            Authority::Known(ScaleFactor::new(1.25).expect("test scale factor must be valid")),
        ))
        .with_input_state(Authority::Known(WindowInputState::PassThrough))
}

fn unavailable_host_window(fixture: &Fixture) -> ObservedWindow {
    let binding = known_host_binding(fixture);
    ObservedWindow::new(binding).with_coordinate_observation(WindowCoordinateObservation::new(
        binding,
        CoordinateObservationGeneration::new(2),
        Authority::Unknown(AuthorityUnavailableReason::NotReported),
        Authority::Unknown(AuthorityUnavailableReason::NotReported),
        Authority::Unknown(AuthorityUnavailableReason::NotReported),
        Authority::Unknown(AuthorityUnavailableReason::NotReported),
    ))
}

fn platform_snapshot(
    focus: FocusObservationEnvelope,
    windows: Vec<ObservedWindow>,
) -> PlatformSnapshot {
    platform_snapshot_at_generation(focus.generation().get(), focus, windows, Vec::new())
}

fn platform_snapshot_at_generation(
    platform_generation: u64,
    focus: FocusObservationEnvelope,
    windows: Vec<ObservedWindow>,
    close_observations: Vec<WindowCloseObservation>,
) -> PlatformSnapshot {
    let inventory_observation = support::known_inventory_observation(platform_generation, &windows);
    PlatformSnapshot::new(
        dockspace::viewport::PlatformSnapshotGeneration::new(platform_generation),
        support::known_capability_observation(platform_generation, platform_capabilities()),
        focus,
        inventory_observation,
        windows,
        close_observations,
        support::known_work_area_observation(
            platform_generation,
            vec![ObservedWorkArea::new(
                WORK_AREA,
                physical_rect(-1920.0, -200.0, 3840.0, 1400.0),
                ScaleFactor::new(1.0).expect("test work-area scale factor must be valid"),
            )],
        ),
    )
    .expect("test platform snapshot must be canonical")
}

fn publish_windows_impl(fixture: &mut Fixture, windows: Vec<ObservedWindow>) -> EngineTransition {
    fixture.input_generation += 1;
    let focus = unknown_focus_observation(
        FocusObservationGeneration::new(fixture.input_generation),
        AuthorityUnavailableReason::NotReported,
    );
    publish_windows_with_focus_observation(fixture, windows, focus)
}

fn publish_windows_with_baseline_presentation_ack(
    fixture: &mut Fixture,
    mut windows: Vec<ObservedWindow>,
) -> EngineTransition {
    fixture.input_generation += 1;
    let generation = fixture.input_generation;
    for window in &mut windows {
        let binding = window.binding();
        let state = window
            .presentation_observation()
            .and_then(WindowPresentationObservation::known_state)
            .unwrap_or(WindowPresentationState::Visible);
        *window = window
            .clone()
            .with_presentation_observation(WindowPresentationObservation::new(
                binding,
                PresentationObservationGeneration::new(generation),
                Authority::Known(state),
                PresentationEffectAcknowledgement::known(None),
            ));
    }
    let focus = unknown_focus_observation(
        FocusObservationGeneration::new(generation),
        AuthorityUnavailableReason::NotReported,
    );
    let snapshot = platform_snapshot_at_generation(generation, focus, windows, Vec::new());
    let transition = submit_test_input(fixture, platform_snapshot_input(&fixture.engine, snapshot))
        .expect("platform snapshot must reduce");
    assert!(matches!(
        transition.reduced_inputs()[0].outcome(),
        InputOutcome::PlatformSnapshotPublished { .. }
    ));
    transition
}

fn publish_windows_with_close_observations_impl(
    fixture: &mut Fixture,
    windows: Vec<ObservedWindow>,
    states: &[(ViewportBinding, WindowCloseState, Option<EffectId>)],
) -> EngineTransition {
    fixture.input_generation += 1;
    let focus = unknown_focus_observation(
        FocusObservationGeneration::new(fixture.input_generation),
        AuthorityUnavailableReason::NotReported,
    );
    let close_observations = next_close_observations(fixture, states);
    publish_windows_with_focus_and_close_observations(fixture, windows, focus, close_observations)
}

fn next_close_observations(
    fixture: &mut Fixture,
    states: &[(ViewportBinding, WindowCloseState, Option<EffectId>)],
) -> Vec<WindowCloseObservation> {
    states
        .iter()
        .map(|(binding, state, acknowledged_effect)| {
            let generation = fixture.close_generations.entry(*binding).or_default();
            *generation += 1;
            WindowCloseObservation::new(
                *binding,
                CloseObservationGeneration::new(*generation),
                Authority::Known(*state),
                CloseEffectAcknowledgement::known(*acknowledged_effect),
            )
        })
        .collect()
}

fn viewport_binding(fixture: &Fixture, surface: SurfaceId) -> ViewportBinding {
    fixture
        .engine
        .viewport()
        .viewport(surface)
        .expect("test viewport must remain registered")
        .binding()
}

fn binding_retirement(
    fixture: &Fixture,
    binding: ViewportBinding,
) -> &dockspace::frame::BindingRetirement {
    fixture
        .engine
        .viewport()
        .binding_retirements()
        .find_map(|(actual, retirement)| (actual == binding).then_some(retirement))
        .expect("exact retired binding must remain queryable")
}

fn assert_unobserved_external_binding_quarantines_token_until_exact_destruction(
    fixture: &mut Fixture,
    binding: ViewportBinding,
    origin: BindingRetirementOrigin,
) {
    let provider = fixture.presentation_host.platform_provider();
    let retirement = binding_retirement(fixture, binding);
    assert_eq!(retirement.ownership(), ViewportOwnership::External);
    assert_eq!(retirement.origin(), origin);
    assert!(!retirement.observed());
    assert!(!retirement.may_reappear());
    assert_eq!(
        retirement.status(),
        BindingRetirementStatus::AwaitingExactDestruction
    );

    let expected = fixture.engine.version();
    let error = submit_test_input_expect_error(
        fixture,
        EngineInput::RegisterViewport {
            provider,
            expected,
            surface: SURFACE_NATIVE,
            token: HOST_TOKEN,
            role: ViewportRole::Root,
            recovery_target: None,
        },
    );
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

    publish_windows!(fixture, vec![source_window(fixture)]);
    assert_eq!(
        binding_retirement(fixture, binding).status(),
        BindingRetirementStatus::AwaitingExactDestruction,
        "inventory absence alone cannot release an unobserved external token"
    );

    publish_windows_with_close_observations!(
        fixture,
        vec![source_window(fixture)],
        &[(binding, WindowCloseState::Destroyed, None)],
    );
    assert!(
        fixture
            .engine
            .viewport()
            .binding_retirements()
            .all(|(actual, _)| actual != binding)
    );

    let expected = fixture.engine.version();
    let rebound = submit_test_input(
        fixture,
        EngineInput::RegisterViewport {
            provider,
            expected,
            surface: SURFACE_NATIVE,
            token: HOST_TOKEN,
            role: ViewportRole::Root,
            recovery_target: None,
        },
    )
    .expect("exact destroyed evidence may release the retired token");
    assert!(matches!(
        rebound.reduced_inputs()[0].outcome(),
        InputOutcome::ViewportRegistered { binding: rebound }
            if rebound.token() == HOST_TOKEN && rebound.incarnation() != binding.incarnation()
    ));
}

fn publish_windows_with_global_focus_impl(
    fixture: &mut Fixture,
    windows: Vec<ObservedWindow>,
    focused: GlobalFocusedWindow,
    acknowledged_effect: Option<dockspace::effect::EffectId>,
) -> EngineTransition {
    fixture.input_generation += 1;
    let focus = FocusObservationEnvelope::new(
        FocusObservationGeneration::new(fixture.input_generation),
        Authority::Known(focused),
        Authority::Known(acknowledged_effect),
    );
    publish_windows_with_focus_observation(fixture, windows, focus)
}

fn publish_windows_with_focus_observation(
    fixture: &mut Fixture,
    windows: Vec<ObservedWindow>,
    focus: FocusObservationEnvelope,
) -> EngineTransition {
    publish_windows_with_focus_and_close_observations(fixture, windows, focus, Vec::new())
}

fn publish_windows_with_focus_and_close_observations(
    fixture: &mut Fixture,
    windows: Vec<ObservedWindow>,
    focus: FocusObservationEnvelope,
    close_observations: Vec<WindowCloseObservation>,
) -> EngineTransition {
    let snapshot = current_platform_snapshot(fixture, windows, focus, close_observations);
    let input = platform_snapshot_input(&fixture.engine, snapshot);
    let transition = submit_test_input(fixture, input).expect("platform snapshot must reduce");
    assert!(matches!(
        transition.reduced_inputs()[0].outcome(),
        InputOutcome::PlatformSnapshotPublished { .. }
    ));
    transition
}

fn close_content_item(fixture: &mut Fixture, item: ItemId) -> EngineTransition {
    let expected = fixture.engine.version();
    let requested = submit_test_input(
        fixture,
        EngineInput::RequestContentClose {
            expected,
            target: ContentCloseTarget::Item(item),
        },
    )
    .expect("content close request must reduce");
    let InputOutcome::ContentCloseRequested { plan, .. } = requested.reduced_inputs()[0].outcome()
    else {
        panic!("content close target must open a plan");
    };
    assert_eq!(plan.items().len(), 1);
    assert_eq!(plan.items()[0].item(), item);
    let request = plan.request();
    let token = plan.items()[0].token();
    let committed = submit_test_input(
        fixture,
        EngineInput::ResolveClose {
            request,
            token,
            decision: dockspace::CloseDecision::Allow,
        },
    )
    .expect("content close decision must reduce");
    let committed_outcome = committed.reduced_inputs()[0].outcome();
    assert!(
        matches!(
            committed_outcome,
            InputOutcome::CloseDecisionProcessed {
                application: Some(Ok(CloseCommitOutcome::ItemClosed {
                    item: actual,
                    root: ROOT_SOURCE,
                })),
                changed: true,
                ..
            } if *actual == item
        ),
        "unexpected content close outcome: {committed_outcome:?}"
    );
    assert!(committed.events().iter().any(|event| matches!(
        event.kind(),
        dockspace::event::WorkspaceEventKind::CloseCommitted(
            CloseCommitOutcome::ItemClosed {
                item: actual,
                root: ROOT_SOURCE,
            }
        ) if *actual == item
    )));
    committed
}

fn current_platform_snapshot(
    fixture: &mut Fixture,
    mut windows: Vec<ObservedWindow>,
    focus: FocusObservationEnvelope,
    close_observations: Vec<WindowCloseObservation>,
) -> PlatformSnapshot {
    if let Some(binding) = fixture
        .engine
        .viewport()
        .viewport(SURFACE_SOURCE)
        .map(dockspace::viewport_registry::ViewportRecord::binding)
    {
        for window in &mut windows {
            let state = match window.input_state() {
                Authority::Known(state) => *state,
                Authority::Unknown(_) => continue,
            };
            if window.binding() == binding {
                *window = window
                    .clone()
                    .with_input_observation(WindowInputObservation::new(
                        binding,
                        InputObservationGeneration::new(fixture.input_generation),
                        Authority::Known(state),
                        InputEffectAcknowledgement::known(None),
                    ));
            }
        }
    }
    for window in &mut windows {
        let binding = window.binding();
        let state = window
            .presentation_observation()
            .and_then(WindowPresentationObservation::known_state)
            .unwrap_or(WindowPresentationState::Visible);
        let acknowledged_effect = fixture
            .engine
            .viewport()
            .native_create_sagas()
            .find_map(|(_, saga)| {
                (saga.binding() == binding).then(|| saga.phase().presentation_correlation_effect())
            })
            .or_else(|| {
                let pending = fixture
                    .engine
                    .viewport()
                    .recovery_pending(binding.surface())?;
                (pending.replacement_binding() == Some(binding))
                    .then(|| match pending.status() {
                        RecoveryPendingStatus::ReplacementRequested { replacement }
                        | RecoveryPendingStatus::ReplacementIndeterminate { replacement }
                        | RecoveryPendingStatus::AwaitingPreShowPresentation { replacement } => {
                            Some(replacement)
                        }
                        RecoveryPendingStatus::AwaitingShowAcknowledgement { show, .. }
                        | RecoveryPendingStatus::AwaitingVisible { show, .. }
                        | RecoveryPendingStatus::AwaitingPostShowPresentation { show, .. } => {
                            Some(show)
                        }
                        RecoveryPendingStatus::AwaitingRecoveryHost
                        | RecoveryPendingStatus::ReplacementFailed { .. }
                        | RecoveryPendingStatus::ReplacementProviderLost { .. }
                        | RecoveryPendingStatus::AwaitingFirstLivePresentation
                        | RecoveryPendingStatus::CompensatingReplacement { .. } => None,
                    })
                    .flatten()
            });
        *window = window
            .clone()
            .with_presentation_observation(WindowPresentationObservation::new(
                binding,
                PresentationObservationGeneration::new(fixture.input_generation),
                Authority::Known(state),
                PresentationEffectAcknowledgement::known(acknowledged_effect),
            ));
    }
    platform_snapshot_at_generation(fixture.input_generation, focus, windows, close_observations)
}

fn register_base_viewports(
    fixture: &mut Fixture,
    host_role: ViewportRole,
) -> (ViewportBinding, ViewportBinding) {
    publish_scene(fixture);
    let provider = fixture.presentation_host.platform_provider();
    let expected = fixture.engine.version();
    let source = submit_test_input(
        fixture,
        EngineInput::RegisterViewport {
            provider,
            expected,
            surface: SURFACE_SOURCE,
            token: SOURCE_TOKEN,
            role: ViewportRole::Root,
            recovery_target: None,
        },
    )
    .expect("source root viewport registration must reduce");
    let source_binding = match source.reduced_inputs()[0].outcome() {
        InputOutcome::ViewportRegistered { binding } => *binding,
        outcome => panic!("unexpected source registration outcome: {outcome:?}"),
    };
    let source_anchor = fixture
        .engine
        .root_recovery_anchor(SURFACE_SOURCE)
        .expect("registered source root must own a recovery anchor");
    let recovery_target = (host_role == ViewportRole::Child)
        .then(|| close_recovery(source_anchor, ROOT_HOST, FloatingPresentationId::new(70)));
    let expected = fixture.engine.version();
    let host = submit_test_input(
        fixture,
        EngineInput::RegisterViewport {
            provider,
            expected,
            surface: SURFACE_HOST,
            token: HOST_TOKEN,
            role: host_role,
            recovery_target,
        },
    )
    .expect("host viewport registration must reduce");
    let host_binding = match host.reduced_inputs()[0].outcome() {
        InputOutcome::ViewportRegistered { binding } => *binding,
        outcome => panic!("unexpected host registration outcome: {outcome:?}"),
    };
    fixture.source_binding = Some(source_binding);
    fixture.host_binding = Some(host_binding);
    (source_binding, host_binding)
}

fn register_child_source_viewports(fixture: &mut Fixture) -> (ViewportBinding, ViewportBinding) {
    publish_scene(fixture);
    let provider = fixture.presentation_host.platform_provider();
    let expected = fixture.engine.version();
    let host = submit_test_input(
        fixture,
        EngineInput::RegisterViewport {
            provider,
            expected,
            surface: SURFACE_HOST,
            token: HOST_TOKEN,
            role: ViewportRole::Root,
            recovery_target: None,
        },
    )
    .expect("stable host root viewport registration must reduce");
    let host_binding = match host.reduced_inputs()[0].outcome() {
        InputOutcome::ViewportRegistered { binding } => *binding,
        outcome => panic!("unexpected stable host registration outcome: {outcome:?}"),
    };
    let host_anchor = fixture
        .engine
        .root_recovery_anchor(SURFACE_HOST)
        .expect("registered stable host root must own a recovery anchor");
    let expected = fixture.engine.version();
    let source = submit_test_input(
        fixture,
        EngineInput::RegisterViewport {
            provider,
            expected,
            surface: SURFACE_SOURCE,
            token: SOURCE_TOKEN,
            role: ViewportRole::Child,
            recovery_target: Some(close_recovery(
                host_anchor,
                ROOT_SOURCE,
                FloatingPresentationId::new(70),
            )),
        },
    )
    .expect("source child viewport registration must reduce");
    let source_binding = match source.reduced_inputs()[0].outcome() {
        InputOutcome::ViewportRegistered { binding } => *binding,
        outcome => panic!("unexpected source child registration outcome: {outcome:?}"),
    };
    fixture.source_binding = Some(source_binding);
    fixture.host_binding = Some(host_binding);
    (source_binding, host_binding)
}

fn prepare_base_platform(fixture: &mut Fixture, host_role: ViewportRole) {
    register_base_viewports(fixture, host_role);
    publish_windows!(fixture, vec![source_window(fixture), host_window(fixture)]);
}

fn prepare_child_source_platform(fixture: &mut Fixture) {
    register_child_source_viewports(fixture);
    publish_windows!(fixture, vec![source_window(fixture), host_window(fixture)]);
}

fn publish_surface_for_fixture(fixture: &mut Fixture, surface: SurfaceId, bounds: LogicalRect) {
    if fixture.engine.pointer_provider().is_none() {
        support::publish_surface(
            &mut fixture.engine,
            &mut fixture.presentation_host,
            surface,
            bounds,
        );
        return;
    }

    let mut frame = fixture.presentation_host.begin(&fixture.engine);
    let ticket = frame
        .view()
        .begin_surface_contribution(surface)
        .expect("active-pointer surface contribution must begin");
    let contribution = frame
        .view()
        .prepare_surface_contribution(
            ticket,
            support::measurements(
                &fixture.engine,
                surface,
                bounds,
                support::MeasurementProfile::default(),
            ),
        )
        .expect("active-pointer surface measurements must prepare");
    stage_pointer_checkpoint(fixture, &mut frame);
    frame
        .push_surface_contribution(contribution)
        .expect("active-pointer frame contributes each surface once");
    support::complete_host_frame_with_retained_or_unavailable(&fixture.engine, &mut frame);
    let transition = fixture.presentation_host.finish(frame, &mut fixture.engine);
    assert!(transition.surface_contributions().iter().any(|outcome| {
        matches!(outcome, SurfaceContributionOutcome::Ready { surface: actual, .. } if *actual == surface)
    }));

    let mut emit_frame = fixture.presentation_host.begin(&fixture.engine);
    stage_pointer_checkpoint(fixture, &mut emit_frame);
    let emit_frame = support::complete_host_frame_with_current_outputs(&fixture.engine, emit_frame);
    fixture
        .presentation_host
        .finish_presentation(emit_frame, &mut fixture.engine);

    let mut observation_frame = fixture.presentation_host.begin(&fixture.engine);
    stage_pointer_checkpoint(fixture, &mut observation_frame);
    support::complete_host_frame_with_retained_or_unavailable(
        &fixture.engine,
        &mut observation_frame,
    );
    fixture
        .presentation_host
        .finish(observation_frame, &mut fixture.engine);
}

fn publish_scene(fixture: &mut Fixture) {
    let surfaces: Vec<_> = fixture
        .engine
        .workspace()
        .surfaces()
        .map(|(surface, _)| surface)
        .collect();
    let mut x = 0.0;
    for surface in surfaces {
        if fixture
            .engine
            .viewport()
            .viewport(surface)
            .is_some_and(|viewport| !viewport.is_ready())
        {
            x += 900.0;
            continue;
        }
        publish_surface_for_fixture(fixture, surface, logical_rect(x, 0.0, 900.0, 700.0));
        x += 900.0;
    }
}

#[derive(Debug, Clone, Copy)]
enum NativeDragSource {
    Item(ItemId),
    Group(NodeId),
}

#[derive(Debug, Clone, Copy)]
struct ActiveJournalDrag {
    provider: PointerInputLease,
    session: DragSessionId,
    source_binding: ViewportBinding,
}

fn arm_journal_drag(fixture: &mut Fixture, source: NativeDragSource) -> ActiveJournalDrag {
    publish_scene(fixture);
    let source_binding = known_source_binding(fixture);
    let source_projection = fixture
        .engine
        .interaction_projection(SURFACE_SOURCE)
        .expect("source surface must have current interaction authority");
    let source_region = source_projection
        .hit_manifest()
        .regions()
        .iter()
        .find(|region| match (source, region.id().kind()) {
            (NativeDragSource::Item(item), PresentationHitRegionKind::TabBody(tab)) => {
                tab.item == item
            }
            (NativeDragSource::Group(tabs), PresentationHitRegionKind::TabGroupGrip(group)) => {
                group.tabs == tabs
            }
            _ => false,
        })
        .expect("journal drag source must have an exact receiver");
    let source_rect = source_region.hit().rect();
    let source_point = LogicalPoint::new(
        source_rect.x() + source_rect.width() * 0.5,
        source_rect.y() + source_rect.height() * 0.5,
    )
    .expect("source tab midpoint must be finite");
    let source_receiver = source_region.id();
    let source_route = DesktopRouteFact::dock(DesktopDockRoute::new(
        source_binding,
        source_projection.authority().coordinate_generation(),
        PhysicalPoint::new(source_point.x(), source_point.y())
            .expect("source desktop point must be finite"),
        source_point,
    ));
    let joined = fixture.presentation_host.has_joined_backend();
    let provider = if joined {
        fixture
            .engine
            .pointer_provider()
            .expect("joined backend must retain its desktop-global pointer provider")
    } else {
        fixture
            .engine
            .create_pointer_provider(
                PointerProviderScope::DesktopGlobal,
                PointerEdgeSequence::new(fixture.pointer_sequence),
            )
            .expect("desktop-global pointer provider must be admitted")
    };
    let press = pointer_journal(
        fixture.pointer_sequence,
        PointerEdgeKind::ButtonPressed(PointerButton::Primary),
        PointerEdgeLocation::Desktop {
            route: source_route,
        },
        Authority::Known(PointerEventDeliveryOwner::Native(source_binding)),
        Authority::Known(PointerCaptureOwner::Native(source_binding)),
    );
    let mut frame = if joined {
        let (frame, progress) =
            fixture
                .presentation_host
                .begin_joined_pointer_frame(&fixture.engine, [], press);
        assert_eq!(progress, BackendIngressProgress::ReceiverReceiptsRequired);
        frame
    } else {
        let mut frame = fixture.presentation_host.begin(&fixture.engine);
        frame
            .submit_pointer_journal(provider, press)
            .expect("journal source press must stage");
        frame
    };
    let candidate = frame
        .pointer_receiver_candidates()
        .expect("journal source press must freeze one receiver candidate")
        .candidates()[0]
        .clone();
    let projection = frame
        .view()
        .interaction_projection(SURFACE_SOURCE)
        .expect("sealed press frame must retain source authority");
    let delivery = PointerReceiverDelivery::new(
        projection,
        PointerReceiverDeliveryDisposition::Dock(source_receiver),
    )
    .expect("journal source press must bind to the presented tab");
    let receipts = PointerReceiverReceiptBatch::new([candidate.receipt(
        PointerReceiverObservation::Presented(
            PresentedPointerReceiverObservation::new([PointerReceiverProbeReceipt::Delivery(
                delivery,
            )])
            .expect("journal source press must answer delivery"),
        ),
    )])
    .expect("journal source press receipt set must be exact");
    if joined {
        assert_eq!(
            frame
                .submit_backend_pointer_receiver_receipts(receipts)
                .expect("joined source press receipts must resume replay"),
            BackendIngressProgress::Complete
        );
    } else {
        frame
            .submit_pointer_receiver_receipts(receipts)
            .expect("journal source press receipt must stage");
    }
    support::complete_host_frame_with_retained_or_unavailable(&fixture.engine, &mut frame);
    let pressed = fixture.presentation_host.finish(frame, &mut fixture.engine);
    let session = match pressed.reduced_pointer_edges()[0].interaction_outcomes() {
        [InteractionOutcome::DragArmed { session, .. }] => *session,
        outcomes => panic!("journal source press must arm one drag, got {outcomes:?}"),
    };
    fixture.pointer_sequence += 1;
    ActiveJournalDrag {
        provider,
        session,
        source_binding,
    }
}

fn move_journal_drag_outside(fixture: &mut Fixture, drag: ActiveJournalDrag) -> EngineTransition {
    let outside =
        PhysicalPoint::new(1_850.0, 900.0).expect("outside-all desktop point must be finite");
    let joined = fixture.presentation_host.has_joined_backend();
    let moved = pointer_journal(
        fixture.pointer_sequence,
        PointerEdgeKind::Moved,
        PointerEdgeLocation::Desktop {
            route: outside_all_route(fixture, outside),
        },
        Authority::Known(PointerEventDeliveryOwner::Native(drag.source_binding)),
        Authority::Known(PointerCaptureOwner::Native(drag.source_binding)),
    );
    let mut frame = if joined {
        let (frame, progress) =
            fixture
                .presentation_host
                .begin_joined_pointer_frame(&fixture.engine, [], moved);
        assert_eq!(progress, BackendIngressProgress::ReceiverReceiptsRequired);
        frame
    } else {
        let mut frame = fixture.presentation_host.begin(&fixture.engine);
        frame
            .submit_pointer_journal(drag.provider, moved)
            .expect("outside-all move must stage");
        frame
    };
    let receipts = unknown_receiver_receipts(&frame);
    if joined {
        assert_eq!(
            frame
                .submit_backend_pointer_receiver_receipts(receipts)
                .expect("joined outside-all receipts must resume replay"),
            BackendIngressProgress::Complete
        );
    } else {
        frame
            .submit_pointer_receiver_receipts(receipts)
            .expect("outside-all move receiver facts must stage");
    }
    support::complete_host_frame_with_retained_or_unavailable(&fixture.engine, &mut frame);
    let transition = fixture.presentation_host.finish(frame, &mut fixture.engine);
    fixture.pointer_sequence += 1;
    transition
}

fn move_journal_drag_to_surface(
    fixture: &mut Fixture,
    drag: ActiveJournalDrag,
    surface: SurfaceId,
    physical_origin_x: f64,
    scale: f64,
) -> EngineTransition {
    let projection = fixture
        .engine
        .interaction_projection(surface)
        .expect("target surface must have current interaction authority");
    let target_region = projection
        .hit_manifest()
        .regions()
        .iter()
        .find(|region| matches!(region.id().kind(), PresentationHitRegionKind::DropTarget(_)))
        .expect("target surface must publish at least one drop receiver");
    let rect = target_region.hit().rect();
    let point = LogicalPoint::new(
        rect.x() + rect.width() * 0.5,
        rect.y() + rect.height() * 0.5,
    )
    .expect("target midpoint must be finite");
    let target_receiver = target_region.id();
    let binding = viewport_binding(fixture, surface);
    let desktop = PhysicalPoint::new(physical_origin_x + point.x() * scale, point.y() * scale)
        .expect("target desktop point must be finite");
    let route = DesktopRouteFact::dock(DesktopDockRoute::new(
        binding,
        projection.authority().coordinate_generation(),
        desktop,
        point,
    ));

    let mut frame = fixture.presentation_host.begin(&fixture.engine);
    frame
        .submit_pointer_journal(
            drag.provider,
            pointer_journal(
                fixture.pointer_sequence,
                PointerEdgeKind::Moved,
                PointerEdgeLocation::Desktop { route },
                Authority::Known(PointerEventDeliveryOwner::Native(drag.source_binding)),
                Authority::Known(PointerCaptureOwner::Native(drag.source_binding)),
            ),
        )
        .expect("target move must stage");
    let candidate = frame
        .pointer_receiver_candidates()
        .expect("target move must freeze one receiver candidate")
        .candidates()[0]
        .clone();
    let projection = frame
        .view()
        .interaction_projection(surface)
        .expect("sealed target frame must retain target authority");
    let hover = PointerReceiverHoverHit::new(
        projection,
        point,
        PointerReceiverHoverHitDisposition::Dock(target_receiver),
    )
    .expect("target hover must bind to the presented drop receiver");
    let receipts = PointerReceiverReceiptBatch::new([candidate.receipt(
        PointerReceiverObservation::Presented(
            PresentedPointerReceiverObservation::new([PointerReceiverProbeReceipt::HoverHit(
                hover,
            )])
            .expect("target move must answer hover"),
        ),
    )])
    .expect("target move receipt set must be exact");
    if let Err(error) = frame.submit_pointer_receiver_receipts(receipts) {
        panic!(
            "target move receipts must stage: {error:?}; source: {:?}",
            frame.input_prefix_error()
        );
    }
    support::complete_host_frame_with_retained_or_unavailable(&fixture.engine, &mut frame);
    let transition = fixture.presentation_host.finish(frame, &mut fixture.engine);
    fixture.pointer_sequence += 1;
    transition
}

fn converted_main_recovery(
    root: RootId,
    floating: FloatingPresentationId,
) -> ConvertedMainRecovery {
    ConvertedMainRecovery::new(
        root,
        floating,
        LogicalSize::new(0.0, 0.0).expect("minimum size must be valid"),
    )
}

fn close_recovery(
    anchor: RootRecoveryAnchor,
    root: RootId,
    floating: FloatingPresentationId,
) -> SurfaceRecoveryTarget {
    SurfaceRecoveryTarget::with_converted_main(anchor, converted_main_recovery(root, floating))
}

fn pointer_journal(
    previous: u64,
    kind: PointerEdgeKind,
    location: PointerEdgeLocation,
    delivery: Authority<PointerEventDeliveryOwner>,
    capture: Authority<PointerCaptureOwner>,
) -> PointerEdgeJournal {
    let sequence = PointerEdgeSequence::new(previous + 1);
    PointerEdgeJournal::new(
        PointerEdgeSequence::new(previous),
        sequence,
        vec![PointerEdge::new_with_delivery(
            sequence, POINTER, kind, location, delivery, capture,
        )],
    )
    .expect("test pointer journal must be contiguous")
}

fn outside_all_route(fixture: &Fixture, position: PhysicalPoint) -> DesktopRouteFact {
    DesktopRouteFact::no_window(
        Authority::Known(position),
        Authority::Known(DesktopWorkAreaRoute::new(
            fixture.presentation_host.platform_provider(),
            fixture.engine.viewport().work_area_generation(),
            WORK_AREA,
        )),
    )
}

fn unknown_receiver_receipts(frame: &CoreHostFrame) -> PointerReceiverReceiptBatch {
    PointerReceiverReceiptBatch::new(
        frame
            .pointer_receiver_candidates()
            .expect("journal must freeze an exact receiver roster")
            .candidates()
            .iter()
            .map(|candidate| {
                let observation = if candidate.receiver_is_applicable() {
                    PointerReceiverObservation::Unknown(
                        PointerReceiverUnknownReason::PresentationAuthorityUnavailable,
                    )
                } else {
                    PointerReceiverObservation::NotApplicable
                };
                candidate.receipt(observation)
            }),
    )
    .expect("test receiver receipts must answer the exact candidate roster")
}

fn request_native_create(fixture: &mut Fixture, source: NativeDragSource) -> NativeCreateRequest {
    let drag = arm_journal_drag(fixture, source);
    let moved = move_journal_drag_outside(fixture, drag);
    assert!(matches!(
        moved.reduced_pointer_edges()[0].interaction_outcomes(),
        [
            InteractionOutcome::DragBegan { .. },
            InteractionOutcome::PreviewUpdated {
                status: PreviewResolutionStatus::Resolved,
                preview: Some(_),
                ..
            }
        ]
    ));

    let outside =
        PhysicalPoint::new(1_850.0, 900.0).expect("outside-all desktop point must be finite");
    let acknowledgement = fixture
        .engine
        .interaction()
        .preview()
        .expect("outside-all move must publish a native preview")
        .acknowledgement();
    let preview_acknowledgement = EngineInput::AcknowledgePreview {
        expected: fixture.engine.version(),
        acknowledgement,
    };
    let release = pointer_journal(
        fixture.pointer_sequence,
        PointerEdgeKind::ButtonReleased(PointerButton::Primary),
        PointerEdgeLocation::Desktop {
            route: outside_all_route(fixture, outside),
        },
        Authority::Known(PointerEventDeliveryOwner::Native(drag.source_binding)),
        Authority::Known(PointerCaptureOwner::None),
    );
    let joined = fixture.presentation_host.has_joined_backend();
    let mut release_frame = if joined {
        let (frame, progress) = fixture.presentation_host.begin_joined_pointer_frame(
            &fixture.engine,
            [preview_acknowledgement],
            release,
        );
        assert_eq!(progress, BackendIngressProgress::ReceiverReceiptsRequired);
        frame
    } else {
        let mut frame = fixture.presentation_host.begin(&fixture.engine);
        support::TestInputStream::resume(&fixture.engine, TEST_INPUT_SOURCE)
            .append(&mut frame, preview_acknowledgement)
            .expect("native preview acknowledgement must stage before release");
        frame
            .submit_pointer_journal(drag.provider, release)
            .expect("outside-all release must stage");
        frame
    };
    let receipts = unknown_receiver_receipts(&release_frame);
    if joined {
        assert_eq!(
            release_frame
                .submit_backend_pointer_receiver_receipts(receipts)
                .expect("joined outside-all release receipts must resume replay"),
            BackendIngressProgress::Complete
        );
    } else {
        release_frame
            .submit_pointer_receiver_receipts(receipts)
            .expect("outside-all release receiver facts must stage");
    }
    support::complete_host_frame_with_retained_or_unavailable(&fixture.engine, &mut release_frame);
    let released = fixture
        .presentation_host
        .finish(release_frame, &mut fixture.engine);
    fixture.pointer_sequence += 1;

    let request = released.reduced_pointer_edges()[0]
        .interaction_outcomes()
        .iter()
        .find_map(|outcome| match outcome {
            InteractionOutcome::DragDelivered {
                delivery: InteractionDelivery::NativeRequested(request),
                ..
            } => Some(*request),
            _ => None,
        })
        .expect("outside-all release must request one native create saga");
    if !joined {
        fixture
            .engine
            .retire_pointer_provider(drag.provider)
            .expect("an idle pointer provider must retire after native release");
    }
    assert!(
        fixture
            .engine
            .viewport()
            .native_create_saga(request.saga())
            .is_some(),
        "pointer-provider retirement must not cancel an independent native saga"
    );
    request
}

fn start_native_create(fixture: &mut Fixture) -> NativeCreateRequest {
    start_native_create_with_presentation_identities(
        fixture,
        SURFACE_NATIVE,
        ROOT_NATIVE,
        FLOATING_RECOVERY,
    )
}

fn start_native_create_with_presentation_identities(
    fixture: &mut Fixture,
    expected_surface: SurfaceId,
    expected_root: RootId,
    expected_floating: FloatingPresentationId,
) -> NativeCreateRequest {
    let request = request_native_create(fixture, NativeDragSource::Item(ItemId::new(1)));
    let proposal = fixture
        .engine
        .viewport()
        .native_create_saga(request.saga())
        .expect("new native create saga must remain active")
        .prepared()
        .proposal();
    assert_eq!(request.binding().surface(), expected_surface);
    assert_eq!(proposal.root(), expected_root);
    assert_eq!(proposal.converted_main().floating(), expected_floating);
    request
}

fn start_native_create_with_payload(
    fixture: &mut Fixture,
    payload: MovePayload,
    expected_root: RootId,
) -> NativeCreateRequest {
    let source = match payload {
        MovePayload::Item(source) => NativeDragSource::Item(source.item()),
        MovePayload::Tabs(source) => NativeDragSource::Group(source.node()),
        MovePayload::Subtree(_) => {
            panic!("native lifecycle fixture requires an item or complete tabs group")
        }
    };
    let request = request_native_create(fixture, source);
    let proposal = fixture
        .engine
        .viewport()
        .native_create_saga(request.saga())
        .expect("new native create saga must remain active")
        .prepared()
        .proposal();
    assert_eq!(request.binding().surface(), SURFACE_NATIVE);
    assert_eq!(proposal.root(), expected_root);
    assert_eq!(proposal.converted_main().floating(), FLOATING_RECOVERY);
    request
}
fn report_create_result(
    fixture: &mut Fixture,
    request: NativeCreateRequest,
    result: EffectDispatchResult,
) -> EngineTransition {
    let input = platform_effect_input(
        &fixture.engine,
        EffectResult::new(request.effect(), request.binding().epoch(), result),
    );
    submit_test_input(fixture, input).expect("effect result must reduce")
}

fn native_window(request: NativeCreateRequest) -> ObservedWindow {
    native_window_with_presentation(request, WindowPresentationState::Visible)
}

fn native_window_with_presentation(
    request: NativeCreateRequest,
    presentation: WindowPresentationState,
) -> ObservedWindow {
    native_window_with_coordinate_generation(
        request,
        presentation,
        CoordinateObservationGeneration::new(1),
    )
}

fn native_window_with_coordinate_generation(
    request: NativeCreateRequest,
    presentation: WindowPresentationState,
    coordinate_generation: CoordinateObservationGeneration,
) -> ObservedWindow {
    observed_window(
        request.binding(),
        coordinate_generation,
        1800.0,
        WindowInputState::ReceivesInput,
    )
    .with_presentation_observation(WindowPresentationObservation::new(
        request.binding(),
        PresentationObservationGeneration::new(0),
        Authority::Known(presentation),
        PresentationEffectAcknowledgement::known(None),
    ))
}

fn recovery_replacement_window(
    binding: ViewportBinding,
    presentation: WindowPresentationState,
) -> ObservedWindow {
    observed_window(
        binding,
        CoordinateObservationGeneration::new(1),
        1_800.0,
        WindowInputState::ReceivesInput,
    )
    .with_presentation_observation(WindowPresentationObservation::new(
        binding,
        PresentationObservationGeneration::new(0),
        Authority::Known(presentation),
        PresentationEffectAcknowledgement::known(None),
    ))
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

fn native_close_effect(
    fixture: &Fixture,
    transition: &EngineTransition,
    binding: ViewportBinding,
    resolution: dockspace::effect::NativeCloseResolution,
) -> EffectId {
    platform_effects(fixture, transition)
        .iter()
        .find_map(|effect| match effect.effect() {
            PlatformEffect::ResolveNativeClose {
                edge: actual,
                resolution: actual_resolution,
                ..
            } if actual.binding() == binding && *actual_resolution == resolution => {
                Some(effect.id())
            }
            _ => None,
        })
        .unwrap_or_else(|| {
            panic!(
                "surface close must emit one matching native resolution effect; effects={:?}",
                platform_effects(fixture, transition)
            )
        })
}

fn effect_of_kind<'a>(
    fixture: &Fixture,
    transition: &'a EngineTransition,
    predicate: impl Fn(&PlatformEffect) -> bool,
) -> &'a PlatformEffectEmission {
    let effects = platform_effects(fixture, transition);
    let matching: Vec<_> = effects
        .iter()
        .filter(|request| predicate(request.effect()))
        .collect();
    assert_eq!(matching.len(), 1, "expected exactly one matching effect");
    matching[0]
}

fn advance_native_create_to_ownership_transfer(
    fixture: &mut Fixture,
    request: NativeCreateRequest,
) -> EngineTransition {
    let hidden = publish_windows!(
        fixture,
        vec![
            source_window(&fixture),
            host_window(&fixture),
            native_window_with_presentation(request, WindowPresentationState::Hidden),
        ],
    );
    assert!(platform_effects(fixture, &hidden).iter().all(|effect| {
        !matches!(effect.effect(), PlatformEffect::ShowWindow { binding, .. } if *binding == request.binding())
    }));
    let pre_show = fixture
        .engine
        .viewport()
        .native_create_saga(request.saga())
        .and_then(|saga| match saga.phase() {
            NativeCreatePhase::AwaitingPreShowPresentation { presentation, .. } => {
                Some(presentation)
            }
            _ => None,
        })
        .expect("hidden proof must request pre-show staging");
    let pre_show_presented = support::present_native_staging(
        &mut fixture.engine,
        &mut fixture.presentation_host,
        pre_show,
    );
    assert!(
        platform_effects(fixture, &pre_show_presented)
            .iter()
            .any(|effect| {
                matches!(
                    effect.effect(),
                    PlatformEffect::ShowWindow {
                        binding,
                        after_pre_show,
                        ..
                    } if *binding == request.binding()
                        && after_pre_show.retained_resource() == pre_show.retained_resource()
                )
            })
    );
    let acknowledged = publish_windows!(
        fixture,
        vec![
            source_window(&fixture),
            host_window(&fixture),
            native_window_with_presentation(request, WindowPresentationState::Hidden),
        ],
    );
    assert!(platform_effects(fixture, &acknowledged).is_empty());
    let visible = publish_windows!(
        fixture,
        vec![
            source_window(&fixture),
            host_window(&fixture),
            native_window_with_presentation(request, WindowPresentationState::Visible),
        ],
    );
    assert!(fixture.engine.workspace().surface(SURFACE_NATIVE).is_none());
    let post_show = fixture
        .engine
        .viewport()
        .native_create_saga(request.saga())
        .and_then(|saga| match saga.phase() {
            NativeCreatePhase::AwaitingPostShowPresentation { presentation, .. } => {
                Some(presentation)
            }
            _ => None,
        })
        .expect("visible proof must request post-show staging");
    let transferred = support::present_native_staging(
        &mut fixture.engine,
        &mut fixture.presentation_host,
        post_show,
    );
    assert!(visible.platform_effects().iter().all(|effect| {
        !matches!(effect.effect(), PlatformEffect::RequestFocus { binding, .. } if *binding == request.binding())
    }));
    transferred
}

fn advance_native_create_to_visible(
    fixture: &mut Fixture,
    request: NativeCreateRequest,
) -> EngineTransition {
    let visible = advance_native_create_to_ownership_transfer(fixture, request);
    if matches!(
        fixture
            .engine
            .viewport()
            .native_create_saga(request.saga())
            .map(|saga| saga.phase()),
        Some(NativeCreatePhase::AwaitingFirstLivePresentation { .. })
    ) {
        publish_scene(fixture);
    }
    visible
}

fn transferred_child_source_awaiting_first_live() -> (
    Fixture,
    NativeCreateRequest,
    dockspace::presentation_observation::NativeStagingResourceId,
) {
    let mut fixture = fixture();
    prepare_child_source_platform(&mut fixture);
    let payload = MovePayload::Tabs(
        fixture
            .engine
            .workspace()
            .capture_node_source(ROOT_SOURCE, fixture.source_tabs)
            .expect("complete source tabs must be current"),
    );
    let request = start_native_create_with_payload(&mut fixture, payload, ROOT_SOURCE);

    advance_native_create_to_ownership_transfer(&mut fixture, request);
    let resource = fixture
        .engine
        .viewport()
        .native_create_saga(request.saga())
        .expect("transferred native create must retain its staging resource")
        .resource();
    publish_surface_for_fixture(
        &mut fixture,
        SURFACE_HOST,
        logical_rect(900.0, 0.0, 900.0, 700.0),
    );
    assert!(retained_native_staging_resource_is_visible(
        &mut fixture,
        resource
    ));

    (fixture, request, resource)
}

fn defer_transferred_native_recovery(
    fixture: &mut Fixture,
    request: NativeCreateRequest,
) -> (EffectId, ViewportBinding) {
    let destroyed = publish_windows_with_close_observations!(
        fixture,
        vec![source_window(fixture), unavailable_host_window(fixture)],
        &[(request.binding(), WindowCloseState::Destroyed, None)],
    );
    assert!(
        fixture
            .engine
            .viewport()
            .native_create_saga(request.saga())
            .is_none(),
        "destroyed target must leave native-create ownership"
    );
    assert!(fixture.engine.workspace().surface(SURFACE_NATIVE).is_some());

    let replacement = effect_of_kind(fixture, &destroyed, |effect| {
        matches!(effect, PlatformEffect::RequestReplacement { .. })
    });
    let replacement_binding = match replacement.effect() {
        PlatformEffect::RequestReplacement { binding, .. } => *binding,
        effect => panic!("expected replacement request, got {effect:?}"),
    };
    let pending = fixture
        .engine
        .viewport()
        .recovery_pending(SURFACE_NATIVE)
        .expect("blocked target recovery must remain queryable");
    assert_eq!(pending.destroyed_binding(), request.binding());
    assert_eq!(pending.replacement_binding(), Some(replacement_binding));
    assert_eq!(
        pending.status(),
        RecoveryPendingStatus::ReplacementRequested {
            replacement: replacement.id()
        }
    );

    (replacement.id(), replacement_binding)
}

fn requested_focus_effect(fixture: &Fixture, binding: ViewportBinding) -> EffectId {
    fixture
        .engine
        .viewport()
        .effects()
        .records()
        .find_map(|(effect, record)| {
            matches!(
                record.request().effect(),
                PlatformEffect::RequestFocus { binding: target, .. } if *target == binding
            )
            .then_some(effect)
        })
        .expect("the admitted native target must retain its exact focus request")
}

fn prepare_runtime_owned_child(fixture: &mut Fixture) -> NativeCreateRequest {
    prepare_base_platform(fixture, ViewportRole::Child);
    let request = start_native_create(fixture);
    advance_native_create_to_visible(fixture, request);
    let record = fixture
        .engine
        .viewport()
        .registry()
        .record(request.binding().surface())
        .expect("committed native child must remain registered");
    assert_eq!(record.binding(), request.binding());
    assert_eq!(record.role(), ViewportRole::Child);
    assert_eq!(record.ownership(), ViewportOwnership::RuntimeOwned);
    request
}

fn observe_ready_and_assert_compensation(fixture: &mut Fixture, request: NativeCreateRequest) {
    let ready = advance_native_create_to_visible(fixture, request);
    let compensation = effect_of_kind(fixture, &ready, |effect| {
        matches!(
            effect,
            PlatformEffect::CompensatingClose {
                binding,
                compensates,
            } if *binding == request.binding() && *compensates == request.effect()
        )
    });
    assert!(
        fixture
            .engine
            .viewport()
            .native_create_saga(request.saga())
            .is_none()
    );
    let retirement = fixture
        .engine
        .viewport()
        .binding_retirements()
        .find_map(|(binding, retirement)| (binding == request.binding()).then_some(retirement))
        .expect("rejected create must retain its exact cleanup obligation");
    assert_eq!(
        retirement.origin(),
        BindingRetirementOrigin::NativeCreateAborted {
            create: request.effect()
        }
    );
    assert_eq!(
        retirement.status(),
        BindingRetirementStatus::CleanupRequested {
            effect: compensation.id()
        }
    );
    assert!(fixture.engine.workspace().surface(SURFACE_NATIVE).is_none());
}

fn assert_old_platform_inputs_are_stale(
    fixture: &mut Fixture,
    request: NativeCreateRequest,
    old_snapshot: PlatformSnapshot,
) {
    let old_epoch = request.binding().epoch();
    let provider = fixture.presentation_host.platform_provider();
    let stale = submit_test_inputs(
        fixture,
        [
            EngineInput::PublishPlatformSnapshot {
                provider,
                expected_epoch: old_epoch,
                snapshot: old_snapshot,
            },
            EngineInput::ReportPlatformEffect {
                provider,
                expected_epoch: old_epoch,
                result: EffectResult::new(
                    request.effect(),
                    old_epoch,
                    EffectDispatchResult::DispatchFailed(DispatchFailureReason::ProviderStopped),
                ),
            },
        ],
    )
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
            ..
        } if *effect == request.effect()
    )));
}

#[test]
fn child_viewport_registration_uses_an_engine_issued_root_recovery_anchor() {
    let mut fixture = fixture();
    let (source, host) = register_base_viewports(&mut fixture, ViewportRole::Child);

    assert_eq!(source.surface(), SURFACE_SOURCE);
    assert_eq!(host.surface(), SURFACE_HOST);
    assert_eq!(
        fixture
            .engine
            .root_recovery_anchor(SURFACE_SOURCE)
            .expect("source root anchor must remain current")
            .surface(),
        SURFACE_SOURCE
    );
}

#[test]
fn create_requires_a_hidden_observation_before_topology_transfer() {
    let mut fixture = fixture();
    prepare_base_platform(&mut fixture, ViewportRole::Child);
    let before = fixture.engine.workspace().clone();
    let request = start_native_create(&mut fixture);
    assert_eq!(fixture.engine.workspace(), &before);
    assert!(matches!(
        fixture
            .engine
            .viewport()
            .native_create_saga(request.saga())
            .expect("create saga must be queryable")
            .phase(),
        NativeCreatePhase::AwaitingHidden { create } if create == request.effect()
    ));

    let visible_without_hidden = publish_windows!(
        &mut fixture,
        vec![
            source_window(&fixture),
            host_window(&fixture),
            native_window(request),
        ],
    );
    assert!(matches!(
        fixture
            .engine
            .viewport()
            .native_create_saga(request.saga())
            .expect("pending saga must remain queryable")
            .phase(),
        NativeCreatePhase::AwaitingHidden { create } if create == request.effect()
    ));
    assert_eq!(fixture.engine.workspace(), &before);
    assert!(
        platform_effects(&fixture, &visible_without_hidden)
            .iter()
            .all(|request| {
                !matches!(
                    request.effect(),
                    PlatformEffect::ShowWindow { .. } | PlatformEffect::RequestFocus { .. }
                )
            })
    );
    assert!(matches!(
        fixture
            .engine
            .viewport()
            .effects()
            .record(request.effect())
            .expect("create effect must remain queryable")
            .phase(),
        EffectPhase::Requested
    ));
}

#[test]
fn hidden_native_create_requests_show_only_after_pre_show_presentation() {
    let mut fixture = fixture();
    prepare_base_platform(&mut fixture, ViewportRole::Child);
    let request = start_native_create(&mut fixture);

    let hidden = publish_windows_with_global_focus!(
        &mut fixture,
        vec![
            source_window(&fixture),
            host_window(&fixture),
            native_window_with_presentation(request, WindowPresentationState::Hidden),
        ],
        GlobalFocusedWindow::None,
        None,
    );
    assert!(platform_effects(&fixture, &hidden).iter().all(|effect| {
        !matches!(
            effect.effect(),
            PlatformEffect::ShowWindow { binding, .. } if *binding == request.binding()
        )
    }));
    assert!(!platform_effects(&fixture, &hidden).iter().any(|effect| {
        matches!(
            effect.effect(),
            PlatformEffect::RequestFocus { binding, .. } if *binding == request.binding()
        )
    }));
    assert!(matches!(
        fixture
            .engine
            .viewport()
            .native_create_saga(request.saga())
            .expect("hidden create saga must remain queryable")
            .phase(),
        NativeCreatePhase::AwaitingPreShowPresentation { .. }
    ));
    let pre_show = fixture
        .engine
        .viewport()
        .native_create_saga(request.saga())
        .and_then(|saga| match saga.phase() {
            NativeCreatePhase::AwaitingPreShowPresentation { presentation, .. } => {
                Some(presentation)
            }
            _ => None,
        })
        .expect("hidden create must expose an exact pre-show staging request");
    let presented = support::present_native_staging(
        &mut fixture.engine,
        &mut fixture.presentation_host,
        pre_show,
    );
    let show = effect_of_kind(&fixture, &presented, |effect| {
        matches!(
            effect,
            PlatformEffect::ShowWindow { binding, .. } if *binding == request.binding()
        )
    });
    assert!(matches!(
        fixture
            .engine
            .viewport()
            .native_create_saga(request.saga())
            .expect("pre-show-presented saga remains queryable")
            .phase(),
        NativeCreatePhase::AwaitingShowAcknowledgement { show: actual, .. } if actual == show.id()
    ));
    assert!(matches!(
        fixture
            .engine
            .viewport()
            .effects()
            .record(show.id())
            .expect("show effect must remain queryable")
            .phase(),
        EffectPhase::Requested
    ));
    assert!(fixture.engine.workspace().surface(SURFACE_NATIVE).is_none());
    assert!(fixture.engine.workspace().root(ROOT_NATIVE).is_none());
}

#[test]
fn show_acknowledgement_and_visible_in_one_generation_do_not_transfer_topology() {
    let mut fixture = fixture();
    prepare_base_platform(&mut fixture, ViewportRole::Child);
    let request = start_native_create(&mut fixture);
    let before = fixture.engine.workspace().clone();

    let _ = publish_windows!(
        &mut fixture,
        vec![
            source_window(&fixture),
            host_window(&fixture),
            native_window_with_coordinate_generation(
                request,
                WindowPresentationState::Hidden,
                CoordinateObservationGeneration::new(1),
            ),
        ],
    );
    let hidden_coordinate_observation_generation = match fixture
        .engine
        .viewport()
        .native_create_saga(request.saga())
        .expect("hidden create saga must remain queryable")
        .phase()
    {
        NativeCreatePhase::AwaitingPreShowPresentation {
            hidden_coordinate_observation_generation,
            ..
        } => hidden_coordinate_observation_generation,
        phase => panic!("unexpected phase after hidden observation: {phase:?}"),
    };
    assert_eq!(
        hidden_coordinate_observation_generation,
        CoordinateObservationGeneration::new(1)
    );
    let pre_show = fixture
        .engine
        .viewport()
        .native_create_saga(request.saga())
        .and_then(|saga| match saga.phase() {
            NativeCreatePhase::AwaitingPreShowPresentation { presentation, .. } => {
                Some(presentation)
            }
            _ => None,
        })
        .expect("hidden observation must request pre-show staging");
    let _ = support::present_native_staging(
        &mut fixture.engine,
        &mut fixture.presentation_host,
        pre_show,
    );

    let acknowledged_visible = publish_windows!(
        &mut fixture,
        vec![
            source_window(&fixture),
            host_window(&fixture),
            native_window_with_coordinate_generation(
                request,
                WindowPresentationState::Visible,
                CoordinateObservationGeneration::new(2),
            ),
        ],
    );
    assert!(platform_effects(&fixture, &acknowledged_visible).is_empty());
    assert_eq!(fixture.engine.workspace(), &before);
    assert!(fixture.engine.workspace().surface(SURFACE_NATIVE).is_none());
    match fixture
        .engine
        .viewport()
        .native_create_saga(request.saga())
        .expect("show-acknowledged saga must remain queryable")
        .phase()
    {
        NativeCreatePhase::AwaitingVisible {
            hidden_coordinate_observation_generation,
            acknowledged_coordinate_observation_generation,
            ..
        } => {
            assert_eq!(
                hidden_coordinate_observation_generation,
                CoordinateObservationGeneration::new(1)
            );
            assert_eq!(
                acknowledged_coordinate_observation_generation,
                CoordinateObservationGeneration::new(2)
            );
        }
        phase => panic!("unexpected phase after show acknowledgement: {phase:?}"),
    }

    let later_visible_windows = vec![
        source_window(&fixture),
        host_window(&fixture),
        native_window_with_coordinate_generation(
            request,
            WindowPresentationState::Visible,
            CoordinateObservationGeneration::new(3),
        ),
    ];
    let later_visible =
        publish_windows_with_baseline_presentation_ack(&mut fixture, later_visible_windows);
    assert!(matches!(
        fixture
            .engine
            .viewport()
            .native_create_saga(request.saga())
            .map(|saga| saga.phase()),
        Some(NativeCreatePhase::AwaitingPostShowPresentation { .. })
    ));
    assert!(fixture.engine.workspace().surface(SURFACE_NATIVE).is_none());
    let post_show = fixture
        .engine
        .viewport()
        .native_create_saga(request.saga())
        .and_then(|saga| match saga.phase() {
            NativeCreatePhase::AwaitingPostShowPresentation { presentation, .. } => {
                Some(presentation)
            }
            _ => None,
        })
        .expect("later visible observation must request post-show staging");
    let _ = support::present_native_staging(
        &mut fixture.engine,
        &mut fixture.presentation_host,
        post_show,
    );
    assert!(matches!(
        fixture
            .engine
            .viewport()
            .native_create_saga(request.saga())
            .map(|saga| saga.phase()),
        Some(NativeCreatePhase::AwaitingFirstLivePresentation { .. })
    ));
    assert!(fixture.engine.workspace().surface(SURFACE_NATIVE).is_some());
    assert_eq!(
        fixture
            .engine
            .viewport()
            .viewport(SURFACE_NATIVE)
            .map(|record| record.admission()),
        Some(dockspace::viewport_registry::ViewportAdmission::Pending)
    );
    assert!(
        platform_effects(&fixture, &later_visible)
            .iter()
            .all(|effect| {
                !matches!(
                    effect.effect(),
                    PlatformEffect::RequestFocus { binding, .. } if *binding == request.binding()
                )
            })
    );
    let InputOutcome::PlatformSnapshotPublished { activations, .. } =
        later_visible.reduced_inputs()[0].outcome()
    else {
        panic!("visible observation must publish its activation outcome");
    };
    assert!(
        activations.is_empty(),
        "an unavailable focus baseline must defer the owner-bound activation instead of consuming it"
    );

    publish_scene(&mut fixture);
    assert!(
        fixture
            .engine
            .viewport()
            .native_create_saga(request.saga())
            .is_none()
    );
    assert_eq!(
        fixture
            .engine
            .viewport()
            .viewport(SURFACE_NATIVE)
            .map(|record| record.admission()),
        Some(dockspace::viewport_registry::ViewportAdmission::Admitted)
    );
}

#[test]
fn native_ownership_transfer_waits_for_the_exact_first_live_target_output() {
    let mut fixture = fixture();
    prepare_base_platform(&mut fixture, ViewportRole::Child);
    let request = start_native_create(&mut fixture);

    let transferred = advance_native_create_to_ownership_transfer(&mut fixture, request);
    assert!(fixture.engine.workspace().surface(SURFACE_NATIVE).is_some());
    assert!(matches!(
        fixture
            .engine
            .viewport()
            .native_create_saga(request.saga())
            .map(|saga| saga.phase()),
        Some(NativeCreatePhase::AwaitingFirstLivePresentation { .. })
    ));
    assert_eq!(
        fixture
            .engine
            .viewport()
            .viewport(SURFACE_NATIVE)
            .map(|record| record.admission()),
        Some(dockspace::viewport_registry::ViewportAdmission::Pending)
    );
    assert!(
        platform_effects(&fixture, &transferred)
            .iter()
            .all(|effect| {
                !matches!(
                    effect.effect(),
                    PlatformEffect::RequestFocus { binding, .. } if *binding == request.binding()
                )
            })
    );
    let first_live_frame = fixture.presentation_host.begin(&fixture.engine);
    assert!(
        first_live_frame
            .surfaces()
            .any(|surface| surface == SURFACE_NATIVE),
        "ordinary native create must restore its live slot before awaiting first-live output",
    );
    assert!(
        first_live_frame
            .view()
            .begin_surface_contribution(SURFACE_NATIVE)
            .is_ok(),
        "first-live surface measurement must not be suppressed by recovery bring-up",
    );
    drop(first_live_frame);

    publish_surface_for_fixture(
        &mut fixture,
        SURFACE_SOURCE,
        logical_rect(0.0, 0.0, 900.0, 700.0),
    );
    assert!(matches!(
        fixture
            .engine
            .viewport()
            .native_create_saga(request.saga())
            .map(|saga| saga.phase()),
        Some(NativeCreatePhase::AwaitingFirstLivePresentation { .. })
    ));

    publish_surface_for_fixture(
        &mut fixture,
        SURFACE_NATIVE,
        logical_rect(1_800.0, 0.0, 900.0, 700.0),
    );
    assert!(
        fixture
            .engine
            .viewport()
            .native_create_saga(request.saga())
            .is_none()
    );
    assert_eq!(
        fixture
            .engine
            .viewport()
            .viewport(SURFACE_NATIVE)
            .map(|record| record.admission()),
        Some(dockspace::viewport_registry::ViewportAdmission::Admitted)
    );
}

#[test]
fn native_source_surface_retirement_waits_for_first_live_output() {
    let mut fixture = fixture();
    prepare_child_source_platform(&mut fixture);
    let source_binding = known_source_binding(&fixture);
    let payload = MovePayload::Tabs(
        fixture
            .engine
            .workspace()
            .capture_node_source(ROOT_SOURCE, fixture.source_tabs)
            .expect("complete source tabs must be current"),
    );
    let request = start_native_create_with_payload(&mut fixture, payload, ROOT_SOURCE);

    advance_native_create_to_ownership_transfer(&mut fixture, request);
    assert!(fixture.engine.workspace().surface(SURFACE_SOURCE).is_none());
    assert_eq!(
        fixture
            .engine
            .viewport()
            .viewport(SURFACE_SOURCE)
            .map(|record| record.binding()),
        Some(source_binding),
        "the source binding must remain live until the target presents"
    );
    assert_eq!(fixture.engine.viewport_focus_binding(SURFACE_SOURCE), None);
    let focused_source = publish_windows_with_global_focus!(
        &mut fixture,
        vec![
            source_window(&fixture),
            host_window(&fixture),
            native_window_with_presentation(request, WindowPresentationState::Visible),
        ],
        GlobalFocusedWindow::Dock(source_binding),
        None,
    );
    assert!(
        platform_effects(&fixture, &focused_source)
            .iter()
            .all(|effect| { !matches!(effect.effect(), PlatformEffect::RequestFocus { .. }) })
    );
    assert!(
        fixture
            .engine
            .viewport()
            .native_create_saga(request.saga())
            .is_some()
    );

    publish_scene(&mut fixture);
    assert!(
        fixture
            .engine
            .viewport()
            .native_create_saga(request.saga())
            .is_none()
    );
    assert!(fixture.engine.viewport().viewport(SURFACE_SOURCE).is_none());
    assert_eq!(
        fixture
            .engine
            .viewport()
            .viewport(SURFACE_NATIVE)
            .map(|record| record.admission()),
        Some(dockspace::viewport_registry::ViewportAdmission::Admitted)
    );
}

#[test]
fn transferred_native_close_uses_surface_protocol_before_first_live() {
    let mut fixture = fixture();
    prepare_base_platform(&mut fixture, ViewportRole::Child);
    let request = start_native_create(&mut fixture);
    advance_native_create_to_ownership_transfer(&mut fixture, request);
    let before = fixture.engine.workspace().clone();

    let expected = fixture.engine.version();
    let cancellation_error = submit_test_input_expect_error(
        &mut fixture,
        EngineInput::CancelNativeCreate {
            expected,
            saga: request.saga(),
        },
    );
    assert!(matches!(
        cancellation_error,
        EngineError::Viewport {
            source: dockspace::frame::ViewportCoordinatorError::CreateSagaAlreadyTransferred {
                saga,
            },
            ..
        } if saga == request.saga()
    ));
    assert_eq!(fixture.engine.workspace(), &before);

    let close_observed = publish_windows_with_close_observations!(
        &mut fixture,
        vec![
            source_window(&fixture),
            host_window(&fixture),
            native_window_with_presentation(request, WindowPresentationState::Visible),
        ],
        &[(request.binding(), WindowCloseState::LiveRequested, None)],
    );
    let edge = native_close_edge_from(&close_observed);

    let expected = fixture.engine.version();
    let requested = submit_test_input(
        &mut fixture,
        EngineInput::RequestSurfaceClose {
            expected,
            edge,
            request: SurfaceCloseRequest::RetainLayout,
        },
    )
    .expect("transferred target close must enter the ordinary surface protocol");
    assert!(matches!(
        requested.reduced_inputs(),
        [input]
            if matches!(
                input.outcome(),
                InputOutcome::SurfaceCloseRequested { edge: actual, .. } if *actual == edge
            )
    ));

    let expected = fixture.engine.version();
    let cancelled = submit_test_input(
        &mut fixture,
        EngineInput::CancelSurfaceClose { expected, edge },
    )
    .expect("transferred target close must support the ordinary cancellation lane");
    assert!(matches!(
        cancelled.reduced_inputs(),
        [input]
            if matches!(
                input.outcome(),
                InputOutcome::SurfaceCloseCancellationRequested { edge: actual, .. }
                    if *actual == edge
            )
    ));
    let cancellation_effect = native_close_effect(
        &fixture,
        &cancelled,
        request.binding(),
        dockspace::effect::NativeCloseResolution::Cancel,
    );

    publish_surface_for_fixture(
        &mut fixture,
        SURFACE_NATIVE,
        logical_rect(1_800.0, 0.0, 900.0, 700.0),
    );
    assert!(matches!(
        fixture
            .engine
            .viewport()
            .native_create_saga(request.saga())
            .map(|saga| saga.phase()),
        Some(NativeCreatePhase::AwaitingFirstLivePresentation { .. })
    ));

    publish_windows_with_close_observations!(
        &mut fixture,
        vec![
            source_window(&fixture),
            host_window(&fixture),
            native_window_with_presentation(request, WindowPresentationState::Visible),
        ],
        &[(
            request.binding(),
            WindowCloseState::LiveClear,
            Some(cancellation_effect),
        )],
    );
    publish_surface_for_fixture(
        &mut fixture,
        SURFACE_NATIVE,
        logical_rect(1_800.0, 0.0, 900.0, 700.0),
    );
    assert!(
        fixture
            .engine
            .viewport()
            .native_create_saga(request.saga())
            .is_none()
    );
}

#[test]
fn destroyed_transferred_target_recovers_before_first_live_and_retires_source() {
    let mut fixture = fixture();
    prepare_child_source_platform(&mut fixture);
    let payload = MovePayload::Tabs(
        fixture
            .engine
            .workspace()
            .capture_node_source(ROOT_SOURCE, fixture.source_tabs)
            .expect("complete source tabs must be current"),
    );
    let request = start_native_create_with_payload(&mut fixture, payload, ROOT_SOURCE);

    advance_native_create_to_ownership_transfer(&mut fixture, request);
    let resource = fixture
        .engine
        .viewport()
        .native_create_saga(request.saga())
        .expect("transferred native create must retain its staging resource")
        .resource();
    assert!(retained_native_staging_resource_is_visible(
        &mut fixture,
        resource
    ));
    publish_surface_for_fixture(
        &mut fixture,
        SURFACE_HOST,
        logical_rect(900.0, 0.0, 900.0, 700.0),
    );
    assert!(matches!(
        fixture
            .engine
            .viewport()
            .native_create_saga(request.saga())
            .map(|saga| saga.phase()),
        Some(NativeCreatePhase::AwaitingFirstLivePresentation { .. })
    ));
    let destroyed = publish_windows_with_close_observations!(
        &mut fixture,
        vec![source_window(&fixture), host_window(&fixture)],
        &[(request.binding(), WindowCloseState::Destroyed, None)],
    );

    assert!(
        fixture
            .engine
            .viewport()
            .native_create_saga(request.saga())
            .is_none()
    );
    assert!(
        fixture.engine.workspace().surface(SURFACE_NATIVE).is_none(),
        "pre-first-live destruction must recover the target roster; blocked={:?}, reduced={:?}",
        fixture.engine.blocked_surface_recovery(SURFACE_NATIVE),
        destroyed.reduced_inputs()
    );
    assert!(fixture.engine.viewport().viewport(SURFACE_SOURCE).is_none());
    assert_eq!(
        fixture
            .engine
            .workspace()
            .presentation_for_root(ROOT_SOURCE),
        Some(RootPresentationOwner::Contained {
            surface: SURFACE_HOST,
            floating: FLOATING_RECOVERY,
        })
    );
    assert!(
        !retained_native_staging_resource_is_visible(&mut fixture, resource),
        "recovery terminal must release the transferred staging resource"
    );
}

#[test]
fn delayed_retry_recovery_releases_transferred_native_staging_resource() {
    let (mut fixture, request, resource) = transferred_child_source_awaiting_first_live();
    let (replacement_effect, _) = defer_transferred_native_recovery(&mut fixture, request);

    let expected_epoch = fixture.engine.version().epoch();
    let failure = platform_effect_input(
        &fixture.engine,
        EffectResult::new(
            replacement_effect,
            expected_epoch,
            EffectDispatchResult::DispatchFailed(DispatchFailureReason::AdapterRejected),
        ),
    );
    let failed =
        submit_test_input(&mut fixture, failure).expect("replacement dispatch failure must reduce");
    assert!(platform_effects(&fixture, &failed).is_empty());
    assert_eq!(
        fixture
            .engine
            .viewport()
            .recovery_pending(SURFACE_NATIVE)
            .expect("failed replacement must retain delayed recovery")
            .status(),
        RecoveryPendingStatus::ReplacementFailed {
            replacement: replacement_effect,
            failed: replacement_effect,
        }
    );
    assert!(retained_native_staging_resource_is_visible(
        &mut fixture,
        resource
    ));

    publish_windows!(
        &mut fixture,
        vec![
            source_window(&fixture),
            host_window_at_coordinate_generation(&fixture, CoordinateObservationGeneration::new(3),),
        ],
    );
    publish_surface_for_fixture(
        &mut fixture,
        SURFACE_HOST,
        logical_rect(900.0, 0.0, 900.0, 700.0),
    );
    assert!(retained_native_staging_resource_is_visible(
        &mut fixture,
        resource
    ));
    publish_windows!(
        &mut fixture,
        vec![
            source_window(&fixture),
            host_window_at_coordinate_generation(&fixture, CoordinateObservationGeneration::new(4),),
        ],
    );

    assert!(fixture.engine.workspace().surface(SURFACE_NATIVE).is_none());
    assert!(
        fixture
            .engine
            .viewport()
            .recovery_pending(SURFACE_NATIVE)
            .is_none()
    );
    assert_eq!(
        fixture
            .engine
            .workspace()
            .presentation_for_root(ROOT_SOURCE),
        Some(RootPresentationOwner::Contained {
            surface: SURFACE_HOST,
            floating: FLOATING_RECOVERY,
        })
    );
    assert!(
        !retained_native_staging_resource_is_visible(&mut fixture, resource),
        "successful delayed RetryRecovery must release the transferred staging resource"
    );
}

#[test]
fn visible_recovery_replacement_retains_resource_until_exact_first_live_presentation() {
    let (mut fixture, request, resource) = transferred_child_source_awaiting_first_live();
    let (_, replacement_binding) = defer_transferred_native_recovery(&mut fixture, request);

    let hidden = publish_windows!(
        &mut fixture,
        vec![
            source_window(&fixture),
            unavailable_host_window(&fixture),
            recovery_replacement_window(replacement_binding, WindowPresentationState::Hidden,),
        ],
    );
    assert!(platform_effects(&fixture, &hidden).is_empty());
    let pre_show = support::present_requested_native_staging(
        &mut fixture.engine,
        &mut fixture.presentation_host,
        replacement_binding,
        NativeStagingPresentationPhase::PreShow,
    );
    assert!(
        platform_effects(&fixture, &pre_show)
            .iter()
            .any(|emission| {
                matches!(
                    emission.effect(),
                    PlatformEffect::ShowWindow { binding, .. } if *binding == replacement_binding
                )
            })
    );
    let acknowledged = publish_windows!(
        &mut fixture,
        vec![
            source_window(&fixture),
            unavailable_host_window(&fixture),
            recovery_replacement_window(replacement_binding, WindowPresentationState::Hidden,),
        ],
    );
    assert!(platform_effects(&fixture, &acknowledged).is_empty());
    let visible = publish_windows!(
        &mut fixture,
        vec![
            source_window(&fixture),
            unavailable_host_window(&fixture),
            recovery_replacement_window(replacement_binding, WindowPresentationState::Visible,),
        ],
    );
    assert!(platform_effects(&fixture, &visible).is_empty());
    let post_show = support::present_requested_native_staging(
        &mut fixture.engine,
        &mut fixture.presentation_host,
        replacement_binding,
        NativeStagingPresentationPhase::PostShow,
    );
    assert!(platform_effects(&fixture, &post_show).is_empty());
    let pending = fixture
        .engine
        .viewport()
        .recovery_pending(SURFACE_NATIVE)
        .expect("visible replacement must await its exact first live presentation");
    assert_eq!(pending.replacement_binding(), Some(replacement_binding));
    assert_eq!(
        pending.status(),
        RecoveryPendingStatus::AwaitingFirstLivePresentation
    );
    assert_eq!(
        fixture
            .engine
            .viewport()
            .viewport(SURFACE_NATIVE)
            .map(|record| (record.binding(), record.admission())),
        Some((replacement_binding, ViewportAdmission::Pending))
    );
    assert!(retained_native_staging_resource_is_visible(
        &mut fixture,
        resource
    ));

    publish_windows!(
        &mut fixture,
        vec![
            source_window(&fixture),
            host_window_at_coordinate_generation(&fixture, CoordinateObservationGeneration::new(3),),
            observed_window(
                replacement_binding,
                CoordinateObservationGeneration::new(1),
                1_800.0,
                WindowInputState::ReceivesInput,
            ),
        ],
    );
    publish_surface_for_fixture(
        &mut fixture,
        SURFACE_HOST,
        logical_rect(900.0, 0.0, 900.0, 700.0),
    );
    assert!(
        retained_native_staging_resource_is_visible(&mut fixture, resource),
        "an unrelated live surface must not settle the replacement barrier"
    );

    publish_surface_for_fixture(
        &mut fixture,
        SURFACE_NATIVE,
        logical_rect(1_800.0, 0.0, 900.0, 700.0),
    );
    assert!(
        fixture
            .engine
            .viewport()
            .recovery_pending(SURFACE_NATIVE)
            .is_none()
    );
    assert_eq!(
        fixture
            .engine
            .viewport()
            .viewport(SURFACE_NATIVE)
            .map(|record| (record.binding(), record.admission())),
        Some((replacement_binding, ViewportAdmission::Admitted))
    );
    assert!(
        !retained_native_staging_resource_is_visible(&mut fixture, resource),
        "only the replacement's exact first live output may release the resource"
    );
}

#[test]
fn hidden_native_create_waits_for_visible_and_focused_observations() {
    let mut fixture = fixture();
    prepare_base_platform(&mut fixture, ViewportRole::Child);
    let request = start_native_create(&mut fixture);
    let _ = publish_windows_with_global_focus!(
        &mut fixture,
        vec![
            source_window(&fixture),
            host_window(&fixture),
            native_window_with_presentation(request, WindowPresentationState::Hidden),
        ],
        GlobalFocusedWindow::None,
        None,
    );

    let pre_show = fixture
        .engine
        .viewport()
        .native_create_saga(request.saga())
        .and_then(|saga| match saga.phase() {
            NativeCreatePhase::AwaitingPreShowPresentation { presentation, .. } => {
                Some(presentation)
            }
            _ => None,
        })
        .expect("hidden observation must request pre-show staging");
    let _ = support::present_native_staging(
        &mut fixture.engine,
        &mut fixture.presentation_host,
        pre_show,
    );

    let acknowledged = publish_windows_with_global_focus!(
        &mut fixture,
        vec![
            source_window(&fixture),
            host_window(&fixture),
            native_window_with_presentation(request, WindowPresentationState::Hidden),
        ],
        GlobalFocusedWindow::None,
        None,
    );
    assert!(platform_effects(&fixture, &acknowledged).is_empty());
    assert!(matches!(
        fixture
            .engine
            .viewport()
            .native_create_saga(request.saga())
            .expect("show-acknowledged saga must remain queryable")
            .phase(),
        NativeCreatePhase::AwaitingVisible { .. }
    ));
    assert!(fixture.engine.workspace().surface(SURFACE_NATIVE).is_none());

    let visible = publish_windows_with_global_focus!(
        &mut fixture,
        vec![
            source_window(&fixture),
            host_window(&fixture),
            native_window_with_presentation(request, WindowPresentationState::Visible),
        ],
        GlobalFocusedWindow::None,
        None,
    );
    assert!(platform_effects(&fixture, &visible).iter().all(|effect| {
        !matches!(
            effect.effect(),
            PlatformEffect::RequestFocus { binding, .. } if *binding == request.binding()
        )
    }));
    assert!(matches!(
        fixture
            .engine
            .viewport()
            .native_create_saga(request.saga())
            .map(|saga| saga.phase()),
        Some(NativeCreatePhase::AwaitingPostShowPresentation { .. })
    ));
    assert!(fixture.engine.workspace().surface(SURFACE_NATIVE).is_none());
    let post_show = fixture
        .engine
        .viewport()
        .native_create_saga(request.saga())
        .and_then(|saga| match saga.phase() {
            NativeCreatePhase::AwaitingPostShowPresentation { presentation, .. } => {
                Some(presentation)
            }
            _ => None,
        })
        .expect("visible observation must request post-show staging");
    let _ = support::present_native_staging(
        &mut fixture.engine,
        &mut fixture.presentation_host,
        post_show,
    );
    assert!(matches!(
        fixture
            .engine
            .viewport()
            .native_create_saga(request.saga())
            .map(|saga| saga.phase()),
        Some(NativeCreatePhase::AwaitingFirstLivePresentation { .. })
    ));
    publish_scene(&mut fixture);
    let focus = requested_focus_effect(&fixture, request.binding());
    assert!(
        fixture
            .engine
            .viewport()
            .native_create_saga(request.saga())
            .is_none()
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
    assert!(matches!(
        fixture
            .engine
            .viewport()
            .effects()
            .record(focus)
            .expect("focus effect must remain queryable")
            .phase(),
        EffectPhase::Requested
    ));

    let focused = publish_windows_with_global_focus!(
        &mut fixture,
        vec![
            source_window(&fixture),
            host_window(&fixture),
            native_window_with_presentation(request, WindowPresentationState::Visible),
        ],
        GlobalFocusedWindow::Dock(request.binding()),
        Some(focus),
    );
    assert!(platform_effects(&fixture, &focused).is_empty());
    assert!(matches!(
        fixture
            .engine
            .viewport()
            .effects()
            .record(focus)
            .expect("focus effect must remain queryable")
            .phase(),
        EffectPhase::ObservedApplied { .. }
    ));
}

#[test]
fn focus_generation_gap_does_not_orphan_native_create_activation() {
    let mut fixture = fixture();
    prepare_base_platform(&mut fixture, ViewportRole::Child);
    let request = start_native_create(&mut fixture);
    let previous_focus_generation = fixture.input_generation;

    fixture.input_generation += 1;
    let gap_windows = vec![
        source_window(&fixture),
        host_window(&fixture),
        native_window_with_presentation(request, WindowPresentationState::Hidden),
    ];
    let gap = publish_windows_with_focus_observation(
        &mut fixture,
        gap_windows,
        FocusObservationEnvelope::new(
            FocusObservationGeneration::new(previous_focus_generation + 2),
            Authority::Known(GlobalFocusedWindow::None),
            Authority::Known(None),
        ),
    );
    assert!(platform_effects(&fixture, &gap).iter().all(|effect| {
        !matches!(
            effect.effect(),
            PlatformEffect::ShowWindow { binding, .. } if *binding == request.binding()
        )
    }));
    let pre_show = fixture
        .engine
        .viewport()
        .native_create_saga(request.saga())
        .and_then(|saga| match saga.phase() {
            NativeCreatePhase::AwaitingPreShowPresentation { presentation, .. } => {
                Some(presentation)
            }
            _ => None,
        })
        .expect("hidden gap observation must request pre-show staging");
    let pre_show_presented = support::present_native_staging(
        &mut fixture.engine,
        &mut fixture.presentation_host,
        pre_show,
    );
    assert!(
        platform_effects(&fixture, &pre_show_presented)
            .iter()
            .any(|effect| {
                matches!(
                    effect.effect(),
                    PlatformEffect::ShowWindow { binding, .. } if *binding == request.binding()
                )
            })
    );

    fixture.input_generation += 1;
    let tombstone_windows = vec![
        source_window(&fixture),
        host_window(&fixture),
        native_window_with_presentation(request, WindowPresentationState::Hidden),
    ];
    let _ = publish_windows_with_focus_observation(
        &mut fixture,
        tombstone_windows,
        unknown_focus_observation(
            FocusObservationGeneration::new(previous_focus_generation + 3),
            AuthorityUnavailableReason::NotReported,
        ),
    );

    fixture.input_generation += 1;
    let baseline_windows = vec![
        source_window(&fixture),
        host_window(&fixture),
        native_window_with_presentation(request, WindowPresentationState::Hidden),
    ];
    let _ = publish_windows_with_focus_observation(
        &mut fixture,
        baseline_windows,
        FocusObservationEnvelope::new(
            FocusObservationGeneration::new(previous_focus_generation + 4),
            Authority::Known(GlobalFocusedWindow::None),
            Authority::Known(None),
        ),
    );
    assert!(fixture.engine.workspace().surface(SURFACE_NATIVE).is_none());
    assert!(
        fixture
            .engine
            .viewport()
            .native_create_saga(request.saga())
            .is_some()
    );

    fixture.input_generation += 1;
    let visible_windows = vec![
        source_window(&fixture),
        host_window(&fixture),
        native_window_with_presentation(request, WindowPresentationState::Visible),
    ];
    let visible = publish_windows_with_focus_observation(
        &mut fixture,
        visible_windows,
        FocusObservationEnvelope::new(
            FocusObservationGeneration::new(previous_focus_generation + 5),
            Authority::Known(GlobalFocusedWindow::None),
            Authority::Known(None),
        ),
    );
    assert!(platform_effects(&fixture, &visible).iter().all(|effect| {
        !matches!(
            effect.effect(),
            PlatformEffect::RequestFocus { binding, .. } if *binding == request.binding()
        )
    }));
    assert!(matches!(
        fixture
            .engine
            .viewport()
            .native_create_saga(request.saga())
            .map(|saga| saga.phase()),
        Some(NativeCreatePhase::AwaitingPostShowPresentation { .. })
    ));
    let post_show = fixture
        .engine
        .viewport()
        .native_create_saga(request.saga())
        .and_then(|saga| match saga.phase() {
            NativeCreatePhase::AwaitingPostShowPresentation { presentation, .. } => {
                Some(presentation)
            }
            _ => None,
        })
        .expect("visible focus observation must request post-show staging");
    let _ = support::present_native_staging(
        &mut fixture.engine,
        &mut fixture.presentation_host,
        post_show,
    );
    assert!(matches!(
        fixture
            .engine
            .viewport()
            .native_create_saga(request.saga())
            .map(|saga| saga.phase()),
        Some(NativeCreatePhase::AwaitingFirstLivePresentation { .. })
    ));
    publish_scene(&mut fixture);
    let _ = requested_focus_effect(&fixture, request.binding());
    assert!(fixture.engine.workspace().surface(SURFACE_NATIVE).is_some());
    assert!(
        fixture
            .engine
            .viewport()
            .native_create_saga(request.saga())
            .is_none()
    );
}

#[test]
fn joined_provider_replacement_aborts_native_create_before_ownership_transfer() {
    let mut fixture = joined_fixture();
    prepare_base_platform(&mut fixture, ViewportRole::Child);
    let request = start_native_create(&mut fixture);
    let predecessor = fixture.presentation_host.take_backend_ingress();
    let mut drained = predecessor.drain();

    let _replacement = fixture
        .engine
        .begin_backend_ingress_provider_replacement(&mut drained)
        .expect("joined provider replacement must abort pre-transfer lifecycle work");

    assert!(drained.is_consumed());
    assert!(
        fixture
            .engine
            .viewport()
            .native_create_saga(request.saga())
            .is_none()
    );
    assert!(fixture.engine.workspace().surface(SURFACE_NATIVE).is_none());
}

#[test]
fn joined_provider_replacement_preserves_transferred_native_create_until_first_live() {
    let mut fixture = joined_fixture();
    prepare_base_platform(&mut fixture, ViewportRole::Child);
    let request = start_native_create(&mut fixture);
    advance_native_create_to_ownership_transfer(&mut fixture, request);
    let predecessor = fixture.presentation_host.take_backend_ingress();
    let mut drained = predecessor.drain();

    let replacement = fixture
        .engine
        .begin_backend_ingress_provider_replacement(&mut drained)
        .expect("joined provider replacement must preserve transferred native ownership");
    assert!(drained.is_consumed());
    assert!(matches!(
        fixture
            .engine
            .viewport()
            .native_create_saga(request.saga())
            .map(|saga| saga.phase()),
        Some(NativeCreatePhase::AwaitingFirstLivePresentation { .. })
    ));
    assert!(fixture.engine.workspace().surface(SURFACE_NATIVE).is_some());
    assert_eq!(
        fixture
            .engine
            .viewport()
            .viewport(SURFACE_NATIVE)
            .map(|record| record.admission()),
        Some(ViewportAdmission::Pending)
    );

    let (mut ticket, _) = replacement.into_parts();
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
    fixture.input_generation = 0;
    fixture.close_generations.clear();

    publish_windows!(
        &mut fixture,
        vec![
            source_window(&fixture),
            host_window(&fixture),
            native_window_with_presentation(request, WindowPresentationState::Visible),
        ],
    );
    assert!(matches!(
        fixture
            .engine
            .viewport()
            .native_create_saga(request.saga())
            .map(|saga| saga.phase()),
        Some(NativeCreatePhase::AwaitingFirstLivePresentation { .. })
    ));

    publish_surface_for_fixture(
        &mut fixture,
        SURFACE_NATIVE,
        logical_rect(1_800.0, 0.0, 900.0, 700.0),
    );
    assert!(
        fixture
            .engine
            .viewport()
            .native_create_saga(request.saga())
            .is_none()
    );
    assert_eq!(
        fixture
            .engine
            .viewport()
            .viewport(SURFACE_NATIVE)
            .map(|record| record.admission()),
        Some(ViewportAdmission::Admitted)
    );
}

#[test]
fn unrelated_viewport_fact_loss_does_not_cancel_a_source_owned_drag() {
    let mut fixture = fixture();
    prepare_base_platform(&mut fixture, ViewportRole::Child);
    let drag = arm_journal_drag(&mut fixture, NativeDragSource::Item(ItemId::new(1)));
    let moved = move_journal_drag_outside(&mut fixture, drag);
    assert!(matches!(
        moved.reduced_pointer_edges()[0].interaction_outcomes(),
        [
            InteractionOutcome::DragBegan { .. },
            InteractionOutcome::PreviewUpdated {
                status: PreviewResolutionStatus::Resolved,
                preview: Some(_),
                ..
            }
        ]
    ));
    let preview = fixture
        .engine
        .interaction()
        .preview()
        .expect("source-owned preview must exist")
        .token();

    publish_windows!(
        &mut fixture,
        vec![source_window(&fixture), unavailable_host_window(&fixture)],
    );

    assert_eq!(
        fixture.engine.interaction().status(),
        InteractionStatus::Dragging {
            session: drag.session
        }
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
fn target_surface_destruction_clears_preview_without_canceling_source_drag() {
    let mut fixture = fixture();
    prepare_base_platform(&mut fixture, ViewportRole::Child);
    let drag = arm_journal_drag(&mut fixture, NativeDragSource::Item(ItemId::new(1)));
    let moved = move_journal_drag_to_surface(&mut fixture, drag, SURFACE_HOST, 900.0, 1.0);
    assert!(
        moved.reduced_pointer_edges()[0]
            .interaction_outcomes()
            .iter()
            .any(|outcome| matches!(
                outcome,
                InteractionOutcome::PreviewUpdated {
                    status: PreviewResolutionStatus::Resolved,
                    preview: Some(_),
                    ..
                }
            ))
    );
    assert!(fixture.engine.interaction().preview().is_some());

    publish_windows!(&mut fixture, vec![source_window(&fixture)]);

    assert_eq!(
        fixture.engine.interaction().status(),
        InteractionStatus::Dragging {
            session: drag.session
        },
        "destroying only the target surface must preserve the source-owned drag"
    );
    assert_eq!(
        fixture.engine.interaction().preview(),
        None,
        "destroying the target surface must clear its preview"
    );
}

#[test]
fn target_coordinate_change_stales_only_target_and_recovers_after_republish() {
    let mut fixture = fixture();
    prepare_base_platform(&mut fixture, ViewportRole::Child);
    let drag = arm_journal_drag(&mut fixture, NativeDragSource::Item(ItemId::new(1)));
    move_journal_drag_to_surface(&mut fixture, drag, SURFACE_HOST, 900.0, 1.0);
    assert!(fixture.engine.interaction().preview().is_some());
    let offer = fixture
        .engine
        .interaction()
        .active_drag_view()
        .and_then(|drag| drag.contained_offer().copied())
        .expect("journal drag must freeze one contained presentation reservation");

    let target_changed = publish_windows!(
        &mut fixture,
        vec![source_window(&fixture), resized_host_window(&fixture)],
    );

    assert_eq!(
        fixture.engine.interaction().status(),
        InteractionStatus::Dragging {
            session: drag.session
        },
        "losing only target geometry authority must preserve the source-owned drag"
    );
    let active_drag = fixture
        .engine
        .interaction()
        .active_drag_view()
        .expect("target invalidation must preserve the drag");
    assert!(active_drag.target().is_none());
    assert_eq!(active_drag.contained_offer(), Some(&offer));
    assert!(fixture.engine.interaction().preview().is_none());
    assert!(fixture.engine.interaction().drop_affordance().is_none());
    assert!(
        !platform_effects(&fixture, &target_changed)
            .iter()
            .any(|request| matches!(
                request.effect(),
                PlatformEffect::SetPointerPassthrough { enabled: false, .. }
            ))
    );
    assert!(matches!(
        fixture.engine.scene().surface(SURFACE_SOURCE),
        Some(SurfaceScene::Ready(_))
    ));
    assert!(matches!(
        fixture.engine.scene().surface(SURFACE_HOST),
        Some(SurfaceScene::Stale(_))
    ));

    publish_scene(&mut fixture);
    assert!(matches!(
        fixture.engine.scene().surface(SURFACE_HOST),
        Some(SurfaceScene::Ready(_))
    ));
    let recovered = move_journal_drag_to_surface(&mut fixture, drag, SURFACE_HOST, 900.0, 1.25);
    assert!(
        recovered.reduced_pointer_edges()[0]
            .interaction_outcomes()
            .iter()
            .any(|outcome| matches!(
                outcome,
                InteractionOutcome::PreviewUpdated {
                    status: PreviewResolutionStatus::Resolved,
                    preview: Some(_),
                    ..
                }
            ))
    );
    assert!(fixture.engine.interaction().preview().is_some());
}

#[test]
fn source_coordinate_change_preserves_drag_and_other_surface_scene() {
    let mut fixture = fixture();
    prepare_base_platform(&mut fixture, ViewportRole::Child);
    let drag = arm_journal_drag(&mut fixture, NativeDragSource::Item(ItemId::new(1)));
    move_journal_drag_to_surface(&mut fixture, drag, SURFACE_HOST, 900.0, 1.0);
    assert!(fixture.engine.interaction().preview().is_some());
    let before = fixture.engine.workspace().clone();

    let transition = publish_windows!(
        &mut fixture,
        vec![resized_source_window(&fixture), host_window(&fixture)],
    );

    assert_eq!(
        fixture.engine.interaction().status(),
        InteractionStatus::Dragging {
            session: drag.session
        }
    );
    assert!(
        !transition.interaction_events().iter().any(|event| matches!(
            event.kind(),
            dockspace::interaction::InteractionEventKind::Cancelled {
                status: InteractionStatus::Dragging { session: cancelled },
                ..
            } if *cancelled == drag.session
        ))
    );
    assert!(matches!(
        fixture.engine.scene().surface(SURFACE_SOURCE),
        Some(SurfaceScene::Stale(_))
    ));
    assert!(matches!(
        fixture.engine.scene().surface(SURFACE_HOST),
        Some(SurfaceScene::Ready(_))
    ));
    let active_drag = fixture
        .engine
        .interaction()
        .active_drag_view()
        .expect("source coordinate invalidation must retain the drag");
    assert!(active_drag.target().is_none());
    assert!(fixture.engine.interaction().preview().is_none());
    assert!(fixture.engine.interaction().drop_affordance().is_none());

    let blocked = move_journal_drag_to_surface(&mut fixture, drag, SURFACE_HOST, 900.0, 1.0);
    assert!(matches!(
        blocked.reduced_pointer_edges()[0].interaction_outcomes(),
        [InteractionOutcome::Rejected(
            InteractionRejection::StaleScene
        )]
    ));
    assert_eq!(
        fixture.engine.interaction().status(),
        InteractionStatus::Dragging {
            session: drag.session
        }
    );
    assert!(
        fixture
            .engine
            .interaction()
            .active_drag_view()
            .is_some_and(|drag| drag.target().is_none())
    );
    assert!(fixture.engine.interaction().preview().is_none());
    assert_eq!(fixture.engine.workspace(), &before);

    publish_scene(&mut fixture);
    move_journal_drag_to_surface(&mut fixture, drag, SURFACE_HOST, 900.0, 1.0);
    assert!(fixture.engine.interaction().preview().is_some());
}

#[test]
fn source_surface_destruction_cancels_the_global_drag_session() {
    let mut fixture = fixture();
    prepare_base_platform(&mut fixture, ViewportRole::Child);
    let drag = arm_journal_drag(&mut fixture, NativeDragSource::Item(ItemId::new(1)));
    move_journal_drag_outside(&mut fixture, drag);

    let source_binding = viewport_binding(&fixture, SURFACE_SOURCE);
    let destroyed = publish_windows_with_close_observations!(
        &mut fixture,
        vec![host_window(&fixture)],
        &[(source_binding, WindowCloseState::Destroyed, None)],
    );

    assert_eq!(
        fixture.engine.interaction().status(),
        InteractionStatus::Idle
    );
    assert!(destroyed.interaction_events().iter().any(|event| matches!(
        event.kind(),
        dockspace::interaction::InteractionEventKind::Cancelled {
            status: InteractionStatus::Dragging { session: cancelled },
            reason: dockspace::interaction::InteractionCancelReason::SurfaceClosed,
        } if *cancelled == drag.session
    )));
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
    let input = command_input(
        &fixture.engine,
        WorkspaceCommand::Select {
            source: changing_source,
        },
    );
    submit_test_input(&mut fixture, input).expect("source mutation must commit");

    let ready = advance_native_create_to_visible(&mut fixture, request);
    let compensation = effect_of_kind(&fixture, &ready, |effect| {
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
    assert!(
        fixture
            .engine
            .viewport()
            .native_create_saga(request.saga())
            .is_none(),
        "a terminal create must not remain in the active saga map"
    );
    let retirement = binding_retirement(&fixture, request.binding());
    assert_eq!(
        retirement.origin(),
        BindingRetirementOrigin::NativeCreateAborted {
            create: request.effect(),
        }
    );
    assert_eq!(
        retirement.status(),
        BindingRetirementStatus::CleanupRequested {
            effect: compensation.id(),
        }
    );
}

#[test]
fn complete_root_create_rejects_frozen_fingerprint_changes() {
    let mut fixture = fixture();
    prepare_child_source_platform(&mut fixture);
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
    let input = command_input(
        &fixture.engine,
        WorkspaceCommand::Select {
            source: changing_source,
        },
    );
    submit_test_input(&mut fixture, input).expect("source mutation must commit");

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
    prepare_child_source_platform(&mut fixture);
    let payload = MovePayload::Item(
        fixture
            .engine
            .workspace()
            .capture_item_source(ROOT_SOURCE, fixture.source_tabs, ItemId::new(1))
            .expect("single source item must be current"),
    );
    let request = start_native_create_with_payload(&mut fixture, payload, ROOT_SOURCE);
    close_content_item(&mut fixture, ItemId::new(1));
    let input = command_input(
        &fixture.engine,
        WorkspaceCommand::CreateSurfaceRoot {
            surface: SURFACE_REPLACEMENT,
            root: ROOT_REPLACEMENT,
            content: RootContent::OpenItem(ItemId::new(99)),
        },
    );
    let replacement = submit_test_input(&mut fixture, input).expect("replacement root must commit");
    let replacement_outcome = replacement.reduced_inputs()[0].outcome();
    assert!(
        matches!(
            replacement_outcome,
            InputOutcome::CommandProcessed { changed: true, .. }
        ),
        "unexpected replacement-root outcome: {replacement_outcome:?}"
    );
    assert!(fixture.engine.workspace().root(ROOT_REPLACEMENT).is_some());
    assert!(
        fixture
            .engine
            .workspace()
            .surface(SURFACE_REPLACEMENT)
            .is_some()
    );

    observe_ready_and_assert_compensation(&mut fixture, request);
    let items = fixture.engine.workspace().item_multiset();
    assert!(items.contains_key(&ItemId::new(99)));
    assert!(!items.contains_key(&ItemId::new(1)));
}

#[test]
fn complete_root_disappearance_rejects_only_the_create_saga() {
    let mut fixture = fixture_with_source_items(&[1]);
    prepare_child_source_platform(&mut fixture);
    let payload = MovePayload::Item(
        fixture
            .engine
            .workspace()
            .capture_item_source(ROOT_SOURCE, fixture.source_tabs, ItemId::new(1))
            .expect("single source item must be current"),
    );
    let request = start_native_create_with_payload(&mut fixture, payload, ROOT_SOURCE);
    close_content_item(&mut fixture, ItemId::new(1));
    assert!(fixture.engine.workspace().root(ROOT_SOURCE).is_none());

    observe_ready_and_assert_compensation(&mut fixture, request);
    assert!(fixture.engine.workspace().root(ROOT_SOURCE).is_none());
}

#[test]
fn definitive_create_failure_releases_the_reservation_without_waiting_for_inventory() {
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
    assert!(
        fixture
            .engine
            .viewport()
            .native_create_saga(first.saga())
            .is_none()
    );
    assert!(fixture.engine.viewport().viewport(SURFACE_NATIVE).is_none());
    assert!(
        fixture
            .engine
            .viewport()
            .binding_retirements()
            .all(|(binding, _)| binding != first.binding()),
        "a definitive non-dispatch must not create a late-appearance retirement"
    );

    let retry = start_native_create_with_presentation_identities(
        &mut fixture,
        SURFACE_NATIVE_RETRY,
        ROOT_NATIVE_RETRY,
        FLOATING_RECOVERY_RETRY,
    );
    assert_ne!(retry.saga(), first.saga());
    assert_ne!(retry.effect(), first.effect());
    assert_eq!(retry.binding().surface(), SURFACE_NATIVE_RETRY);
    assert_ne!(retry.binding().surface(), first.binding().surface());
    assert_ne!(retry.binding().token(), first.binding().token());
    assert_ne!(retry.binding().incarnation(), first.binding().incarnation());
    let proposal = fixture
        .engine
        .viewport()
        .native_create_saga(retry.saga())
        .expect("retry saga must remain active")
        .prepared()
        .proposal();
    assert_eq!(proposal.root(), ROOT_NATIVE_RETRY);
    assert_eq!(
        proposal.converted_main().floating(),
        FLOATING_RECOVERY_RETRY
    );
}

#[test]
fn indeterminate_create_survives_authoritative_inventory_absence() {
    let mut fixture = fixture();
    prepare_base_platform(&mut fixture, ViewportRole::Child);
    let request = start_native_create(&mut fixture);
    report_create_result(
        &mut fixture,
        request,
        EffectDispatchResult::Indeterminate(EffectIndeterminateReason::AcknowledgementLost),
    );
    assert!(matches!(
        fixture
            .engine
            .viewport()
            .native_create_saga(request.saga())
            .expect("indeterminate saga must remain queryable")
            .phase(),
        NativeCreatePhase::AwaitingHidden { create } if create == request.effect()
    ));

    let absence = publish_windows!(
        &mut fixture,
        vec![source_window(&fixture), host_window(&fixture)],
    );
    assert!(platform_effects(&fixture, &absence).is_empty());
    assert!(
        fixture
            .engine
            .viewport()
            .native_create_saga(request.saga())
            .is_some(),
        "inventory absence cannot infer that an indeterminate create failed"
    );
    assert!(fixture.engine.viewport().viewport(SURFACE_NATIVE).is_some());
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
    assert!(
        fixture
            .engine
            .viewport()
            .binding_retirements()
            .all(|(binding, _)| binding != request.binding()),
        "an active saga owns the binding until an explicit cancel or terminal proof"
    );
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

    let expected = fixture.engine.version();
    let cancelled = submit_test_input(
        &mut fixture,
        EngineInput::CancelNativeCreate {
            expected,
            saga: request.saga(),
        },
    )
    .expect("explicit create cancellation must reduce");
    assert!(matches!(
        cancelled.reduced_inputs()[0].outcome(),
        InputOutcome::NativeCreateCancelled {
            saga,
            compensation: None,
        } if *saga == request.saga()
    ));
    assert!(
        fixture
            .engine
            .viewport()
            .native_create_saga(request.saga())
            .is_none()
    );
    let retirement = binding_retirement(&fixture, request.binding());
    assert_eq!(
        retirement.origin(),
        BindingRetirementOrigin::NativeCreateAborted {
            create: request.effect(),
        }
    );
    assert_eq!(
        retirement.status(),
        BindingRetirementStatus::AwaitingAppearance
    );
    assert!(retirement.may_reappear());
}

#[test]
fn a_cancelled_create_which_appears_late_is_compensated_exactly_once() {
    let mut fixture = fixture();
    prepare_base_platform(&mut fixture, ViewportRole::Child);
    let request = start_native_create(&mut fixture);
    report_create_result(
        &mut fixture,
        request,
        EffectDispatchResult::Indeterminate(EffectIndeterminateReason::AcknowledgementLost),
    );
    let expected = fixture.engine.version();
    submit_test_input(
        &mut fixture,
        EngineInput::CancelNativeCreate {
            expected,
            saga: request.saga(),
        },
    )
    .expect("explicit cancellation must retain an emitted create for compensation");
    assert!(
        fixture
            .engine
            .viewport()
            .native_create_saga(request.saga())
            .is_none()
    );
    assert_eq!(
        binding_retirement(&fixture, request.binding()).status(),
        BindingRetirementStatus::AwaitingAppearance
    );

    let late = publish_windows!(
        &mut fixture,
        vec![
            source_window(&fixture),
            host_window(&fixture),
            native_window(request),
        ],
    );
    let compensation = effect_of_kind(&fixture, &late, |effect| {
        matches!(
            effect,
            PlatformEffect::CompensatingClose {
                binding,
                compensates,
            } if *binding == request.binding() && *compensates == request.effect()
        )
    });
    assert_eq!(
        binding_retirement(&fixture, request.binding()).status(),
        BindingRetirementStatus::CleanupRequested {
            effect: compensation.id(),
        }
    );

    let repeated = publish_windows!(
        &mut fixture,
        vec![
            source_window(&fixture),
            host_window(&fixture),
            native_window(request),
        ],
    );
    assert!(platform_effects(&fixture, &repeated).is_empty());
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

    publish_windows_with_close_observations!(
        &mut fixture,
        vec![source_window(&fixture), host_window(&fixture)],
        &[(
            request.binding(),
            WindowCloseState::Destroyed,
            Some(compensation.id()),
        )],
    );
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
    let input = command_input(
        &fixture.engine,
        WorkspaceCommand::Select {
            source: changing_source,
        },
    );
    submit_test_input(&mut fixture, input).expect("source mutation must commit");
    let ready = advance_native_create_to_visible(&mut fixture, request);
    let failed_cleanup = effect_of_kind(&fixture, &ready, |effect| {
        matches!(
            effect,
            PlatformEffect::CompensatingClose { binding, .. }
                if *binding == request.binding()
        )
    })
    .id();
    let input = platform_effect_input(
        &fixture.engine,
        EffectResult::new(
            failed_cleanup,
            fixture.engine.version().epoch(),
            EffectDispatchResult::DispatchFailed(DispatchFailureReason::WindowUnavailable),
        ),
    );
    let failed = submit_test_input(&mut fixture, input).expect("cleanup failure must reduce");
    assert!(platform_effects(&fixture, &failed).is_empty());

    let expected = fixture.engine.version();
    let retried = submit_test_input(
        &mut fixture,
        EngineInput::RetryViewportCleanup {
            expected,
            failed_effect: failed_cleanup,
        },
    )
    .expect("cleanup retry must reduce");
    let retry = effect_of_kind(&fixture, &retried, |effect| {
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
    assert_eq!(
        binding_retirement(&fixture, request.binding()).status(),
        BindingRetirementStatus::CleanupRequested { effect: retry.id() }
    );

    let repeated = publish_windows!(
        &mut fixture,
        vec![
            source_window(&fixture),
            host_window(&fixture),
            native_window(request),
        ],
    );
    assert!(platform_effects(&fixture, &repeated).is_empty());
}

#[test]
fn staging_native_create_close_never_enters_surface_close_or_admission() {
    let mut fixture = fixture();
    prepare_base_platform(&mut fixture, ViewportRole::Child);
    let request = start_native_create(&mut fixture);
    let before = fixture.engine.workspace().clone();

    let requested = publish_windows_with_close_observations!(
        &mut fixture,
        vec![
            source_window(&fixture),
            host_window(&fixture),
            native_window_with_presentation(request, WindowPresentationState::Hidden),
        ],
        &[(request.binding(), WindowCloseState::LiveRequested, None)],
    );
    assert!(requested.reduced_inputs().iter().any(|input| matches!(
        input.outcome(),
        InputOutcome::PlatformSnapshotPublished {
            native_close_edges,
            ..
        } if native_close_edges.is_empty()
    )));
    assert_eq!(fixture.engine.active_close_plans().count(), 0);
    assert_eq!(fixture.engine.workspace(), &before);
    assert!(fixture.engine.workspace().surface(SURFACE_NATIVE).is_none());
    assert!(
        fixture
            .engine
            .viewport()
            .native_create_saga(request.saga())
            .is_none()
    );
    assert!(fixture.engine.viewport().viewport(SURFACE_NATIVE).is_none());
    let cleanup = effect_of_kind(&fixture, &requested, |effect| {
        matches!(
            effect,
            PlatformEffect::CompensatingClose {
                binding,
                compensates,
            } if *binding == request.binding() && *compensates == request.effect()
        )
    });

    let expected_epoch = fixture.engine.version().epoch();
    let failure = platform_effect_input(
        &fixture.engine,
        EffectResult::new(
            cleanup.id(),
            expected_epoch,
            EffectDispatchResult::DispatchFailed(DispatchFailureReason::WindowUnavailable),
        ),
    );
    let failed = submit_test_input(&mut fixture, failure)
        .expect("staging cleanup failure must reduce without admitting the binding");
    assert!(platform_effects(&fixture, &failed).is_empty());
    assert_eq!(fixture.engine.active_close_plans().count(), 0);
    assert_eq!(fixture.engine.workspace(), &before);
    assert!(fixture.engine.viewport().viewport(SURFACE_NATIVE).is_none());
    assert_eq!(
        binding_retirement(&fixture, request.binding()).status(),
        BindingRetirementStatus::CleanupFailed {
            effect: cleanup.id()
        }
    );

    publish_windows_with_close_observations!(
        &mut fixture,
        vec![source_window(&fixture), host_window(&fixture)],
        &[(
            request.binding(),
            WindowCloseState::Destroyed,
            Some(cleanup.id()),
        )],
    );
    assert_eq!(fixture.engine.workspace(), &before);
    assert!(fixture.engine.workspace().surface(SURFACE_NATIVE).is_none());
    assert!(
        fixture
            .engine
            .viewport()
            .binding_retirements()
            .all(|(binding, _)| binding != request.binding())
    );
}

#[test]
fn staging_native_create_close_does_not_consume_same_snapshot_non_staging_close() {
    let mut fixture = fixture();
    prepare_base_platform(&mut fixture, ViewportRole::Child);
    let request = start_native_create(&mut fixture);
    let host = known_host_binding(&fixture);

    let observed = publish_windows_with_close_observations!(
        &mut fixture,
        vec![
            source_window(&fixture),
            host_window(&fixture),
            native_window_with_presentation(request, WindowPresentationState::Hidden),
        ],
        &[
            (request.binding(), WindowCloseState::LiveRequested, None),
            (host, WindowCloseState::LiveRequested, None),
        ],
    );
    let native_close_edges = observed
        .reduced_inputs()
        .iter()
        .find_map(|input| match input.outcome() {
            InputOutcome::PlatformSnapshotPublished {
                native_close_edges, ..
            } => Some(native_close_edges.as_slice()),
            _ => None,
        })
        .expect("platform snapshot must retain its native close edge summary");
    assert_eq!(native_close_edges.len(), 1);
    assert_eq!(native_close_edges[0].binding(), host);
    assert_eq!(fixture.engine.active_close_plans().count(), 0);
    assert!(fixture.engine.workspace().surface(SURFACE_NATIVE).is_none());
    assert!(
        fixture
            .engine
            .viewport()
            .native_create_saga(request.saga())
            .is_none()
    );
}

#[test]
fn retain_layout_native_close_waits_for_the_matching_destroyed_proof() {
    let mut fixture = fixture();
    prepare_base_platform(&mut fixture, ViewportRole::Child);
    let before = fixture.engine.workspace().clone();
    let binding = viewport_binding(&fixture, SURFACE_HOST);
    let observed = publish_windows_with_close_observations!(
        &mut fixture,
        vec![source_window(&fixture), host_window(&fixture)],
        &[(binding, WindowCloseState::LiveRequested, None)],
    );
    let edge = native_close_edge_from(&observed);

    let expected = fixture.engine.version();
    let accepted = submit_test_input(
        &mut fixture,
        EngineInput::RequestSurfaceClose {
            expected,
            edge,
            request: SurfaceCloseRequest::RetainLayout,
        },
    )
    .expect("surface close must reduce");
    let plan = match accepted.reduced_inputs() {
        [input] => match input.outcome() {
            InputOutcome::SurfaceCloseRequested {
                edge: actual_edge,
                request: SurfaceCloseRequest::RetainLayout,
                plan,
                ..
            } if *actual_edge == edge => plan.clone(),
            outcome => panic!("retain close request was rejected: {outcome:?}"),
        },
        inputs => panic!("retain close request must reduce exactly once: {inputs:?}"),
    };
    let mut accepted = accepted;
    for item in plan.items() {
        accepted = submit_test_input(
            &mut fixture,
            EngineInput::ResolveClose {
                request: plan.request(),
                token: item.token(),
                decision: dockspace::CloseDecision::Allow,
            },
        )
        .expect("every retain-layout pane decision must reduce");
    }
    let effect = native_close_effect(
        &fixture,
        &accepted,
        binding,
        dockspace::effect::NativeCloseResolution::Accept,
    );
    assert_eq!(fixture.engine.workspace(), &before);
    assert!(fixture.engine.viewport().viewport(SURFACE_HOST).is_some());

    publish_windows_with_close_observations!(
        &mut fixture,
        vec![source_window(&fixture)],
        &[(binding, WindowCloseState::Destroyed, Some(effect))],
    );

    assert_eq!(fixture.engine.workspace(), &before);
    assert!(fixture.engine.viewport().viewport(SURFACE_HOST).is_none());
}

#[test]
fn cancelled_native_close_requires_an_acknowledged_live_clear() {
    let mut fixture = fixture();
    prepare_base_platform(&mut fixture, ViewportRole::Child);
    let before = fixture.engine.workspace().clone();
    let binding = viewport_binding(&fixture, SURFACE_HOST);
    let observed = publish_windows_with_close_observations!(
        &mut fixture,
        vec![source_window(&fixture), host_window(&fixture)],
        &[(binding, WindowCloseState::LiveRequested, None)],
    );
    let edge = native_close_edge_from(&observed);

    let expected = fixture.engine.version();
    let cancellation = submit_test_input(
        &mut fixture,
        EngineInput::CancelSurfaceClose { expected, edge },
    )
    .expect("surface close cancellation must reduce");
    assert!(matches!(
        cancellation.reduced_inputs(),
        [input]
            if matches!(
                input.outcome(),
                InputOutcome::SurfaceCloseCancellationRequested { edge: actual_edge, .. }
                    if *actual_edge == edge
            )
    ));
    let effect = native_close_effect(
        &fixture,
        &cancellation,
        binding,
        dockspace::effect::NativeCloseResolution::Cancel,
    );
    assert_eq!(fixture.engine.workspace(), &before);

    let cleared = publish_windows_with_close_observations!(
        &mut fixture,
        vec![source_window(&fixture), host_window(&fixture)],
        &[(binding, WindowCloseState::LiveClear, Some(effect))],
    );

    assert!(platform_effects(&fixture, &cleared).is_empty());
    assert_eq!(fixture.engine.workspace(), &before);
    assert!(fixture.engine.viewport().viewport(SURFACE_HOST).is_some());
    assert_eq!(fixture.engine.active_close_plans().count(), 0);
}

#[test]
fn direct_native_destruction_recovers_the_whole_root_to_its_derived_source_host() {
    let mut fixture = fixture();
    prepare_base_platform(&mut fixture, ViewportRole::Child);
    let request = start_native_create(&mut fixture);
    advance_native_create_to_visible(&mut fixture, request);
    assert!(fixture.engine.workspace().surface(SURFACE_NATIVE).is_some());
    publish_scene(&mut fixture);

    publish_windows_with_close_observations!(
        &mut fixture,
        vec![source_window(&fixture), host_window(&fixture)],
        &[(request.binding(), WindowCloseState::Destroyed, None)],
    );

    assert!(
        fixture.engine.workspace().surface(SURFACE_NATIVE).is_none(),
        "destroyed native surface remained pending: {:?}",
        fixture.engine.blocked_surface_recovery(SURFACE_NATIVE)
    );
    let workspace = fixture.engine.workspace();
    let recovered = workspace
        .contained_floating(FLOATING_RECOVERY)
        .expect("direct destruction must recover the complete root");
    assert_eq!(recovered.root, ROOT_NATIVE);
    assert_eq!(
        workspace.presentation_for_root(ROOT_NATIVE),
        Some(RootPresentationOwner::Contained {
            surface: SURFACE_SOURCE,
            floating: FLOATING_RECOVERY,
        })
    );
}

#[test]
fn restore_rebinds_roots_but_fails_closed_for_child_recovery_contracts() {
    let mut fixture = fixture();
    let (source_before, host_before) = register_base_viewports(&mut fixture, ViewportRole::Child);
    publish_windows!(
        &mut fixture,
        vec![source_window(&fixture), host_window(&fixture)],
    );
    let workspace = fixture.engine.workspace().clone();
    let version_before = fixture.engine.version();

    let transition = submit_test_input(&mut fixture, EngineInput::ReplaceWorkspace(workspace))
        .expect("workspace replacement must reduce");
    let reconciliation = match transition.reduced_inputs()[0].outcome() {
        InputOutcome::WorkspaceReplaced {
            before,
            after,
            reconciliation,
            ..
        } => {
            assert_eq!(*before, version_before);
            assert_ne!(after.epoch(), before.epoch());
            reconciliation
        }
        outcome => panic!("unexpected restore outcome: {outcome:?}"),
    };
    let [(actual_source, source_after)] = reconciliation.rebound() else {
        panic!("only the root binding may be rebound: {reconciliation:?}");
    };
    assert_eq!(*actual_source, source_before);
    assert_eq!(source_after.surface(), source_before.surface());
    assert_eq!(source_after.token(), source_before.token());
    assert_ne!(source_after.epoch(), source_before.epoch());
    assert_ne!(source_after.incarnation(), source_before.incarnation());
    assert_eq!(reconciliation.retired(), &[host_before]);
    assert_eq!(reconciliation.unbound_surfaces(), &[SURFACE_HOST]);
    assert_eq!(
        fixture
            .engine
            .viewport()
            .viewport(SURFACE_SOURCE)
            .expect("root viewport must remain registered")
            .lifecycle(),
        ViewportLifecycle::AwaitingObservation
    );
    assert!(fixture.engine.viewport().viewport(SURFACE_HOST).is_none());
    assert!(
        platform_effects(&fixture, &transition)
            .iter()
            .all(|request| {
                !matches!(request.effect(), PlatformEffect::RequestReplacement { .. })
            })
    );
}

#[test]
fn restore_unbinds_an_observed_external_child_without_destroying_it_or_reusing_its_token() {
    let mut fixture = fixture();
    let (_, host_binding) = register_base_viewports(&mut fixture, ViewportRole::Child);
    publish_windows!(
        &mut fixture,
        vec![source_window(&fixture), host_window(&fixture)],
    );

    let transition = submit_test_input(
        &mut fixture,
        EngineInput::ReplaceWorkspace(source_only_workspace()),
    )
    .expect("workspace replacement must reduce");
    let reconciliation = match transition.reduced_inputs()[0].outcome() {
        InputOutcome::WorkspaceReplaced { reconciliation, .. } => reconciliation,
        outcome => panic!("unexpected restore outcome: {outcome:?}"),
    };
    assert_eq!(reconciliation.retired(), &[host_binding]);
    assert!(fixture.engine.viewport().viewport(SURFACE_HOST).is_none());
    assert!(reconciliation.cleanup_effects().is_empty());
    assert!(
        platform_effects(&fixture, &transition)
            .iter()
            .all(|request| {
                !matches!(
                    request.effect(),
                    PlatformEffect::ReleaseChild { binding }
                        | PlatformEffect::RequestRootClose { binding }
                        | PlatformEffect::CompensatingClose { binding, .. }
                        if *binding == host_binding
                )
            })
    );
    let retirement = binding_retirement(&fixture, host_binding);
    assert_eq!(retirement.ownership(), ViewportOwnership::External);
    assert_eq!(
        retirement.status(),
        BindingRetirementStatus::AwaitingExactDestruction
    );
}

#[test]
fn unobserved_external_replace_workspace_binding_quarantines_token_until_exact_destruction() {
    let mut fixture = fixture();
    let (_, host_binding) = register_base_viewports(&mut fixture, ViewportRole::Child);

    submit_test_input(
        &mut fixture,
        EngineInput::ReplaceWorkspace(source_only_workspace()),
    )
    .expect("workspace replacement must retire the unobserved external binding");
    submit_test_input(
        &mut fixture,
        EngineInput::ReplaceWorkspace(source_and_new_surface_workspace()),
    )
    .expect("replacement must provide a distinct surface for token reuse validation");

    assert_unobserved_external_binding_quarantines_token_until_exact_destruction(
        &mut fixture,
        host_binding,
        BindingRetirementOrigin::WorkspaceReplaced,
    );
}

#[test]
fn unobserved_external_tick_final_vacancy_quarantines_token_until_exact_destruction() {
    let mut fixture = fixture();
    let (_, host_binding) = register_base_viewports(&mut fixture, ViewportRole::Child);
    let host_node = fixture
        .engine
        .workspace()
        .root(ROOT_HOST)
        .expect("host root must exist before rehome")
        .node;
    let command = WorkspaceCommand::RehomeRoot {
        source: fixture
            .engine
            .workspace()
            .capture_node_source(ROOT_HOST, host_node)
            .expect("host root must be capturable"),
        target: RootPresentationTarget::Contained {
            surface: SURFACE_SOURCE,
            floating: FloatingPresentationId::new(11),
            rect: logical_rect(32.0, 32.0, 360.0, 240.0),
            position: ContainedPosition::Front,
        },
    };
    let input = command_input(&fixture.engine, command);
    submit_test_input(&mut fixture, input)
        .expect("ordinary command must vacate the host surface at tick finalization");
    assert!(fixture.engine.workspace().surface(SURFACE_HOST).is_none());
    assert!(fixture.engine.viewport().viewport(SURFACE_HOST).is_none());

    let input = command_input(
        &fixture.engine,
        WorkspaceCommand::CreateSurfaceRoot {
            surface: SURFACE_NATIVE,
            root: ROOT_NATIVE,
            content: RootContent::OpenItem(ItemId::new(5)),
        },
    );
    submit_test_input(&mut fixture, input)
        .expect("ordinary command must create a distinct surface for token reuse validation");

    assert_unobserved_external_binding_quarantines_token_until_exact_destruction(
        &mut fixture,
        host_binding,
        BindingRetirementOrigin::SurfaceVacated,
    );
}

#[test]
fn restore_removes_an_observed_runtime_child_and_requests_exact_cleanup() {
    let mut fixture = fixture();
    let request = prepare_runtime_owned_child(&mut fixture);
    let binding = request.binding();

    let transition = submit_test_input(
        &mut fixture,
        EngineInput::ReplaceWorkspace(source_only_workspace()),
    )
    .expect("workspace replacement must reduce");
    let reconciliation = match transition.reduced_inputs()[0].outcome() {
        InputOutcome::WorkspaceReplaced { reconciliation, .. } => reconciliation,
        outcome => panic!("unexpected restore outcome: {outcome:?}"),
    };
    assert!(reconciliation.retired().contains(&binding));
    assert!(fixture.engine.viewport().viewport(SURFACE_NATIVE).is_none());
    let cleanup = effect_of_kind(
        &fixture,
        &transition,
        |effect| matches!(effect, PlatformEffect::ReleaseChild { binding: actual } if *actual == binding),
    );
    assert!(reconciliation.cleanup_effects().contains(&cleanup.id()));
    let retired = binding_retirement(&fixture, binding);
    assert!(retired.observed());
    assert_eq!(retired.ownership(), ViewportOwnership::RuntimeOwned);
    assert_eq!(
        retired.status(),
        BindingRetirementStatus::CleanupRequested {
            effect: cleanup.id()
        }
    );
}

#[test]
fn failed_retired_cleanup_retries_only_after_explicit_input() {
    let mut fixture = fixture();
    let host_binding = prepare_runtime_owned_child(&mut fixture).binding();
    let restored = submit_test_input(
        &mut fixture,
        EngineInput::ReplaceWorkspace(source_only_workspace()),
    )
    .expect("workspace replacement must reduce");
    let failed_cleanup = effect_of_kind(&fixture, &restored, |effect| {
        matches!(effect, PlatformEffect::ReleaseChild { binding } if *binding == host_binding)
    })
    .id();
    let input = platform_effect_input(
        &fixture.engine,
        EffectResult::new(
            failed_cleanup,
            fixture.engine.version().epoch(),
            EffectDispatchResult::DispatchFailed(DispatchFailureReason::WindowUnavailable),
        ),
    );
    submit_test_input(&mut fixture, input).expect("retired cleanup failure must reduce");

    let expected = fixture.engine.version();
    let retried = submit_test_input(
        &mut fixture,
        EngineInput::RetryViewportCleanup {
            expected,
            failed_effect: failed_cleanup,
        },
    )
    .expect("retired cleanup retry must reduce");
    let retry = effect_of_kind(
        &fixture,
        &retried,
        |effect| matches!(effect, PlatformEffect::ReleaseChild { binding } if *binding == host_binding),
    );
    assert_ne!(retry.id(), failed_cleanup);
    assert_eq!(retry.epoch(), fixture.engine.version().epoch());
    let retired = binding_retirement(&fixture, host_binding);
    assert!(retired.observed());
    assert_eq!(retired.ownership(), ViewportOwnership::RuntimeOwned);
    assert_eq!(
        retired.status(),
        BindingRetirementStatus::CleanupRequested { effect: retry.id() }
    );
}

#[test]
fn retired_tombstone_reserves_its_token_until_exact_destroyed_observation() {
    let mut fixture = fixture();
    let (_, host_binding) = register_base_viewports(&mut fixture, ViewportRole::Child);
    publish_windows!(
        &mut fixture,
        vec![source_window(&fixture), host_window(&fixture)],
    );
    submit_test_input(
        &mut fixture,
        EngineInput::ReplaceWorkspace(source_only_workspace()),
    )
    .expect("first workspace replacement must reduce");
    submit_test_input(
        &mut fixture,
        EngineInput::ReplaceWorkspace(source_and_new_surface_workspace()),
    )
    .expect("second workspace replacement must reduce");
    assert_eq!(
        binding_retirement(&fixture, host_binding).status(),
        BindingRetirementStatus::AwaitingExactDestruction
    );

    let provider = fixture.presentation_host.platform_provider();
    let expected = fixture.engine.version();
    let error = submit_test_input_expect_error(
        &mut fixture,
        EngineInput::RegisterViewport {
            provider,
            expected,
            surface: SURFACE_NATIVE,
            token: HOST_TOKEN,
            role: ViewportRole::Root,
            recovery_target: None,
        },
    );
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

    publish_windows!(&mut fixture, vec![source_window(&fixture)]);
    assert_eq!(
        binding_retirement(&fixture, host_binding).status(),
        BindingRetirementStatus::AwaitingExactDestruction,
        "inventory absence alone cannot release an externally owned token"
    );

    publish_windows_with_close_observations!(
        &mut fixture,
        vec![source_window(&fixture)],
        &[(host_binding, WindowCloseState::Destroyed, None)],
    );
    assert!(
        fixture
            .engine
            .viewport()
            .binding_retirements()
            .all(|(binding, _)| binding != host_binding)
    );

    let expected = fixture.engine.version();
    let rebound = submit_test_input(
        &mut fixture,
        EngineInput::RegisterViewport {
            provider,
            expected,
            surface: SURFACE_NATIVE,
            token: HOST_TOKEN,
            role: ViewportRole::Root,
            recovery_target: None,
        },
    )
    .expect("exact destroyed evidence may release the retired token");
    assert!(matches!(
        rebound.reduced_inputs()[0].outcome(),
        InputOutcome::ViewportRegistered { binding }
            if binding.token() == HOST_TOKEN && binding.incarnation() != host_binding.incarnation()
    ));
}

#[test]
fn repeated_restore_in_one_boundary_reissues_only_the_current_tombstone_cleanup() {
    let mut fixture = fixture();
    let host_binding = prepare_runtime_owned_child(&mut fixture).binding();
    let replacement = source_only_workspace();
    let transition = submit_test_inputs(
        &mut fixture,
        [
            EngineInput::ReplaceWorkspace(replacement.clone()),
            EngineInput::ReplaceWorkspace(replacement),
        ],
    )
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
        EffectPhase::Invalidated { .. }
    ));
    let emitted = effect_of_kind(&fixture, &transition, |effect| {
        matches!(
            effect,
            PlatformEffect::ReleaseChild { binding } if *binding == host_binding
        )
    });
    assert_eq!(emitted.id(), cleanup_ids[1]);
    assert_eq!(platform_effects(&fixture, &transition).len(), 1);
    let retired = binding_retirement(&fixture, host_binding);
    assert_eq!(
        retired.status(),
        BindingRetirementStatus::CleanupRequested {
            effect: cleanup_ids[1]
        }
    );
}

#[derive(Clone, Copy)]
struct EffectIdentity {
    id: EffectId,
    epoch: WorkspaceEpoch,
}

impl EffectIdentity {
    fn from_request(request: &EffectRequest) -> Self {
        Self {
            id: request.id(),
            epoch: request.epoch(),
        }
    }
}

struct CleanupContinuationFixture {
    fixture: Fixture,
    host_binding: ViewportBinding,
    destructive: EffectIdentity,
    successor: EffectIdentity,
    observer_result: EffectDispatchResult,
    observer_phase: EffectPhase,
}

fn cleanup_continuation_fixture() -> CleanupContinuationFixture {
    let mut fixture = fixture();
    let host_binding = prepare_runtime_owned_child(&mut fixture).binding();

    let replacement = source_only_workspace();
    let first = submit_test_input(
        &mut fixture,
        EngineInput::ReplaceWorkspace(replacement.clone()),
    )
    .expect("first workspace replacement must reduce");
    let destructive = EffectIdentity::from_request(
        effect_of_kind(&fixture, &first, |effect| {
            matches!(
                effect,
                PlatformEffect::ReleaseChild { binding } if *binding == host_binding
            )
        })
        .request(),
    );

    let second = submit_test_input(&mut fixture, EngineInput::ReplaceWorkspace(replacement))
        .expect("second workspace replacement must reduce independently");
    let successor = EffectIdentity::from_request(
        effect_of_kind(&fixture, &second, |effect| {
            matches!(
                effect,
                PlatformEffect::ContinueCleanup {
                    binding,
                    predecessor,
                    ..
                } if *binding == host_binding && *predecessor == destructive.id
            )
        })
        .request(),
    );
    assert!(platform_effects(&fixture, &second).iter().all(|request| {
        !matches!(
            request.effect(),
            PlatformEffect::ReleaseChild { binding }
                | PlatformEffect::RequestRootClose { binding }
                if *binding == host_binding
        )
    }));

    CleanupContinuationFixture {
        fixture,
        host_binding,
        destructive,
        successor,
        observer_result: EffectDispatchResult::Unsupported(
            EffectUnsupportedReason::BackendUnsupported,
        ),
        observer_phase: EffectPhase::ObservationUnsupported(
            EffectUnsupportedReason::BackendUnsupported,
        ),
    }
}

fn fail_cleanup_continuation_observer(case: &mut CleanupContinuationFixture) {
    let input = platform_effect_input(
        &case.fixture.engine,
        EffectResult::new(
            case.successor.id,
            case.successor.epoch,
            case.observer_result,
        ),
    );
    let observer = submit_test_input(&mut case.fixture, input)
        .expect("continuation dispatch result must reduce");
    assert!(matches!(
        observer.reduced_inputs(),
        [input]
            if matches!(
                input.outcome(),
                InputOutcome::PlatformEffectReported {
                    effect,
                    transition: EffectTransition::Applied,
                    ..
                } if *effect == case.successor.id
            )
    ));
    assert_eq!(
        binding_retirement(&case.fixture, case.host_binding).status(),
        BindingRetirementStatus::CleanupBlocked {
            effect: case.successor.id,
            reason: EffectUnsupportedReason::BackendUnsupported,
        }
    );
    assert_eq!(
        case.fixture
            .engine
            .viewport()
            .effects()
            .record(case.successor.id)
            .expect("continuation effect must remain auditable")
            .phase(),
        case.observer_phase
    );
}

fn retry_cleanup_continuation(case: &mut CleanupContinuationFixture) -> EffectIdentity {
    let retired_window = observed_window(
        case.host_binding,
        CoordinateObservationGeneration::new(2),
        1800.0,
        WindowInputState::ReceivesInput,
    );
    let still_present = publish_windows!(
        &mut case.fixture,
        vec![source_window(&case.fixture), retired_window],
    );
    assert!(
        platform_effects(&case.fixture, &still_present)
            .iter()
            .all(|request| { !matches!(request.effect(), PlatformEffect::ContinueCleanup { .. }) })
    );
    let expected = case.fixture.engine.version();
    let retried = submit_test_input(
        &mut case.fixture,
        EngineInput::RetryViewportCleanup {
            expected,
            failed_effect: case.successor.id,
        },
    )
    .expect("provider recovery must retry cleanup observation");
    let retry = EffectIdentity::from_request(
        effect_of_kind(&case.fixture, &retried, |effect| {
            matches!(
                effect,
                PlatformEffect::ContinueCleanup {
                    binding,
                    predecessor,
                    after: Some(after),
                } if *binding == case.host_binding
                    && *predecessor == case.destructive.id
                    && *after == case.successor.id
            )
        })
        .request(),
    );
    assert!(
        platform_effects(&case.fixture, &retried)
            .iter()
            .all(|request| {
                !matches!(
                    request.effect(),
                    PlatformEffect::ReleaseChild { binding }
                        | PlatformEffect::RequestRootClose { binding }
                        if *binding == case.host_binding
                )
            })
    );
    retry
}

fn retry_failed_cleanup_continuation(
    case: &mut CleanupContinuationFixture,
    retry: EffectIdentity,
) -> CleanupObservationToken {
    let input = platform_effect_input(
        &case.fixture.engine,
        EffectResult::new(
            retry.id,
            retry.epoch,
            EffectDispatchResult::DispatchFailed(DispatchFailureReason::ProviderStopped),
        ),
    );
    submit_test_input(&mut case.fixture, input)
        .expect("retry dispatch failure must remain observation-only");
    let expected = case.fixture.engine.version();
    let retried_again = submit_test_input(
        &mut case.fixture,
        EngineInput::RetryViewportCleanup {
            expected,
            failed_effect: retry.id,
        },
    )
    .expect("a second observation retry must reduce");
    let retry_again = effect_of_kind(&case.fixture, &retried_again, |effect| {
        matches!(
            effect,
            PlatformEffect::ContinueCleanup {
                binding,
                predecessor,
                after: Some(after),
            } if *binding == case.host_binding
                && *predecessor == case.destructive.id
                && *after == retry.id
        )
    });
    assert_eq!(retry_again.epoch(), case.successor.epoch);
    retry_again
        .cleanup_observation_token()
        .expect("the retry must carry exact delayed-result authority")
}

fn assert_cleanup_predecessor_epoch_fence(
    case: &mut CleanupContinuationFixture,
    token: CleanupObservationToken,
) {
    let input = platform_effect_input(
        &case.fixture.engine,
        EffectResult::new(
            case.destructive.id,
            case.successor.epoch,
            EffectDispatchResult::DispatchFailed(DispatchFailureReason::ProviderStopped),
        ),
    );
    let wrong_epoch = submit_test_input(&mut case.fixture, input)
        .expect("wrong-epoch predecessor evidence must reduce harmlessly");
    assert!(matches!(
        wrong_epoch.reduced_inputs(),
        [input]
            if matches!(
                input.outcome(),
                InputOutcome::PlatformEffectReported {
                    effect,
                    transition: EffectTransition::StaleEpoch,
                    ..
                } if *effect == case.destructive.id
            )
    ));

    let input = platform_effect_input(
        &case.fixture.engine,
        EffectResult::new(
            case.destructive.id,
            case.destructive.epoch,
            EffectDispatchResult::DispatchFailed(DispatchFailureReason::ProviderStopped),
        ),
    );
    let terminal = submit_test_input(&mut case.fixture, input)
        .expect("an uncorrelated cross-epoch result must reduce harmlessly");
    assert!(matches!(
        terminal.reduced_inputs(),
        [input]
            if matches!(
                input.outcome(),
                InputOutcome::PlatformEffectReported {
                    effect,
                    transition: EffectTransition::StaleEpoch,
                    ..
                } if *effect == case.destructive.id
            )
    ));

    let input = platform_effect_input(
        &case.fixture.engine,
        EffectResult::observed_via_cleanup(
            token,
            case.destructive.epoch,
            EffectDispatchResult::DispatchFailed(DispatchFailureReason::ProviderStopped),
        ),
    );
    let terminal = submit_test_input(&mut case.fixture, input)
        .expect("the exact cleanup observation token must reduce");
    assert!(matches!(
        terminal.reduced_inputs(),
        [input]
            if matches!(
                input.outcome(),
                InputOutcome::PlatformEffectReported {
                    effect,
                    transition: EffectTransition::Applied,
                    ..
                } if *effect == case.destructive.id
            )
    ));
    assert_eq!(
        binding_retirement(&case.fixture, case.host_binding).status(),
        BindingRetirementStatus::CleanupFailed {
            effect: case.destructive.id,
        }
    );
}

#[test]
fn repeated_restore_across_boundaries_continues_cleanup_without_redispatch() {
    let mut case = cleanup_continuation_fixture();
    fail_cleanup_continuation_observer(&mut case);
    let retry = retry_cleanup_continuation(&mut case);
    let token = retry_failed_cleanup_continuation(&mut case, retry);
    assert_cleanup_predecessor_epoch_fence(&mut case, token);
}

struct PendingCreateTombstoneFixture {
    fixture: Fixture,
    request: NativeCreateRequest,
    old_snapshot: PlatformSnapshot,
}

fn pending_create_tombstone_fixture() -> PendingCreateTombstoneFixture {
    let mut fixture = fixture();
    prepare_base_platform(&mut fixture, ViewportRole::Child);
    let request = start_native_create(&mut fixture);
    let old_snapshot = platform_snapshot(
        unknown_focus_observation(
            FocusObservationGeneration::new(fixture.input_generation),
            AuthorityUnavailableReason::NotReported,
        ),
        vec![source_window(&fixture), host_window(&fixture)],
    );
    let replacement = fixture.engine.workspace().clone();

    let restored = submit_test_input(&mut fixture, EngineInput::ReplaceWorkspace(replacement))
        .expect("workspace replacement must reduce");
    let reconciliation = match restored.reduced_inputs()[0].outcome() {
        InputOutcome::WorkspaceReplaced { reconciliation, .. } => reconciliation,
        outcome => panic!("unexpected restore outcome: {outcome:?}"),
    };
    assert!(reconciliation.retired().contains(&request.binding()));
    assert!(reconciliation.cleanup_effects().iter().all(|effect| {
        !matches!(
            fixture
                .engine
                .viewport()
                .effects()
                .record(*effect)
                .expect("restore cleanup effect must remain queryable")
                .request()
                .effect(),
            PlatformEffect::CompensatingClose {
                binding,
                compensates,
            } if *binding == request.binding() && *compensates == request.effect()
        )
    }));
    let tombstone = binding_retirement(&fixture, request.binding());
    assert!(!tombstone.observed());
    assert!(tombstone.may_reappear());
    assert_eq!(
        tombstone.status(),
        BindingRetirementStatus::AwaitingAppearance
    );
    PendingCreateTombstoneFixture {
        fixture,
        request,
        old_snapshot,
    }
}

#[test]
fn restore_tombstones_a_pending_create_and_compensates_one_late_appearance() {
    let PendingCreateTombstoneFixture {
        mut fixture,
        request,
        old_snapshot,
    } = pending_create_tombstone_fixture();

    assert_old_platform_inputs_are_stale(&mut fixture, request, old_snapshot);

    let late = publish_windows!(
        &mut fixture,
        vec![
            source_window(&fixture),
            host_window(&fixture),
            native_window(request),
        ],
    );
    let compensation = effect_of_kind(&fixture, &late, |effect| {
        matches!(
            effect,
            PlatformEffect::CompensatingClose {
                binding,
                compensates,
            } if *binding == request.binding() && *compensates == request.effect()
        )
    });
    let retired = binding_retirement(&fixture, request.binding());
    assert!(retired.observed());
    assert_eq!(
        retired.status(),
        BindingRetirementStatus::CleanupRequested {
            effect: compensation.id()
        }
    );

    let repeated = publish_windows!(
        &mut fixture,
        vec![
            source_window(&fixture),
            host_window(&fixture),
            native_window(request),
        ],
    );
    assert!(platform_effects(&fixture, &repeated).is_empty());
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
