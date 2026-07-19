//! Native provider boundary between egui/eframe and the headless viewport protocol.
//!
//! Providers report capabilities and observations independently. Dispatching an
//! effect never changes an observation or proves that the effect was applied;
//! only a later authoritative observation may establish that fact in the core.

use dockspace::effect::{EffectRequest, EffectResult};
use dockspace::platform::{
    ObservedWindow, ObservedWorkArea, PlatformCapabilities, PlatformSnapshot,
    PlatformSnapshotError, PointerObservation,
};

/// One complete provider observation without capability claims.
#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct PlatformObservations {
    windows: Vec<ObservedWindow>,
    pointers: Vec<PointerObservation>,
    work_areas: Vec<ObservedWorkArea>,
}

impl PlatformObservations {
    pub(crate) fn new(
        windows: Vec<ObservedWindow>,
        pointers: Vec<PointerObservation>,
        work_areas: Vec<ObservedWorkArea>,
    ) -> Self {
        Self {
            windows,
            pointers,
            work_areas,
        }
    }

    fn into_snapshot(
        self,
        capabilities: PlatformCapabilities,
    ) -> Result<PlatformSnapshot, PlatformSnapshotError> {
        PlatformSnapshot::new(capabilities, self.windows, self.pointers, self.work_areas)
    }
}

/// One atomic provider sample accepted as a single core snapshot candidate.
///
/// Capabilities remain semantically distinct from observations, but a provider
/// cannot update either half between calls. Generation identities begin when
/// `ViewportCoordinator` accepts the resulting [`PlatformSnapshot`]; keeping an
/// unrelated provider generation here would create an uncorrelated protocol.
#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct ProviderSample {
    capabilities: PlatformCapabilities,
    observations: PlatformObservations,
}

impl ProviderSample {
    pub(crate) const fn new(
        capabilities: PlatformCapabilities,
        observations: PlatformObservations,
    ) -> Self {
        Self {
            capabilities,
            observations,
        }
    }

    fn into_snapshot(self) -> Result<PlatformSnapshot, PlatformSnapshotError> {
        self.observations.into_snapshot(self.capabilities)
    }
}

/// Platform provider contract consumed by the native runtime adapter.
///
/// A provider owns only backend observation and dispatch plumbing. Docking,
/// viewport lifecycle, effect phases, and stale-result handling remain owned by
/// `dockspace`.
pub(crate) trait NativePlatformProvider {
    /// Atomically samples capabilities and complete pre-frame observations.
    fn sample(&mut self) -> ProviderSample;

    /// Submits one core-owned effect request to the backend.
    ///
    /// Successful submission produces no result because it does not prove that
    /// the requested platform state was applied. A provider queues only a
    /// negative or indeterminate [`EffectResult`] for later collection.
    fn dispatch_effect(&mut self, request: &EffectRequest);

    /// Drains correlated dispatch outcomes without claiming observed state.
    fn take_effect_results(&mut self) -> Vec<EffectResult>;
}

/// Thin protocol bridge around one provider implementation.
pub(crate) struct NativePlatformAdapter<P> {
    provider: P,
}

impl<P> NativePlatformAdapter<P>
where
    P: NativePlatformProvider,
{
    pub(crate) const fn new(provider: P) -> Self {
        Self { provider }
    }

    pub(crate) const fn provider(&self) -> &P {
        &self.provider
    }

    pub(crate) const fn provider_mut(&mut self) -> &mut P {
        &mut self.provider
    }

    /// Builds the exact headless snapshot accepted by `ViewportCoordinator`.
    pub(crate) fn snapshot(&mut self) -> Result<PlatformSnapshot, PlatformSnapshotError> {
        self.provider.sample().into_snapshot()
    }

    /// Dispatches immutable core requests without updating provider observations.
    pub(crate) fn dispatch_effects(&mut self, requests: &[EffectRequest]) {
        for request in requests {
            self.provider.dispatch_effect(request);
        }
    }

    /// Drains provider outcomes for explicit enqueueing into `DockEngine`.
    pub(crate) fn take_effect_results(&mut self) -> Vec<EffectResult> {
        self.provider.take_effect_results()
    }
}

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;

    use dockspace::command::MovePayload;
    use dockspace::coordinates::TearOffPlacementRequest;
    use dockspace::effect::{
        DispatchFailureReason, EffectDispatchResult, EffectId, EffectIndeterminateReason,
        EffectPhase, EffectRequest, EffectResult, EffectTransition, EffectUnsupportedReason,
        PlatformEffect,
    };
    use dockspace::engine::DockEngine;
    use dockspace::frame::{
        NativeCreateRequest, NativeCreateStatus, ViewportCloseDecision, ViewportClosePlan,
        ViewportCloseStatus,
    };
    use dockspace::geometry::{LogicalRect, LogicalSize, PhysicalPoint, PhysicalRect, ScaleFactor};
    use dockspace::graph::{Node, RootRecord, SurfacePresentation, Workspace};
    use dockspace::ids::{FloatingPresentationId, ItemId, NodeId, RootId, SurfaceId};
    use dockspace::intent::{
        Authority, AuthorityUnavailableReason, ContainedTearOffProposal, NativeTearOffProposal,
        PointerButton, PointerButtonState, PointerId, RendererIntent, TargetAuthority,
        TearOffRequest,
    };
    use dockspace::interaction::{
        DragSessionId, InteractionCancelReason, InteractionDelivery, InteractionOutcome,
        InteractionStatus, PreviewResolutionStatus,
    };
    use dockspace::platform::{
        ButtonObservation, ObservedWindow, ObservedWorkArea, PlatformCapabilities,
        PlatformCapability, PlatformCapabilityReason, PlatformRequirement, PointerObservation,
        PointerWindow, WindowInputState, WindowPresentationState,
    };
    use dockspace::policy::DockPolicy;
    use dockspace::scene::{BuildingScene, ReadySurfaceScene};
    use dockspace::transition::{EngineTransition, InputOutcome};
    use dockspace::viewport::{ViewportRole, WindowToken, WorkAreaToken};

    use super::{
        NativePlatformAdapter, NativePlatformProvider, PlatformObservations, ProviderSample,
    };

    const SOURCE_ROOT: RootId = RootId::new(1);
    const TARGET_ROOT: RootId = RootId::new(2);
    const NATIVE_ROOT: RootId = RootId::new(10);
    const SOURCE_SURFACE: SurfaceId = SurfaceId::new(1);
    const TARGET_SURFACE: SurfaceId = SurfaceId::new(2);
    const NATIVE_SURFACE: SurfaceId = SurfaceId::new(10);
    const SOURCE_TOKEN: WindowToken = WindowToken::new(10);
    const TARGET_TOKEN: WindowToken = WindowToken::new(20);
    const WORK_AREA: WorkAreaToken = WorkAreaToken::new(30);
    const POINTER: PointerId = PointerId::new(1);
    const SOURCE_ITEM: ItemId = ItemId::new(1);
    const TARGET_RECOVERY: FloatingPresentationId = FloatingPresentationId::new(20);
    const NATIVE_RECOVERY: FloatingPresentationId = FloatingPresentationId::new(30);
    const CLOSE_RECOVERY: FloatingPresentationId = FloatingPresentationId::new(40);

    #[derive(Debug, Clone, Copy)]
    enum ConformanceDispatchOutcome {
        Submitted,
        Result(EffectDispatchResult),
    }

    trait ProviderConformanceDriver: NativePlatformProvider {
        fn set_capabilities(&mut self, capabilities: PlatformCapabilities);

        fn set_observations(&mut self, observations: PlatformObservations);

        fn queue_dispatch_outcome(&mut self, outcome: ConformanceDispatchOutcome);

        fn queue_result(&mut self, result: EffectResult);

        fn dispatched(&self) -> &[EffectRequest];
    }

    trait ProviderConformanceFactory {
        type Driver: ProviderConformanceDriver;

        fn create(
            capabilities: PlatformCapabilities,
            observations: PlatformObservations,
        ) -> Self::Driver;
    }

    #[derive(Debug, Default)]
    struct FakeNativeProvider {
        sample: ProviderSample,
        dispatch_outcomes: VecDeque<ConformanceDispatchOutcome>,
        dispatched: Vec<EffectRequest>,
        effect_results: Vec<EffectResult>,
    }

    impl FakeNativeProvider {
        fn new(capabilities: PlatformCapabilities, observations: PlatformObservations) -> Self {
            Self {
                sample: ProviderSample::new(capabilities, observations),
                ..Self::default()
            }
        }
    }

    impl NativePlatformProvider for FakeNativeProvider {
        fn sample(&mut self) -> ProviderSample {
            self.sample.clone()
        }

        fn dispatch_effect(&mut self, request: &EffectRequest) {
            self.dispatched.push(request.clone());
            match self
                .dispatch_outcomes
                .pop_front()
                .unwrap_or(ConformanceDispatchOutcome::Submitted)
            {
                ConformanceDispatchOutcome::Submitted => {}
                ConformanceDispatchOutcome::Result(result) => {
                    self.effect_results.push(EffectResult::new(
                        request.id(),
                        request.epoch(),
                        result,
                    ));
                }
            }
        }

        fn take_effect_results(&mut self) -> Vec<EffectResult> {
            std::mem::take(&mut self.effect_results)
        }
    }

    impl ProviderConformanceDriver for FakeNativeProvider {
        fn set_capabilities(&mut self, capabilities: PlatformCapabilities) {
            self.sample.capabilities = capabilities;
        }

        fn set_observations(&mut self, observations: PlatformObservations) {
            self.sample.observations = observations;
        }

        fn queue_dispatch_outcome(&mut self, outcome: ConformanceDispatchOutcome) {
            self.dispatch_outcomes.push_back(outcome);
        }

        fn queue_result(&mut self, result: EffectResult) {
            self.effect_results.push(result);
        }

        fn dispatched(&self) -> &[EffectRequest] {
            &self.dispatched
        }
    }

    struct FakeProviderFactory;

    impl ProviderConformanceFactory for FakeProviderFactory {
        type Driver = FakeNativeProvider;

        fn create(
            capabilities: PlatformCapabilities,
            observations: PlatformObservations,
        ) -> Self::Driver {
            FakeNativeProvider::new(capabilities, observations)
        }
    }

    struct ProviderHarness<P> {
        engine: DockEngine,
        source_tabs: NodeId,
        adapter: NativePlatformAdapter<P>,
    }

    impl<P> ProviderHarness<P>
    where
        P: NativePlatformProvider,
    {
        fn new(provider: P) -> Self {
            let mut builder = Workspace::builder();
            let source_tabs = builder.insert_node(Node::tabs([SOURCE_ITEM, ItemId::new(3)]));
            let target_tabs = builder.insert_node(Node::tabs([ItemId::new(2)]));
            builder.set_root(SOURCE_ROOT, RootRecord::new(source_tabs));
            builder.set_root(TARGET_ROOT, RootRecord::new(target_tabs));
            builder.set_surface(SOURCE_SURFACE, SurfacePresentation::new(SOURCE_ROOT));
            builder.set_surface(TARGET_SURFACE, SurfacePresentation::new(TARGET_ROOT));
            let workspace = builder.build().expect("test workspace must be valid");
            let mut policy = DockPolicy::default();
            policy.set_allow_native_surfaces(true);
            let mut engine = DockEngine::new(workspace, policy).expect("test engine must be valid");

            let mut scene = BuildingScene::new([SOURCE_SURFACE, TARGET_SURFACE])
                .expect("surface roster must be unique");
            for surface in [SOURCE_SURFACE, TARGET_SURFACE] {
                scene
                    .insert_ready(ReadySurfaceScene::new(
                        surface,
                        logical_rect(0.0, 0.0, 600.0, 400.0),
                    ))
                    .expect("initial surface facts must be unique");
            }
            engine.enqueue_scene(scene).expect("scene must enqueue");
            engine.reduce_pending().expect("scene must publish");

            let target_recovery =
                contained_recovery(&engine, TARGET_ROOT, TARGET_RECOVERY, SOURCE_SURFACE);
            engine
                .enqueue_viewport_registration(
                    SOURCE_SURFACE,
                    SOURCE_TOKEN,
                    ViewportRole::Root,
                    None,
                )
                .expect("source viewport registration must enqueue");
            engine
                .enqueue_viewport_registration(
                    TARGET_SURFACE,
                    TARGET_TOKEN,
                    ViewportRole::Child,
                    Some(target_recovery),
                )
                .expect("target viewport registration must enqueue");
            let transition = engine
                .reduce_pending()
                .expect("viewport registrations must reduce");
            assert!(
                transition.reduced_inputs().iter().all(|input| matches!(
                    input.outcome(),
                    InputOutcome::ViewportRegistered { .. }
                ))
            );

            Self {
                engine,
                source_tabs,
                adapter: NativePlatformAdapter::new(provider),
            }
        }

        fn publish_sample(&mut self) -> EngineTransition {
            let snapshot = self
                .adapter
                .snapshot()
                .expect("fake provider sample must be canonical");
            self.engine
                .enqueue_platform_snapshot(snapshot)
                .expect("platform snapshot must enqueue");
            let transition = self
                .engine
                .reduce_pending()
                .expect("platform snapshot must reduce");
            assert!(matches!(
                transition.reduced_inputs()[0].outcome(),
                InputOutcome::PlatformSnapshotPublished { .. }
            ));
            transition
        }

        fn dispatch(&mut self, transition: &EngineTransition) {
            self.adapter.dispatch_effects(transition.platform_effects());
        }

        fn report_results(&mut self, results: &[EffectResult]) -> EngineTransition {
            for result in results {
                self.engine
                    .enqueue_platform_effect_result(*result)
                    .expect("provider result must enqueue");
            }
            self.engine
                .reduce_pending()
                .expect("provider results must reduce")
        }

        fn begin_drag(&mut self) -> (DragSessionId, EngineTransition) {
            let payload = MovePayload::Item(
                self.engine
                    .workspace()
                    .capture_item_source(SOURCE_ROOT, self.source_tabs, SOURCE_ITEM)
                    .expect("source item must be current"),
            );
            self.engine
                .enqueue_renderer_intent(RendererIntent::ArmDrag {
                    pointer: POINTER,
                    button: PointerButton::Primary,
                    payload,
                })
                .expect("arm drag must enqueue");
            let armed = self.engine.reduce_pending().expect("arm drag must reduce");
            let session = match armed.reduced_inputs()[0].outcome() {
                InputOutcome::InteractionProcessed {
                    outcome: InteractionOutcome::DragArmed { session, .. },
                    ..
                } => *session,
                outcome => panic!("unexpected arm outcome: {outcome:?}"),
            };
            self.engine
                .enqueue_renderer_intent(RendererIntent::BeginDrag {
                    session,
                    pointer: POINTER,
                    button: PointerButton::Primary,
                })
                .expect("begin drag must enqueue");
            let transition = self
                .engine
                .reduce_pending()
                .expect("begin drag must reduce");
            (session, transition)
        }

        fn current_route(&self) -> TargetAuthority {
            TargetAuthority::routed(
                self.engine
                    .viewport()
                    .route(POINTER)
                    .expect("current sample must publish a pointer route")
                    .clone(),
            )
        }
    }

    fn logical_rect(x: f64, y: f64, width: f64, height: f64) -> LogicalRect {
        LogicalRect::new(x, y, width, height).expect("test logical rectangle must be valid")
    }

    fn physical_rect(x: f64, y: f64, width: f64, height: f64) -> PhysicalRect {
        PhysicalRect::new(x, y, width, height).expect("test physical rectangle must be valid")
    }

    fn conformance_harness<F>(
        capabilities: PlatformCapabilities,
        observations: PlatformObservations,
    ) -> ProviderHarness<F::Driver>
    where
        F: ProviderConformanceFactory,
    {
        ProviderHarness::new(F::create(capabilities, observations))
    }

    fn contained_recovery(
        engine: &DockEngine,
        root: RootId,
        floating: FloatingPresentationId,
        host: SurfaceId,
    ) -> ContainedTearOffProposal {
        let placement = engine
            .contained_placement(
                host,
                logical_rect(30.0, 40.0, 420.0, 320.0),
                LogicalSize::new(0.0, 0.0).expect("minimum size must be valid"),
            )
            .expect("ready host scene must authorize recovery placement");
        ContainedTearOffProposal::new(root, floating, placement, 9)
    }

    fn routing_capabilities() -> PlatformCapabilities {
        let mut capabilities = PlatformCapabilities::default();
        capabilities.set_authoritative_inventory(PlatformCapability::Supported);
        capabilities.set_hovered_window(PlatformCapability::Supported);
        capabilities.set_desktop_pointer_position(PlatformCapability::Supported);
        capabilities.set_authoritative_button_state(PlatformCapability::Supported);
        capabilities.set_pointer_hit_test_observation(PlatformCapability::Supported);
        capabilities.set_pointer_hit_test_control(PlatformCapability::Supported);
        capabilities
    }

    fn lifecycle_capabilities() -> PlatformCapabilities {
        let mut capabilities = routing_capabilities();
        capabilities.set_native_window_lifecycle(PlatformCapability::Supported);
        capabilities.set_global_window_placement(PlatformCapability::Supported);
        capabilities.set_work_area(PlatformCapability::Supported);
        capabilities.set_window_focus(PlatformCapability::Supported);
        capabilities.set_close_cancellation(PlatformCapability::Supported);
        capabilities
    }

    fn observed_window(
        token: WindowToken,
        x: f64,
        input: WindowInputState,
        close_requested: bool,
    ) -> ObservedWindow {
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
            .with_input_state(Authority::Known(input))
            .with_presentation(Authority::Known(WindowPresentationState::Visible))
            .with_focused(Authority::Known(false))
            .with_close_requested(Authority::Known(close_requested))
    }

    fn pointer_observation(
        hovered: Authority<PointerWindow>,
        button: PointerButtonState,
    ) -> PointerObservation {
        PointerObservation::new(
            POINTER,
            hovered,
            Authority::Known(
                PhysicalPoint::new(750.0, 150.0).expect("test desktop point must be valid"),
            ),
            Authority::Known(vec![ButtonObservation::new(PointerButton::Primary, button)]),
        )
        .expect("test pointer observation must be unambiguous")
    }

    fn observations(
        source_input: WindowInputState,
        hovered: Authority<PointerWindow>,
        button: PointerButtonState,
    ) -> PlatformObservations {
        PlatformObservations::new(
            vec![
                observed_window(SOURCE_TOKEN, 0.0, source_input, false),
                observed_window(TARGET_TOKEN, 600.0, WindowInputState::ReceivesInput, false),
            ],
            vec![pointer_observation(hovered, button)],
            Vec::new(),
        )
    }

    fn lifecycle_observations(
        source_input: WindowInputState,
        hovered: PointerWindow,
        button: PointerButtonState,
        target_close_requested: bool,
        extra: Option<ObservedWindow>,
    ) -> PlatformObservations {
        let mut windows = vec![
            observed_window(SOURCE_TOKEN, 0.0, source_input, false),
            observed_window(
                TARGET_TOKEN,
                600.0,
                WindowInputState::ReceivesInput,
                target_close_requested,
            ),
        ];
        windows.extend(extra);
        PlatformObservations::new(
            windows,
            vec![pointer_observation(Authority::Known(hovered), button)],
            vec![work_area_observation()],
        )
    }

    fn source_only_lifecycle_observations() -> PlatformObservations {
        PlatformObservations::new(
            vec![observed_window(
                SOURCE_TOKEN,
                0.0,
                WindowInputState::ReceivesInput,
                false,
            )],
            vec![pointer_observation(
                Authority::Known(PointerWindow::None),
                PointerButtonState::Released,
            )],
            vec![work_area_observation()],
        )
    }

    fn work_area_observation() -> ObservedWorkArea {
        ObservedWorkArea::new(
            WORK_AREA,
            physical_rect(-1920.0, -200.0, 3840.0, 1400.0),
            ScaleFactor::new(1.0).expect("work-area scale must be valid"),
        )
    }

    fn request_passthrough_result<F>(
        result: EffectDispatchResult,
    ) -> (ProviderHarness<F::Driver>, EffectRequest)
    where
        F: ProviderConformanceFactory,
    {
        let mut harness = conformance_harness::<F>(
            routing_capabilities(),
            observations(
                WindowInputState::ReceivesInput,
                Authority::Known(PointerWindow::Dock(TARGET_TOKEN)),
                PointerButtonState::Pressed,
            ),
        );
        harness.publish_sample();
        let (_, transition) = harness.begin_drag();
        harness
            .adapter
            .provider_mut()
            .queue_dispatch_outcome(ConformanceDispatchOutcome::Result(result));
        harness.dispatch(&transition);
        let request = transition
            .platform_effects()
            .iter()
            .find(|request| {
                matches!(
                    request.effect(),
                    PlatformEffect::SetPointerPassthrough { enabled: true, .. }
                )
            })
            .expect("drag must request source pass-through")
            .clone();
        (harness, request)
    }

    fn start_native_create<P>(
        harness: &mut ProviderHarness<P>,
    ) -> (NativeCreateRequest, EngineTransition)
    where
        P: ProviderConformanceDriver,
    {
        let before_items = harness.engine.workspace().item_multiset();
        let (session, _) = harness.begin_drag();
        harness.publish_sample();
        let placement = harness
            .engine
            .tear_off_placement(
                POINTER,
                TearOffPlacementRequest::new(
                    LogicalSize::new(50.0, 40.0).expect("cursor offset must be valid"),
                    LogicalSize::new(640.0, 480.0).expect("preferred size must be valid"),
                    LogicalSize::new(0.0, 0.0).expect("minimum size must be valid"),
                    WORK_AREA,
                ),
            )
            .expect("current provider sample must authorize native placement");
        let request = TearOffRequest::native(
            NativeTearOffProposal::new(
                NATIVE_SURFACE,
                NATIVE_ROOT,
                placement,
                contained_recovery(
                    &harness.engine,
                    NATIVE_ROOT,
                    NATIVE_RECOVERY,
                    TARGET_SURFACE,
                ),
            ),
            None,
        );
        harness
            .engine
            .enqueue_renderer_intent(RendererIntent::UpdateDrag {
                session,
                target: harness.current_route(),
                tear_off: Some(request.clone()),
            })
            .expect("native preview must enqueue");
        let preview = harness
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
        let acknowledgement = harness
            .engine
            .interaction()
            .preview()
            .expect("native preview must exist")
            .acknowledgement();
        harness
            .engine
            .enqueue_renderer_intent(RendererIntent::ReleaseDrag {
                session,
                pointer: POINTER,
                button: PointerButton::Primary,
                button_state: Authority::Known(PointerButtonState::Released),
                target: harness.current_route(),
                tear_off: Some(request),
            })
            .expect("native release must enqueue");
        harness
            .engine
            .enqueue_renderer_intent(RendererIntent::AcknowledgePreview(acknowledgement))
            .expect("native preview acknowledgement must enqueue");
        let transition = harness
            .engine
            .reduce_pending()
            .expect("native release must reduce");
        let request = transition
            .reduced_inputs()
            .iter()
            .find_map(|input| match input.outcome() {
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
            .expect("native release must create one coordinator saga");
        assert_eq!(harness.engine.workspace().item_multiset(), before_items);
        (request, transition)
    }

    fn run_atomic_sample_case<F>()
    where
        F: ProviderConformanceFactory,
    {
        let mut capabilities = PlatformCapabilities::default();
        capabilities.set_native_window_lifecycle(PlatformCapability::Supported);
        capabilities.set_hovered_window(PlatformCapability::unsupported(
            PlatformRequirement::HoveredWindow,
            PlatformCapabilityReason::BackendUnsupported,
        ));
        capabilities.set_authoritative_button_state(PlatformCapability::unknown(
            PlatformRequirement::AuthoritativeButtonState,
            PlatformCapabilityReason::EnvironmentUnavailable,
        ));
        let mut adapter =
            NativePlatformAdapter::new(F::create(capabilities, PlatformObservations::default()));

        let snapshot = adapter
            .snapshot()
            .expect("empty observations must remain a valid atomic sample");

        assert_eq!(
            snapshot.capabilities().native_window_lifecycle(),
            PlatformCapability::Supported
        );
        assert!(matches!(
            snapshot.capabilities().hovered_window(),
            PlatformCapability::Unsupported(issue)
                if issue.requirement() == PlatformRequirement::HoveredWindow
        ));
        assert!(matches!(
            snapshot.capabilities().authoritative_button_state(),
            PlatformCapability::Unknown(issue)
                if issue.requirement() == PlatformRequirement::AuthoritativeButtonState
        ));
        assert!(snapshot.windows().is_empty());
        assert!(snapshot.pointers().is_empty());
    }

    fn run_requested_passthrough_case<F>()
    where
        F: ProviderConformanceFactory,
    {
        let mut harness = conformance_harness::<F>(
            routing_capabilities(),
            observations(
                WindowInputState::ReceivesInput,
                Authority::Known(PointerWindow::Dock(TARGET_TOKEN)),
                PointerButtonState::Pressed,
            ),
        );
        harness.publish_sample();

        let (_, transition) = harness.begin_drag();
        harness.dispatch(&transition);
        assert!(
            harness
                .adapter
                .provider()
                .dispatched()
                .iter()
                .any(|request| matches!(
                    request.effect(),
                    PlatformEffect::SetPointerPassthrough { enabled: true, .. }
                ))
        );
        assert!(harness.adapter.take_effect_results().is_empty());

        harness.publish_sample();
        let pending = harness
            .engine
            .viewport()
            .route(POINTER)
            .expect("pending observation must publish an unavailable route proof");
        assert_eq!(
            pending.target(),
            &Authority::Unknown(AuthorityUnavailableReason::SurfaceUnavailable)
        );

        harness
            .adapter
            .provider_mut()
            .set_observations(observations(
                WindowInputState::PassThrough,
                Authority::Known(PointerWindow::Dock(TARGET_TOKEN)),
                PointerButtonState::Pressed,
            ));
        harness.publish_sample();
        let observed = harness
            .engine
            .viewport()
            .route(POINTER)
            .expect("observed pass-through must publish a route proof");
        assert!(
            matches!(observed.target(), Authority::Known(Some(target)) if target.surface() == TARGET_SURFACE)
        );
    }

    fn run_authoritative_hover_and_release_case<F>()
    where
        F: ProviderConformanceFactory,
    {
        let mut harness = conformance_harness::<F>(
            routing_capabilities(),
            observations(
                WindowInputState::PassThrough,
                Authority::Known(PointerWindow::Dock(TARGET_TOKEN)),
                PointerButtonState::Released,
            ),
        );
        harness.publish_sample();

        let proof = harness
            .engine
            .viewport()
            .route(POINTER)
            .expect("authoritative provider facts must publish a route proof");
        assert!(
            matches!(proof.target(), Authority::Known(Some(target)) if target.surface() == TARGET_SURFACE)
        );
        assert_eq!(
            proof.button_state(PointerButton::Primary),
            Authority::Known(PointerButtonState::Released)
        );
    }

    fn run_current_epoch_dispatch_outcomes_case<F>()
    where
        F: ProviderConformanceFactory,
    {
        let cases = [
            EffectDispatchResult::DispatchFailed(DispatchFailureReason::ProviderStopped),
            EffectDispatchResult::Unsupported(EffectUnsupportedReason::CapabilityRevoked),
            EffectDispatchResult::Indeterminate(EffectIndeterminateReason::AcknowledgementLost),
        ];
        for result in cases {
            let (mut harness, request) = request_passthrough_result::<F>(result);
            let results = harness.adapter.take_effect_results();
            let transition = harness.report_results(&results);
            assert!(matches!(
                transition.reduced_inputs()[0].outcome(),
                InputOutcome::PlatformEffectReported {
                    transition: EffectTransition::Applied,
                    ..
                }
            ));
            let phase = harness
                .engine
                .viewport()
                .effects()
                .record(request.id())
                .expect("effect must remain auditable")
                .phase();
            match (result, phase) {
                (
                    EffectDispatchResult::DispatchFailed(expected),
                    EffectPhase::DispatchFailed(actual),
                ) if expected == actual => {}
                (EffectDispatchResult::Unsupported(expected), EffectPhase::Unsupported(actual))
                    if expected == actual => {}
                (
                    EffectDispatchResult::Indeterminate(expected),
                    EffectPhase::Indeterminate(actual),
                ) if expected == actual => {}
                unexpected => panic!("unexpected provider result phase: {unexpected:?}"),
            }
        }
    }

    fn run_duplicate_result_case<F>()
    where
        F: ProviderConformanceFactory,
    {
        let (mut duplicate, _) = request_passthrough_result::<F>(
            EffectDispatchResult::DispatchFailed(DispatchFailureReason::AdapterRejected),
        );
        let result = duplicate.adapter.take_effect_results()[0];
        let first = duplicate.report_results(&[result]);
        let second = duplicate.report_results(&[result]);
        assert!(matches!(
            first.reduced_inputs()[0].outcome(),
            InputOutcome::PlatformEffectReported {
                transition: EffectTransition::Applied,
                ..
            }
        ));
        assert!(matches!(
            second.reduced_inputs()[0].outcome(),
            InputOutcome::PlatformEffectReported {
                transition: EffectTransition::Duplicate,
                ..
            }
        ));
    }

    fn run_unknown_result_case<F>()
    where
        F: ProviderConformanceFactory,
    {
        let mut harness = conformance_harness::<F>(
            routing_capabilities(),
            observations(
                WindowInputState::ReceivesInput,
                Authority::Known(PointerWindow::None),
                PointerButtonState::Released,
            ),
        );
        let current_epoch = harness.engine.version().epoch();
        harness
            .adapter
            .provider_mut()
            .queue_result(EffectResult::new(
                EffectId::new(u64::MAX),
                current_epoch,
                EffectDispatchResult::DispatchFailed(DispatchFailureReason::AdapterRejected),
            ));
        let unknown = harness.adapter.take_effect_results();
        let unknown = harness.report_results(&unknown);
        assert!(matches!(
            unknown.reduced_inputs()[0].outcome(),
            InputOutcome::PlatformEffectReported {
                transition: EffectTransition::UnknownEffect,
                ..
            }
        ));
    }

    fn run_previous_epoch_result_case<F>()
    where
        F: ProviderConformanceFactory,
    {
        let (mut stale, _) = request_passthrough_result::<F>(EffectDispatchResult::DispatchFailed(
            DispatchFailureReason::ProviderStopped,
        ));
        let stale_result = stale.adapter.take_effect_results()[0];
        let replacement = stale.engine.workspace().clone();
        stale
            .engine
            .enqueue_workspace_replacement(replacement)
            .expect("workspace replacement must enqueue");
        stale
            .engine
            .reduce_pending()
            .expect("workspace replacement must reduce");
        let stale_transition = stale.report_results(&[stale_result]);
        assert!(matches!(
            stale_transition.reduced_inputs()[0].outcome(),
            InputOutcome::PlatformEffectReported {
                transition: EffectTransition::StaleEpoch,
                ..
            }
        ));
    }

    fn run_out_of_order_results_case<F>()
    where
        F: ProviderConformanceFactory,
    {
        let mut reversed = conformance_harness::<F>(
            routing_capabilities(),
            observations(
                WindowInputState::ReceivesInput,
                Authority::Known(PointerWindow::Dock(TARGET_TOKEN)),
                PointerButtonState::Pressed,
            ),
        );
        reversed.publish_sample();
        let (session, enable) = reversed.begin_drag();
        reversed
            .adapter
            .provider_mut()
            .queue_dispatch_outcome(ConformanceDispatchOutcome::Result(
                EffectDispatchResult::DispatchFailed(DispatchFailureReason::WindowUnavailable),
            ));
        reversed.dispatch(&enable);
        reversed
            .engine
            .enqueue_renderer_intent(RendererIntent::CancelDrag {
                session,
                reason: InteractionCancelReason::Escape,
            })
            .expect("drag cancellation must enqueue");
        let disable = reversed
            .engine
            .reduce_pending()
            .expect("drag cancellation must reduce");
        reversed
            .adapter
            .provider_mut()
            .queue_dispatch_outcome(ConformanceDispatchOutcome::Result(
                EffectDispatchResult::Unsupported(EffectUnsupportedReason::CapabilityRevoked),
            ));
        reversed.dispatch(&disable);
        let mut results = reversed.adapter.take_effect_results();
        assert_eq!(results.len(), 2);
        results.reverse();
        let out_of_order = reversed.report_results(&results);
        assert!(out_of_order.reduced_inputs().iter().all(|input| matches!(
            input.outcome(),
            InputOutcome::PlatformEffectReported {
                transition: EffectTransition::Applied,
                ..
            }
        )));
    }

    fn run_capability_revocation_case<F>()
    where
        F: ProviderConformanceFactory,
    {
        let mut harness = conformance_harness::<F>(
            routing_capabilities(),
            observations(
                WindowInputState::ReceivesInput,
                Authority::Known(PointerWindow::Dock(TARGET_TOKEN)),
                PointerButtonState::Pressed,
            ),
        );
        harness.publish_sample();
        let (session, enable) = harness.begin_drag();
        harness.dispatch(&enable);
        harness
            .adapter
            .provider_mut()
            .set_observations(observations(
                WindowInputState::PassThrough,
                Authority::Known(PointerWindow::Dock(TARGET_TOKEN)),
                PointerButtonState::Pressed,
            ));
        harness.publish_sample();
        assert_eq!(
            harness.engine.interaction().status(),
            InteractionStatus::Dragging { session }
        );

        let mut revoked = routing_capabilities();
        revoked.set_pointer_hit_test_observation(PlatformCapability::unsupported(
            PlatformRequirement::PointerHitTestObservation,
            PlatformCapabilityReason::BackendUnsupported,
        ));
        harness.adapter.provider_mut().set_capabilities(revoked);
        let cancelled = harness.publish_sample();
        assert_eq!(
            harness.engine.interaction().status(),
            InteractionStatus::Idle
        );
        let disable = cancelled
            .platform_effects()
            .iter()
            .find(|request| {
                matches!(
                    request.effect(),
                    PlatformEffect::SetPointerPassthrough { enabled: false, .. }
                )
            })
            .expect("capability revocation must request source input restoration")
            .clone();
        harness.dispatch(&cancelled);
        assert!(harness.adapter.take_effect_results().is_empty());

        let still_passthrough = harness.publish_sample();
        assert!(still_passthrough.platform_effects().is_empty());
        assert_eq!(
            harness
                .engine
                .viewport()
                .effects()
                .record(disable.id())
                .expect("disable effect must remain auditable")
                .phase(),
            EffectPhase::Requested
        );

        harness
            .adapter
            .provider_mut()
            .set_observations(observations(
                WindowInputState::ReceivesInput,
                Authority::Known(PointerWindow::Dock(TARGET_TOKEN)),
                PointerButtonState::Pressed,
            ));
        let restored = harness.publish_sample();
        assert!(restored.platform_effects().is_empty());
        assert!(matches!(
            harness
                .engine
                .viewport()
                .effects()
                .record(disable.id())
                .expect("observed disable effect must remain auditable")
                .phase(),
            EffectPhase::ObservedApplied { .. }
        ));
    }

    fn run_native_create_case<F>()
    where
        F: ProviderConformanceFactory,
    {
        let mut harness = conformance_harness::<F>(
            lifecycle_capabilities(),
            lifecycle_observations(
                WindowInputState::PassThrough,
                PointerWindow::None,
                PointerButtonState::Released,
                false,
                None,
            ),
        );
        harness.publish_sample();
        let before = harness.engine.workspace().clone();
        let (request, transition) = start_native_create(&mut harness);
        harness.dispatch(&transition);
        assert_eq!(harness.engine.workspace(), &before);
        assert!(matches!(
            harness
                .engine
                .viewport()
                .native_create_saga(request.saga())
                .expect("create saga must remain queryable")
                .status(),
            NativeCreateStatus::Requested
        ));
        assert!(matches!(
            harness
                .engine
                .viewport()
                .effects()
                .record(request.effect())
                .expect("create effect must remain auditable")
                .phase(),
            EffectPhase::Requested
        ));

        harness
            .adapter
            .provider_mut()
            .set_observations(lifecycle_observations(
                WindowInputState::PassThrough,
                PointerWindow::None,
                PointerButtonState::Released,
                false,
                Some(observed_window(
                    request.binding().token(),
                    1200.0,
                    WindowInputState::ReceivesInput,
                    false,
                )),
            ));
        harness.publish_sample();
        assert!(harness.engine.workspace().surface(NATIVE_SURFACE).is_some());
        assert!(harness.engine.workspace().root(NATIVE_ROOT).is_some());
        assert_eq!(
            harness.engine.workspace().item_multiset(),
            before.item_multiset()
        );
        assert!(matches!(
            harness
                .engine
                .viewport()
                .effects()
                .record(request.effect())
                .expect("observed create effect must remain auditable")
                .phase(),
            EffectPhase::ObservedApplied { .. }
        ));
    }

    fn run_child_close_recovery_case<F>()
    where
        F: ProviderConformanceFactory,
    {
        let mut harness = conformance_harness::<F>(
            lifecycle_capabilities(),
            lifecycle_observations(
                WindowInputState::ReceivesInput,
                PointerWindow::None,
                PointerButtonState::Released,
                false,
                None,
            ),
        );
        harness.publish_sample();
        harness
            .adapter
            .provider_mut()
            .set_observations(lifecycle_observations(
                WindowInputState::ReceivesInput,
                PointerWindow::None,
                PointerButtonState::Released,
                true,
                None,
            ));
        let close_edge = harness.publish_sample();
        let close_request = close_edge
            .reduced_inputs()
            .iter()
            .find_map(|input| match input.outcome() {
                InputOutcome::PlatformSnapshotPublished { transition } => {
                    transition.close_requests().first().copied()
                }
                _ => None,
            })
            .expect("close observation must create one exact request");
        let recovery =
            contained_recovery(&harness.engine, TARGET_ROOT, CLOSE_RECOVERY, SOURCE_SURFACE);
        harness
            .engine
            .enqueue_viewport_close_decision(
                close_request,
                ViewportCloseDecision::Accept(ViewportClosePlan::new(None, recovery)),
            )
            .expect("close acceptance must enqueue");
        let decision = harness
            .engine
            .reduce_pending()
            .expect("close acceptance must reduce");
        let release = decision
            .platform_effects()
            .iter()
            .find(|request| matches!(request.effect(), PlatformEffect::ReleaseChild { .. }))
            .expect("child acceptance must request roster release")
            .clone();
        harness.dispatch(&decision);
        assert!(harness.engine.workspace().surface(TARGET_SURFACE).is_some());
        assert!(matches!(
            harness
                .engine
                .viewport()
                .viewport_close_request(close_request)
                .expect("close request must remain queryable")
                .status(),
            ViewportCloseStatus::AwaitingDestroyed { .. }
        ));

        harness
            .adapter
            .provider_mut()
            .set_observations(source_only_lifecycle_observations());
        harness.publish_sample();
        assert!(harness.engine.workspace().surface(TARGET_SURFACE).is_none());
        let recovered = harness
            .engine
            .workspace()
            .contained_floating(CLOSE_RECOVERY)
            .expect("destroyed child root must recover as contained floating");
        assert_eq!(recovered.root, TARGET_ROOT);
        assert_eq!(recovered.surface, SOURCE_SURFACE);
        assert!(matches!(
            harness
                .engine
                .viewport()
                .effects()
                .record(release.id())
                .expect("close effect must remain auditable")
                .phase(),
            EffectPhase::Destroyed { .. }
        ));
    }

    macro_rules! provider_conformance_suite {
        ($module:ident, $factory:ty) => {
            mod $module {
                use super::*;

                #[test]
                fn atomic_sample_keeps_capabilities_and_observations_in_one_generation() {
                    run_atomic_sample_case::<$factory>();
                }

                #[test]
                fn requested_passthrough_waits_for_an_authoritative_observation() {
                    run_requested_passthrough_case::<$factory>();
                }

                #[test]
                fn authoritative_hover_and_release_reach_the_core_without_inference() {
                    run_authoritative_hover_and_release_case::<$factory>();
                }

                #[test]
                fn current_epoch_dispatch_outcomes_reach_the_core_ledger() {
                    run_current_epoch_dispatch_outcomes_case::<$factory>();
                }

                #[test]
                fn duplicate_result_is_a_deterministic_no_op() {
                    run_duplicate_result_case::<$factory>();
                }

                #[test]
                fn unknown_result_is_a_deterministic_no_op() {
                    run_unknown_result_case::<$factory>();
                }

                #[test]
                fn previous_epoch_result_is_a_deterministic_no_op() {
                    run_previous_epoch_result_case::<$factory>();
                }

                #[test]
                fn out_of_order_results_remain_correlated_by_effect_identity() {
                    run_out_of_order_results_case::<$factory>();
                }

                #[test]
                fn capability_revocation_restores_source_input_to_receives_input() {
                    run_capability_revocation_case::<$factory>();
                }

                #[test]
                fn native_create_commits_only_after_authoritative_observation() {
                    run_native_create_case::<$factory>();
                }

                #[test]
                fn child_close_recovers_the_destroyed_root_as_contained() {
                    run_child_close_recovery_case::<$factory>();
                }
            }
        };
    }

    provider_conformance_suite!(fake_provider, FakeProviderFactory);
}
