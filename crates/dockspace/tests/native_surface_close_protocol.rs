//! Native surface-close protocol contracts.
//!
//! These tests exercise the public reducer boundary only. In particular, a
//! platform snapshot creates a [`NativeCloseEdge`], the application resolves
//! that exact edge with a complete [`SurfaceCloseRequest`], and a later typed
//! close observation proves the platform outcome.

use super::support;

use support::TestPresentationHost;

use dockspace::close::{
    CloseDecision, CloseItemDecisionState, ClosePlanPhase, DeferredCloseDecision,
};
use dockspace::close_plan::{
    CloseCancellationState, ClosePlan, ClosePlanLookup, NativeCloseEdge, SurfaceCloseRequest,
    SurfaceContainedRehomeTarget, SurfaceMainRehomeTarget, SurfaceRehomeTarget,
};
use dockspace::command::ContainedPosition;
use dockspace::effect::{
    DispatchFailureReason, EffectDispatchResult, EffectId, EffectRecordLookup, EffectResult,
    NativeCloseResolution, PlatformEffect,
};
use dockspace::engine::{DockEngine, EngineInput};
use dockspace::frame::PanelFocus;
use dockspace::geometry::{LogicalRect, PhysicalRect, ScaleFactor};
use dockspace::graph::{ContainedFloating, Node, RootRecord, SurfacePresentation, Workspace};
use dockspace::ids::{
    FloatingPresentationId, ItemId, RootId, SourceSequence, StableInputSourceId, SurfaceId,
};
use dockspace::intent::{Authority, AuthorityUnavailableReason};
use dockspace::platform::{
    CloseEffectAcknowledgement, ObservedWindow, PlatformCapabilities, PlatformCapability,
    PlatformCapabilityReason, PlatformRequirement, PlatformSnapshot,
    PresentationEffectAcknowledgement, WindowCloseObservation, WindowCloseState,
    WindowCoordinateObservation, WindowInputState, WindowPresentationObservation,
    WindowPresentationState,
};
use dockspace::policy::{
    CloseCapability, DockItemRule, DockPayloadKind, DockPolicy, DockPresentationMode,
    DockSurfaceRule, PolicyRejection,
};
use dockspace::transition::{
    EngineTransition, InputOutcome, InputPriority, SurfaceCloseRequestRejection,
};
use dockspace::viewport::{
    CloseObservationGeneration, CoordinateObservationGeneration, PresentationObservationGeneration,
    ViewportBinding, ViewportRole, WindowToken,
};
use dockspace::viewport_focus::{
    FocusObservationGeneration, PaneFocusObservation, PaneFocusObservationGeneration,
    unknown_focus_observation,
};

const SOURCE: SurfaceId = SurfaceId::new(1);
const TARGET: SurfaceId = SurfaceId::new(2);

const SOURCE_ROOT: RootId = RootId::new(10);
const SOURCE_CONTAINED_ROOT: RootId = RootId::new(11);
const TARGET_ROOT: RootId = RootId::new(20);

const SOURCE_CONTAINED: FloatingPresentationId = FloatingPresentationId::new(30);
const REHOMED_MAIN: FloatingPresentationId = FloatingPresentationId::new(31);

const SOURCE_TOKEN: WindowToken = WindowToken::new(100);
const TARGET_TOKEN: WindowToken = WindowToken::new(101);
const TEST_SOURCE: StableInputSourceId = StableInputSourceId::new(0xc105_e000_0000_0021);

struct Harness {
    engine: DockEngine,
    presentation_host: TestPresentationHost,
    capabilities: PlatformCapabilities,
    source_sequence: u64,
    snapshot_generation: u64,
}

impl Harness {
    fn new(workspace: Workspace) -> Self {
        Self::with_policy(workspace, close_policy())
    }

    fn with_policy(workspace: Workspace, policy: DockPolicy) -> Self {
        Self::with_policy_and_capabilities(workspace, policy, close_capabilities())
    }

    fn with_policy_and_capabilities(
        workspace: Workspace,
        policy: DockPolicy,
        capabilities: PlatformCapabilities,
    ) -> Self {
        let mut engine = DockEngine::new(workspace, policy)
            .expect("fixture workspace must initialize the engine");
        let presentation_host = TestPresentationHost::new(&mut engine);
        Self {
            engine,
            presentation_host,
            capabilities,
            source_sequence: 0,
            snapshot_generation: 0,
        }
    }

    fn submit(&mut self, input: EngineInput) -> EngineTransition {
        self.source_sequence += 1;
        let mut frame = self.presentation_host.begin(&self.engine);
        let sequence = SourceSequence::new(self.source_sequence);
        if input.priority() == InputPriority::ConfigurationCommit {
            frame
                .append_configuration(TEST_SOURCE, sequence, input)
                .expect("configuration input must fit the host frame");
        } else {
            frame
                .append_input(TEST_SOURCE, sequence, input)
                .expect("semantic input must fit the host frame");
        }
        support::complete_host_frame_with_retained_or_unavailable(&self.engine, &mut frame);
        self.presentation_host.finish(frame, &mut self.engine)
    }

    fn register_viewport(&mut self, surface: SurfaceId, token: WindowToken) -> ViewportBinding {
        let expected = self.engine.version();
        let provider = self.presentation_host.platform_provider();
        let transition = self.submit(EngineInput::RegisterViewport {
            provider,
            expected,
            surface,
            token,
            role: ViewportRole::Root,
            recovery_target: None,
        });
        match only_outcome(&transition) {
            InputOutcome::ViewportRegistered { binding } => *binding,
            outcome => panic!("viewport registration was rejected: {outcome:?}"),
        }
    }

    fn register_source_viewport(&mut self) -> ViewportBinding {
        self.register_viewport(SOURCE, SOURCE_TOKEN)
    }

    fn register_target_viewport(&mut self) -> ViewportBinding {
        self.register_viewport(TARGET, TARGET_TOKEN)
    }

    fn publish_close(
        &mut self,
        binding: ViewportBinding,
        close_generation: u64,
        state: WindowCloseState,
        acknowledged_effect: Option<EffectId>,
    ) -> EngineTransition {
        let windows = match state {
            WindowCloseState::LiveClear | WindowCloseState::LiveRequested => {
                vec![live_window(binding)]
            }
            WindowCloseState::Destroyed => Vec::new(),
        };
        self.publish_close_with_windows(
            binding,
            close_generation,
            state,
            acknowledged_effect,
            windows,
        )
    }

    fn publish_close_with_windows(
        &mut self,
        binding: ViewportBinding,
        close_generation: u64,
        state: WindowCloseState,
        acknowledged_effect: Option<EffectId>,
        windows: Vec<ObservedWindow>,
    ) -> EngineTransition {
        self.snapshot_generation += 1;
        let inventory_observation =
            support::known_inventory_observation(self.snapshot_generation, &windows);
        let snapshot = PlatformSnapshot::new(
            dockspace::viewport::PlatformSnapshotGeneration::new(self.snapshot_generation),
            support::known_capability_observation(
                self.snapshot_generation,
                self.capabilities.clone(),
            ),
            unknown_focus_observation(
                FocusObservationGeneration::new(self.snapshot_generation),
                AuthorityUnavailableReason::NotReported,
            ),
            inventory_observation,
            windows,
            vec![WindowCloseObservation::new(
                binding,
                CloseObservationGeneration::new(close_generation),
                Authority::Known(state),
                CloseEffectAcknowledgement::known(acknowledged_effect),
            )],
            support::unknown_work_area_observation(self.snapshot_generation),
        )
        .expect("typed close snapshot must be canonical");
        let expected_epoch = self.engine.version().epoch();
        let provider = self.presentation_host.platform_provider();
        self.submit(EngineInput::PublishPlatformSnapshot {
            provider,
            expected_epoch,
            snapshot,
        })
    }

    fn open_native_close_edge(&mut self, binding: ViewportBinding) -> NativeCloseEdge {
        self.publish_close(binding, 1, WindowCloseState::LiveClear, None);
        let transition = self.publish_close(binding, 2, WindowCloseState::LiveRequested, None);
        Self::only_native_close_edge(&transition)
    }

    fn open_native_close_edge_with_windows(
        &mut self,
        binding: ViewportBinding,
        windows: Vec<ObservedWindow>,
    ) -> NativeCloseEdge {
        self.publish_close_with_windows(
            binding,
            1,
            WindowCloseState::LiveClear,
            None,
            windows.clone(),
        );
        let transition = self.publish_close_with_windows(
            binding,
            2,
            WindowCloseState::LiveRequested,
            None,
            windows,
        );
        Self::only_native_close_edge(&transition)
    }

    fn only_native_close_edge(transition: &EngineTransition) -> NativeCloseEdge {
        let InputOutcome::PlatformSnapshotPublished {
            native_close_edges, ..
        } = only_outcome(&transition)
        else {
            panic!("live native close request did not publish a platform transition");
        };
        let [edge] = native_close_edges.as_slice() else {
            panic!("expected exactly one native close edge, got {native_close_edges:?}");
        };
        *edge
    }

    fn request_surface_close(
        &mut self,
        edge: NativeCloseEdge,
        request: SurfaceCloseRequest,
    ) -> (ClosePlan, EffectId) {
        let (plan, mut transition) = self.begin_surface_close(edge, request);
        for item in plan.items() {
            transition = self.submit(EngineInput::ResolveClose {
                request: plan.request(),
                token: item.token(),
                decision: CloseDecision::Allow,
            });
        }
        let effect =
            native_close_effect(&transition, edge.binding(), NativeCloseResolution::Accept);
        (plan, effect)
    }

    fn begin_surface_close(
        &mut self,
        edge: NativeCloseEdge,
        request: SurfaceCloseRequest,
    ) -> (ClosePlan, EngineTransition) {
        let expected = self.engine.version();
        let transition = self.submit(EngineInput::RequestSurfaceClose {
            expected,
            edge,
            request: request.clone(),
        });
        let plan = match only_outcome(&transition) {
            InputOutcome::SurfaceCloseRequested {
                edge: actual_edge,
                request: actual_request,
                plan,
                ..
            } if *actual_edge == edge && actual_request == &request => plan.clone(),
            outcome => panic!("surface close request was rejected: {outcome:?}"),
        };
        (plan, transition)
    }

    fn cancel_surface_close(&mut self, edge: NativeCloseEdge) -> (ClosePlan, EffectId) {
        let expected = self.engine.version();
        let transition = self.submit(EngineInput::CancelSurfaceClose { expected, edge });
        let plan = match only_outcome(&transition) {
            InputOutcome::SurfaceCloseCancellationRequested {
                edge: actual_edge,
                plan,
                ..
            } if *actual_edge == edge => plan.clone(),
            outcome => panic!("surface close cancellation was rejected: {outcome:?}"),
        };
        let effect =
            native_close_effect(&transition, edge.binding(), NativeCloseResolution::Cancel);
        (plan, effect)
    }
}

fn close_policy() -> DockPolicy {
    let mut policy = DockPolicy::default();
    policy.set_allow_native_surfaces(true);
    policy
}

fn close_capabilities() -> PlatformCapabilities {
    close_capabilities_with_cancellation(PlatformCapability::Supported)
}

fn close_capabilities_with_cancellation(
    close_cancellation: PlatformCapability,
) -> PlatformCapabilities {
    let mut capabilities = PlatformCapabilities::default();
    capabilities.set_native_window_lifecycle(PlatformCapability::Supported);
    capabilities.set_authoritative_inventory(PlatformCapability::Supported);
    capabilities.set_close_cancellation(close_cancellation);
    capabilities
}

fn logical_rect(x: f64, y: f64, width: f64, height: f64) -> LogicalRect {
    LogicalRect::new(x, y, width, height).expect("fixture logical rectangle must be valid")
}

fn live_window(binding: ViewportBinding) -> ObservedWindow {
    ObservedWindow::new(binding)
        .with_coordinate_observation(WindowCoordinateObservation::new(
            binding,
            CoordinateObservationGeneration::new(1),
            Authority::Known(
                PhysicalRect::new(0.0, 0.0, 1_024.0, 768.0)
                    .expect("fixture content bounds must be valid"),
            ),
            Authority::Known(
                PhysicalRect::new(-8.0, -30.0, 1_040.0, 806.0)
                    .expect("fixture outer bounds must be valid"),
            ),
            Authority::Known(ScaleFactor::new(1.0).expect("fixture scale factor must be valid")),
            Authority::Known(ScaleFactor::new(1.0).expect("fixture scale factor must be valid")),
        ))
        .with_input_state(Authority::Known(WindowInputState::ReceivesInput))
        .with_presentation_observation(WindowPresentationObservation::new(
            binding,
            PresentationObservationGeneration::new(1),
            Authority::Known(WindowPresentationState::Visible),
            PresentationEffectAcknowledgement::known(None),
        ))
}

fn retain_workspace() -> Workspace {
    let mut builder = Workspace::builder();
    let source_tabs = builder.insert_node(Node::tabs([ItemId::new(1), ItemId::new(2)]));
    builder.set_root(SOURCE_ROOT, RootRecord::new(source_tabs));
    builder.set_surface(SOURCE, SurfacePresentation::with_main(SOURCE_ROOT));
    builder.build().expect("retained workspace must be valid")
}

fn rehome_workspace() -> Workspace {
    let mut builder = Workspace::builder();
    let source_main = builder.insert_node(Node::tabs([ItemId::new(1), ItemId::new(2)]));
    let source_contained = builder.insert_node(Node::tabs([ItemId::new(3)]));
    let target_main = builder.insert_node(Node::tabs([ItemId::new(100)]));

    builder.set_root(SOURCE_ROOT, RootRecord::new(source_main));
    builder.set_root(SOURCE_CONTAINED_ROOT, RootRecord::new(source_contained));
    builder.set_root(TARGET_ROOT, RootRecord::new(target_main));
    builder.set_surface(SOURCE, SurfacePresentation::with_main(SOURCE_ROOT));
    builder.set_surface(TARGET, SurfacePresentation::with_main(TARGET_ROOT));
    builder.set_contained_floating(
        SOURCE_CONTAINED,
        ContainedFloating::new(
            SOURCE_CONTAINED_ROOT,
            logical_rect(24.0, 36.0, 360.0, 220.0),
        ),
    );
    builder
        .attach_contained(SOURCE, SOURCE_CONTAINED)
        .expect("source surface must exist");
    builder.build().expect("rehome workspace must be valid")
}

fn complete_rehome_request() -> SurfaceCloseRequest {
    SurfaceCloseRequest::RehomeAll {
        target: SurfaceRehomeTarget::new(
            TARGET,
            Some(SurfaceMainRehomeTarget::Contained {
                floating: REHOMED_MAIN,
                rect: logical_rect(42.0, 54.0, 520.0, 300.0),
                position: ContainedPosition::Front,
            }),
            vec![SurfaceContainedRehomeTarget::new(
                SOURCE_CONTAINED,
                logical_rect(84.0, 96.0, 300.0, 180.0),
            )],
        ),
    }
}

fn only_outcome(transition: &EngineTransition) -> &InputOutcome {
    let [reduced] = transition.reduced_inputs() else {
        panic!(
            "expected exactly one reduced input, got {:?}",
            transition.reduced_inputs()
        );
    };
    reduced.outcome()
}

fn native_close_effect(
    transition: &EngineTransition,
    binding: ViewportBinding,
    resolution: NativeCloseResolution,
) -> EffectId {
    let effects = transition
        .platform_effects()
        .iter()
        .filter(|effect| {
            matches!(
                effect.effect(),
                PlatformEffect::ResolveNativeClose {
                    edge: actual_edge,
                    resolution: actual_resolution,
                    ..
                } if actual_edge.binding() == binding && *actual_resolution == resolution
            )
        })
        .collect::<Vec<_>>();
    let [effect] = effects.as_slice() else {
        panic!(
            "expected exactly one {resolution:?} native close effect for {binding:?}, got {effects:?}"
        );
    };
    effect.id()
}

fn unavailable_close_cancellation() -> [PlatformCapability; 2] {
    [
        PlatformCapability::unknown(
            PlatformRequirement::CloseCancellation,
            PlatformCapabilityReason::NotReported,
        ),
        PlatformCapability::unsupported(
            PlatformRequirement::CloseCancellation,
            PlatformCapabilityReason::BackendUnsupported,
        ),
    ]
}

#[test]
fn zero_decision_rehome_accepts_without_close_cancellation() {
    for close_cancellation in unavailable_close_cancellation() {
        let capabilities = close_capabilities_with_cancellation(close_cancellation);
        let mut fixture =
            Harness::with_policy_and_capabilities(rehome_workspace(), close_policy(), capabilities);
        let before = fixture.engine.workspace().clone();
        let binding = fixture.register_source_viewport();
        let edge = fixture.open_native_close_edge(binding);
        let request = complete_rehome_request();
        let expected = fixture.engine.version();

        let transition = fixture.submit(EngineInput::RequestSurfaceClose {
            expected,
            edge,
            request: request.clone(),
        });

        let plan = match only_outcome(&transition) {
            InputOutcome::SurfaceCloseRequested {
                edge: actual_edge,
                request: actual_request,
                plan,
                ..
            } if *actual_edge == edge && actual_request == &request => plan,
            outcome => {
                panic!("zero-decision rehome was rejected for {close_cancellation:?}: {outcome:?}")
            }
        };
        assert!(plan.items().is_empty());
        assert_eq!(plan.phase(), ClosePlanPhase::Approved);
        let _ = native_close_effect(&transition, binding, NativeCloseResolution::Accept);
        assert!(transition.platform_effects().iter().all(|effect| {
            !matches!(
                effect.effect(),
                PlatformEffect::ResolveNativeClose {
                    resolution: NativeCloseResolution::Cancel,
                    ..
                }
            )
        }));
        assert_eq!(fixture.engine.workspace(), &before);
    }
}

#[test]
fn decision_bearing_close_still_requires_close_cancellation() {
    let capabilities = close_capabilities_with_cancellation(unavailable_close_cancellation()[1]);
    let mut fixture =
        Harness::with_policy_and_capabilities(retain_workspace(), close_policy(), capabilities);
    let binding = fixture.register_source_viewport();
    let edge = fixture.open_native_close_edge(binding);
    let expected = fixture.engine.version();

    let transition = fixture.submit(EngineInput::RequestSurfaceClose {
        expected,
        edge,
        request: SurfaceCloseRequest::RetainLayout,
    });

    assert!(matches!(
        only_outcome(&transition),
        InputOutcome::SurfaceCloseRejected {
            reason: SurfaceCloseRequestRejection::CancellationUnsupported,
            ..
        }
    ));
    assert!(transition.platform_effects().is_empty());
}

#[test]
fn rejected_rehome_cannot_use_the_accept_only_capability_exception() {
    let mut policy = close_policy();
    let mut target_rule = DockSurfaceRule::new();
    target_rule.set_allowed_target_payloads([DockPayloadKind::Item]);
    policy.set_surface_rule(TARGET, target_rule);
    let capabilities = close_capabilities_with_cancellation(unavailable_close_cancellation()[1]);
    let mut fixture =
        Harness::with_policy_and_capabilities(rehome_workspace(), policy, capabilities);
    let binding = fixture.register_source_viewport();
    let edge = fixture.open_native_close_edge(binding);
    let expected = fixture.engine.version();

    let transition = fixture.submit(EngineInput::RequestSurfaceClose {
        expected,
        edge,
        request: complete_rehome_request(),
    });

    assert!(matches!(
        only_outcome(&transition),
        InputOutcome::SurfaceCloseRejected {
            reason: SurfaceCloseRequestRejection::CancellationUnsupported,
            ..
        }
    ));
    assert!(transition.platform_effects().is_empty());
}

fn assert_rehome_policy_rejected(policy: DockPolicy, expected_reason: PolicyRejection) {
    let mut fixture = Harness::with_policy(rehome_workspace(), policy);
    let before_workspace = fixture.engine.workspace().clone();
    let binding = fixture.register_source_viewport();
    let before_binding = fixture
        .engine
        .viewport()
        .viewport(SOURCE)
        .map(|record| record.binding());
    assert_eq!(before_binding, Some(binding));
    let edge = fixture.open_native_close_edge(binding);
    let request = complete_rehome_request();
    let expected = fixture.engine.version();

    let rejected = fixture.submit(EngineInput::RequestSurfaceClose {
        expected,
        edge,
        request: request.clone(),
    });

    assert!(matches!(
        only_outcome(&rejected),
        InputOutcome::SurfaceCloseRejected {
            edge: actual_edge,
            request: actual_request,
            reason: SurfaceCloseRequestRejection::RehomePolicyRejected(actual_reason),
            ..
        } if *actual_edge == edge
            && actual_request == &request
            && actual_reason == &expected_reason
    ));
    assert!(
        !rejected.platform_effects().iter().any(|effect| {
            matches!(
                effect.effect(),
                PlatformEffect::ResolveNativeClose {
                    edge: actual_edge,
                    resolution: NativeCloseResolution::Accept,
                    ..
                } if *actual_edge == edge
            )
        }),
        "a rejected rehome program must never authorize native destruction"
    );
    assert!(
        rejected.platform_effects().iter().any(|effect| {
            matches!(
                effect.effect(),
                PlatformEffect::ResolveNativeClose {
                    edge: actual_edge,
                    resolution: NativeCloseResolution::Cancel,
                    ..
                } if *actual_edge == edge
            )
        }),
        "an invalid rehome program must veto the exact native close edge"
    );
    assert_eq!(fixture.engine.workspace(), &before_workspace);
    assert_eq!(
        fixture
            .engine
            .viewport()
            .viewport(SOURCE)
            .map(|record| record.binding()),
        before_binding,
        "a rehome policy rejection must preserve the exact native binding"
    );
}

#[test]
fn retain_layout_requires_every_frozen_pane_decision_before_accepting() {
    let mut fixture = Harness::new(retain_workspace());
    let binding = fixture.register_source_viewport();
    let edge = fixture.open_native_close_edge(binding);
    let (plan, request) = fixture.begin_surface_close(edge, SurfaceCloseRequest::RetainLayout);

    assert_eq!(plan.phase(), ClosePlanPhase::Requested);
    assert_eq!(
        plan.items()
            .iter()
            .map(|item| item.item())
            .collect::<Vec<_>>(),
        vec![ItemId::new(1), ItemId::new(2)],
        "retain-layout freezes every pane in normative roster order"
    );
    assert!(request.platform_effects().is_empty());

    let first = plan.items()[0];
    let first_allow = fixture.submit(EngineInput::ResolveClose {
        request: plan.request(),
        token: first.token(),
        decision: CloseDecision::Allow,
    });
    assert!(first_allow.platform_effects().is_empty());
    assert_eq!(
        fixture
            .engine
            .close_plan(plan.request())
            .and_then(|current| current.items().first())
            .map(|item| item.state()),
        Some(CloseItemDecisionState::Allowed),
    );

    let second = plan.items()[1];
    let second_allow = fixture.submit(EngineInput::ResolveClose {
        request: plan.request(),
        token: second.token(),
        decision: CloseDecision::Allow,
    });
    let _ = native_close_effect(&second_allow, binding, NativeCloseResolution::Accept);
    assert_eq!(
        fixture
            .engine
            .close_plan(plan.request())
            .map(|current| current.phase()),
        Some(ClosePlanPhase::EffectEmitted),
    );
}

#[test]
fn retain_layout_deferred_and_veto_decisions_hold_or_cancel_the_native_edge() {
    let mut deferred_policy = close_policy();
    deferred_policy.set_close_capability(CloseCapability::DeferredAllowed);
    let mut deferred = Harness::with_policy(retain_workspace(), deferred_policy);
    let binding = deferred.register_source_viewport();
    let edge = deferred.open_native_close_edge(binding);
    let (plan, _) = deferred.begin_surface_close(edge, SurfaceCloseRequest::RetainLayout);
    let first = plan.items()[0];

    let deferred_transition = deferred.submit(EngineInput::ResolveClose {
        request: plan.request(),
        token: first.token(),
        decision: CloseDecision::Deferred,
    });
    assert!(deferred_transition.platform_effects().is_empty());
    let continuation = match deferred
        .engine
        .close_plan(plan.request())
        .expect("deferred surface close plan remains inspectable")
        .items()[0]
        .state()
    {
        CloseItemDecisionState::Deferred { continuation } => continuation,
        state => panic!("first retain item must be deferred, got {state:?}"),
    };
    let duplicate_initial = deferred.submit(EngineInput::ResolveClose {
        request: plan.request(),
        token: first.token(),
        decision: CloseDecision::Deferred,
    });
    assert!(
        duplicate_initial.platform_effects().is_empty(),
        "a consumed initial token must never mint another continuation"
    );
    assert_eq!(
        deferred
            .engine
            .close_plan(plan.request())
            .expect("duplicate input must preserve the plan")
            .items()[0]
            .state(),
        CloseItemDecisionState::Deferred { continuation },
    );
    let second = plan.items()[1];
    let second_allow = deferred.submit(EngineInput::ResolveClose {
        request: plan.request(),
        token: second.token(),
        decision: CloseDecision::Allow,
    });
    assert!(second_allow.platform_effects().is_empty());
    let deferred_allow = deferred.submit(EngineInput::ContinueDeferredClose {
        request: plan.request(),
        token: continuation,
        decision: DeferredCloseDecision::Allow,
    });
    let _ = native_close_effect(&deferred_allow, binding, NativeCloseResolution::Accept);
    let duplicate_deferred = deferred.submit(EngineInput::ContinueDeferredClose {
        request: plan.request(),
        token: continuation,
        decision: DeferredCloseDecision::Allow,
    });
    assert!(
        duplicate_deferred.platform_effects().is_empty(),
        "a consumed deferred token must not emit a second native effect"
    );

    let mut vetoed = Harness::new(retain_workspace());
    let binding = vetoed.register_source_viewport();
    let edge = vetoed.open_native_close_edge(binding);
    let (plan, _) = vetoed.begin_surface_close(edge, SurfaceCloseRequest::RetainLayout);
    let cancel = vetoed.submit(EngineInput::ResolveClose {
        request: plan.request(),
        token: plan.items()[0].token(),
        decision: CloseDecision::Veto,
    });
    let _ = native_close_effect(&cancel, binding, NativeCloseResolution::Cancel);
    assert_eq!(
        vetoed
            .engine
            .close_plan(plan.request())
            .map(|current| current.phase()),
        Some(ClosePlanPhase::CancelRequested),
    );
}

#[test]
fn retain_layout_with_a_disabled_pane_immediately_cancels_the_native_close_edge() {
    let mut policy = close_policy();
    policy.set_close_capability(CloseCapability::Disabled);
    let mut fixture = Harness::with_policy(retain_workspace(), policy);
    let before = fixture.engine.workspace().clone();
    let binding = fixture.register_source_viewport();
    let edge = fixture.open_native_close_edge(binding);
    let expected = fixture.engine.version();
    let rejected = fixture.submit(EngineInput::RequestSurfaceClose {
        expected,
        edge,
        request: SurfaceCloseRequest::RetainLayout,
    });

    assert!(matches!(
        only_outcome(&rejected),
        InputOutcome::SurfaceCloseRejected {
            reason: dockspace::transition::SurfaceCloseRequestRejection::PaneCloseDisabled { item },
            ..
        } if *item == ItemId::new(1)
    ));
    let cancellation = native_close_effect(&rejected, binding, NativeCloseResolution::Cancel);

    fixture.publish_close(binding, 3, WindowCloseState::LiveClear, Some(cancellation));
    assert_eq!(fixture.engine.workspace(), &before);
    assert!(fixture.engine.viewport().viewport(SOURCE).is_some());
}

#[test]
fn retain_layout_applies_only_after_exact_destroyed_effect_and_keeps_graph() {
    let mut fixture = Harness::new(retain_workspace());
    let before = fixture.engine.workspace().clone();
    let binding = fixture.register_source_viewport();
    let edge = fixture.open_native_close_edge(binding);
    let (plan, effect) = fixture.request_surface_close(edge, SurfaceCloseRequest::RetainLayout);

    assert_eq!(plan.phase(), ClosePlanPhase::Requested);
    assert_eq!(
        fixture
            .engine
            .close_plan(plan.request())
            .map(|current| current.phase()),
        Some(ClosePlanPhase::EffectEmitted),
        "the effect is emitted only after the complete retain request is frozen"
    );

    fixture.publish_close(binding, 3, WindowCloseState::Destroyed, Some(effect));

    assert_eq!(fixture.engine.workspace(), &before);
    assert!(fixture.engine.workspace().surface(SOURCE).is_some());
    assert!(fixture.engine.viewport().viewport(SOURCE).is_none());
    assert_eq!(
        fixture
            .engine
            .close_plan(plan.request())
            .map(|current| current.phase()),
        Some(ClosePlanPhase::Applied),
    );
}

#[test]
fn destroyed_with_a_wrong_effect_acknowledgement_never_applies_rehome_payload() {
    let mut fixture = Harness::new(rehome_workspace());
    let before = fixture.engine.workspace().clone();
    let binding = fixture.register_source_viewport();
    let edge = fixture.open_native_close_edge(binding);
    let (plan, accepted_effect) = fixture.request_surface_close(edge, complete_rehome_request());
    let wrong_effect = EffectId::new(accepted_effect.get().checked_add(1).unwrap_or(0));
    assert_ne!(wrong_effect, accepted_effect);

    fixture.publish_close(binding, 3, WindowCloseState::Destroyed, Some(wrong_effect));

    assert_eq!(fixture.engine.workspace(), &before);
    assert!(fixture.engine.workspace().surface(SOURCE).is_some());
    assert!(
        fixture
            .engine
            .workspace()
            .contained_floating(REHOMED_MAIN)
            .is_none()
    );
    assert_eq!(
        fixture
            .engine
            .close_plan(plan.request())
            .map(|current| current.phase()),
        Some(ClosePlanPhase::ExternallyDestroyedUnproved),
        "a foreign close-lane acknowledgement must not authorize a frozen payload"
    );
}

#[test]
fn cancellation_requires_the_exact_compensating_effect_frontier() {
    let mut rejected = Harness::new(retain_workspace());
    let binding = rejected.register_source_viewport();
    let edge = rejected.open_native_close_edge(binding);
    let (plan, accepted_effect) =
        rejected.request_surface_close(edge, SurfaceCloseRequest::RetainLayout);
    let _ = rejected.cancel_surface_close(edge);

    rejected.publish_close(
        binding,
        3,
        WindowCloseState::LiveClear,
        Some(accepted_effect),
    );
    let rejected_plan = rejected
        .engine
        .close_plan(plan.request())
        .expect("the rejected cancellation plan remains inspectable");
    assert_eq!(rejected_plan.phase(), ClosePlanPhase::Indeterminate);
    assert_eq!(
        rejected_plan.cancellation_state(),
        CloseCancellationState::Indeterminate,
        "the older destructive effect is not a cancellation proof, so the old edge remains auditable"
    );

    let mut accepted = Harness::new(retain_workspace());
    let binding = accepted.register_source_viewport();
    let edge = accepted.open_native_close_edge(binding);
    let (plan, _) = accepted.request_surface_close(edge, SurfaceCloseRequest::RetainLayout);
    let (_, cancellation_effect) = accepted.cancel_surface_close(edge);
    accepted.publish_close(
        binding,
        3,
        WindowCloseState::LiveClear,
        Some(cancellation_effect),
    );

    assert_eq!(
        accepted
            .engine
            .close_plan(plan.request())
            .map(|current| current.phase()),
        Some(ClosePlanPhase::Cancelled),
    );
    assert_ne!(
        cancellation_effect, accepted_effect,
        "the compensating effect has an independent exact identity"
    );
}

#[test]
fn superseded_close_edge_cannot_block_or_absorb_the_next_edge() {
    let mut fixture = Harness::new(retain_workspace());
    let binding = fixture.register_source_viewport();
    let first_edge = fixture.open_native_close_edge(binding);
    let (first_plan, first_effect) =
        fixture.request_surface_close(first_edge, SurfaceCloseRequest::RetainLayout);

    fixture.publish_close(binding, 3, WindowCloseState::LiveClear, None);
    assert_eq!(
        fixture
            .engine
            .close_plan(first_plan.request())
            .map(|plan| plan.phase()),
        Some(ClosePlanPhase::Indeterminate),
        "a clear without an exact effect frontier keeps E1 auditable"
    );

    let second_edge_transition =
        fixture.publish_close(binding, 4, WindowCloseState::LiveRequested, None);
    let InputOutcome::PlatformSnapshotPublished {
        native_close_edges, ..
    } = only_outcome(&second_edge_transition)
    else {
        panic!("the reissued native close edge was not published");
    };
    let [second_edge] = native_close_edges.as_slice() else {
        panic!("expected one reissued edge, got {native_close_edges:?}");
    };
    assert_ne!(*second_edge, first_edge);

    let (second_plan, second_effect) =
        fixture.request_surface_close(*second_edge, SurfaceCloseRequest::RetainLayout);
    assert_ne!(second_plan.request(), first_plan.request());
    assert_eq!(
        fixture
            .engine
            .close_plan(second_plan.request())
            .map(|plan| plan.phase()),
        Some(ClosePlanPhase::EffectEmitted)
    );

    let expected_epoch = fixture.engine.version().epoch();
    let provider = fixture.presentation_host.platform_provider();
    fixture.submit(EngineInput::ReportPlatformEffect {
        provider,
        expected_epoch,
        result: EffectResult::new(
            first_effect,
            expected_epoch,
            EffectDispatchResult::DispatchFailed(DispatchFailureReason::ProviderStopped),
        ),
    });
    assert_eq!(
        fixture
            .engine
            .close_plan(first_plan.request())
            .map(|plan| plan.phase()),
        Some(ClosePlanPhase::Indeterminate),
        "a late E1 report cannot mutate E2"
    );
    assert_eq!(
        fixture
            .engine
            .close_plan(second_plan.request())
            .map(|plan| plan.phase()),
        Some(ClosePlanPhase::EffectEmitted)
    );

    fixture.publish_close(binding, 5, WindowCloseState::Destroyed, Some(second_effect));
    assert_eq!(
        fixture
            .engine
            .close_plan(second_plan.request())
            .map(|plan| plan.phase()),
        Some(ClosePlanPhase::Applied)
    );
    assert_eq!(
        fixture
            .engine
            .close_plan(first_plan.request())
            .map(|plan| plan.phase()),
        Some(ClosePlanPhase::ExternallyDestroyedUnproved)
    );
}

#[test]
fn late_effect_report_after_destroyed_settlement_is_ledger_only() {
    let mut fixture = Harness::new(retain_workspace());
    let binding = fixture.register_source_viewport();
    let edge = fixture.open_native_close_edge(binding);
    let (plan, effect) = fixture.request_surface_close(edge, SurfaceCloseRequest::RetainLayout);

    fixture.publish_close(binding, 3, WindowCloseState::Destroyed, Some(effect));
    assert_eq!(
        fixture
            .engine
            .close_plan(plan.request())
            .map(|current| current.phase()),
        Some(ClosePlanPhase::Applied)
    );

    let expected_epoch = fixture.engine.version().epoch();
    let provider = fixture.presentation_host.platform_provider();
    let late = fixture.submit(EngineInput::ReportPlatformEffect {
        provider,
        expected_epoch,
        result: EffectResult::new(
            effect,
            expected_epoch,
            EffectDispatchResult::DispatchFailed(DispatchFailureReason::ProviderStopped),
        ),
    });
    assert!(matches!(
        only_outcome(&late),
        InputOutcome::PlatformEffectReported {
            transition: dockspace::effect::EffectTransition::RetiredTerminal,
            ..
        }
    ));
    assert_eq!(
        fixture.engine.lookup_close_plan(plan.request()),
        ClosePlanLookup::RetiredTerminal,
    );
    assert_eq!(
        fixture.engine.viewport().effects().lookup(effect),
        EffectRecordLookup::RetiredTerminal,
    );
}

#[test]
fn late_cancellation_result_after_terminal_compaction_is_inert() {
    let mut fixture = Harness::new(retain_workspace());
    let before = fixture.engine.workspace().clone();
    let binding = fixture.register_source_viewport();
    let edge = fixture.open_native_close_edge(binding);
    let (plan, _) = fixture.request_surface_close(edge, SurfaceCloseRequest::RetainLayout);
    let (_, cancellation_effect) = fixture.cancel_surface_close(edge);

    fixture.publish_close(
        binding,
        3,
        WindowCloseState::LiveClear,
        Some(cancellation_effect),
    );
    assert_eq!(
        fixture
            .engine
            .close_plan(plan.request())
            .map(|current| current.phase()),
        Some(ClosePlanPhase::Cancelled),
    );

    let _ = fixture.submit(EngineInput::ValidateWorkspace);
    assert_eq!(
        fixture.engine.lookup_close_plan(plan.request()),
        ClosePlanLookup::RetiredTerminal,
    );
    assert_eq!(
        fixture
            .engine
            .viewport()
            .effects()
            .lookup(cancellation_effect),
        EffectRecordLookup::RetiredTerminal,
    );

    let expected_epoch = fixture.engine.version().epoch();
    let provider = fixture.presentation_host.platform_provider();
    let late = fixture.submit(EngineInput::ReportPlatformEffect {
        provider,
        expected_epoch,
        result: EffectResult::new(
            cancellation_effect,
            expected_epoch,
            EffectDispatchResult::DispatchFailed(DispatchFailureReason::ProviderStopped),
        ),
    });
    assert!(matches!(
        only_outcome(&late),
        InputOutcome::PlatformEffectReported {
            transition: dockspace::effect::EffectTransition::RetiredTerminal,
            ..
        }
    ));
    assert_eq!(fixture.engine.workspace(), &before);
    assert!(fixture.engine.viewport().viewport(SOURCE).is_some());
    assert_eq!(
        fixture.engine.lookup_close_plan(plan.request()),
        ClosePlanLookup::RetiredTerminal,
    );
}

#[test]
fn rehome_all_preserves_no_undocking_policy_rejection_at_native_boundary() {
    let mut policy = close_policy();
    let mut item_rule = DockItemRule::new();
    item_rule.set_allow_undocking(false);
    policy.set_item_rule(ItemId::new(1), item_rule);

    assert_rehome_policy_rejected(
        policy,
        PolicyRejection::ItemUndockingDisabled {
            item: ItemId::new(1),
        },
    );
}

#[test]
fn rehome_all_preserves_target_contained_presentation_rejection_at_native_boundary() {
    let mut policy = close_policy();
    let mut target_rule = DockSurfaceRule::new();
    target_rule.set_allowed_presentations([DockPresentationMode::Tiled]);
    policy.set_surface_rule(TARGET, target_rule);

    assert_rehome_policy_rejected(
        policy,
        PolicyRejection::SurfacePresentationModeRejected {
            surface: TARGET,
            mode: DockPresentationMode::Contained,
        },
    );
}

#[test]
fn rehome_all_preserves_target_payload_rejection_at_native_boundary() {
    let mut policy = close_policy();
    let mut target_rule = DockSurfaceRule::new();
    target_rule.set_allowed_target_payloads([DockPayloadKind::Item]);
    policy.set_surface_rule(TARGET, target_rule);

    assert_rehome_policy_rejected(
        policy,
        PolicyRejection::SurfaceTargetPayloadRejected {
            surface: TARGET,
            payload: DockPayloadKind::Root,
        },
    );
}

#[test]
fn close_content_veto_cancels_native_close_without_mutating_the_surface_roster() {
    let mut fixture = Harness::new(rehome_workspace());
    let before_workspace = fixture.engine.workspace().clone();
    let binding = fixture.register_source_viewport();
    let edge = fixture.open_native_close_edge(binding);
    let (plan, _) = fixture.begin_surface_close(edge, SurfaceCloseRequest::CloseContent);

    assert_eq!(
        plan.items()
            .iter()
            .map(|item| item.item())
            .collect::<Vec<_>>(),
        vec![ItemId::new(1), ItemId::new(2), ItemId::new(3)],
        "CloseContent must freeze the complete main-plus-contained item roster"
    );

    let veto = fixture.submit(EngineInput::ResolveClose {
        request: plan.request(),
        token: plan.items()[0].token(),
        decision: CloseDecision::Veto,
    });
    let cancellation = native_close_effect(&veto, binding, NativeCloseResolution::Cancel);
    assert_eq!(fixture.engine.workspace(), &before_workspace);

    fixture.publish_close(binding, 3, WindowCloseState::LiveClear, Some(cancellation));

    assert_eq!(fixture.engine.workspace(), &before_workspace);
    assert_eq!(
        fixture
            .engine
            .viewport()
            .viewport(SOURCE)
            .map(|record| record.binding()),
        Some(binding),
        "the cancelled close keeps the native source binding live"
    );
    assert_eq!(
        fixture
            .engine
            .close_plan(plan.request())
            .map(|current| current.phase()),
        Some(ClosePlanPhase::Cancelled),
    );
}

#[test]
fn close_content_removes_complete_surface_roster_only_after_exact_destroyed_proof() {
    let mut fixture = Harness::new(rehome_workspace());
    let target_surface = fixture.engine.workspace().surface(TARGET).cloned();
    let target_root = fixture.engine.workspace().root(TARGET_ROOT).cloned();
    let target_items = fixture
        .engine
        .workspace()
        .item_multiset()
        .into_iter()
        .filter(|(item, _)| *item == ItemId::new(100))
        .collect::<std::collections::BTreeMap<_, _>>();
    let binding = fixture.register_source_viewport();
    let edge = fixture.open_native_close_edge(binding);
    let (plan, effect) = fixture.request_surface_close(edge, SurfaceCloseRequest::CloseContent);

    assert_eq!(
        fixture
            .engine
            .close_plan(plan.request())
            .map(|current| current.phase()),
        Some(ClosePlanPhase::EffectEmitted),
        "all frozen item votes must resolve before CloseContent accepts native destruction"
    );

    fixture.publish_close(binding, 3, WindowCloseState::Destroyed, Some(effect));

    let workspace = fixture.engine.workspace();
    assert!(workspace.surface(SOURCE).is_none());
    assert!(workspace.root(SOURCE_ROOT).is_none());
    assert!(workspace.root(SOURCE_CONTAINED_ROOT).is_none());
    assert!(workspace.contained_floating(SOURCE_CONTAINED).is_none());
    assert_eq!(workspace.surface(TARGET), target_surface.as_ref());
    assert_eq!(workspace.root(TARGET_ROOT), target_root.as_ref());
    assert_eq!(workspace.item_multiset(), target_items);
    assert!(fixture.engine.viewport().viewport(SOURCE).is_none());
    assert_eq!(
        fixture
            .engine
            .close_plan(plan.request())
            .map(|current| current.phase()),
        Some(ClosePlanPhase::Applied),
    );
}

#[test]
fn rehome_all_moves_main_and_every_contained_root_in_one_destroyed_commit() {
    let mut fixture = Harness::new(rehome_workspace());
    let before_items = fixture.engine.workspace().item_multiset();
    let binding = fixture.register_source_viewport();
    let edge = fixture.open_native_close_edge(binding);
    let (plan, effect) = fixture.request_surface_close(edge, complete_rehome_request());

    fixture.publish_close(binding, 3, WindowCloseState::Destroyed, Some(effect));

    let workspace = fixture.engine.workspace();
    assert!(workspace.surface(SOURCE).is_none());
    let target = workspace
        .surface(TARGET)
        .expect("target surface remains present after atomic rehome");
    assert_eq!(target.main_root, Some(TARGET_ROOT));
    assert!(target.contained.contains(&REHOMED_MAIN));
    assert!(target.contained.contains(&SOURCE_CONTAINED));
    assert_eq!(
        workspace
            .contained_floating(REHOMED_MAIN)
            .expect("source main is converted into its requested contained presentation")
            .root,
        SOURCE_ROOT,
    );
    assert_eq!(
        workspace
            .contained_floating(SOURCE_CONTAINED)
            .expect("source contained root retains its stable presentation identity")
            .root,
        SOURCE_CONTAINED_ROOT,
    );
    assert_eq!(workspace.item_multiset(), before_items);
    assert_eq!(
        fixture
            .engine
            .close_plan(plan.request())
            .map(|current| current.phase()),
        Some(ClosePlanPhase::Applied),
    );
}

#[test]
fn rehome_close_with_no_source_focus_history_does_not_invent_explicit_none_focus() {
    let mut fixture = Harness::new(rehome_workspace());
    let source = fixture.register_source_viewport();
    let target = fixture.register_target_viewport();
    let edge = fixture.open_native_close_edge_with_windows(
        source,
        vec![live_window(source), live_window(target)],
    );
    let (_, effect) = fixture.request_surface_close(edge, complete_rehome_request());

    let destroyed = fixture.publish_close_with_windows(
        source,
        3,
        WindowCloseState::Destroyed,
        Some(effect),
        vec![live_window(target)],
    );

    assert!(
        fixture
            .engine
            .viewport_focus()
            .recorded_observe_only_activation()
            .is_none(),
        "NoHistory must not be collapsed into an explicit no-pane close-recovery activation"
    );
    assert!(
        destroyed.focus_delta().observe_only_activation().is_none(),
        "the public focus delta must not ask an adapter to enact an invented no-pane state"
    );
}

#[test]
fn rehome_close_preserves_an_explicit_no_pane_focus_observation() {
    let mut fixture = Harness::new(rehome_workspace());
    let source = fixture.register_source_viewport();
    let target = fixture.register_target_viewport();
    let edge = fixture.open_native_close_edge_with_windows(
        source,
        vec![live_window(source), live_window(target)],
    );
    let expected_epoch = fixture.engine.version().epoch();
    fixture.submit(EngineInput::PublishPaneFocusObservation {
        expected_epoch,
        observation: PaneFocusObservation::new(
            PaneFocusObservationGeneration::new(1),
            source,
            PanelFocus::None,
        ),
    });
    let (_, effect) = fixture.request_surface_close(edge, complete_rehome_request());

    let destroyed = fixture.publish_close_with_windows(
        source,
        3,
        WindowCloseState::Destroyed,
        Some(effect),
        vec![live_window(target)],
    );

    assert_eq!(
        fixture
            .engine
            .viewport_focus()
            .recorded_observe_only_activation()
            .and_then(|record| record.request().focus()),
        Some(PanelFocus::None),
        "an observed no-pane focus fact must remain distinguishable from NoHistory"
    );
    assert!(
        destroyed.focus_delta().observe_only_activation().is_some(),
        "the adapter must still receive the explicit no-pane recovery disposition"
    );
}
