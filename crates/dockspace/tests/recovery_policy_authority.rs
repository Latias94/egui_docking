mod support;

use std::collections::BTreeMap;

use dockspace::command::{ContainedPosition, RootPresentationTarget, WorkspaceCommand};
use dockspace::engine::{DockEngine, EngineError, EngineInput};
use dockspace::geometry::{LogicalRect, LogicalSize, PhysicalRect, ScaleFactor};
use dockspace::graph::{Node, RootRecord, SurfacePresentation, Workspace};
use dockspace::ids::{FloatingPresentationId, ItemId, RootId, StableInputSourceId, SurfaceId};
use dockspace::intent::{Authority, AuthorityUnavailableReason};
use dockspace::platform::{
    CloseEffectAcknowledgement, ObservedWindow, ObservedWorkArea, PlatformCapabilities,
    PlatformCapability, PlatformSnapshot, PresentationEffectAcknowledgement,
    WindowCloseObservation, WindowCloseState, WindowCoordinateObservation, WindowInputState,
    WindowPresentationObservation, WindowPresentationState,
};
use dockspace::policy::{
    DockClassId, DockItemRule, DockPayloadKind, DockPolicy, DockPolicyRequest,
    DockPresentationMode, DockSourceRule, DockSurfaceRecoveryPolicyRequest,
    DockSurfaceRecoveryRootFacts, DockSurfaceRule, PolicyDecision, PolicyRejection, PolicyRevision,
    PolicyRuleScope,
};
use dockspace::surface_recovery::{ConvertedMainRecovery, SurfaceRecoveryTarget};
use dockspace::transaction::WorkspaceTransaction;
use dockspace::transition::{EngineTransition, InputOutcome};
use dockspace::viewport::{
    CloseObservationGeneration, CoordinateObservationGeneration, PresentationObservationGeneration,
    ViewportBinding, ViewportRole, WindowToken, WorkAreaToken,
};
use dockspace::viewport_focus::{FocusObservationGeneration, unknown_focus_observation};

const HOST_ROOT: RootId = RootId::new(1);
const CHILD_ROOT: RootId = RootId::new(2);
const HOST_SURFACE: SurfaceId = SurfaceId::new(10);
const CHILD_SURFACE: SurfaceId = SurfaceId::new(20);
const HOST_ITEM: ItemId = ItemId::new(100);
const CHILD_ITEM: ItemId = ItemId::new(200);
const RECOVERY_FLOATING: FloatingPresentationId = FloatingPresentationId::new(300);
const HOST_TOKEN: WindowToken = WindowToken::new(1_000);
const CHILD_TOKEN: WindowToken = WindowToken::new(2_000);
const WORK_AREA: WorkAreaToken = WorkAreaToken::new(3_000);
const INPUT_SOURCE: StableInputSourceId = StableInputSourceId::new(0xd0c5_0000_0000_0501);

fn logical_rect(x: f64) -> LogicalRect {
    LogicalRect::new(x, 0.0, 900.0, 700.0).expect("test logical rectangle is valid")
}

fn physical_rect(x: f64, y: f64, width: f64, height: f64) -> PhysicalRect {
    PhysicalRect::new(x, y, width, height).expect("test physical rectangle is valid")
}

fn workspace() -> Workspace {
    let mut builder = Workspace::builder();
    let host_tabs = builder.insert_node(Node::tabs([HOST_ITEM]));
    let child_tabs = builder.insert_node(Node::tabs([CHILD_ITEM]));
    builder.set_root(HOST_ROOT, RootRecord::new(host_tabs));
    builder.set_root(CHILD_ROOT, RootRecord::new(child_tabs));
    builder.set_surface(HOST_SURFACE, SurfacePresentation::with_main(HOST_ROOT));
    builder.set_surface(CHILD_SURFACE, SurfacePresentation::with_main(CHILD_ROOT));
    builder.build().expect("test workspace is valid")
}

fn recovery_request() -> DockSurfaceRecoveryPolicyRequest {
    DockSurfaceRecoveryPolicyRequest::new(
        CHILD_SURFACE,
        HOST_SURFACE,
        BTreeMap::from([(
            CHILD_ROOT,
            DockSurfaceRecoveryRootFacts::new(DockPresentationMode::Native, [CHILD_ITEM]),
        )]),
    )
}

fn submit(
    engine: &mut DockEngine,
    presentation_host: &mut support::TestPresentationHost,
    input: EngineInput,
) -> Result<EngineTransition, EngineError> {
    support::submit_input(engine, presentation_host, INPUT_SOURCE, input)
}

fn engine_with_registered_host(
    policy: DockPolicy,
) -> (
    DockEngine,
    support::TestPresentationHost,
    SurfaceRecoveryTarget,
) {
    let mut engine = DockEngine::new(workspace(), policy).expect("test engine is valid");
    let mut presentation_host = support::TestPresentationHost::new(&mut engine);
    let provider = presentation_host.platform_provider();
    let expected = engine.version();
    let registered = submit(
        &mut engine,
        &mut presentation_host,
        EngineInput::RegisterViewport {
            provider,
            expected,
            surface: HOST_SURFACE,
            token: HOST_TOKEN,
            role: ViewportRole::Root,
            recovery_target: None,
        },
    )
    .expect("host viewport registration reduces");
    assert!(matches!(
        registered.reduced_inputs()[0].outcome(),
        InputOutcome::ViewportRegistered { binding }
            if binding.surface() == HOST_SURFACE && binding.token() == HOST_TOKEN
    ));
    let anchor = engine
        .root_recovery_anchor(HOST_SURFACE)
        .expect("a registered Root surface owns a recovery anchor");
    let target = SurfaceRecoveryTarget::with_converted_main(
        anchor,
        ConvertedMainRecovery::new(
            CHILD_ROOT,
            RECOVERY_FLOATING,
            LogicalSize::new(0.0, 0.0).expect("zero recovery minimum is valid"),
        ),
    );
    (engine, presentation_host, target)
}

fn assert_initial_child_registration_rejected(
    policy: DockPolicy,
    expected_rejection: PolicyRejection,
) {
    assert_eq!(
        policy
            .snapshot(PolicyRevision::new(1))
            .evaluate(&DockPolicyRequest::RecoverSurface(recovery_request())),
        PolicyDecision::Reject(expected_rejection),
        "the pure policy evaluator must identify the exact recovery rejection"
    );

    let (mut engine, mut presentation_host, recovery_target) = engine_with_registered_host(policy);
    let before_version = engine.version();
    let before_workspace = engine.workspace().clone();
    let before_viewport = engine.viewport().clone();
    let provider = presentation_host.platform_provider();
    let rejected = submit(
        &mut engine,
        &mut presentation_host,
        EngineInput::RegisterViewport {
            provider,
            expected: before_version,
            surface: CHILD_SURFACE,
            token: CHILD_TOKEN,
            role: ViewportRole::Child,
            recovery_target: Some(recovery_target),
        },
    )
    .expect("policy rejection is a reduced input outcome");

    assert!(matches!(
        rejected.reduced_inputs()[0].outcome(),
        InputOutcome::ViewportRegistrationRejected {
            surface: CHILD_SURFACE
        }
    ));
    assert_eq!(rejected.before(), rejected.after());
    assert!(rejected.platform_effects().is_empty());
    assert!(rejected.events().is_empty());
    assert_eq!(engine.version(), before_version);
    assert_eq!(engine.workspace(), &before_workspace);
    assert_eq!(engine.viewport(), &before_viewport);
    assert!(engine.viewport().viewport(CHILD_SURFACE).is_none());
    assert_eq!(engine.surface_recovery_target(CHILD_SURFACE), None);
}

#[test]
fn disabled_source_root_rejects_initial_child_registration_atomically() {
    let mut source = DockSourceRule::new();
    source.set_enabled(false);
    let mut policy = DockPolicy::default();
    policy.set_source_rule(CHILD_ROOT, source);

    assert_initial_child_registration_rejected(
        policy,
        PolicyRejection::SourceDisabled { root: CHILD_ROOT },
    );
}

#[test]
fn disabled_source_surface_rejects_initial_child_registration_atomically() {
    let mut source = DockSurfaceRule::new();
    source.set_source_enabled(false);
    let mut policy = DockPolicy::default();
    policy.set_surface_rule(CHILD_SURFACE, source);

    assert_initial_child_registration_rejected(
        policy,
        PolicyRejection::SurfaceSourceDisabled {
            surface: CHILD_SURFACE,
        },
    );
}

#[test]
fn no_undocking_rejects_initial_child_registration_atomically() {
    let mut source = DockSourceRule::new();
    source.set_allow_undocking(false);
    let mut policy = DockPolicy::default();
    policy.set_source_rule(CHILD_ROOT, source);

    assert_initial_child_registration_rejected(
        policy,
        PolicyRejection::SourceUndockingDisabled { root: CHILD_ROOT },
    );
}

#[test]
fn disabled_host_surface_target_rejects_initial_child_registration_atomically() {
    let mut host = DockSurfaceRule::new();
    host.set_target_enabled(false);
    let mut policy = DockPolicy::default();
    policy.set_surface_rule(HOST_SURFACE, host);

    assert_initial_child_registration_rejected(
        policy,
        PolicyRejection::SurfaceTargetDisabled {
            surface: HOST_SURFACE,
        },
    );
}

#[test]
fn host_rejecting_root_payloads_rejects_initial_child_registration_atomically() {
    let mut host = DockSurfaceRule::new();
    host.set_allowed_target_payloads([DockPayloadKind::Item]);
    let mut policy = DockPolicy::default();
    policy.set_surface_rule(HOST_SURFACE, host);

    assert_initial_child_registration_rejected(
        policy,
        PolicyRejection::SurfaceTargetPayloadRejected {
            surface: HOST_SURFACE,
            payload: DockPayloadKind::Root,
        },
    );
}

#[test]
fn incompatible_dock_class_rejects_initial_child_registration_atomically() {
    let editor = DockClassId::new(1);
    let inspector = DockClassId::new(2);
    let mut child = DockItemRule::new();
    child.set_dock_class(Some(editor));
    let mut host = DockSurfaceRule::new();
    host.set_accepted_classes([inspector], false);
    let mut policy = DockPolicy::default();
    policy.set_item_rule(CHILD_ITEM, child);
    policy.set_surface_rule(HOST_SURFACE, host);

    assert_initial_child_registration_rejected(
        policy,
        PolicyRejection::DockClassRejected {
            item: CHILD_ITEM,
            class: Some(editor),
            scope: PolicyRuleScope::Surface(HOST_SURFACE),
        },
    );
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
    ObservedWindow::new(binding)
        .with_coordinate_observation(WindowCoordinateObservation::new(
            binding,
            CoordinateObservationGeneration::new(0),
            Authority::Known(physical_rect(x, 0.0, 900.0, 700.0)),
            Authority::Known(physical_rect(x - 8.0, -30.0, 916.0, 738.0)),
            Authority::Known(ScaleFactor::new(1.0).expect("test scale factor is valid")),
            Authority::Known(ScaleFactor::new(1.0).expect("test scale factor is valid")),
        ))
        .with_input_state(Authority::Known(WindowInputState::ReceivesInput))
}

fn publish_platform(
    engine: &mut DockEngine,
    presentation_host: &mut support::TestPresentationHost,
    mut windows: Vec<ObservedWindow>,
    destroyed: Option<ViewportBinding>,
) -> EngineTransition {
    let generation = presentation_host.next_platform_observation_generation();
    for window in &mut windows {
        let binding = window.binding();
        let coordinate_observation = window
            .coordinate_observation()
            .expect("ready test window must carry coordinate facts");
        *window = window
            .clone()
            .with_coordinate_observation(WindowCoordinateObservation::new(
                binding,
                CoordinateObservationGeneration::new(generation),
                *coordinate_observation.content_bounds(),
                *coordinate_observation.outer_bounds(),
                *coordinate_observation.native_scale_factor(),
                *coordinate_observation.presentation_scale_factor(),
            ))
            .with_presentation_observation(WindowPresentationObservation::new(
                binding,
                PresentationObservationGeneration::new(generation),
                Authority::Known(WindowPresentationState::Visible),
                PresentationEffectAcknowledgement::known(None),
            ));
    }
    let close_observations = destroyed.into_iter().map(|binding| {
        WindowCloseObservation::new(
            binding,
            CloseObservationGeneration::new(generation),
            Authority::Known(WindowCloseState::Destroyed),
            CloseEffectAcknowledgement::known(None),
        )
    });
    let inventory_observation = support::known_inventory_observation(generation, &windows);
    let snapshot = PlatformSnapshot::new(
        dockspace::viewport::PlatformSnapshotGeneration::new(generation),
        support::known_capability_observation(generation, platform_capabilities()),
        unknown_focus_observation(
            FocusObservationGeneration::new(generation),
            AuthorityUnavailableReason::NotReported,
        ),
        inventory_observation,
        windows,
        close_observations.collect(),
        support::known_work_area_observation(
            generation,
            vec![ObservedWorkArea::new(
                WORK_AREA,
                physical_rect(-1_920.0, -200.0, 3_840.0, 1_400.0),
                ScaleFactor::new(1.0).expect("test work-area scale factor is valid"),
            )],
        ),
    )
    .expect("test platform snapshot is canonical");
    let expected_epoch = engine.version().epoch();
    let provider = presentation_host.platform_provider();
    submit(
        engine,
        presentation_host,
        EngineInput::PublishPlatformSnapshot {
            provider,
            expected_epoch,
            snapshot,
        },
    )
    .expect("platform snapshot reduces")
}

fn publish_scenes(engine: &mut DockEngine, presentation_host: &mut support::TestPresentationHost) {
    let surfaces = engine
        .workspace()
        .surfaces()
        .map(|(surface, _)| surface)
        .collect::<Vec<_>>();
    for surface in surfaces {
        if engine
            .viewport()
            .viewport(surface)
            .is_some_and(|record| !record.is_ready())
        {
            continue;
        }
        support::publish_surface(engine, presentation_host, surface, logical_rect(0.0));
    }
}

#[test]
fn exact_authorized_obligation_survives_policy_change_and_recovers() {
    let (mut engine, mut presentation_host, recovery_target) =
        engine_with_registered_host(DockPolicy::default());
    let provider = presentation_host.platform_provider();
    let expected = engine.version();
    let registered = submit(
        &mut engine,
        &mut presentation_host,
        EngineInput::RegisterViewport {
            provider,
            expected,
            surface: CHILD_SURFACE,
            token: CHILD_TOKEN,
            role: ViewportRole::Child,
            recovery_target: Some(recovery_target),
        },
    )
    .expect("initially authorized Child registration reduces");
    let child_binding = match registered.reduced_inputs()[0].outcome() {
        InputOutcome::ViewportRegistered { binding } => *binding,
        outcome => panic!("unexpected Child registration outcome: {outcome:?}"),
    };
    let host_binding = engine
        .viewport()
        .viewport(HOST_SURFACE)
        .expect("host viewport remains registered")
        .binding();
    publish_platform(
        &mut engine,
        &mut presentation_host,
        vec![
            ready_window(host_binding, 0.0),
            ready_window(child_binding, 1_200.0),
        ],
        None,
    );
    publish_scenes(&mut engine, &mut presentation_host);

    let mut replacement = DockPolicy::default();
    let mut disabled_source = DockSourceRule::new();
    disabled_source.set_enabled(false);
    disabled_source.set_allow_undocking(false);
    replacement.set_source_rule(CHILD_ROOT, disabled_source);
    assert!(matches!(
        replacement
            .snapshot(PolicyRevision::new(99))
            .evaluate(&DockPolicyRequest::RecoverSurface(recovery_request())),
        PolicyDecision::Reject(_)
    ));
    let expected = engine.version();
    submit(
        &mut engine,
        &mut presentation_host,
        EngineInput::ReplacePolicy {
            expected,
            policy: replacement,
        },
    )
    .expect("policy replacement preserves an exact existing obligation");
    assert_eq!(
        engine.surface_recovery_target(CHILD_SURFACE),
        Some(recovery_target)
    );
    publish_scenes(&mut engine, &mut presentation_host);

    let initial_items = engine.workspace().item_multiset();
    let destroyed = publish_platform(
        &mut engine,
        &mut presentation_host,
        vec![ready_window(host_binding, 0.0)],
        Some(child_binding),
    );

    assert!(destroyed.platform_effects().is_empty());
    assert!(engine.workspace().surface(CHILD_SURFACE).is_none());
    let recovered = engine
        .workspace()
        .contained_floating(RECOVERY_FLOATING)
        .expect("the exact authorized main root recovers after the policy change");
    assert_eq!(recovered.root, CHILD_ROOT);
    assert_eq!(engine.workspace().item_multiset(), initial_items);
    assert!(engine.viewport().recovery_pending(CHILD_SURFACE).is_none());
    assert_eq!(engine.surface_recovery_target(CHILD_SURFACE), None);
}

#[test]
fn child_vacate_then_repopulate_in_one_tick_preserves_obligation_for_later_destroy() {
    const TRANSIENT_FLOATING: FloatingPresentationId = FloatingPresentationId::new(301);
    let mut policy = DockPolicy::default();
    policy.set_allow_native_surfaces(true);
    let (mut engine, mut presentation_host, recovery_target) = engine_with_registered_host(policy);
    let provider = presentation_host.platform_provider();
    let expected = engine.version();
    let registered = submit(
        &mut engine,
        &mut presentation_host,
        EngineInput::RegisterViewport {
            provider,
            expected,
            surface: CHILD_SURFACE,
            token: CHILD_TOKEN,
            role: ViewportRole::Child,
            recovery_target: Some(recovery_target),
        },
    )
    .expect("Child registration reduces");
    let child_binding = match registered.reduced_inputs()[0].outcome() {
        InputOutcome::ViewportRegistered { binding } => *binding,
        outcome => panic!("unexpected Child registration outcome: {outcome:?}"),
    };
    let host_binding = engine
        .viewport()
        .viewport(HOST_SURFACE)
        .expect("host viewport remains registered")
        .binding();
    publish_platform(
        &mut engine,
        &mut presentation_host,
        vec![
            ready_window(host_binding, 0.0),
            ready_window(child_binding, 1_200.0),
        ],
        None,
    );
    publish_scenes(&mut engine, &mut presentation_host);

    let child_node = engine
        .workspace()
        .root(CHILD_ROOT)
        .expect("Child root exists")
        .node;
    let source = engine
        .workspace()
        .capture_node_source(CHILD_ROOT, child_node)
        .expect("Child root source is current");
    let vacate = WorkspaceCommand::RehomeRoot {
        source,
        target: RootPresentationTarget::Contained {
            surface: HOST_SURFACE,
            floating: TRANSIENT_FLOATING,
            rect: logical_rect(0.0),
            position: ContainedPosition::Front,
        },
    };
    let mut staged = engine.workspace().clone();
    WorkspaceTransaction::from_commands([vacate.clone()])
        .apply(&mut staged, engine.policy_snapshot())
        .expect("vacated Child topology stages");
    let repopulate_source = staged
        .capture_node_source(CHILD_ROOT, child_node)
        .expect("post-vacancy Child source is exact");
    let before = engine.version();
    let transition = support::submit_inputs(
        &mut engine,
        &mut presentation_host,
        INPUT_SOURCE,
        [
            EngineInput::WorkspaceCommand {
                expected: before,
                command: vacate,
            },
            EngineInput::WorkspaceCommand {
                expected: before,
                command: WorkspaceCommand::RehomeRoot {
                    source: repopulate_source,
                    target: RootPresentationTarget::NewSurface {
                        surface: CHILD_SURFACE,
                    },
                },
            },
        ],
    )
    .expect("same-tick Child vacancy and repopulation reduce atomically");
    assert!(
        transition
            .reduced_inputs()
            .iter()
            .all(|input| matches!(input.outcome(), InputOutcome::CommandProcessed { .. })),
        "both Child commands must commit: {:?}",
        transition.reduced_inputs()
    );

    assert_eq!(
        engine.surface_recovery_target(CHILD_SURFACE),
        Some(recovery_target),
        "an intermediate vacancy must not retire the exact obligation"
    );
    assert_eq!(
        engine
            .viewport()
            .viewport(CHILD_SURFACE)
            .expect("Child binding remains registered")
            .binding(),
        child_binding
    );
    publish_scenes(&mut engine, &mut presentation_host);
    let initial_items = engine.workspace().item_multiset();
    publish_platform(
        &mut engine,
        &mut presentation_host,
        vec![ready_window(host_binding, 0.0)],
        Some(child_binding),
    );

    assert!(engine.workspace().surface(CHILD_SURFACE).is_none());
    assert_eq!(
        engine
            .workspace()
            .contained_floating(RECOVERY_FLOATING)
            .expect("later destruction consumes the preserved reservation")
            .root,
        CHILD_ROOT
    );
    assert_eq!(engine.workspace().item_multiset(), initial_items);
    assert!(engine.viewport().recovery_pending(CHILD_SURFACE).is_none());
}

#[test]
fn root_vacate_then_repopulate_in_one_tick_preserves_the_exact_anchor() {
    const TRANSIENT_FLOATING: FloatingPresentationId = FloatingPresentationId::new(302);
    let mut policy = DockPolicy::default();
    policy.set_allow_native_surfaces(true);
    let (mut engine, mut presentation_host, _) = engine_with_registered_host(policy);
    let anchor = engine
        .root_recovery_anchor(HOST_SURFACE)
        .expect("registered Root owns an anchor");
    let binding = engine
        .viewport()
        .viewport(HOST_SURFACE)
        .expect("registered Root binding exists")
        .binding();
    let host_node = engine
        .workspace()
        .root(HOST_ROOT)
        .expect("host root exists")
        .node;
    let source = engine
        .workspace()
        .capture_node_source(HOST_ROOT, host_node)
        .expect("host root source is current");
    let vacate = WorkspaceCommand::RehomeRoot {
        source,
        target: RootPresentationTarget::Contained {
            surface: CHILD_SURFACE,
            floating: TRANSIENT_FLOATING,
            rect: logical_rect(0.0),
            position: ContainedPosition::Front,
        },
    };
    let mut staged = engine.workspace().clone();
    WorkspaceTransaction::from_commands([vacate.clone()])
        .apply(&mut staged, engine.policy_snapshot())
        .expect("vacated Root topology stages");
    let repopulate_source = staged
        .capture_node_source(HOST_ROOT, host_node)
        .expect("post-vacancy Root source is exact");
    let before = engine.version();

    let transition = support::submit_inputs(
        &mut engine,
        &mut presentation_host,
        INPUT_SOURCE,
        [
            EngineInput::WorkspaceCommand {
                expected: before,
                command: vacate,
            },
            EngineInput::WorkspaceCommand {
                expected: before,
                command: WorkspaceCommand::RehomeRoot {
                    source: repopulate_source,
                    target: RootPresentationTarget::NewSurface {
                        surface: HOST_SURFACE,
                    },
                },
            },
        ],
    )
    .expect("same-tick Root vacancy and repopulation reduce atomically");
    assert!(
        transition
            .reduced_inputs()
            .iter()
            .all(|input| matches!(input.outcome(), InputOutcome::CommandProcessed { .. })),
        "both Root commands must commit: {:?}",
        transition.reduced_inputs()
    );

    assert_eq!(engine.root_recovery_anchor(HOST_SURFACE), Some(anchor));
    assert_eq!(
        engine
            .viewport()
            .viewport(HOST_SURFACE)
            .expect("Root binding remains registered")
            .binding(),
        binding
    );
}
