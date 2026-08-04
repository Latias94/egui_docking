use super::*;
use crate::close_plan::{CloseAuthority, CloseCoordinator, NativeCloseEdge, SurfaceCloseRequest};
use crate::command::MovePayload;
use crate::effect::{EffectPhase, NativeCloseResolution};
use crate::geometry::{PhysicalRect, ScaleFactor};
use crate::graph::{Node, RootRecord, SurfacePresentation, Workspace};
use crate::ids::{EngineAuthorityDomainId, ItemId, RootId, WorkspaceRevision};
use crate::intent::{Authority, AuthorityUnavailableReason};
use crate::platform::{
    CapabilityRosterObservation, CloseEffectAcknowledgement, InputEffectAcknowledgement,
    ObservedWindow, PlatformSnapshotError, PresentationEffectAcknowledgement, WindowCloseState,
    WindowCoordinateObservation, WindowInputObservation, WindowInputState,
    WindowInventoryObservation, WindowPresentationObservation, WindowPresentationState,
    WorkAreaRosterObservation,
};
use crate::presentation_config::PresentationConfigRevision;
use crate::presentation_observation::{
    NativeStagingResourceDescriptor, PresentationOutputSerial, PresentedNativeStagingPresentation,
    PresentedSurfaceAuthority, SurfacePresentationOutputTicket,
};
use crate::scene::SurfaceSceneStamp;
use crate::scene_manifest::{
    PolicyRevision, SurfaceMeasurementTicket, SurfaceRequirementRevision, SurfaceSceneRevision,
};
use crate::transition::WorkspaceVersion;
use crate::viewport::{
    CapabilityObservationGeneration, CloseObservationGeneration, CoordinateGeneration,
    CoordinateObservationGeneration, InventoryObservationGeneration,
    PresentationObservationGeneration, WindowIncarnation, WorkAreaObservationGeneration,
};
use crate::viewport_focus::{FocusObservationGeneration, unknown_focus_observation};

fn retirement(coordinator: &ViewportCoordinator, binding: ViewportBinding) -> &BindingRetirement {
    coordinator
        .binding_retirement
        .get(&binding)
        .expect("binding retirement must remain queryable")
}

fn observed_window(binding: ViewportBinding) -> ObservedWindow {
    observed_window_at(binding, 1)
}

fn observed_window_at(binding: ViewportBinding, generation: u64) -> ObservedWindow {
    ObservedWindow::new(binding)
        .with_coordinate_observation(WindowCoordinateObservation::new(
            binding,
            CoordinateObservationGeneration::new(generation),
            Authority::Known(
                PhysicalRect::new(10.0, 20.0, 300.0, 200.0)
                    .expect("test content bounds must be valid"),
            ),
            Authority::Known(
                PhysicalRect::new(5.0, 0.0, 310.0, 225.0).expect("test outer bounds must be valid"),
            ),
            Authority::Known(ScaleFactor::new(1.0).expect("test scale must be valid")),
            Authority::Known(ScaleFactor::new(1.0).expect("test scale must be valid")),
        ))
        .with_input_state(Authority::Known(WindowInputState::ReceivesInput))
        .with_presentation_observation(WindowPresentationObservation::new(
            binding,
            PresentationObservationGeneration::new(generation),
            Authority::Known(WindowPresentationState::Visible),
            PresentationEffectAcknowledgement::known(None),
        ))
}

fn unknown_work_areas(generation: u64) -> WorkAreaRosterObservation {
    WorkAreaRosterObservation::new(
        WorkAreaObservationGeneration::new(generation),
        Authority::Unknown(AuthorityUnavailableReason::NotReported),
    )
    .expect("work-area tombstone must be valid")
}

fn known_work_areas(
    generation: u64,
    work_areas: Vec<ObservedWorkArea>,
) -> WorkAreaRosterObservation {
    WorkAreaRosterObservation::new(
        WorkAreaObservationGeneration::new(generation),
        Authority::Known(work_areas),
    )
    .expect("known work-area roster must be valid")
}

fn platform_snapshot(
    provider_generation: u64,
    capabilities: PlatformCapabilities,
    focus: crate::viewport_focus::FocusObservationEnvelope,
    windows: Vec<ObservedWindow>,
    close_observations: Vec<WindowCloseObservation>,
    work_area_observation: WorkAreaRosterObservation,
) -> Result<PlatformSnapshot, PlatformSnapshotError> {
    platform_snapshot_with_inventory_generation(
        provider_generation,
        provider_generation,
        capabilities,
        focus,
        windows,
        close_observations,
        work_area_observation,
    )
}

fn platform_snapshot_with_inventory_generation(
    provider_generation: u64,
    inventory_generation: u64,
    capabilities: PlatformCapabilities,
    focus: crate::viewport_focus::FocusObservationEnvelope,
    windows: Vec<ObservedWindow>,
    close_observations: Vec<WindowCloseObservation>,
    work_area_observation: WorkAreaRosterObservation,
) -> Result<PlatformSnapshot, PlatformSnapshotError> {
    platform_snapshot_with_generations(
        inventory_generation,
        provider_generation,
        inventory_generation,
        capabilities,
        focus,
        windows,
        close_observations,
        work_area_observation,
    )
}

#[allow(clippy::too_many_arguments)]
fn platform_snapshot_with_generations(
    snapshot_generation: u64,
    provider_generation: u64,
    inventory_generation: u64,
    capabilities: PlatformCapabilities,
    focus: crate::viewport_focus::FocusObservationEnvelope,
    windows: Vec<ObservedWindow>,
    close_observations: Vec<WindowCloseObservation>,
    work_area_observation: WorkAreaRosterObservation,
) -> Result<PlatformSnapshot, PlatformSnapshotError> {
    let inventory_observation = if capabilities.authoritative_inventory().is_supported() {
        WindowInventoryObservation::new(
            InventoryObservationGeneration::new(inventory_generation),
            Authority::Known(windows.iter().map(ObservedWindow::binding).collect()),
        )?
    } else {
        WindowInventoryObservation::unknown(
            InventoryObservationGeneration::new(inventory_generation),
            AuthorityUnavailableReason::NotReported,
        )
    };
    PlatformSnapshot::new(
        PlatformSnapshotGeneration::new(snapshot_generation),
        CapabilityRosterObservation::new(
            CapabilityObservationGeneration::new(provider_generation),
            Authority::Known(capabilities),
        ),
        focus,
        inventory_observation,
        windows,
        close_observations,
        work_area_observation,
    )
}

fn snapshot(windows: Vec<ObservedWindow>) -> PlatformSnapshot {
    snapshot_at(1, windows)
}

fn snapshot_at(generation: u64, windows: Vec<ObservedWindow>) -> PlatformSnapshot {
    snapshot_with_close_at(generation, windows, Vec::new())
}

fn snapshot_with_close_at(
    generation: u64,
    windows: Vec<ObservedWindow>,
    close_observations: Vec<WindowCloseObservation>,
) -> PlatformSnapshot {
    let mut capabilities = PlatformCapabilities::default();
    capabilities.set_authoritative_inventory(PlatformCapability::Supported);
    platform_snapshot(
        generation,
        capabilities,
        unknown_focus_observation(
            FocusObservationGeneration::new(generation),
            AuthorityUnavailableReason::NotReported,
        ),
        windows,
        close_observations,
        unknown_work_areas(generation),
    )
    .expect("test snapshot must be valid")
}

fn close_observation(
    binding: ViewportBinding,
    generation: u64,
    state: WindowCloseState,
) -> WindowCloseObservation {
    WindowCloseObservation::new(
        binding,
        CloseObservationGeneration::new(generation),
        Authority::Known(state),
        CloseEffectAcknowledgement::known(None),
    )
}

fn native_close_request(
    binding: ViewportBinding,
    observed_at: CloseObservationGeneration,
    received_at: InventoryGeneration,
) -> (crate::close_plan::CloseRequestId, NativeCloseEdge) {
    let domain = EngineAuthorityDomainId::new_for_test(77);
    let authority = CloseAuthority::new(
        domain,
        WorkspaceVersion::new(binding.epoch(), WorkspaceRevision::new(1)),
        PolicyRevision::new(1),
    );
    let edge =
        NativeCloseEdge::from_authoritative_requested(domain, binding, observed_at, received_at);
    let mut closes = CloseCoordinator::new(domain);
    let request = closes
        .open_surface(authority, edge, SurfaceCloseRequest::RetainLayout, [], ())
        .expect("test native close plan must open")
        .request();
    (request, edge)
}

fn routing_coordinator(
    input: WindowInputState,
    presentation: WindowPresentationState,
    control: PlatformCapability,
) -> (ViewportCoordinator, ViewportBinding) {
    let token = WindowToken::new(41);
    let surface = SurfaceId::new(42);
    let mut coordinator = ViewportCoordinator::default();
    let binding = coordinator
        .register_existing(
            WorkspaceEpoch::default(),
            surface,
            token,
            ViewportRole::Root,
            None,
        )
        .expect("test external viewport must register");
    let facts = routing_snapshot(
        binding,
        1,
        input,
        presentation,
        control,
        InputEffectAcknowledgement::known(None),
    );
    coordinator
        .publish_snapshot(&facts)
        .expect("test routing facts must publish");
    (coordinator, binding)
}

fn external_routing_coordinator() -> (ViewportCoordinator, ViewportBinding) {
    let mut coordinator = ViewportCoordinator::default();
    let binding = coordinator
        .register_existing(
            WorkspaceEpoch::default(),
            SurfaceId::new(142),
            WindowToken::new(141),
            ViewportRole::Root,
            None,
        )
        .expect("test external viewport must register");
    coordinator
        .publish_snapshot(&routing_snapshot(
            binding,
            1,
            WindowInputState::ReceivesInput,
            WindowPresentationState::Visible,
            PlatformCapability::Supported,
            InputEffectAcknowledgement::known(None),
        ))
        .expect("test external viewport must become ready");
    (coordinator, binding)
}

fn runtime_child_routing_coordinator() -> (ViewportCoordinator, ViewportBinding) {
    let mut coordinator = ViewportCoordinator::default();
    let binding = coordinator
        .registry
        .reserve(
            WorkspaceEpoch::default(),
            SurfaceId::new(242),
            ViewportRole::Child,
        )
        .expect("test runtime child must reserve");
    coordinator
        .publish_snapshot(&routing_snapshot(
            binding,
            1,
            WindowInputState::ReceivesInput,
            WindowPresentationState::Visible,
            PlatformCapability::Supported,
            InputEffectAcknowledgement::known(None),
        ))
        .expect("test runtime child must become ready");
    coordinator
        .registry
        .admit(binding)
        .expect("test runtime child must admit");
    (coordinator, binding)
}

fn runtime_child_vacancy() -> (
    ViewportCoordinator,
    ViewportBinding,
    SurfaceVacancyAuthority,
    EffectId,
) {
    let (mut coordinator, binding) = runtime_child_routing_coordinator();
    let authority = coordinator.capture_surface_vacancy_authority(binding.surface());
    let settlement = coordinator
        .settle_surface_vacancies(&[authority], &BTreeSet::new())
        .expect("runtime child vacancy must settle");
    let [release] = settlement.effects() else {
        panic!("runtime child vacancy must own one release: {settlement:?}");
    };
    let release = *release;
    assert_eq!(settlement.logical_vacated_bindings(), &[binding]);
    assert!(coordinator.registry.record(binding.surface()).is_none());
    assert!(matches!(
        coordinator.binding_retirement.get(&binding).map(BindingRetirement::status),
        Some(BindingRetirementStatus::CleanupRequested { effect }) if effect == release
    ));
    let emitted = coordinator.take_new_effects();
    assert!(matches!(
        emitted.as_slice(),
        [request]
            if request.id() == release
                && matches!(
                    request.effect(),
                    PlatformEffect::ReleaseChild { binding: actual }
                        if *actual == binding
                )
    ));
    (coordinator, binding, authority, release)
}

fn routing_snapshot(
    binding: ViewportBinding,
    generation: u64,
    input: WindowInputState,
    presentation: WindowPresentationState,
    control: PlatformCapability,
    acknowledgement: InputEffectAcknowledgement,
) -> PlatformSnapshot {
    routing_snapshot_with_generations(
        binding,
        generation,
        generation,
        Authority::Known(input),
        presentation,
        control,
        acknowledgement,
    )
}

fn routing_snapshot_with_input_authority(
    binding: ViewportBinding,
    generation: u64,
    input: Authority<WindowInputState>,
    presentation: WindowPresentationState,
    control: PlatformCapability,
    acknowledgement: InputEffectAcknowledgement,
) -> PlatformSnapshot {
    routing_snapshot_with_generations(
        binding,
        generation,
        generation,
        input,
        presentation,
        control,
        acknowledgement,
    )
}

fn routing_snapshot_with_generations(
    binding: ViewportBinding,
    inventory_generation: u64,
    observation_generation: u64,
    input: Authority<WindowInputState>,
    presentation: WindowPresentationState,
    control: PlatformCapability,
    acknowledgement: InputEffectAcknowledgement,
) -> PlatformSnapshot {
    routing_snapshot_with_batch_and_generations(
        binding,
        inventory_generation,
        inventory_generation,
        observation_generation,
        input,
        presentation,
        control,
        acknowledgement,
    )
}

#[allow(clippy::too_many_arguments)]
fn routing_snapshot_with_batch_and_generations(
    binding: ViewportBinding,
    snapshot_generation: u64,
    inventory_generation: u64,
    observation_generation: u64,
    input: Authority<WindowInputState>,
    presentation: WindowPresentationState,
    control: PlatformCapability,
    acknowledgement: InputEffectAcknowledgement,
) -> PlatformSnapshot {
    let window = observed_window(binding)
        .with_input_observation(WindowInputObservation::new(
            binding,
            InputObservationGeneration::new(observation_generation),
            input,
            acknowledgement,
        ))
        .with_presentation_observation(WindowPresentationObservation::new(
            binding,
            PresentationObservationGeneration::new(observation_generation),
            Authority::Known(presentation),
            PresentationEffectAcknowledgement::known(None),
        ));
    let mut capabilities = PlatformCapabilities::default();
    capabilities.set_authoritative_inventory(PlatformCapability::Supported);
    capabilities.set_pointer_hit_test_observation(PlatformCapability::Supported);
    capabilities.set_pointer_hit_test_control(control);
    platform_snapshot_with_generations(
        snapshot_generation,
        observation_generation,
        inventory_generation,
        capabilities,
        unknown_focus_observation(
            FocusObservationGeneration::new(observation_generation),
            AuthorityUnavailableReason::NotReported,
        ),
        vec![window],
        Vec::new(),
        unknown_work_areas(observation_generation),
    )
    .expect("test routing snapshot must be valid")
}

fn recovery_presentation_snapshot(
    binding: ViewportBinding,
    generation: u64,
    state: WindowPresentationState,
    acknowledges: Option<EffectId>,
) -> PlatformSnapshot {
    let window = ObservedWindow::new(binding)
        .with_coordinate_observation(WindowCoordinateObservation::new(
            binding,
            CoordinateObservationGeneration::new(generation),
            Authority::Known(
                PhysicalRect::new(10.0, 20.0, 300.0, 200.0)
                    .expect("test content bounds must be valid"),
            ),
            Authority::Known(
                PhysicalRect::new(5.0, 0.0, 310.0, 225.0).expect("test outer bounds must be valid"),
            ),
            Authority::Known(ScaleFactor::new(1.0).expect("test scale must be valid")),
            Authority::Known(ScaleFactor::new(1.0).expect("test scale must be valid")),
        ))
        .with_input_state(Authority::Known(WindowInputState::ReceivesInput))
        .with_presentation_observation(WindowPresentationObservation::new(
            binding,
            PresentationObservationGeneration::new(generation),
            Authority::Known(state),
            PresentationEffectAcknowledgement::known(acknowledges),
        ));
    let mut capabilities = PlatformCapabilities::default();
    capabilities.set_authoritative_inventory(PlatformCapability::Supported);
    platform_snapshot_with_generations(
        generation,
        generation,
        generation,
        capabilities,
        unknown_focus_observation(
            FocusObservationGeneration::new(generation),
            AuthorityUnavailableReason::NotReported,
        ),
        vec![window],
        Vec::new(),
        unknown_work_areas(generation),
    )
    .expect("recovery presentation snapshot must be valid")
}

fn released_unobserved_enable() -> (ViewportCoordinator, ViewportBinding, EffectId, EffectId) {
    released_unobserved_enable_for(routing_coordinator(
        WindowInputState::ReceivesInput,
        WindowPresentationState::Visible,
        PlatformCapability::Supported,
    ))
}

fn released_unobserved_runtime_child_enable()
-> (ViewportCoordinator, ViewportBinding, EffectId, EffectId) {
    released_unobserved_enable_for(runtime_child_routing_coordinator())
}

fn released_unobserved_enable_for(
    (mut coordinator, binding): (ViewportCoordinator, ViewportBinding),
) -> (ViewportCoordinator, ViewportBinding, EffectId, EffectId) {
    let enable = coordinator
        .begin_drag_routing(PointerId::new(1), binding.surface())
        .expect("drag routing must begin")
        .expect("enable effect must exist");
    let _ = coordinator.take_new_effects();
    let restore = coordinator
        .end_drag_routing(PointerId::new(1))
        .expect("drag release must preserve restoration")
        .expect("release must queue restoration behind the enable attempt");
    let requests = coordinator.take_new_effects();
    assert!(matches!(
        requests.as_slice(),
        [request]
            if request.id() == restore
                && matches!(
                    request.effect(),
                    PlatformEffect::SetPointerPassthrough {
                        binding: actual,
                        enabled: false,
                        after: Some(predecessor),
                    } if *actual == binding && *predecessor == enable
                )
    ));
    (coordinator, binding, enable, restore)
}

fn take_single_pointer_restore(
    coordinator: &mut ViewportCoordinator,
    binding: ViewportBinding,
    expected_after: Option<EffectId>,
) -> EffectId {
    let restore_effects = coordinator.take_new_effects();
    let [restore_request] = restore_effects.as_slice() else {
        panic!("passthrough edge must issue exactly one restore: {restore_effects:?}");
    };
    assert_eq!(
        restore_request.effect(),
        &PlatformEffect::SetPointerPassthrough {
            binding,
            enabled: false,
            after: expected_after,
        }
    );
    restore_request.id()
}

fn report_effect_result(
    coordinator: &mut ViewportCoordinator,
    current_epoch: WorkspaceEpoch,
    effect: EffectId,
    issuance_epoch: WorkspaceEpoch,
    result: EffectDispatchResult,
) -> EffectTransition {
    coordinator
        .report_effect(
            current_epoch,
            EffectResult::new(effect, issuance_epoch, result),
        )
        .expect("effect result must reduce")
}

fn failed_cleanup_observation() -> (
    ViewportCoordinator,
    ViewportBinding,
    EffectId,
    EffectId,
    WorkspaceEpoch,
    WorkspaceEpoch,
) {
    let (mut coordinator, binding) = runtime_child_routing_coordinator();
    let destructive_epoch = WorkspaceEpoch::new(1);
    coordinator
        .reconcile_workspace_epoch(destructive_epoch, &BTreeSet::new())
        .expect("first restore must retire the binding");
    let destructive = coordinator
        .take_new_effects()
        .into_iter()
        .find_map(|request| {
            matches!(
                request.effect(),
                PlatformEffect::ReleaseChild { binding: actual } if *actual == binding
            )
            .then_some(request.id())
        })
        .expect("first restore must emit destructive cleanup");
    let observation_epoch = WorkspaceEpoch::new(2);
    coordinator
        .reconcile_workspace_epoch(observation_epoch, &BTreeSet::new())
        .expect("second restore must continue cleanup observation");
    let continuation = coordinator
        .take_new_effects()
        .into_iter()
        .find_map(|request| {
            matches!(
                request.effect(),
                PlatformEffect::ContinueCleanup {
                    binding: actual,
                    predecessor,
                    ..
                } if *actual == binding && *predecessor == destructive
            )
            .then_some(request.id())
        })
        .expect("second restore must emit cleanup continuation");
    assert_eq!(
        report_effect_result(
            &mut coordinator,
            observation_epoch,
            continuation,
            observation_epoch,
            EffectDispatchResult::DispatchFailed(
                crate::effect::DispatchFailureReason::ProviderStopped,
            ),
        ),
        EffectTransition::Applied
    );
    (
        coordinator,
        binding,
        destructive,
        continuation,
        destructive_epoch,
        observation_epoch,
    )
}

fn recovery(root: u64) -> SurfaceRecoveryObligationId {
    SurfaceRecoveryObligationId::new_for_test(root)
}

fn retained_resource_descriptor(saga: NativeCreateSagaId) -> NativeStagingResourceDescriptor {
    let domain = EngineAuthorityDomainId::new_for_test(1);
    let source_surface = SurfaceId::new(700);
    let root = RootId::new(701);
    let item = ItemId::new(702);
    let mut builder = Workspace::builder();
    let tabs = builder.insert_node(Node::tabs([item]));
    builder.set_root(root, RootRecord::new(tabs));
    builder.set_surface(source_surface, SurfacePresentation::with_main(root));
    let workspace = builder.build().expect("test workspace must be valid");
    let payload = MovePayload::Item(
        workspace
            .capture_item_source(root, tabs, item)
            .expect("test item source must be current"),
    );
    let measurement = SurfaceMeasurementTicket::new(
        domain,
        WorkspaceEpoch::default(),
        PresentationConfigRevision::new(1),
        PolicyRevision::new(1),
        SurfaceRequirementRevision::new(1),
        source_surface,
    );
    let output = SurfacePresentationOutputTicket::mint(
        domain,
        PresentationOutputSerial::new_for_test(1),
        SurfaceSceneStamp::new(measurement, SurfaceSceneRevision::new(1)),
    );
    let presentation =
        PresentedSurfaceAuthority::mint_observed_for_test(output, CoordinateGeneration::new(1));
    NativeStagingResourceDescriptor::mint(
        NativeStagingResourceId::mint(domain, saga),
        presentation,
        payload,
    )
    .expect("test retained resource must share one authority domain")
}

fn ready_recovery_replacement() -> (
    ViewportCoordinator,
    ViewportBinding,
    ViewportBinding,
    SurfaceRecoveryObligationId,
    NativeStagingResourceId,
) {
    let domain = EngineAuthorityDomainId::new_for_test(1);
    let epoch = WorkspaceEpoch::default();
    let surface = SurfaceId::new(710);
    let mut coordinator = ViewportCoordinator::default();
    let replacement = coordinator
        .registry
        .reserve(epoch, surface, ViewportRole::Child)
        .expect("test replacement must register pending");
    let destroyed = ViewportBinding::new(
        domain,
        epoch,
        surface,
        WindowToken::new(709),
        WindowIncarnation::new(709),
    );
    let obligation = recovery(712);
    let descriptor = retained_resource_descriptor(NativeCreateSagaId::new(713));
    let resource = descriptor.id();
    coordinator
        .insert_native_staging_resource_for_test(
            descriptor,
            NativeStagingResourceOwner::SurfaceRecovery {
                obligation,
                binding: destroyed,
            },
        )
        .expect("test recovery must retain its resource");
    assert_eq!(
        coordinator.validate_native_staging_resource_conservation(),
        Err(ViewportCoordinatorError::UnreferencedNativeStagingResource { resource })
    );
    let replacement_effect = coordinator
        .effects
        .request(PlatformEffect::RequestReplacement {
            binding: replacement,
            placement: PhysicalRect::new(5.0, 0.0, 310.0, 225.0)
                .expect("test replacement placement must be valid"),
            role: ViewportRole::Child,
        })
        .expect("test replacement effect must enter the ledger");
    coordinator
        .recovery_replacements
        .begin(RecoveryPendingRequest::replacement(
            destroyed,
            ViewportRole::Child,
            obligation,
            Some(resource),
            replacement,
            replacement_effect,
        ))
        .expect("test recovery replacement must register");
    coordinator
        .validate_native_staging_resource_conservation()
        .expect("surface recovery must own exactly one retained resource");

    let emitted = coordinator.take_new_effects();
    assert!(matches!(
        emitted.as_slice(),
        [request] if request.id() == replacement_effect
    ));
    let hidden = coordinator
        .publish_snapshot(&recovery_presentation_snapshot(
            replacement,
            1,
            WindowPresentationState::Hidden,
            Some(replacement_effect),
        ))
        .expect("hidden replacement acknowledgement must publish");
    assert!(hidden.actions().is_empty());
    let pre_show = coordinator
        .native_staging_presentations()
        .next()
        .expect("hidden replacement must request pre-show staging");
    let pre_show_transition = coordinator
        .observe_native_staging_presentation(
            PresentedNativeStagingPresentation::mint_observed_for_test(pre_show, 1),
        )
        .expect("pre-show staging must reduce");
    assert!(pre_show_transition.is_none());
    let emitted = coordinator.take_new_effects();
    let show = emitted
        .iter()
        .find_map(|request| match request.effect() {
            PlatformEffect::ShowWindow { binding, .. } if *binding == replacement => {
                Some(request.id())
            }
            _ => None,
        })
        .expect("pre-show staging must emit ShowWindow");
    let acknowledged = coordinator
        .publish_snapshot(&recovery_presentation_snapshot(
            replacement,
            2,
            WindowPresentationState::Hidden,
            Some(show),
        ))
        .expect("exact ShowWindow acknowledgement must publish");
    assert!(acknowledged.actions().is_empty());
    let visible = coordinator
        .publish_snapshot(&recovery_presentation_snapshot(
            replacement,
            3,
            WindowPresentationState::Visible,
            None,
        ))
        .expect("later visible replacement must publish");
    assert!(visible.actions().is_empty());
    let post_show = coordinator
        .native_staging_presentations()
        .next()
        .expect("visible replacement must request post-show staging");
    let action = coordinator
        .observe_native_staging_presentation(
            PresentedNativeStagingPresentation::mint_observed_for_test(post_show, 2),
        )
        .expect("post-show staging must reduce");
    assert!(matches!(
        action,
        Some(ViewportLifecycleAction::RecoveryReplacementReady {
            destroyed_binding,
            replacement_binding,
            recovery_obligation,
        }) if destroyed_binding == destroyed
            && replacement_binding == replacement
            && recovery_obligation == obligation
    ));
    assert_eq!(
        coordinator.native_staging_resource_owner(resource),
        Some(NativeStagingResourceOwner::RecoveryReplacementFirstLive {
            obligation,
            binding: replacement,
        })
    );
    coordinator
        .validate_native_staging_resource_conservation()
        .expect("first-live replacement must own exactly one retained resource");
    (coordinator, destroyed, replacement, obligation, resource)
}

fn register(
    coordinator: &mut ViewportCoordinator,
    surface: u64,
    token: u64,
    role: ViewportRole,
) -> ViewportBinding {
    coordinator
        .register_existing(
            WorkspaceEpoch::new(1),
            SurfaceId::new(surface),
            WindowToken::new(token),
            role,
            (role == ViewportRole::Child).then(|| recovery(surface)),
        )
        .expect("test viewport must register")
}

#[test]
fn recovery_first_live_admission_releases_retained_resource_atomically() {
    let (mut coordinator, destroyed, replacement, obligation, resource) =
        ready_recovery_replacement();

    coordinator
        .admit_recovery_replacement_first_live(destroyed, replacement, obligation)
        .expect("first-live replacement must admit");

    assert_eq!(coordinator.native_staging_resource_owner(resource), None);
    assert!(coordinator.recovery_pending(destroyed.surface()).is_none());
    assert_eq!(
        coordinator
            .viewport(destroyed.surface())
            .map(ViewportRecord::admission),
        Some(ViewportAdmission::Admitted)
    );
    coordinator
        .validate_native_staging_resource_conservation()
        .expect("admission must release the terminal retained resource");
}

#[test]
fn recovered_host_can_preempt_first_live_without_losing_the_retained_resource() {
    let (mut coordinator, destroyed, replacement, _obligation, resource) =
        ready_recovery_replacement();

    coordinator
        .complete_pending_recovery(destroyed.surface())
        .expect("host recovery must atomically close the pre-admission replacement");

    assert_eq!(coordinator.native_staging_resource_owner(resource), None);
    assert!(matches!(
        coordinator
            .recovery_pending(destroyed.surface())
            .expect("replacement cleanup remains queryable")
            .status(),
        RecoveryPendingStatus::CompensatingReplacement { .. }
    ));
    assert_eq!(
        coordinator
            .viewport(destroyed.surface())
            .map(ViewportRecord::binding),
        Some(replacement)
    );
    coordinator
        .validate_native_staging_resource_conservation()
        .expect("compensation must not retain an unreferenced staging resource");
}

#[test]
fn workspace_replacement_quarantines_runtime_recovery_resource_until_exact_destruction() {
    let (mut coordinator, _destroyed, replacement, _obligation, resource) =
        ready_recovery_replacement();

    coordinator
        .reconcile_workspace_epoch(WorkspaceEpoch::new(1), &BTreeSet::new())
        .expect("workspace replacement must retire the runtime recovery binding");

    let retirement = coordinator
        .binding_retirement
        .get(&replacement)
        .expect("the exact runtime replacement must remain quarantined");
    assert!(matches!(
        retirement.status(),
        BindingRetirementStatus::CleanupRequested { .. }
    ));
    assert_eq!(
        retirement.origin(),
        BindingRetirementOrigin::WorkspaceReplaced
    );
    assert_eq!(retirement.retained_staging_resource(), Some(resource));
    assert!(coordinator.recovery_replacements.is_empty());
    assert_eq!(
        coordinator.native_staging_resource_owner(resource),
        Some(NativeStagingResourceOwner::BindingRetirement(replacement))
    );
    coordinator
        .validate_native_staging_resource_conservation()
        .expect("binding retirement must conserve the runtime recovery resource");

    coordinator
        .publish_snapshot(&snapshot_with_close_at(
            4,
            Vec::new(),
            vec![close_observation(
                replacement,
                4,
                WindowCloseState::Destroyed,
            )],
        ))
        .expect("exact replacement destruction must publish");

    assert!(!coordinator.binding_retirement.contains_key(&replacement));
    assert_eq!(coordinator.native_staging_resource_owner(resource), None);
    coordinator
        .validate_native_staging_resource_conservation()
        .expect("exact destruction must release the terminal retained resource");
}

#[test]
fn staging_close_destruction_returns_first_live_resource_to_surface_recovery() {
    let (mut coordinator, destroyed, replacement, obligation, resource) =
        ready_recovery_replacement();
    let requested = close_observation(replacement, 4, WindowCloseState::LiveRequested);
    let close = coordinator
        .publish_snapshot(&snapshot_with_close_at(
            4,
            vec![observed_window_at(replacement, 4)],
            vec![requested],
        ))
        .expect("pre-admission close must publish");
    let consumed = close
        .registry_events()
        .iter()
        .find_map(|event| match event {
            RegistryEvent::CloseRequested { observation } => Some(*observation),
            RegistryEvent::Ready { .. }
            | RegistryEvent::PresentationChanged { .. }
            | RegistryEvent::FactsUnavailable { .. }
            | RegistryEvent::BindingMissing { .. }
            | RegistryEvent::CloseRequestCleared { .. }
            | RegistryEvent::Destroyed { .. } => None,
        })
        .expect("registry must publish the exact staging close edge");
    assert_eq!(consumed.binding(), replacement);
    assert_eq!(
        consumed.known_state(),
        Some(WindowCloseState::LiveRequested)
    );
    assert!(close.staging_close_was_consumed(consumed));

    let destroyed_transition = coordinator
        .publish_snapshot(&snapshot_with_close_at(
            5,
            Vec::new(),
            vec![close_observation(
                replacement,
                5,
                WindowCloseState::Destroyed,
            )],
        ))
        .expect("staging replacement destruction must publish");

    assert!(matches!(
        destroyed_transition.actions(),
        [ViewportLifecycleAction::RecoveryReplacementLost {
            destroyed_binding,
            replacement_binding,
            recovery_obligation,
        }] if *destroyed_binding == destroyed
            && *replacement_binding == replacement
            && *recovery_obligation == obligation
    ));
    assert_eq!(
        coordinator.native_staging_resource_owner(resource),
        Some(NativeStagingResourceOwner::SurfaceRecovery {
            obligation,
            binding: destroyed,
        })
    );
    let pending = coordinator
        .recovery_pending(destroyed.surface())
        .expect("source recovery must remain pending");
    assert_eq!(pending.replacement_binding(), None);
    assert_eq!(
        pending.status(),
        RecoveryPendingStatus::AwaitingRecoveryHost
    );
    coordinator
        .validate_native_staging_resource_conservation()
        .expect("staging destruction must restore exact recovery ownership");
}

#[test]
fn capability_generation_exhaustion_rolls_back_the_complete_snapshot() {
    let mut coordinator = ViewportCoordinator::default();
    coordinator.exhaust_capability_generation();
    let before = coordinator.clone();
    let mut capabilities = PlatformCapabilities::default();
    capabilities.set_window_activation_control(PlatformCapability::Supported);
    let snapshot = platform_snapshot(
        1,
        capabilities,
        unknown_focus_observation(
            FocusObservationGeneration::new(1),
            AuthorityUnavailableReason::NotReported,
        ),
        Vec::new(),
        Vec::new(),
        unknown_work_areas(1),
    )
    .expect("empty snapshot must be valid");

    assert_eq!(
        coordinator.publish_snapshot(&snapshot),
        Err(ViewportCoordinatorError::CapabilityGenerationExhausted)
    );
    assert_eq!(coordinator, before);
}

#[test]
fn work_area_generation_exhaustion_rolls_back_the_complete_snapshot() {
    let mut coordinator = ViewportCoordinator::default();
    coordinator.exhaust_work_area_generation();
    let before = coordinator.clone();
    let mut capabilities = PlatformCapabilities::default();
    capabilities.set_work_area(PlatformCapability::Supported);
    let snapshot = platform_snapshot(
        1,
        capabilities,
        unknown_focus_observation(
            FocusObservationGeneration::new(1),
            AuthorityUnavailableReason::NotReported,
        ),
        Vec::new(),
        Vec::new(),
        known_work_areas(
            1,
            vec![ObservedWorkArea::new(
                WorkAreaToken::new(1),
                PhysicalRect::new(-1920.0, 0.0, 1920.0, 1080.0)
                    .expect("test work area must be valid"),
                ScaleFactor::new(1.0).expect("test work-area scale must be valid"),
            )],
        ),
    )
    .expect("work-area snapshot must be valid");

    assert_eq!(
        coordinator.publish_snapshot(&snapshot),
        Err(ViewportCoordinatorError::WorkAreaGenerationExhausted)
    );
    assert_eq!(coordinator, before);
}

#[test]
fn stale_platform_batch_rejects_newer_lane_facts_atomically() {
    let mut coordinator = ViewportCoordinator::default();
    let current = register(&mut coordinator, 1, 1, ViewportRole::Root);
    let delayed = register(&mut coordinator, 2, 2, ViewportRole::Root);
    let mut capabilities = PlatformCapabilities::default();
    capabilities.set_authoritative_inventory(PlatformCapability::Supported);

    coordinator
        .publish_snapshot(
            &platform_snapshot_with_generations(
                2,
                1,
                1,
                capabilities.clone(),
                unknown_focus_observation(
                    FocusObservationGeneration::new(1),
                    AuthorityUnavailableReason::NotReported,
                ),
                vec![observed_window(current)],
                Vec::new(),
                unknown_work_areas(1),
            )
            .expect("current platform batch must be valid"),
        )
        .expect("current platform batch must publish");
    let before = coordinator.clone();

    let stale = platform_snapshot_with_generations(
        1,
        2,
        2,
        capabilities,
        unknown_focus_observation(
            FocusObservationGeneration::new(2),
            AuthorityUnavailableReason::NotReported,
        ),
        vec![observed_window(delayed)],
        Vec::new(),
        unknown_work_areas(2),
    )
    .expect("delayed platform batch remains structurally valid");

    assert_eq!(
        coordinator.publish_snapshot(&stale),
        Err(ViewportCoordinatorError::StalePlatformSnapshot {
            submitted: PlatformSnapshotGeneration::new(1),
            current: PlatformSnapshotGeneration::new(2),
        })
    );
    assert_eq!(
        coordinator, before,
        "a delayed complete batch cannot publish newer per-lane facts"
    );
}

#[test]
fn equal_platform_batch_is_idempotent_and_conflicting_window_facts_fail_closed() {
    let mut coordinator = ViewportCoordinator::default();
    let binding = register(&mut coordinator, 1, 1, ViewportRole::Root);
    let mut capabilities = PlatformCapabilities::default();
    capabilities.set_authoritative_inventory(PlatformCapability::Supported);
    let snapshot = platform_snapshot_with_generations(
        1,
        1,
        1,
        capabilities.clone(),
        unknown_focus_observation(
            FocusObservationGeneration::new(1),
            AuthorityUnavailableReason::NotReported,
        ),
        vec![observed_window(binding)],
        Vec::new(),
        unknown_work_areas(1),
    )
    .expect("initial platform batch must be valid");
    coordinator
        .publish_snapshot(&snapshot)
        .expect("initial platform batch must publish");
    let before = coordinator.clone();

    let duplicate = coordinator
        .publish_snapshot(&snapshot)
        .expect("an exact retry must be an idempotent success");
    assert!(duplicate.registry_events().is_empty());
    assert!(duplicate.actions().is_empty());
    assert_eq!(coordinator, before);

    let conflict = platform_snapshot_with_generations(
        1,
        1,
        1,
        capabilities,
        unknown_focus_observation(
            FocusObservationGeneration::new(1),
            AuthorityUnavailableReason::NotReported,
        ),
        vec![
            observed_window(binding)
                .with_input_state(Authority::Known(WindowInputState::PassThrough)),
        ],
        Vec::new(),
        unknown_work_areas(1),
    )
    .expect("conflicting window batch remains structurally valid");
    assert_eq!(
        coordinator.publish_snapshot(&conflict),
        Err(ViewportCoordinatorError::ConflictingPlatformSnapshot {
            generation: PlatformSnapshotGeneration::new(1),
        })
    );
    assert_eq!(coordinator, before);
}

#[test]
fn stale_inventory_envelope_rejects_the_complete_platform_snapshot() {
    let mut coordinator = ViewportCoordinator::default();
    let current = coordinator
        .register_existing(
            WorkspaceEpoch::new(0),
            SurfaceId::new(1),
            WindowToken::new(1),
            ViewportRole::Root,
            None,
        )
        .expect("current binding must register");
    let delayed = coordinator
        .register_existing(
            WorkspaceEpoch::new(0),
            SurfaceId::new(2),
            WindowToken::new(2),
            ViewportRole::Root,
            None,
        )
        .expect("delayed binding must register");
    let mut capabilities = PlatformCapabilities::default();
    capabilities.set_authoritative_inventory(PlatformCapability::Supported);

    coordinator
        .publish_snapshot(
            &platform_snapshot(
                2,
                capabilities.clone(),
                unknown_focus_observation(
                    FocusObservationGeneration::new(2),
                    AuthorityUnavailableReason::NotReported,
                ),
                vec![observed_window(current)],
                Vec::new(),
                unknown_work_areas(2),
            )
            .expect("current snapshot must be valid"),
        )
        .expect("current snapshot must publish");
    let before = coordinator.clone();

    let stale = platform_snapshot(
        1,
        capabilities,
        unknown_focus_observation(
            FocusObservationGeneration::new(1),
            AuthorityUnavailableReason::NotReported,
        ),
        vec![observed_window(delayed)],
        Vec::new(),
        unknown_work_areas(1),
    )
    .expect("stale snapshot must remain structurally valid");

    assert_eq!(
        coordinator.publish_snapshot(&stale),
        Err(ViewportCoordinatorError::StaleInventoryObservation {
            submitted: InventoryObservationGeneration::new(1),
            current: InventoryObservationGeneration::new(2),
        })
    );
    assert_eq!(
        coordinator, before,
        "no fact from a stale inventory batch may mutate current authority"
    );
}

#[test]
fn conflicting_inventory_envelope_rejects_the_complete_platform_snapshot() {
    let mut coordinator = ViewportCoordinator::default();
    let current = coordinator
        .register_existing(
            WorkspaceEpoch::new(0),
            SurfaceId::new(1),
            WindowToken::new(1),
            ViewportRole::Root,
            None,
        )
        .expect("current binding must register");
    let conflicting = coordinator
        .register_existing(
            WorkspaceEpoch::new(0),
            SurfaceId::new(2),
            WindowToken::new(2),
            ViewportRole::Root,
            None,
        )
        .expect("conflicting binding must register");
    let mut capabilities = PlatformCapabilities::default();
    capabilities.set_authoritative_inventory(PlatformCapability::Supported);

    coordinator
        .publish_snapshot(
            &platform_snapshot(
                1,
                capabilities.clone(),
                unknown_focus_observation(
                    FocusObservationGeneration::new(1),
                    AuthorityUnavailableReason::NotReported,
                ),
                vec![observed_window(current)],
                Vec::new(),
                unknown_work_areas(1),
            )
            .expect("initial snapshot must be valid"),
        )
        .expect("initial snapshot must publish");
    let before = coordinator.clone();

    let conflict = platform_snapshot(
        1,
        capabilities,
        unknown_focus_observation(
            FocusObservationGeneration::new(2),
            AuthorityUnavailableReason::NotReported,
        ),
        vec![observed_window(conflicting)],
        Vec::new(),
        unknown_work_areas(2),
    )
    .expect("conflicting snapshot remains structurally valid");

    assert_eq!(
        coordinator.publish_snapshot(&conflict),
        Err(ViewportCoordinatorError::ConflictingInventoryObservation {
            generation: InventoryObservationGeneration::new(1),
        })
    );
    assert_eq!(
        coordinator, before,
        "no binding-scoped fact from a conflicting inventory batch may publish"
    );
}

#[test]
fn skipped_inventory_generation_rejects_window_facts_until_explicit_resync() {
    let mut coordinator = ViewportCoordinator::default();
    let current = coordinator
        .register_existing(
            WorkspaceEpoch::new(0),
            SurfaceId::new(1),
            WindowToken::new(1),
            ViewportRole::Root,
            None,
        )
        .expect("current binding must register");
    let skipped_batch_binding = coordinator
        .register_existing(
            WorkspaceEpoch::new(0),
            SurfaceId::new(2),
            WindowToken::new(2),
            ViewportRole::Root,
            None,
        )
        .expect("skipped-batch binding must register");
    let mut capabilities = PlatformCapabilities::default();
    capabilities.set_authoritative_inventory(PlatformCapability::Supported);

    coordinator
        .publish_snapshot(
            &platform_snapshot(
                1,
                capabilities.clone(),
                unknown_focus_observation(
                    FocusObservationGeneration::new(1),
                    AuthorityUnavailableReason::NotReported,
                ),
                vec![observed_window(current)],
                Vec::new(),
                unknown_work_areas(1),
            )
            .expect("initial snapshot must be valid"),
        )
        .expect("initial snapshot must publish");
    let before = coordinator.clone();

    let skipped = platform_snapshot(
        3,
        capabilities.clone(),
        unknown_focus_observation(
            FocusObservationGeneration::new(3),
            AuthorityUnavailableReason::NotReported,
        ),
        vec![observed_window(skipped_batch_binding)],
        Vec::new(),
        unknown_work_areas(3),
    )
    .expect("skipped snapshot remains structurally valid");
    assert_eq!(
        coordinator.publish_snapshot(&skipped),
        Err(
            ViewportCoordinatorError::InventoryObservationGenerationGap {
                previous: InventoryObservationGeneration::new(1),
                submitted: InventoryObservationGeneration::new(3),
            }
        )
    );
    assert_eq!(
        coordinator, before,
        "no fact from a discontinuous inventory batch may publish"
    );

    let tombstone = PlatformSnapshot::new(
        PlatformSnapshotGeneration::new(2),
        CapabilityRosterObservation::new(
            CapabilityObservationGeneration::new(2),
            Authority::Known(capabilities.clone()),
        ),
        unknown_focus_observation(
            FocusObservationGeneration::new(2),
            AuthorityUnavailableReason::NotReported,
        ),
        WindowInventoryObservation::unknown(
            InventoryObservationGeneration::new(2),
            AuthorityUnavailableReason::NotReported,
        ),
        Vec::new(),
        Vec::new(),
        unknown_work_areas(2),
    )
    .expect("explicit inventory tombstone must be valid");
    coordinator
        .publish_snapshot(&tombstone)
        .expect("the missing generation may be restored by an explicit tombstone");

    coordinator
        .publish_snapshot(
            &platform_snapshot(
                3,
                capabilities,
                unknown_focus_observation(
                    FocusObservationGeneration::new(3),
                    AuthorityUnavailableReason::NotReported,
                ),
                vec![observed_window(current)],
                Vec::new(),
                unknown_work_areas(3),
            )
            .expect("post-tombstone inventory must be valid"),
        )
        .expect("the next contiguous known inventory must publish");
    assert_eq!(
        coordinator.inventory_observations.generation_watermark(),
        Some(InventoryObservationGeneration::new(3))
    );
}

#[test]
fn work_area_provider_stream_rejects_resurrection_and_separates_generations() {
    let bounds = PhysicalRect::new(0.0, 0.0, 1920.0, 1080.0).expect("test work area must be valid");
    let scale = ScaleFactor::new(1.0).expect("test work-area scale must be valid");
    let area = |token| ObservedWorkArea::new(WorkAreaToken::new(token), bounds, scale);
    let snapshot = |focus_generation, observation| {
        let mut capabilities = PlatformCapabilities::default();
        capabilities.set_work_area(PlatformCapability::Supported);
        platform_snapshot(
            focus_generation,
            capabilities,
            unknown_focus_observation(
                FocusObservationGeneration::new(focus_generation),
                AuthorityUnavailableReason::NotReported,
            ),
            Vec::new(),
            Vec::new(),
            observation,
        )
        .expect("work-area snapshot must be valid")
    };
    let mut coordinator = ViewportCoordinator::default();

    let initial = snapshot(1, known_work_areas(1, vec![area(1), area(2)]));
    let initial_transition = coordinator
        .publish_snapshot(&initial)
        .expect("initial roster must publish");
    assert!(initial_transition.work_areas_changed());
    assert_eq!(
        coordinator.work_area_generation(),
        WorkAreaGeneration::new(1)
    );

    let reduced = snapshot(2, known_work_areas(2, vec![area(1)]));
    coordinator
        .publish_snapshot(&reduced)
        .expect("reduced roster must publish");
    assert_eq!(
        coordinator.work_area_generation(),
        WorkAreaGeneration::new(2)
    );
    assert!(coordinator.work_area(WorkAreaToken::new(1)).is_some());
    assert!(coordinator.work_area(WorkAreaToken::new(2)).is_none());

    let delayed = snapshot(3, known_work_areas(1, vec![area(1), area(2)]));
    let delayed_transition = coordinator
        .publish_snapshot(&delayed)
        .expect("older provider roster must be inert");
    assert!(!delayed_transition.work_areas_changed());
    assert_eq!(
        coordinator.work_area_generation(),
        WorkAreaGeneration::new(2)
    );
    assert_eq!(
        coordinator.work_area_observation_generation(),
        Some(WorkAreaObservationGeneration::new(2))
    );
    assert!(coordinator.work_area(WorkAreaToken::new(2)).is_none());

    let same_semantics = snapshot(4, known_work_areas(3, vec![area(1)]));
    let same_transition = coordinator
        .publish_snapshot(&same_semantics)
        .expect("new provider capture must publish");
    assert!(!same_transition.work_areas_changed());
    assert_eq!(
        coordinator.work_area_generation(),
        WorkAreaGeneration::new(2)
    );
    assert_eq!(
        coordinator.work_area_observation_generation(),
        Some(WorkAreaObservationGeneration::new(3))
    );
}

#[test]
fn work_area_provider_conflict_requires_tombstone_before_restore() {
    let bounds = PhysicalRect::new(0.0, 0.0, 1920.0, 1080.0).expect("test work area must be valid");
    let scale = ScaleFactor::new(1.0).expect("test work-area scale must be valid");
    let area = ObservedWorkArea::new(WorkAreaToken::new(1), bounds, scale);
    let snapshot = |focus_generation, observation| {
        let mut capabilities = PlatformCapabilities::default();
        capabilities.set_work_area(PlatformCapability::Supported);
        platform_snapshot(
            focus_generation,
            capabilities,
            unknown_focus_observation(
                FocusObservationGeneration::new(focus_generation),
                AuthorityUnavailableReason::NotReported,
            ),
            Vec::new(),
            Vec::new(),
            observation,
        )
        .expect("work-area snapshot must be valid")
    };
    let mut coordinator = ViewportCoordinator::default();

    coordinator
        .publish_snapshot(&snapshot(1, known_work_areas(1, vec![area])))
        .expect("initial roster must publish");
    coordinator
        .publish_snapshot(&snapshot(
            2,
            known_work_areas(
                1,
                vec![ObservedWorkArea::new(
                    WorkAreaToken::new(1),
                    PhysicalRect::new(0.0, 0.0, 1600.0, 900.0)
                        .expect("conflicting bounds must be valid"),
                    scale,
                )],
            ),
        ))
        .expect("conflict must fail closed without rejecting the host snapshot");
    assert!(coordinator.work_area(WorkAreaToken::new(1)).is_none());
    assert_eq!(
        coordinator.work_area_generation(),
        WorkAreaGeneration::new(2)
    );

    coordinator
        .publish_snapshot(&snapshot(3, known_work_areas(2, vec![area])))
        .expect("known roster after conflict remains quarantined");
    assert!(coordinator.work_area(WorkAreaToken::new(1)).is_none());
    assert_eq!(
        coordinator.work_area_generation(),
        WorkAreaGeneration::new(2)
    );

    coordinator
        .publish_snapshot(&snapshot(4, unknown_work_areas(3)))
        .expect("versioned tombstone must clear quarantine");
    coordinator
        .publish_snapshot(&snapshot(5, known_work_areas(4, vec![area])))
        .expect("known roster after tombstone must restore authority");
    assert!(coordinator.work_area(WorkAreaToken::new(1)).is_some());
    assert_eq!(
        coordinator.work_area_generation(),
        WorkAreaGeneration::new(3)
    );
}

#[test]
fn focus_requests_form_one_provider_serialized_global_lane() {
    let mut coordinator = ViewportCoordinator::default();
    let first_binding = register(&mut coordinator, 7, 70, ViewportRole::Root);
    let second_binding = register(&mut coordinator, 8, 80, ViewportRole::Root);
    let first = coordinator
        .request_focus_binding(first_binding)
        .expect("first focus request must enter the ledger");
    let second = coordinator
        .request_focus_binding(second_binding)
        .expect("second focus request must enter the ledger");
    assert!(matches!(
        coordinator.take_new_effects().as_slice(),
        [first_request, second_request]
            if first_request.id() == first
                && second_request.id() == second
                && matches!(
                    first_request.effect(),
                    PlatformEffect::RequestFocus {
                        binding,
                        after: None,
                    } if *binding == first_binding
                )
                && matches!(
                    second_request.effect(),
                    PlatformEffect::RequestFocus {
                        binding,
                        after: Some(predecessor),
                    } if *binding == second_binding && *predecessor == first
                )
    ));
}

#[test]
fn restore_skips_only_a_focus_predecessor_the_provider_never_received() {
    for emit_old in [false, true] {
        let mut coordinator = ViewportCoordinator::default();
        let old_binding = register(&mut coordinator, 9, 90, ViewportRole::Root);
        let old = coordinator
            .request_focus_binding(old_binding)
            .expect("old focus request must allocate");
        if emit_old {
            let emitted = coordinator.take_new_effects();
            assert_eq!(emitted.len(), 1);
            assert_eq!(emitted[0].id(), old);
        }

        let desired = BTreeSet::from([old_binding.surface()]);
        coordinator
            .reconcile_workspace_epoch(WorkspaceEpoch::new(2), &desired)
            .expect("restore must reconcile focus history");
        let existing_binding = coordinator
            .registry()
            .record(old_binding.surface())
            .map(ViewportRecord::binding);
        let new_binding = existing_binding.unwrap_or_else(|| {
            coordinator
                .register_existing(
                    WorkspaceEpoch::new(2),
                    old_binding.surface(),
                    WindowToken::new(91),
                    ViewportRole::Root,
                    None,
                )
                .expect("restored binding must register")
        });
        let new = coordinator
            .request_focus_binding(new_binding)
            .expect("new focus request must allocate");
        let request = coordinator
            .take_new_effects()
            .into_iter()
            .find(|request| request.id() == new)
            .expect("new focus request must be emitted");
        assert!(matches!(
            request.effect(),
            PlatformEffect::RequestFocus { after, .. }
                if *after == emit_old.then_some(old)
        ));
    }
}

#[test]
fn typed_close_observations_emit_one_exact_request_and_clear_edge() {
    let mut coordinator = ViewportCoordinator::default();
    let binding = register(&mut coordinator, 1, 10, ViewportRole::Root);
    coordinator
        .publish_snapshot(&snapshot(vec![observed_window(binding)]))
        .expect("ready observation must publish");
    let requested = close_observation(binding, 1, WindowCloseState::LiveRequested);
    let request_edge = coordinator
        .publish_snapshot(&snapshot_with_close_at(
            2,
            vec![observed_window(binding)],
            vec![requested],
        ))
        .expect("typed close request must publish");
    let expected_requested = requested.with_inventory_generation(InventoryGeneration::new(2));
    assert_eq!(
        request_edge.registry_events(),
        &[RegistryEvent::CloseRequested {
            observation: expected_requested,
        }]
    );

    let repeated = coordinator
        .publish_snapshot(&snapshot_with_close_at(
            3,
            vec![observed_window(binding)],
            vec![requested],
        ))
        .expect("same typed observation must remain idempotent");
    assert!(
        repeated
            .registry_events()
            .iter()
            .all(|event| !matches!(event, RegistryEvent::CloseRequested { .. }))
    );

    let cleared = close_observation(binding, 2, WindowCloseState::LiveClear);
    let clear_edge = coordinator
        .publish_snapshot(&snapshot_with_close_at(
            4,
            vec![observed_window(binding)],
            vec![cleared],
        ))
        .expect("typed close clear must publish");
    let expected_cleared = cleared.with_inventory_generation(InventoryGeneration::new(4));
    assert!(clear_edge.registry_events().iter().any(|event| {
        matches!(
            event,
            RegistryEvent::CloseRequestCleared {
                requested: actual_requested,
                observation,
            }
                if *actual_requested == expected_requested && *observation == expected_cleared
        )
    }));
}

#[test]
fn native_close_effect_waits_for_the_exact_typed_close_edge() {
    let mut coordinator = ViewportCoordinator::default();
    let binding = register(&mut coordinator, 2, 20, ViewportRole::Root);
    coordinator
        .publish_snapshot(&snapshot(vec![observed_window(binding)]))
        .expect("ready observation must publish");

    let (request, edge) = native_close_request(
        binding,
        CloseObservationGeneration::new(1),
        InventoryGeneration::new(2),
    );
    let effect = coordinator
        .request_native_close_resolution(request, edge, NativeCloseResolution::Accept, None)
        .expect("native close effect must queue");
    assert!(coordinator.take_new_effects().is_empty());

    let requested = close_observation(binding, 1, WindowCloseState::LiveRequested);
    coordinator
        .publish_snapshot(&snapshot_with_close_at(
            2,
            vec![observed_window(binding)],
            vec![requested],
        ))
        .expect("matching close edge must publish");
    let emitted = coordinator.take_new_effects();
    let [emitted] = emitted.as_slice() else {
        panic!("one causally fenced native close effect must emit: {emitted:?}");
    };
    assert_eq!(emitted.id(), effect);
    assert!(matches!(
        emitted.effect(),
        PlatformEffect::ResolveNativeClose {
            request: actual_request,
            edge: actual_edge,
            resolution: NativeCloseResolution::Accept,
        } if *actual_request == request && *actual_edge == edge
    ));
    let fence = emitted
        .native_close_emission_fence()
        .expect("native close emission must retain its observation fence");
    assert_eq!(fence.binding(), binding);
    assert_eq!(fence.observed_through(), CloseObservationGeneration::new(1));
    assert_eq!(fence.received_through(), InventoryGeneration::new(2));
    assert_eq!(fence.after_effect(), None);
}

#[test]
fn native_close_effect_does_not_cross_to_a_reissued_same_binding_edge() {
    let mut coordinator = ViewportCoordinator::default();
    let binding = register(&mut coordinator, 4, 40, ViewportRole::Root);
    coordinator
        .publish_snapshot(&snapshot(vec![observed_window(binding)]))
        .expect("ready observation must publish");

    let (request, edge) = native_close_request(
        binding,
        CloseObservationGeneration::new(1),
        InventoryGeneration::new(2),
    );
    let effect = coordinator
        .request_native_close_resolution(request, edge, NativeCloseResolution::Cancel, None)
        .expect("native close effect must queue");

    coordinator
        .publish_snapshot(&snapshot_with_close_at(
            2,
            vec![observed_window(binding)],
            vec![close_observation(
                binding,
                1,
                WindowCloseState::LiveRequested,
            )],
        ))
        .expect("first close edge must publish");
    coordinator
        .publish_snapshot(&snapshot_with_close_at(
            3,
            vec![observed_window(binding)],
            vec![close_observation(binding, 2, WindowCloseState::LiveClear)],
        ))
        .expect("close clear must publish");
    let reissued = close_observation(binding, 3, WindowCloseState::LiveRequested);
    coordinator
        .publish_snapshot(&snapshot_with_close_at(
            4,
            vec![observed_window(binding)],
            vec![reissued],
        ))
        .expect("reissued close edge must publish");

    assert_eq!(
        coordinator.native_close_edge_disposition(edge),
        NativeCloseEdgeDisposition::DifferentPending {
            observation: reissued.with_inventory_generation(InventoryGeneration::new(4)),
        }
    );
    assert!(coordinator.take_new_effects().is_empty());
    let queued = coordinator
        .effects()
        .record(effect)
        .expect("old native close request must remain queryable");
    assert!(!queued.was_emitted());
    assert_eq!(queued.request().native_close_edge(), Some(edge));
}

#[test]
fn destroyed_tombstone_preserves_exact_binding_and_complete_recovery() {
    let mut coordinator = ViewportCoordinator::default();
    let binding = register(&mut coordinator, 3, 30, ViewportRole::Child);
    coordinator
        .publish_snapshot(&snapshot(vec![observed_window(binding)]))
        .expect("ready observation must publish");
    let focus_effect = coordinator
        .request_focus_binding(binding)
        .expect("focus effect must queue");
    let _ = coordinator.take_new_effects();

    let destroyed_observation = close_observation(binding, 1, WindowCloseState::Destroyed);
    let destroyed = coordinator
        .publish_snapshot(&snapshot_with_close_at(
            2,
            Vec::new(),
            vec![destroyed_observation],
        ))
        .expect("destroyed tombstone must publish");
    let expected_destroyed =
        destroyed_observation.with_inventory_generation(InventoryGeneration::new(2));
    assert_eq!(
        destroyed.registry_events(),
        &[RegistryEvent::Destroyed {
            observation: expected_destroyed,
        }]
    );
    assert!(matches!(
        destroyed.actions(),
        [ViewportLifecycleAction::SurfaceDestroyed {
            observation,
            source_coordinates: Some(_),
        }] if *observation == expected_destroyed
    ));
    assert_eq!(
        coordinator
            .effects()
            .record(focus_effect)
            .map(crate::effect::EffectRecord::phase),
        Some(EffectPhase::Destroyed {
            inventory_generation: InventoryGeneration::new(2),
        })
    );
}

#[test]
fn external_vacancy_waits_for_late_input_restore_then_exact_destruction() {
    let (mut coordinator, binding) = external_routing_coordinator();
    let pointer = PointerId::new(1);
    let enable = coordinator
        .begin_drag_routing(pointer, binding.surface())
        .expect("external drag route must begin")
        .expect("receiving input must request pass-through");
    let _ = coordinator.take_new_effects();
    let authority = coordinator.capture_surface_vacancy_authority(binding.surface());

    let first = coordinator
        .settle_surface_vacancies(&[authority], &BTreeSet::new())
        .expect("first vacancy edge must revoke logical authority");
    assert_eq!(first.logical_vacated_bindings(), &[binding]);
    assert!(first.effects().is_empty());
    assert!(coordinator.registry.record(binding.surface()).is_none());
    assert!(matches!(
        coordinator
            .binding_retirement
            .get(&binding)
            .map(BindingRetirement::status),
        Some(BindingRetirementStatus::AwaitingInputRestore)
    ));
    assert_eq!(coordinator.drag_source(pointer), None);
    let restore = take_single_pointer_restore(&mut coordinator, binding, Some(enable));

    coordinator
        .publish_snapshot(&routing_snapshot(
            binding,
            2,
            WindowInputState::PassThrough,
            WindowPresentationState::Visible,
            PlatformCapability::Supported,
            InputEffectAcknowledgement::known(Some(enable)),
        ))
        .expect("late enable acknowledgement must remain observable while vacating");
    assert!(coordinator.pointer_passthrough.contains_binding(binding));
    assert!(coordinator.binding_retirement.contains_key(&binding));

    coordinator
        .publish_snapshot(&routing_snapshot(
            binding,
            3,
            WindowInputState::ReceivesInput,
            WindowPresentationState::Visible,
            PlatformCapability::Supported,
            InputEffectAcknowledgement::known(Some(restore)),
        ))
        .expect("exact restore acknowledgement must settle the input obligation");
    assert!(!coordinator.pointer_passthrough.contains_binding(binding));
    assert!(coordinator.registry.record(binding.surface()).is_none());
    assert!(matches!(
        coordinator
            .binding_retirement
            .get(&binding)
            .map(BindingRetirement::status),
        Some(BindingRetirementStatus::AwaitingExactDestruction)
    ));
    assert!(coordinator.effects.records().all(|(_, record)| {
        !matches!(
            record.request().effect(),
            PlatformEffect::ReleaseChild { binding: actual }
                | PlatformEffect::RequestRootClose { binding: actual }
                | PlatformEffect::CompensatingClose {
                    binding: actual,
                    ..
                } if *actual == binding
        )
    }));

    coordinator
        .publish_snapshot(&snapshot_with_close_at(
            4,
            Vec::new(),
            vec![close_observation(binding, 1, WindowCloseState::Destroyed)],
        ))
        .expect("exact destruction must release the external token quarantine");
    assert!(!coordinator.binding_retirement.contains_key(&binding));
}

#[test]
fn failed_runtime_vacancy_release_keeps_an_exact_retry_owner() {
    let (mut coordinator, binding, authority, release) = runtime_child_vacancy();
    assert_eq!(
        report_effect_result(
            &mut coordinator,
            binding.epoch(),
            release,
            binding.epoch(),
            EffectDispatchResult::DispatchFailed(
                crate::effect::DispatchFailureReason::WindowUnavailable,
            ),
        ),
        EffectTransition::Applied
    );
    assert!(matches!(
        coordinator
            .effects()
            .record(release)
            .map(EffectRecord::phase),
        Some(EffectPhase::DispatchFailed(
            crate::effect::DispatchFailureReason::WindowUnavailable
        ))
    ));
    assert!(matches!(
        retirement(&coordinator, binding).status(),
        BindingRetirementStatus::CleanupFailed { effect } if effect == release
    ));
    coordinator
        .settle_surface_vacancies(&[authority], &BTreeSet::new())
        .expect("failed cleanup must remain durably owned without redispatch");
    assert!(coordinator.take_new_effects().is_empty());

    let retry = coordinator
        .retry_cleanup(release)
        .expect("an explicit provider recovery edge may retry the release");
    let emitted = coordinator.take_new_effects();
    assert!(matches!(
        emitted.as_slice(),
        [request]
            if request.id() == retry
                && matches!(
                    request.effect(),
                    PlatformEffect::ReleaseChild { binding: actual }
                        if *actual == binding
                )
    ));
    assert!(matches!(
        retirement(&coordinator, binding).status(),
        BindingRetirementStatus::CleanupRequested { effect } if effect == retry
    ));
}

#[test]
fn unsupported_runtime_cleanup_remains_blocked_until_explicit_retry() {
    let (mut coordinator, binding, authority, release) = runtime_child_vacancy();
    let reason = crate::effect::EffectUnsupportedReason::BackendUnsupported;
    assert_eq!(
        report_effect_result(
            &mut coordinator,
            binding.epoch(),
            release,
            binding.epoch(),
            EffectDispatchResult::Unsupported(reason),
        ),
        EffectTransition::Applied
    );
    assert_eq!(
        retirement(&coordinator, binding).status(),
        BindingRetirementStatus::CleanupBlocked {
            effect: release,
            reason,
        }
    );

    coordinator
        .settle_surface_vacancies(&[authority], &BTreeSet::new())
        .expect("a blocked cleanup must retain its exact owner");
    coordinator
        .publish_snapshot(&snapshot_at(2, Vec::new()))
        .expect("ordinary inventory publication must not retry cleanup");
    assert!(coordinator.take_new_effects().is_empty());

    let retry = coordinator
        .retry_cleanup(release)
        .expect("only an explicit recovery edge may retry blocked cleanup");
    assert!(matches!(
        coordinator.take_new_effects().as_slice(),
        [request]
            if request.id() == retry
                && matches!(
                    request.effect(),
                    PlatformEffect::ReleaseChild { binding: actual }
                        if *actual == binding
                )
    ));
    assert_eq!(
        retirement(&coordinator, binding).status(),
        BindingRetirementStatus::CleanupRequested { effect: retry }
    );
}

#[test]
fn indeterminate_runtime_vacancy_release_remains_owned_without_redispatch() {
    let (mut coordinator, binding, authority, release) = runtime_child_vacancy();
    assert_eq!(
        report_effect_result(
            &mut coordinator,
            binding.epoch(),
            release,
            binding.epoch(),
            EffectDispatchResult::Indeterminate(
                crate::effect::EffectIndeterminateReason::AcknowledgementLost,
            ),
        ),
        EffectTransition::Applied
    );
    assert!(matches!(
        coordinator
            .effects()
            .record(release)
            .map(EffectRecord::phase),
        Some(EffectPhase::Indeterminate(
            crate::effect::EffectIndeterminateReason::AcknowledgementLost
        ))
    ));
    coordinator
        .settle_surface_vacancies(&[authority], &BTreeSet::new())
        .expect("indeterminate cleanup must retain its exact owner");
    assert!(coordinator.take_new_effects().is_empty());
    assert!(matches!(
        coordinator.retry_cleanup(release),
        Err(ViewportCoordinatorError::CleanupEffectNotRetryable {
            effect,
            phase: EffectPhase::Indeterminate(
                crate::effect::EffectIndeterminateReason::AcknowledgementLost
            ),
        }) if effect == release
    ));
    assert!(matches!(
        retirement(&coordinator, binding).status(),
        BindingRetirementStatus::CleanupIndeterminate { effect } if effect == release
    ));
}

#[test]
fn runtime_vacancy_cleanup_retires_only_on_exact_destroyed_evidence() {
    let (mut coordinator, binding, authority, release) = runtime_child_vacancy();

    coordinator
        .publish_snapshot(&snapshot_at(2, Vec::new()))
        .expect("authoritative inventory absence must not imply destruction");
    assert!(coordinator.registry.record(binding.surface()).is_none());
    assert!(coordinator.binding_retirement.contains_key(&binding));
    assert!(!coordinator.binding_retirement.was_destroyed(binding));
    assert_eq!(
        coordinator
            .effects()
            .record(release)
            .map(EffectRecord::phase),
        Some(EffectPhase::Requested)
    );
    coordinator
        .settle_surface_vacancies(&[authority], &BTreeSet::new())
        .expect("inventory absence must leave the cleanup obligation pending");
    assert!(coordinator.take_new_effects().is_empty());

    let destroyed = coordinator
        .publish_snapshot(&snapshot_with_close_at(
            3,
            Vec::new(),
            vec![close_observation(binding, 1, WindowCloseState::Destroyed)],
        ))
        .expect("exact destroyed evidence must publish");
    assert!(destroyed.actions().is_empty());
    assert!(!coordinator.binding_retirement.contains_key(&binding));
    assert!(coordinator.binding_retirement.was_destroyed(binding));
    assert!(matches!(
        coordinator
            .effects()
            .record(release)
            .map(EffectRecord::phase),
        Some(EffectPhase::Destroyed { .. })
    ));

    let settled = coordinator
        .settle_surface_vacancies(&[authority], &BTreeSet::new())
        .expect("exact tombstone must satisfy the stale tick authority");
    assert_eq!(settled.logical_vacated_bindings(), &[binding]);
    assert!(coordinator.registry.record(binding.surface()).is_none());
}

#[test]
fn retired_close_stream_ignores_stale_destroyed_generation() {
    let (mut coordinator, binding, _, release) = runtime_child_vacancy();
    coordinator
        .publish_snapshot(&snapshot_with_close_at(
            2,
            vec![observed_window(binding)],
            vec![close_observation(binding, 10, WindowCloseState::LiveClear)],
        ))
        .expect("new live observation must advance retirement authority");
    coordinator
        .publish_snapshot(&snapshot_with_close_at(
            3,
            Vec::new(),
            vec![close_observation(binding, 9, WindowCloseState::Destroyed)],
        ))
        .expect("stale destroyed observation must be inert");

    assert!(coordinator.binding_retirement.contains_key(&binding));
    assert!(!coordinator.binding_retirement.was_destroyed(binding));
    assert_eq!(
        retirement(&coordinator, binding).status(),
        BindingRetirementStatus::CleanupRequested { effect: release }
    );
    assert_eq!(
        coordinator
            .effects()
            .record(release)
            .map(EffectRecord::phase),
        Some(EffectPhase::Requested)
    );
}

#[test]
fn terminal_tombstone_is_idempotent_after_same_token_aba_rebinding() {
    let (mut coordinator, old_binding, _, _) = runtime_child_vacancy();
    let mut initial_capabilities = PlatformCapabilities::default();
    initial_capabilities.set_authoritative_inventory(PlatformCapability::Supported);
    initial_capabilities.set_pointer_hit_test_observation(PlatformCapability::Supported);
    initial_capabilities.set_pointer_hit_test_control(PlatformCapability::Supported);
    let destroyed_snapshot = platform_snapshot(
        2,
        initial_capabilities,
        unknown_focus_observation(
            FocusObservationGeneration::new(2),
            AuthorityUnavailableReason::NotReported,
        ),
        Vec::new(),
        vec![close_observation(
            old_binding,
            1,
            WindowCloseState::Destroyed,
        )],
        unknown_work_areas(2),
    )
    .expect("destroyed snapshot must be valid");
    coordinator
        .publish_snapshot(&destroyed_snapshot)
        .expect("exact destruction must retire the old binding");
    assert!(coordinator.binding_retirement.was_destroyed(old_binding));
    assert!(!coordinator.binding_retirement.contains_key(&old_binding));

    let new_binding = coordinator
        .register_existing(
            old_binding.epoch(),
            old_binding.surface(),
            old_binding.token(),
            ViewportRole::Root,
            None,
        )
        .expect("exact destruction must allow the token to receive a new incarnation");
    assert_ne!(new_binding, old_binding);
    assert_eq!(new_binding.token(), old_binding.token());

    let new_window = observed_window(new_binding)
        .with_input_observation(WindowInputObservation::new(
            new_binding,
            InputObservationGeneration::new(1),
            Authority::Known(WindowInputState::ReceivesInput),
            InputEffectAcknowledgement::known(None),
        ))
        .with_presentation_observation(WindowPresentationObservation::new(
            new_binding,
            PresentationObservationGeneration::new(1),
            Authority::Known(WindowPresentationState::Visible),
            PresentationEffectAcknowledgement::known(None),
        ));
    let mut capabilities = PlatformCapabilities::default();
    capabilities.set_authoritative_inventory(PlatformCapability::Supported);
    capabilities.set_pointer_hit_test_observation(PlatformCapability::Supported);
    let combined_snapshot = platform_snapshot(
        3,
        capabilities.clone(),
        unknown_focus_observation(
            FocusObservationGeneration::new(3),
            AuthorityUnavailableReason::NotReported,
        ),
        vec![new_window],
        vec![close_observation(
            old_binding,
            2,
            WindowCloseState::Destroyed,
        )],
        unknown_work_areas(3),
    )
    .expect("terminal old tombstone and new exact window may share one snapshot");
    coordinator
        .publish_snapshot(&combined_snapshot)
        .expect("a repeated terminal tombstone must be inert beside the new incarnation");

    let new_record = coordinator
        .registry
        .record(new_binding.surface())
        .expect("new binding must remain current");
    assert_eq!(new_record.binding(), new_binding);
    assert_eq!(new_record.lifecycle(), ViewportLifecycle::Ready);
    assert!(new_record.coordinates().is_some());
    assert!(coordinator.binding_retirement.was_destroyed(old_binding));

    let stale_window =
        observed_window(old_binding).with_input_observation(WindowInputObservation::new(
            old_binding,
            InputObservationGeneration::new(2),
            Authority::Known(WindowInputState::ReceivesInput),
            InputEffectAcknowledgement::known(None),
        ));
    let before_stale_window = coordinator.clone();
    assert_eq!(
        coordinator.publish_snapshot(&snapshot_at(4, vec![stale_window])),
        Err(ViewportCoordinatorError::Registry(
            ViewportRegistryError::StaleObservedBinding {
                observed: old_binding,
                current: new_binding,
            }
        ))
    );
    assert_eq!(coordinator, before_stale_window);
}

#[test]
fn surface_aba_keeps_old_retirement_isolated_from_the_new_binding() {
    const REBOUND_TOKEN: WindowToken = WindowToken::new(999);
    let (mut coordinator, old_binding, _, release) = runtime_child_vacancy();
    let new_binding = coordinator
        .register_existing(
            old_binding.epoch(),
            old_binding.surface(),
            REBOUND_TOKEN,
            ViewportRole::Root,
            None,
        )
        .expect("a detached surface may receive a new exact binding");

    assert_ne!(new_binding, old_binding);
    assert_eq!(
        coordinator
            .registry
            .record(old_binding.surface())
            .map(ViewportRecord::binding),
        Some(new_binding)
    );
    assert!(coordinator.binding_retirement.contains_key(&old_binding));
    let old_retirement = retirement(&coordinator, old_binding).clone();

    coordinator
        .publish_snapshot(&snapshot_at(2, vec![observed_window(new_binding)]))
        .expect("the rebound binding must publish independently");
    assert_eq!(
        retirement(&coordinator, old_binding),
        &old_retirement,
        "a new binding observation must not mutate the retired binding"
    );

    coordinator
        .publish_snapshot(&snapshot_with_close_at(
            3,
            vec![observed_window(new_binding)],
            vec![close_observation(
                old_binding,
                1,
                WindowCloseState::Destroyed,
            )],
        ))
        .expect("old exact destruction must not affect the rebound surface");

    assert_eq!(
        coordinator
            .registry
            .record(old_binding.surface())
            .map(ViewportRecord::binding),
        Some(new_binding)
    );
    assert!(!coordinator.binding_retirement.contains_key(&old_binding));
    assert!(coordinator.binding_retirement.was_destroyed(old_binding));
    assert_eq!(
        coordinator
            .effects()
            .record(release)
            .map(EffectRecord::phase),
        Some(EffectPhase::Destroyed {
            inventory_generation: coordinator.registry.inventory_generation()
        })
    );
}

#[test]
fn pointer_hit_test_lease_restores_only_after_the_last_holder() {
    let (mut coordinator, binding) = routing_coordinator(
        WindowInputState::ReceivesInput,
        WindowPresentationState::Visible,
        PlatformCapability::Supported,
    );
    let first = PointerId::new(1);
    let second = PointerId::new(2);

    assert!(
        coordinator
            .begin_drag_routing(first, binding.surface())
            .expect("first lease must begin")
            .is_some()
    );
    assert_eq!(
        coordinator
            .begin_drag_routing(second, binding.surface())
            .expect("second lease holder must begin"),
        None
    );
    let requested = coordinator.take_new_effects();
    let enable = requested[0].id();
    assert!(matches!(
        requested.as_slice(),
        [request]
            if matches!(
                request.effect(),
                PlatformEffect::SetPointerPassthrough {
                    binding: actual,
                    enabled: true,
                    ..
                } if *actual == binding
            )
    ));
    coordinator
        .publish_snapshot(&routing_snapshot(
            binding,
            2,
            WindowInputState::PassThrough,
            WindowPresentationState::Visible,
            PlatformCapability::Supported,
            InputEffectAcknowledgement::known(Some(enable)),
        ))
        .expect("enable must be observed before restoration can be confirmed");

    assert_eq!(
        coordinator
            .end_drag_routing(first)
            .expect("first holder must release"),
        None
    );
    assert!(coordinator.take_new_effects().is_empty());
    assert!(
        coordinator
            .end_drag_routing(second)
            .expect("last holder must release")
            .is_some()
    );
    let restored = coordinator.take_new_effects();
    let restore = restored[0].id();
    assert!(matches!(
        restored.as_slice(),
        [request]
            if matches!(
                request.effect(),
                PlatformEffect::SetPointerPassthrough {
                    binding: actual,
                    enabled: false,
                    ..
                } if *actual == binding
            )
    ));
    assert!(coordinator.pointer_passthrough.contains_binding(binding));

    coordinator
        .publish_snapshot(&routing_snapshot(
            binding,
            3,
            WindowInputState::ReceivesInput,
            WindowPresentationState::Visible,
            PlatformCapability::Supported,
            InputEffectAcknowledgement::known(Some(restore)),
        ))
        .expect("restored input observation must publish");
    assert!(!coordinator.pointer_passthrough.contains_binding(binding));
}

#[test]
fn ending_all_pointer_passthrough_holders_is_atomic() {
    let (mut coordinator, binding) = routing_coordinator(
        WindowInputState::ReceivesInput,
        WindowPresentationState::Visible,
        PlatformCapability::Supported,
    );
    let first = PointerId::new(1);
    let second = PointerId::new(2);
    let _ = coordinator
        .begin_drag_routing(first, binding.surface())
        .expect("first route holder must begin");
    let _ = coordinator
        .begin_drag_routing(second, binding.surface())
        .expect("second route holder must begin");
    coordinator
        .end_all_drag_routing()
        .expect("all route holders must end atomically");

    assert_eq!(coordinator.drag_source(first), None);
    assert_eq!(coordinator.drag_source(second), None);
}

#[test]
fn wrong_effect_and_stale_capture_cannot_attribute_a_late_enable() {
    let (mut coordinator, binding) = routing_coordinator(
        WindowInputState::ReceivesInput,
        WindowPresentationState::Visible,
        PlatformCapability::Supported,
    );
    let pointer = PointerId::new(1);
    let enable = coordinator
        .begin_drag_routing(pointer, binding.surface())
        .expect("drag routing must begin")
        .expect("receiving input must request passthrough");
    let _ = coordinator.take_new_effects();
    let restore = coordinator
        .end_drag_routing(pointer)
        .expect("drag routing must end")
        .expect("release must queue restoration");
    assert_eq!(
        take_single_pointer_restore(&mut coordinator, binding, Some(enable)),
        restore
    );

    coordinator
        .publish_snapshot(&routing_snapshot_with_generations(
            binding,
            2,
            3,
            Authority::Known(WindowInputState::ReceivesInput),
            WindowPresentationState::Visible,
            PlatformCapability::Supported,
            InputEffectAcknowledgement::known(None),
        ))
        .expect("newer receiving observation must publish");
    let stale_window = observed_window(binding)
        .with_input_observation(WindowInputObservation::new(
            binding,
            InputObservationGeneration::new(2),
            Authority::Known(WindowInputState::PassThrough),
            InputEffectAcknowledgement::known(Some(enable)),
        ))
        .with_presentation_observation(WindowPresentationObservation::new(
            binding,
            PresentationObservationGeneration::new(3),
            Authority::Known(WindowPresentationState::Visible),
            PresentationEffectAcknowledgement::known(None),
        ));
    let mut capabilities = PlatformCapabilities::default();
    capabilities.set_authoritative_inventory(PlatformCapability::Supported);
    capabilities.set_pointer_hit_test_observation(PlatformCapability::Supported);
    capabilities.set_pointer_hit_test_control(PlatformCapability::Supported);
    let stale_input_snapshot = platform_snapshot_with_generations(
        3,
        3,
        2,
        capabilities,
        unknown_focus_observation(
            FocusObservationGeneration::new(3),
            AuthorityUnavailableReason::NotReported,
        ),
        vec![stale_window],
        Vec::new(),
        unknown_work_areas(3),
    )
    .expect("stale input snapshot must remain structurally valid");
    coordinator
        .publish_snapshot(&stale_input_snapshot)
        .expect("stale exact acknowledgement must be ignored");
    assert!(coordinator.take_new_effects().is_empty());

    coordinator
        .publish_snapshot(&routing_snapshot_with_batch_and_generations(
            binding,
            4,
            3,
            4,
            Authority::Known(WindowInputState::PassThrough),
            WindowPresentationState::Visible,
            PlatformCapability::Supported,
            InputEffectAcknowledgement::known(Some(EffectId::new(900))),
        ))
        .expect("wrong effect acknowledgement must publish without attribution");
    assert!(coordinator.take_new_effects().is_empty());
    assert!(coordinator.pointer_passthrough.contains_binding(binding));

    coordinator
        .publish_snapshot(&routing_snapshot_with_batch_and_generations(
            binding,
            5,
            4,
            5,
            Authority::Known(WindowInputState::PassThrough),
            WindowPresentationState::Visible,
            PlatformCapability::Supported,
            InputEffectAcknowledgement::known(Some(enable)),
        ))
        .expect("fresh exact enable acknowledgement must publish");
    assert!(coordinator.take_new_effects().is_empty());
    assert!(matches!(
        coordinator
            .effects()
            .record(restore)
            .expect("queued restore must remain")
            .phase(),
        EffectPhase::Requested
    ));

    coordinator
        .publish_snapshot(&routing_snapshot_with_batch_and_generations(
            binding,
            6,
            5,
            6,
            Authority::Known(WindowInputState::ReceivesInput),
            WindowPresentationState::Visible,
            PlatformCapability::Supported,
            InputEffectAcknowledgement::known(Some(restore)),
        ))
        .expect("exact restore acknowledgement must settle the saga");
    assert!(!coordinator.pointer_passthrough.contains_binding(binding));
}

#[test]
fn unknown_acknowledgement_does_not_erase_known_input_state_authority() {
    let (mut coordinator, binding) = routing_coordinator(
        WindowInputState::ReceivesInput,
        WindowPresentationState::Visible,
        PlatformCapability::Supported,
    );
    coordinator
        .publish_snapshot(&routing_snapshot(
            binding,
            2,
            WindowInputState::ReceivesInput,
            WindowPresentationState::Visible,
            PlatformCapability::Supported,
            InputEffectAcknowledgement::unknown(
                crate::intent::AuthorityUnavailableReason::NotReported,
            ),
        ))
        .expect("unknown acknowledgement authority must publish");

    let enable = coordinator
        .begin_drag_routing(PointerId::new(1), binding.surface())
        .expect("drag routing must remain total")
        .expect("known receiving state must request passthrough");
    assert!(matches!(
        coordinator.take_new_effects().as_slice(),
        [request]
            if request.id() == enable
                && matches!(
                    request.effect(),
                    PlatformEffect::SetPointerPassthrough {
                        binding: actual,
                        enabled: true,
                        ..
                    } if *actual == binding
                )
    ));
}

#[test]
fn newer_passthrough_state_with_unknown_ack_does_not_retry_failed_enable() {
    let (mut coordinator, binding) = routing_coordinator(
        WindowInputState::ReceivesInput,
        WindowPresentationState::Visible,
        PlatformCapability::Supported,
    );
    let pointer = PointerId::new(1);
    let enable = coordinator
        .begin_drag_routing(pointer, binding.surface())
        .expect("drag routing must begin")
        .expect("receiving input must request passthrough");
    let _ = coordinator.take_new_effects();
    assert_eq!(
        coordinator
            .report_effect(
                binding.epoch(),
                EffectResult::new(
                    enable,
                    binding.epoch(),
                    EffectDispatchResult::DispatchFailed(
                        crate::effect::DispatchFailureReason::AdapterRejected,
                    ),
                ),
            )
            .expect("enable result must reduce"),
        EffectTransition::Applied
    );
    let unavailable = crate::intent::AuthorityUnavailableReason::NotReported;
    coordinator
        .publish_snapshot(&routing_snapshot(
            binding,
            2,
            WindowInputState::PassThrough,
            WindowPresentationState::Visible,
            PlatformCapability::Supported,
            InputEffectAcknowledgement::unknown(unavailable),
        ))
        .expect("newer passthrough state must publish without exact attribution");
    assert!(coordinator.take_new_effects().is_empty());

    let restore = coordinator
        .end_drag_routing(pointer)
        .expect("drag release must preserve restoration")
        .expect("known passthrough state must still be restored");
    assert_eq!(
        take_single_pointer_restore(&mut coordinator, binding, Some(enable)),
        restore
    );
}

#[test]
fn definitive_enable_failure_after_release_preserves_queued_restoration() {
    for result in [
        EffectDispatchResult::DispatchFailed(crate::effect::DispatchFailureReason::AdapterRejected),
        EffectDispatchResult::Unsupported(
            crate::effect::EffectUnsupportedReason::BackendUnsupported,
        ),
    ] {
        let (mut coordinator, binding, enable, restore) = released_unobserved_enable();
        assert_eq!(
            coordinator
                .report_effect(
                    binding.epoch(),
                    EffectResult::new(enable, binding.epoch(), result),
                )
                .expect("effect report must reduce"),
            EffectTransition::Applied
        );
        assert!(coordinator.pointer_passthrough.contains_binding(binding));
        assert!(coordinator.take_new_effects().is_empty());
        assert!(matches!(
            coordinator
                .effects()
                .record(restore)
                .expect("queued restore must remain durable")
                .phase(),
            EffectPhase::Requested
        ));
    }
}

#[test]
fn late_enable_after_active_failure_still_restores_after_release() {
    let (mut coordinator, binding) = routing_coordinator(
        WindowInputState::ReceivesInput,
        WindowPresentationState::Visible,
        PlatformCapability::Supported,
    );
    let pointer = PointerId::new(1);
    let enable = coordinator
        .begin_drag_routing(pointer, binding.surface())
        .expect("drag routing must begin")
        .expect("receiving input must request passthrough");
    let _ = coordinator.take_new_effects();
    assert_eq!(
        coordinator
            .report_effect(
                binding.epoch(),
                EffectResult::new(
                    enable,
                    binding.epoch(),
                    EffectDispatchResult::DispatchFailed(
                        crate::effect::DispatchFailureReason::AdapterRejected,
                    ),
                ),
            )
            .expect("effect report must reduce"),
        EffectTransition::Applied
    );

    coordinator
        .publish_snapshot(&routing_snapshot(
            binding,
            2,
            WindowInputState::PassThrough,
            WindowPresentationState::Visible,
            PlatformCapability::Supported,
            InputEffectAcknowledgement::known(Some(enable)),
        ))
        .expect("late enable observation must remain causal");
    assert!(matches!(
        coordinator
            .effects()
            .record(enable)
            .expect("enable effect must remain queryable")
            .phase(),
        EffectPhase::ObservedApplied { .. }
    ));

    let restore = coordinator
        .end_drag_routing(pointer)
        .expect("drag release must preserve restoration")
        .expect("late enable must be followed by restoration");
    assert_eq!(
        take_single_pointer_restore(&mut coordinator, binding, Some(enable)),
        restore
    );
}

#[test]
fn lost_enable_acknowledgement_cannot_block_restore_queue() {
    let (mut coordinator, binding) = routing_coordinator(
        WindowInputState::ReceivesInput,
        WindowPresentationState::Visible,
        PlatformCapability::Supported,
    );
    let pointer = PointerId::new(1);
    let enable = coordinator
        .begin_drag_routing(pointer, binding.surface())
        .expect("drag routing must begin")
        .expect("receiving input must request passthrough");
    let _ = coordinator.take_new_effects();
    assert_eq!(
        coordinator
            .report_effect(
                binding.epoch(),
                EffectResult::new(
                    enable,
                    binding.epoch(),
                    EffectDispatchResult::Indeterminate(
                        crate::effect::EffectIndeterminateReason::AcknowledgementLost,
                    ),
                ),
            )
            .expect("effect report must reduce"),
        EffectTransition::Applied
    );

    let restore = coordinator
        .end_drag_routing(pointer)
        .expect("drag release must preserve restoration")
        .expect("indeterminate enable must still queue restoration");
    assert_eq!(
        take_single_pointer_restore(&mut coordinator, binding, Some(enable)),
        restore
    );
    assert!(coordinator.pointer_passthrough.contains_binding(binding));
}

#[test]
fn control_loss_between_enable_and_release_cannot_block_the_first_restore() {
    let (mut coordinator, binding) = routing_coordinator(
        WindowInputState::ReceivesInput,
        WindowPresentationState::Visible,
        PlatformCapability::Supported,
    );
    let pointer = PointerId::new(1);
    let enable = coordinator
        .begin_drag_routing(pointer, binding.surface())
        .expect("drag routing must begin")
        .expect("receiving input must request passthrough");
    let _ = coordinator.take_new_effects();
    let unavailable = PlatformCapability::unsupported(
        crate::platform::PlatformRequirement::PointerHitTestControl,
        crate::platform::PlatformCapabilityReason::BackendUnsupported,
    );
    coordinator
        .publish_snapshot(&routing_snapshot(
            binding,
            2,
            WindowInputState::ReceivesInput,
            WindowPresentationState::Visible,
            unavailable,
            InputEffectAcknowledgement::known(None),
        ))
        .expect("control loss must remain representable");
    let restore = coordinator
        .end_drag_routing(pointer)
        .expect("drag release must preserve the restore obligation")
        .expect("control loss must not block the first restore request");
    assert_eq!(
        take_single_pointer_restore(&mut coordinator, binding, Some(enable)),
        restore
    );
    assert_eq!(
        coordinator
            .report_effect(
                binding.epoch(),
                EffectResult::new(
                    restore,
                    binding.epoch(),
                    EffectDispatchResult::Unsupported(
                        crate::effect::EffectUnsupportedReason::CapabilityRevoked,
                    ),
                ),
            )
            .expect("effect report must reduce"),
        EffectTransition::Applied
    );
    assert!(coordinator.take_new_effects().is_empty());

    coordinator
        .publish_snapshot(&routing_snapshot(
            binding,
            3,
            WindowInputState::PassThrough,
            WindowPresentationState::Visible,
            unavailable,
            InputEffectAcknowledgement::known(Some(enable)),
        ))
        .expect("late enable under control loss must preserve restoration");
    assert!(coordinator.take_new_effects().is_empty());

    coordinator
        .publish_snapshot(&routing_snapshot(
            binding,
            4,
            WindowInputState::PassThrough,
            WindowPresentationState::Visible,
            PlatformCapability::Supported,
            InputEffectAcknowledgement::known(Some(enable)),
        ))
        .expect("control recovery must unlock one restore retry");
    let retry = take_single_pointer_restore(&mut coordinator, binding, Some(restore));
    assert!(coordinator.pointer_passthrough.contains_binding(binding));
    assert_ne!(retry, restore);
}

#[test]
fn versioned_gap_watermark_allows_newer_safe_state_to_settle_failed_restore() {
    let (mut coordinator, binding) = routing_coordinator(
        WindowInputState::ReceivesInput,
        WindowPresentationState::Visible,
        PlatformCapability::Supported,
    );
    let pointer = PointerId::new(1);
    let enable = coordinator
        .begin_drag_routing(pointer, binding.surface())
        .expect("drag routing must begin")
        .expect("receiving input must request passthrough");
    let _ = coordinator.take_new_effects();
    let unavailable = crate::intent::AuthorityUnavailableReason::NotReported;
    coordinator
        .publish_snapshot(&routing_snapshot_with_input_authority(
            binding,
            2,
            Authority::Unknown(unavailable),
            WindowPresentationState::Visible,
            PlatformCapability::Supported,
            InputEffectAcknowledgement::unknown(unavailable),
        ))
        .expect("versioned input tombstone must publish");
    assert_eq!(
        coordinator
            .registry()
            .record(binding.surface())
            .expect("source binding must remain registered")
            .input_observation(),
        None
    );

    let restore = coordinator
        .end_drag_routing(pointer)
        .expect("drag release must preserve restoration")
        .expect("versioned input gap must not block restoration");
    assert_eq!(
        take_single_pointer_restore(&mut coordinator, binding, Some(enable)),
        restore
    );
    assert_eq!(
        coordinator
            .pointer_passthrough
            .restore_issued_after(binding)
            .flatten(),
        Some(InputObservationGeneration::new(2))
    );
    assert_eq!(
        coordinator
            .report_effect(
                binding.epoch(),
                EffectResult::new(
                    restore,
                    binding.epoch(),
                    EffectDispatchResult::DispatchFailed(
                        crate::effect::DispatchFailureReason::AdapterRejected,
                    ),
                ),
            )
            .expect("restore result must reduce"),
        EffectTransition::Applied
    );
    assert!(coordinator.take_new_effects().is_empty());
    assert!(coordinator.pointer_passthrough.contains_binding(binding));

    coordinator
        .publish_snapshot(&routing_snapshot(
            binding,
            3,
            WindowInputState::ReceivesInput,
            WindowPresentationState::Visible,
            PlatformCapability::Supported,
            InputEffectAcknowledgement::unknown(unavailable),
        ))
        .expect("newer safe state must publish independently of effect acknowledgement");
    assert!(!coordinator.pointer_passthrough.contains_binding(binding));
    assert!(coordinator.take_new_effects().is_empty());
}

#[test]
fn stale_pre_restore_receiving_state_cannot_settle_a_failed_restore() {
    let (mut coordinator, binding) = routing_coordinator(
        WindowInputState::ReceivesInput,
        WindowPresentationState::Visible,
        PlatformCapability::Supported,
    );
    let pointer = PointerId::new(1);
    let enable = coordinator
        .begin_drag_routing(pointer, binding.surface())
        .expect("drag routing must begin")
        .expect("receiving input must request passthrough");
    let _ = coordinator.take_new_effects();
    let restore = coordinator
        .end_drag_routing(pointer)
        .expect("drag release must preserve restoration")
        .expect("release must queue restoration");
    assert_eq!(
        take_single_pointer_restore(&mut coordinator, binding, Some(enable)),
        restore
    );

    assert_eq!(
        coordinator
            .report_effect(
                binding.epoch(),
                EffectResult::new(
                    restore,
                    binding.epoch(),
                    EffectDispatchResult::DispatchFailed(
                        crate::effect::DispatchFailureReason::AdapterRejected,
                    ),
                ),
            )
            .expect("effect report must reduce"),
        EffectTransition::Applied
    );
    assert!(coordinator.pointer_passthrough.contains_binding(binding));

    coordinator
        .publish_snapshot(&routing_snapshot(
            binding,
            2,
            WindowInputState::PassThrough,
            WindowPresentationState::Visible,
            PlatformCapability::Supported,
            InputEffectAcknowledgement::known(Some(enable)),
        ))
        .expect("late enable must not erase the restore obligation");
    assert!(coordinator.pointer_passthrough.contains_binding(binding));
    let retry = take_single_pointer_restore(&mut coordinator, binding, Some(restore));

    coordinator
        .publish_snapshot(&routing_snapshot(
            binding,
            3,
            WindowInputState::ReceivesInput,
            WindowPresentationState::Visible,
            PlatformCapability::Supported,
            InputEffectAcknowledgement::known(Some(retry)),
        ))
        .expect("the retry acknowledgement must settle late enable restoration");
    assert!(!coordinator.pointer_passthrough.contains_binding(binding));
}

#[test]
fn safe_state_keeps_requested_restore_as_the_next_drag_predecessor() {
    let (mut coordinator, binding) = routing_coordinator(
        WindowInputState::ReceivesInput,
        WindowPresentationState::Visible,
        PlatformCapability::Supported,
    );
    let first_pointer = PointerId::new(1);
    let enable = coordinator
        .begin_drag_routing(first_pointer, binding.surface())
        .expect("drag routing must begin")
        .expect("receiving input must request passthrough");
    let _ = coordinator.take_new_effects();
    coordinator
        .publish_snapshot(&routing_snapshot(
            binding,
            2,
            WindowInputState::PassThrough,
            WindowPresentationState::Visible,
            PlatformCapability::Supported,
            InputEffectAcknowledgement::known(Some(enable)),
        ))
        .expect("enable acknowledgement must publish");
    let restore = coordinator
        .end_drag_routing(first_pointer)
        .expect("drag routing must end")
        .expect("restoration must be requested");
    let _ = coordinator.take_new_effects();
    coordinator
        .publish_snapshot(&routing_snapshot(
            binding,
            3,
            WindowInputState::ReceivesInput,
            WindowPresentationState::Visible,
            PlatformCapability::Supported,
            InputEffectAcknowledgement::known(Some(enable)),
        ))
        .expect("safe state without restore attribution must publish");
    assert!(coordinator.pointer_passthrough.contains_binding(binding));
    assert!(matches!(
        coordinator
            .effects()
            .record(restore)
            .expect("restore history must remain")
            .phase(),
        EffectPhase::Requested
    ));

    let second_enable = coordinator
        .begin_drag_routing(PointerId::new(2), binding.surface())
        .expect("a new drag must preserve the pointer-input lane")
        .expect("the new drag must request pass-through again");
    assert!(matches!(
        coordinator.take_new_effects().as_slice(),
        [request]
            if request.id() == second_enable
                && matches!(
                    request.effect(),
                    PlatformEffect::SetPointerPassthrough {
                        binding: actual,
                        enabled: true,
                        after: Some(predecessor),
                    } if *actual == binding && *predecessor == restore
                )
    ));
}

#[test]
fn restore_failure_requires_a_safe_observation_newer_than_its_report() {
    let (mut coordinator, binding) = routing_coordinator(
        WindowInputState::ReceivesInput,
        WindowPresentationState::Visible,
        PlatformCapability::Supported,
    );
    let pointer = PointerId::new(1);
    let enable = coordinator
        .begin_drag_routing(pointer, binding.surface())
        .expect("drag routing must begin")
        .expect("receiving input must request passthrough");
    let _ = coordinator.take_new_effects();
    let restore = coordinator
        .end_drag_routing(pointer)
        .expect("drag routing must end")
        .expect("release must queue restoration");
    let _ = coordinator.take_new_effects();

    coordinator
        .publish_snapshot(&routing_snapshot(
            binding,
            2,
            WindowInputState::ReceivesInput,
            WindowPresentationState::Visible,
            PlatformCapability::Supported,
            InputEffectAcknowledgement::known(None),
        ))
        .expect("the pre-dispatch safe observation must publish");
    assert!(matches!(
        coordinator
            .effects()
            .record(enable)
            .expect("enable history must remain")
            .phase(),
        EffectPhase::Requested
    ));

    assert_eq!(
        coordinator
            .report_effect(
                binding.epoch(),
                EffectResult::new(
                    restore,
                    binding.epoch(),
                    EffectDispatchResult::DispatchFailed(
                        crate::effect::DispatchFailureReason::AdapterRejected,
                    ),
                ),
            )
            .expect("restore failure must reduce"),
        EffectTransition::Applied
    );
    let terminal_reported_after = coordinator
        .pointer_passthrough
        .restore_terminal_reported_after(binding)
        .expect("the pre-report observation must not settle restoration");
    assert_eq!(
        terminal_reported_after,
        Some(InputObservationGeneration::new(2))
    );
    assert!(
        !coordinator
            .pointer_passthrough
            .restore_state_settled(binding)
            .expect("restore obligation must remain")
    );

    coordinator
        .publish_snapshot(&routing_snapshot(
            binding,
            3,
            WindowInputState::ReceivesInput,
            WindowPresentationState::Visible,
            PlatformCapability::Supported,
            InputEffectAcknowledgement::known(None),
        ))
        .expect("a post-report safe observation must publish");
    assert!(!coordinator.pointer_passthrough.contains_binding(binding));
}

#[test]
fn preexisting_passthrough_needs_neither_control_capability_nor_restore() {
    let (mut coordinator, binding) = routing_coordinator(
        WindowInputState::PassThrough,
        WindowPresentationState::Visible,
        PlatformCapability::unsupported(
            crate::platform::PlatformRequirement::PointerHitTestControl,
            crate::platform::PlatformCapabilityReason::BackendUnsupported,
        ),
    );
    let pointer = PointerId::new(1);

    assert_eq!(
        coordinator
            .begin_drag_routing(pointer, binding.surface())
            .expect("existing passthrough must be leased"),
        None
    );
    assert_eq!(coordinator.drag_source(pointer), Some(binding));
    assert!(coordinator.take_new_effects().is_empty());
    assert_eq!(
        coordinator
            .end_drag_routing(pointer)
            .expect("existing passthrough lease must end"),
        None
    );
    assert!(coordinator.take_new_effects().is_empty());
    assert!(!coordinator.pointer_passthrough.contains_binding(binding));
}

#[test]
fn receiving_source_without_hit_test_control_still_freezes_the_source() {
    let (mut coordinator, binding) = routing_coordinator(
        WindowInputState::ReceivesInput,
        WindowPresentationState::Visible,
        PlatformCapability::unsupported(
            crate::platform::PlatformRequirement::PointerHitTestControl,
            crate::platform::PlatformCapabilityReason::BackendUnsupported,
        ),
    );
    let pointer = PointerId::new(1);

    assert_eq!(
        coordinator
            .begin_drag_routing(pointer, binding.surface())
            .expect("missing control must still freeze the drag source"),
        None
    );
    assert_eq!(coordinator.drag_source(pointer), Some(binding));
    assert!(coordinator.pointer_passthrough.contains_binding(binding));
    assert!(coordinator.take_new_effects().is_empty());

    assert_eq!(
        coordinator
            .end_drag_routing(pointer)
            .expect("uncontrolled source freeze must end cleanly"),
        None
    );
    assert!(!coordinator.pointer_passthrough.contains_binding(binding));
}

#[test]
fn definitive_enable_failure_waits_for_a_new_fact_edge_before_retry() {
    let (mut coordinator, binding) = routing_coordinator(
        WindowInputState::ReceivesInput,
        WindowPresentationState::Visible,
        PlatformCapability::Supported,
    );
    let pointer = PointerId::new(1);
    let first = coordinator
        .begin_drag_routing(pointer, binding.surface())
        .expect("initial enable must begin")
        .expect("receiving input must request passthrough");
    let _ = coordinator.take_new_effects();
    assert_eq!(
        coordinator
            .report_effect(
                binding.epoch(),
                EffectResult::new(
                    first,
                    binding.epoch(),
                    EffectDispatchResult::DispatchFailed(
                        crate::effect::DispatchFailureReason::AdapterRejected,
                    ),
                ),
            )
            .expect("effect report must reduce"),
        EffectTransition::Applied
    );
    let effect_count = coordinator.effects().records().count();

    for generation in 2..5 {
        coordinator
            .publish_snapshot(&routing_snapshot(
                binding,
                generation,
                WindowInputState::ReceivesInput,
                WindowPresentationState::Visible,
                PlatformCapability::Supported,
                InputEffectAcknowledgement::known(None),
            ))
            .expect("unchanged observation must publish without retrying");
        assert!(coordinator.take_new_effects().is_empty());
        assert_eq!(coordinator.effects().records().count(), effect_count);
    }

    coordinator
        .publish_snapshot(&routing_snapshot(
            binding,
            5,
            WindowInputState::ReceivesInput,
            WindowPresentationState::Hidden,
            PlatformCapability::Supported,
            InputEffectAcknowledgement::known(None),
        ))
        .expect("hidden edge must publish without enabling");
    assert!(coordinator.take_new_effects().is_empty());
    coordinator
        .publish_snapshot(&routing_snapshot(
            binding,
            6,
            WindowInputState::ReceivesInput,
            WindowPresentationState::Visible,
            PlatformCapability::Supported,
            InputEffectAcknowledgement::known(None),
        ))
        .expect("routeable edge must unlock one retry");
    assert!(matches!(
        coordinator.take_new_effects().as_slice(),
        [request]
            if request.id() != first
                && matches!(
                    request.effect(),
                    PlatformEffect::SetPointerPassthrough {
                        binding: actual,
                        enabled: true,
                        ..
                    } if *actual == binding
                )
    ));
    assert_eq!(coordinator.drag_source(pointer), Some(binding));
}

#[test]
fn unsupported_and_indeterminate_enable_attempts_do_not_retry_each_snapshot() {
    for result in [
        EffectDispatchResult::Unsupported(
            crate::effect::EffectUnsupportedReason::BackendUnsupported,
        ),
        EffectDispatchResult::Indeterminate(
            crate::effect::EffectIndeterminateReason::AcknowledgementLost,
        ),
    ] {
        let (mut coordinator, binding) = routing_coordinator(
            WindowInputState::ReceivesInput,
            WindowPresentationState::Visible,
            PlatformCapability::Supported,
        );
        let first = coordinator
            .begin_drag_routing(PointerId::new(1), binding.surface())
            .expect("initial enable must begin")
            .expect("receiving input must request passthrough");
        let _ = coordinator.take_new_effects();
        assert_eq!(
            coordinator
                .report_effect(
                    binding.epoch(),
                    EffectResult::new(first, binding.epoch(), result),
                )
                .expect("effect report must reduce"),
            EffectTransition::Applied
        );
        let effect_count = coordinator.effects().records().count();

        for generation in 2..5 {
            coordinator
                .publish_snapshot(&routing_snapshot(
                    binding,
                    generation,
                    WindowInputState::ReceivesInput,
                    WindowPresentationState::Visible,
                    PlatformCapability::Supported,
                    InputEffectAcknowledgement::known(None),
                ))
                .expect("unchanged observation must remain stable");
            assert!(coordinator.take_new_effects().is_empty());
            assert_eq!(coordinator.effects().records().count(), effect_count);
        }
    }
}

#[test]
fn late_enable_after_drag_end_cannot_overtake_the_queued_restore() {
    let (mut coordinator, binding) = routing_coordinator(
        WindowInputState::ReceivesInput,
        WindowPresentationState::Visible,
        PlatformCapability::Supported,
    );
    let pointer = PointerId::new(1);
    let enable = coordinator
        .begin_drag_routing(pointer, binding.surface())
        .expect("enable must begin")
        .expect("receiving input must request passthrough");
    let _ = coordinator.take_new_effects();
    let restore = coordinator
        .end_drag_routing(pointer)
        .expect("drag end must preserve restoration")
        .expect("drag end must queue restoration immediately");
    assert_eq!(
        take_single_pointer_restore(&mut coordinator, binding, Some(enable)),
        restore
    );
    assert!(coordinator.pointer_passthrough.contains_binding(binding));

    let effect_count = coordinator.effects().records().count();
    for generation in 2..5 {
        coordinator
            .publish_snapshot(&routing_snapshot(
                binding,
                generation,
                WindowInputState::ReceivesInput,
                WindowPresentationState::Visible,
                PlatformCapability::Supported,
                InputEffectAcknowledgement::known(None),
            ))
            .expect("pre-enable input observation must not satisfy restoration");
        assert!(coordinator.pointer_passthrough.contains_binding(binding));
        assert!(coordinator.take_new_effects().is_empty());
        assert_eq!(coordinator.effects().records().count(), effect_count);
    }

    coordinator
        .publish_snapshot(&routing_snapshot(
            binding,
            5,
            WindowInputState::PassThrough,
            WindowPresentationState::Visible,
            PlatformCapability::Supported,
            InputEffectAcknowledgement::known(Some(enable)),
        ))
        .expect("late enable observation must publish");
    assert!(coordinator.take_new_effects().is_empty());
    assert!(matches!(
        coordinator
            .effects()
            .record(enable)
            .expect("enable effect must remain queryable")
            .phase(),
        EffectPhase::ObservedApplied { .. }
    ));
    assert!(coordinator.pointer_passthrough.contains_binding(binding));

    coordinator
        .publish_snapshot(&routing_snapshot(
            binding,
            6,
            WindowInputState::ReceivesInput,
            WindowPresentationState::Visible,
            PlatformCapability::Supported,
            InputEffectAcknowledgement::known(Some(restore)),
        ))
        .expect("restored input observation must publish");
    assert!(matches!(
        coordinator
            .effects()
            .record(restore)
            .expect("restore effect must remain queryable")
            .phase(),
        EffectPhase::ObservedApplied { .. }
    ));
    assert!(!coordinator.pointer_passthrough.contains_binding(binding));
}

#[test]
fn failed_restore_waits_for_a_capability_edge_before_retry() {
    let (mut coordinator, binding) = routing_coordinator(
        WindowInputState::ReceivesInput,
        WindowPresentationState::Visible,
        PlatformCapability::Supported,
    );
    let pointer = PointerId::new(1);
    let enable = coordinator
        .begin_drag_routing(pointer, binding.surface())
        .expect("enable must begin")
        .expect("receiving input must request passthrough");
    let _ = coordinator.take_new_effects();
    coordinator
        .publish_snapshot(&routing_snapshot(
            binding,
            2,
            WindowInputState::PassThrough,
            WindowPresentationState::Visible,
            PlatformCapability::Supported,
            InputEffectAcknowledgement::known(Some(enable)),
        ))
        .expect("passthrough observation must publish");

    let restore = coordinator
        .end_drag_routing(pointer)
        .expect("drag end must preserve restoration")
        .expect("drag end must request restoration");
    let _ = coordinator.take_new_effects();
    assert_eq!(
        coordinator
            .report_effect(
                binding.epoch(),
                EffectResult::new(
                    restore,
                    binding.epoch(),
                    EffectDispatchResult::DispatchFailed(
                        crate::effect::DispatchFailureReason::AdapterRejected,
                    ),
                ),
            )
            .expect("effect report must reduce"),
        EffectTransition::Applied
    );
    let effect_count = coordinator.effects().records().count();
    for generation in 3..6 {
        coordinator
            .publish_snapshot(&routing_snapshot(
                binding,
                generation,
                WindowInputState::PassThrough,
                WindowPresentationState::Visible,
                PlatformCapability::Supported,
                InputEffectAcknowledgement::known(Some(enable)),
            ))
            .expect("unchanged passthrough must not retry restoration");
        assert!(coordinator.take_new_effects().is_empty());
        assert_eq!(coordinator.effects().records().count(), effect_count);
    }

    let unsupported = PlatformCapability::unsupported(
        crate::platform::PlatformRequirement::PointerHitTestControl,
        crate::platform::PlatformCapabilityReason::BackendUnsupported,
    );
    coordinator
        .publish_snapshot(&routing_snapshot(
            binding,
            6,
            WindowInputState::PassThrough,
            WindowPresentationState::Visible,
            unsupported,
            InputEffectAcknowledgement::known(Some(enable)),
        ))
        .expect("capability loss must publish without restoring");
    assert!(coordinator.take_new_effects().is_empty());
    coordinator
        .publish_snapshot(&routing_snapshot(
            binding,
            7,
            WindowInputState::PassThrough,
            WindowPresentationState::Visible,
            PlatformCapability::Supported,
            InputEffectAcknowledgement::known(Some(enable)),
        ))
        .expect("capability recovery must unlock one restore retry");
    assert!(matches!(
        coordinator.take_new_effects().as_slice(),
        [request]
            if request.id() != restore
                && matches!(
                    request.effect(),
                    PlatformEffect::SetPointerPassthrough {
                        binding: actual,
                        enabled: false,
                        ..
                    } if *actual == binding
                )
    ));
}

#[test]
fn indeterminate_restore_is_not_reissued_concurrently() {
    let (mut coordinator, binding) = routing_coordinator(
        WindowInputState::ReceivesInput,
        WindowPresentationState::Visible,
        PlatformCapability::Supported,
    );
    let pointer = PointerId::new(1);
    let enable = coordinator
        .begin_drag_routing(pointer, binding.surface())
        .expect("enable must begin")
        .expect("receiving input must request passthrough");
    let _ = coordinator.take_new_effects();
    coordinator
        .publish_snapshot(&routing_snapshot(
            binding,
            2,
            WindowInputState::PassThrough,
            WindowPresentationState::Visible,
            PlatformCapability::Supported,
            InputEffectAcknowledgement::known(Some(enable)),
        ))
        .expect("passthrough observation must publish");
    let restore = coordinator
        .end_drag_routing(pointer)
        .expect("drag end must preserve restoration")
        .expect("drag end must request restoration");
    let _ = coordinator.take_new_effects();
    assert_eq!(
        coordinator
            .report_effect(
                binding.epoch(),
                EffectResult::new(
                    restore,
                    binding.epoch(),
                    EffectDispatchResult::Indeterminate(
                        crate::effect::EffectIndeterminateReason::AcknowledgementLost,
                    ),
                ),
            )
            .expect("effect report must reduce"),
        EffectTransition::Applied
    );
    for generation in 3..6 {
        coordinator
            .publish_snapshot(&routing_snapshot(
                binding,
                generation,
                WindowInputState::PassThrough,
                WindowPresentationState::Visible,
                PlatformCapability::Supported,
                InputEffectAcknowledgement::known(Some(enable)),
            ))
            .expect("indeterminate restore must remain singular");
        assert!(coordinator.take_new_effects().is_empty());
    }
    coordinator
        .publish_snapshot(&routing_snapshot(
            binding,
            6,
            WindowInputState::ReceivesInput,
            WindowPresentationState::Visible,
            PlatformCapability::Supported,
            InputEffectAcknowledgement::known(Some(restore)),
        ))
        .expect("authoritative restored input must settle the attempt");
    assert!(!coordinator.pointer_passthrough.contains_binding(binding));
}

#[test]
fn explicit_destruction_tombstone_terminates_active_and_released_pointer_sagas() {
    for release_before_destroy in [false, true] {
        let (mut coordinator, binding) = routing_coordinator(
            WindowInputState::ReceivesInput,
            WindowPresentationState::Visible,
            PlatformCapability::Supported,
        );
        let pointer = PointerId::new(1);
        let enable = coordinator
            .begin_drag_routing(pointer, binding.surface())
            .expect("drag routing must begin")
            .expect("enable effect must exist");
        let _ = coordinator.take_new_effects();
        if release_before_destroy {
            let restore = coordinator
                .end_drag_routing(pointer)
                .expect("drag routing must release")
                .expect("release must queue restoration");
            assert_eq!(
                take_single_pointer_restore(&mut coordinator, binding, Some(enable)),
                restore
            );
        }

        coordinator
            .publish_snapshot(&snapshot_at(2, Vec::new()))
            .expect("inventory absence must remain non-destructive");
        assert_eq!(
            coordinator.drag_source(pointer),
            (!release_before_destroy).then_some(binding)
        );
        assert!(coordinator.pointer_passthrough.contains_binding(binding));
        assert!(coordinator.take_new_effects().is_empty());

        coordinator
            .publish_snapshot(&snapshot_with_close_at(
                3,
                Vec::new(),
                vec![close_observation(binding, 1, WindowCloseState::Destroyed)],
            ))
            .expect("destroyed tombstone must terminate pointer saga");
        assert_eq!(coordinator.drag_source(pointer), None);
        assert!(!coordinator.pointer_passthrough.contains_binding(binding));
        assert!(coordinator.take_new_effects().is_empty());
        assert!(matches!(
            coordinator
                .effects()
                .record(enable)
                .expect("enable effect history must remain")
                .phase(),
            EffectPhase::Destroyed { .. }
        ));
    }
}

#[test]
fn window_unavailable_waits_for_explicit_destruction_without_retrying() {
    let (mut coordinator, binding) = routing_coordinator(
        WindowInputState::ReceivesInput,
        WindowPresentationState::Visible,
        PlatformCapability::Supported,
    );
    let pointer = PointerId::new(1);
    let enable = coordinator
        .begin_drag_routing(pointer, binding.surface())
        .expect("drag routing must begin")
        .expect("enable effect must exist");
    let _ = coordinator.take_new_effects();
    assert_eq!(
        coordinator
            .report_effect(
                binding.epoch(),
                EffectResult::new(
                    enable,
                    binding.epoch(),
                    EffectDispatchResult::DispatchFailed(
                        crate::effect::DispatchFailureReason::WindowUnavailable,
                    ),
                ),
            )
            .expect("effect report must reduce"),
        EffectTransition::Applied
    );
    coordinator
        .publish_snapshot(&snapshot_at(2, Vec::new()))
        .expect("inventory absence must remain non-destructive");
    assert_eq!(coordinator.drag_source(pointer), Some(binding));
    assert!(coordinator.pointer_passthrough.contains_binding(binding));
    assert!(coordinator.take_new_effects().is_empty());

    coordinator
        .publish_snapshot(&snapshot_with_close_at(
            3,
            Vec::new(),
            vec![close_observation(binding, 1, WindowCloseState::Destroyed)],
        ))
        .expect("destroyed tombstone must terminate unavailable binding");
    assert_eq!(coordinator.drag_source(pointer), None);
    assert!(!coordinator.pointer_passthrough.contains_binding(binding));
    assert!(coordinator.take_new_effects().is_empty());
    assert!(matches!(
        coordinator
            .effects()
            .record(enable)
            .expect("enable effect history must remain")
            .phase(),
        EffectPhase::Destroyed { .. }
    ));
}

#[test]
fn workspace_rebound_requires_a_fresh_restore_ack_and_ignores_old_effects() {
    let (mut coordinator, old_binding, old_enable, old_restore) = released_unobserved_enable();

    let new_epoch = WorkspaceEpoch::new(1);
    let reconciliation = coordinator
        .reconcile_workspace_epoch(new_epoch, &BTreeSet::from([old_binding.surface()]))
        .expect("workspace replacement must reconcile the binding");
    let &[(actual_old, new_binding)] = reconciliation.rebound() else {
        panic!("one binding must rebound: {reconciliation:?}");
    };
    assert_eq!(actual_old, old_binding);
    let rebound_restore =
        take_single_pointer_restore(&mut coordinator, new_binding, Some(old_restore));
    assert!(
        coordinator
            .pointer_passthrough
            .contains_binding(new_binding)
    );

    coordinator
        .publish_snapshot(&routing_snapshot(
            new_binding,
            2,
            WindowInputState::ReceivesInput,
            WindowPresentationState::Visible,
            PlatformCapability::Supported,
            InputEffectAcknowledgement::unknown(
                crate::intent::AuthorityUnavailableReason::NotReported,
            ),
        ))
        .expect("transient acknowledgement loss must preserve the queued safety restore");
    assert!(coordinator.take_new_effects().is_empty());
    assert!(
        coordinator
            .pointer_passthrough
            .contains_binding(new_binding)
    );
    assert_eq!(
        coordinator
            .report_effect(
                new_epoch,
                EffectResult::new(
                    old_enable,
                    old_binding.epoch(),
                    EffectDispatchResult::DispatchFailed(
                        crate::effect::DispatchFailureReason::AdapterRejected,
                    ),
                ),
            )
            .expect("effect report must reduce"),
        EffectTransition::StaleEpoch
    );
    coordinator
        .publish_snapshot(&routing_snapshot(
            new_binding,
            3,
            WindowInputState::PassThrough,
            WindowPresentationState::Visible,
            PlatformCapability::Supported,
            InputEffectAcknowledgement::known(Some(old_enable)),
        ))
        .expect("an old-incarnation acknowledgement must not settle the new restore");
    assert!(
        coordinator
            .pointer_passthrough
            .contains_binding(new_binding)
    );
    assert!(coordinator.take_new_effects().is_empty());
    coordinator
        .publish_snapshot(&routing_snapshot(
            new_binding,
            4,
            WindowInputState::ReceivesInput,
            WindowPresentationState::Visible,
            PlatformCapability::Supported,
            InputEffectAcknowledgement::known(Some(rebound_restore)),
        ))
        .expect("post-barrier restored input must settle the rebound saga");
    assert!(
        !coordinator
            .pointer_passthrough
            .contains_binding(new_binding)
    );
    assert!(matches!(
        coordinator
            .effects()
            .record(rebound_restore)
            .expect("rebound restore must remain queryable")
            .phase(),
        EffectPhase::ObservedApplied { .. }
    ));
}

#[test]
fn provider_replacement_reissues_observation_for_emitted_cleanup() {
    let (mut coordinator, binding, _, destructive) = runtime_child_vacancy();
    let predecessor = coordinator
        .platform_provider()
        .expect("the test coordinator has one active provider");

    let ticket = coordinator
        .begin_platform_provider_replacement(predecessor)
        .expect("provider replacement revokes the old cleanup observer");
    assert_eq!(coordinator.platform_provider(), None);
    assert_eq!(
        coordinator
            .effects()
            .record(destructive)
            .map(crate::effect::EffectRecord::phase),
        Some(EffectPhase::Indeterminate(
            crate::effect::EffectIndeterminateReason::ProviderRestarted,
        ))
    );

    let successor_provider = coordinator
        .finish_platform_provider_replacement(ticket)
        .expect("the exact replacement ticket activates its successor");
    let BindingRetirementStatus::CleanupRequested {
        effect: continuation,
    } = retirement(&coordinator, binding).status()
    else {
        panic!("retirement must point at a replacement observation continuation");
    };
    assert_ne!(continuation, destructive);
    assert!(matches!(
        coordinator
            .effects()
            .record(continuation)
            .map(|record| record.request().effect()),
        Some(PlatformEffect::ContinueCleanup {
            binding: actual,
            predecessor: actual_predecessor,
            after: None,
        }) if *actual == binding && *actual_predecessor == destructive
    ));

    let emitted = coordinator.take_new_effects();
    assert!(matches!(
        emitted.as_slice(),
        [request] if request.id() == continuation
            && matches!(
                request.effect(),
                PlatformEffect::ContinueCleanup {
                    binding: actual,
                    predecessor: actual_predecessor,
                    after: None,
                } if *actual == binding && *actual_predecessor == destructive
            )
    ));
    assert_eq!(
        coordinator
            .effects()
            .record(continuation)
            .and_then(crate::effect::EffectRecord::delivery)
            .map(crate::effect::EffectDelivery::provider),
        Some(successor_provider)
    );
}

#[test]
fn provider_replacement_rebases_a_failed_cleanup_observation_lane() {
    let (mut coordinator, binding, destructive, failed, _, _) = failed_cleanup_observation();
    assert!(matches!(
        retirement(&coordinator, binding).status(),
        BindingRetirementStatus::CleanupObservationFailed { effect } if effect == failed
    ));
    let predecessor = coordinator
        .platform_provider()
        .expect("the failed observation still belongs to one active provider");

    let ticket = coordinator
        .begin_platform_provider_replacement(predecessor)
        .expect("provider replacement must revoke the failed observation lane");
    let successor_provider = coordinator
        .finish_platform_provider_replacement(ticket)
        .expect("the exact replacement ticket must activate a successor");
    let BindingRetirementStatus::CleanupRequested {
        effect: continuation,
    } = retirement(&coordinator, binding).status()
    else {
        panic!("retirement must own a rebased observation continuation");
    };
    assert_ne!(continuation, failed);
    assert!(matches!(
        coordinator
            .effects()
            .record(continuation)
            .map(|record| record.request().effect()),
        Some(PlatformEffect::ContinueCleanup {
            binding: actual,
            predecessor: actual_predecessor,
            after: None,
        }) if *actual == binding && *actual_predecessor == destructive
    ));

    let emitted = coordinator.take_new_effects();
    assert!(matches!(
        emitted.as_slice(),
        [request] if request.id() == continuation
    ));
    assert_eq!(
        coordinator
            .effects()
            .record(continuation)
            .and_then(crate::effect::EffectRecord::delivery)
            .map(crate::effect::EffectDelivery::provider),
        Some(successor_provider)
    );
}

#[test]
fn exact_retirement_destruction_terminalizes_the_complete_binding_lineage() {
    let (mut coordinator, binding, destructive, continuation, _, _) = failed_cleanup_observation();
    let destroyed = close_observation(binding, 2, WindowCloseState::Destroyed);

    coordinator
        .publish_snapshot(&snapshot_with_close_at(2, Vec::new(), vec![destroyed]))
        .expect("exact destruction must settle binding retirement");

    for effect in [destructive, continuation] {
        assert_eq!(
            coordinator
                .effects()
                .record(effect)
                .map(crate::effect::EffectRecord::phase),
            Some(EffectPhase::Destroyed {
                inventory_generation: InventoryGeneration::new(2),
            }),
            "every effect targeting the destroyed binding must become terminal"
        );
    }
    assert!(coordinator.binding_retirement.get(&binding).is_none());
}

#[test]
fn repeated_restore_preserves_the_pointer_barrier_before_destructive_cleanup() {
    let (mut coordinator, binding, enable, old_restore) =
        released_unobserved_runtime_child_enable();
    coordinator
        .reconcile_workspace_epoch(WorkspaceEpoch::new(1), &BTreeSet::new())
        .expect("first restore must retire the source binding");
    assert!(coordinator.take_new_effects().is_empty());
    assert_eq!(
        retirement(&coordinator, binding).status(),
        BindingRetirementStatus::AwaitingInputRestore
    );

    coordinator
        .reconcile_workspace_epoch(WorkspaceEpoch::new(2), &BTreeSet::new())
        .expect("second restore must preserve the pointer obligation");
    let second = coordinator.take_new_effects();
    let emitted_restore = second
        .iter()
        .find_map(|request| match request.effect() {
            PlatformEffect::SetPointerPassthrough {
                binding: actual,
                enabled: false,
                after: Some(predecessor),
            } if *actual == binding && *predecessor == old_restore => Some(request.id()),
            _ => None,
        })
        .expect("second restore must serialize behind the emitted predecessor");

    let third = coordinator
        .reconcile_workspace_epoch(WorkspaceEpoch::new(3), &BTreeSet::new())
        .expect("third restore must queue another pointer successor");
    let unemitted_restore = third
        .cleanup_effects()
        .iter()
        .copied()
        .find(|effect| {
            matches!(
                coordinator
                    .effects()
                    .record(*effect)
                    .map(|record| record.request().effect()),
                Some(PlatformEffect::SetPointerPassthrough {
                    binding: actual,
                    enabled: false,
                    ..
                }) if *actual == binding
            )
        })
        .expect("third restore must own one pointer successor");

    let fourth_epoch = WorkspaceEpoch::new(4);
    coordinator
        .reconcile_workspace_epoch(fourth_epoch, &BTreeSet::new())
        .expect("fourth restore must replace the unemitted successor");
    assert_eq!(
        coordinator
            .effects()
            .record(unemitted_restore)
            .map(crate::effect::EffectRecord::phase),
        Some(EffectPhase::Invalidated {
            cause: crate::effect::EffectInvalidation::WorkspaceReplaced {
                replacement_epoch: fourth_epoch,
            },
        })
    );
    let current = coordinator.take_new_effects();
    let current_restore = current
        .iter()
        .find(|request| {
            matches!(
                request.effect(),
                PlatformEffect::SetPointerPassthrough { enabled: false, .. }
            )
        })
        .expect("pointer restore obligation must remain queued");
    assert!(matches!(
        current_restore.effect(),
        PlatformEffect::SetPointerPassthrough {
            binding: actual,
            enabled: false,
            after: Some(after),
        } if *actual == binding && *after == emitted_restore
    ));
    assert!(current.iter().all(|request| {
        !matches!(
            request.effect(),
            PlatformEffect::RequestRootClose { binding: actual }
                | PlatformEffect::ReleaseChild { binding: actual }
                if *actual == binding
        )
    }));

    coordinator
        .publish_snapshot(&routing_snapshot(
            binding,
            2,
            WindowInputState::PassThrough,
            WindowPresentationState::Visible,
            PlatformCapability::Supported,
            InputEffectAcknowledgement::known(Some(enable)),
        ))
        .expect("late enable acknowledgement must remain observable");
    assert!(coordinator.pointer_passthrough.contains_binding(binding));
    assert!(coordinator.take_new_effects().is_empty());
    coordinator
        .publish_snapshot(&routing_snapshot(
            binding,
            3,
            WindowInputState::ReceivesInput,
            WindowPresentationState::Visible,
            PlatformCapability::Supported,
            InputEffectAcknowledgement::known(Some(current_restore.id())),
        ))
        .expect("only the exact current restore may settle the obligation");
    assert!(!coordinator.pointer_passthrough.contains_binding(binding));
    let cleanup = coordinator.take_new_effects();
    assert!(matches!(
        cleanup.as_slice(),
        [request]
            if matches!(
                request.effect(),
                PlatformEffect::ReleaseChild { binding: actual } if *actual == binding
            )
    ));
    assert!(matches!(
        retirement(&coordinator, binding).status(),
        BindingRetirementStatus::CleanupRequested { effect }
            if effect == cleanup[0].id()
    ));
}

#[test]
fn old_cleanup_result_requires_a_current_exact_binding_continuation() {
    let (mut coordinator, binding) = runtime_child_routing_coordinator();
    let first_epoch = WorkspaceEpoch::new(1);
    coordinator
        .reconcile_workspace_epoch(first_epoch, &BTreeSet::new())
        .expect("first restore must retire the binding");
    let destructive = coordinator
        .take_new_effects()
        .into_iter()
        .find_map(|request| {
            matches!(
                request.effect(),
                PlatformEffect::ReleaseChild { binding: actual } if *actual == binding
            )
            .then_some(request.id())
        })
        .expect("first restore must emit destructive cleanup");
    let current_epoch = WorkspaceEpoch::new(2);
    coordinator
        .reconcile_workspace_epoch(current_epoch, &BTreeSet::new())
        .expect("second restore must create a cleanup continuation");
    let _ = coordinator.take_new_effects();

    let wrong_incarnation = ViewportBinding::new(
        binding.authority_domain(),
        binding.epoch(),
        binding.surface(),
        binding.token(),
        binding
            .incarnation()
            .checked_next()
            .expect("test incarnation must advance"),
    );
    coordinator
        .binding_retirement
        .corrupt_binding_identity_for_test(binding, wrong_incarnation);

    assert_eq!(
        report_effect_result(
            &mut coordinator,
            current_epoch,
            destructive,
            first_epoch,
            EffectDispatchResult::DispatchFailed(
                crate::effect::DispatchFailureReason::ProviderStopped,
            ),
        ),
        EffectTransition::StaleEpoch
    );
    assert_eq!(
        coordinator
            .effects()
            .record(destructive)
            .map(crate::effect::EffectRecord::phase),
        Some(EffectPhase::Requested)
    );
}

#[test]
fn old_cleanup_result_is_stale_while_same_boundary_continuation_is_unemitted() {
    let (mut coordinator, binding) = runtime_child_routing_coordinator();
    let first_epoch = WorkspaceEpoch::new(1);
    coordinator
        .reconcile_workspace_epoch(first_epoch, &BTreeSet::new())
        .expect("first restore must retire the binding");
    let destructive = coordinator
        .take_new_effects()
        .into_iter()
        .find_map(|request| {
            matches!(
                request.effect(),
                PlatformEffect::ReleaseChild { binding: actual } if *actual == binding
            )
            .then_some(request.id())
        })
        .expect("first restore must emit destructive cleanup");

    let current_epoch = WorkspaceEpoch::new(2);
    let reconciliation = coordinator
        .reconcile_workspace_epoch(current_epoch, &BTreeSet::new())
        .expect("same boundary restore must queue a continuation");
    let continuation = reconciliation
        .cleanup_effects()
        .iter()
        .copied()
        .find(|effect| {
            matches!(
                coordinator.effects().record(*effect).map(|record| record.request().effect()),
                Some(PlatformEffect::ContinueCleanup {
                    predecessor,
                    ..
                }) if *predecessor == destructive
            )
        })
        .expect("restore must queue the exact cleanup continuation");
    assert!(
        !coordinator
            .effects()
            .record(continuation)
            .expect("continuation must remain queryable")
            .was_emitted()
    );

    assert_eq!(
        report_effect_result(
            &mut coordinator,
            current_epoch,
            destructive,
            first_epoch,
            EffectDispatchResult::DispatchFailed(
                crate::effect::DispatchFailureReason::ProviderStopped,
            ),
        ),
        EffectTransition::StaleEpoch
    );
    assert_eq!(
        coordinator
            .effects()
            .record(destructive)
            .map(crate::effect::EffectRecord::phase),
        Some(EffectPhase::Requested)
    );
}

#[test]
fn cleanup_observation_retry_rejects_ineligible_protocol_state_atomically() {
    #[derive(Debug, Clone, Copy)]
    enum IneligibleState {
        WrongTombstoneBinding,
        TerminalDestructivePredecessor,
        ExactDestroyed,
    }

    for state in [
        IneligibleState::WrongTombstoneBinding,
        IneligibleState::TerminalDestructivePredecessor,
        IneligibleState::ExactDestroyed,
    ] {
        let (mut coordinator, binding, destructive, continuation, destructive_epoch, _) =
            failed_cleanup_observation();
        match state {
            IneligibleState::WrongTombstoneBinding => {
                let wrong_binding = ViewportBinding::new(
                    binding.authority_domain(),
                    binding.epoch(),
                    binding.surface(),
                    binding.token(),
                    binding
                        .incarnation()
                        .checked_next()
                        .expect("test incarnation must advance"),
                );
                coordinator
                    .binding_retirement
                    .corrupt_binding_identity_for_test(binding, wrong_binding);
            }
            IneligibleState::TerminalDestructivePredecessor => {
                let provider = coordinator
                    .platform_provider()
                    .expect("test coordinator must retain its platform provider");
                assert_eq!(
                    coordinator.effects.report_exact(
                        provider,
                        EffectResult::new(
                            destructive,
                            destructive_epoch,
                            EffectDispatchResult::DispatchFailed(
                                crate::effect::DispatchFailureReason::ProviderStopped,
                            ),
                        ),
                    ),
                    EffectTransition::Applied
                );
            }
            IneligibleState::ExactDestroyed => {
                coordinator
                    .publish_snapshot(&snapshot_with_close_at(
                        2,
                        Vec::new(),
                        vec![close_observation(binding, 1, WindowCloseState::Destroyed)],
                    ))
                    .expect("exact destroyed evidence must retire cleanup ownership");
                assert!(!coordinator.binding_retirement.contains_key(&binding));
            }
        }

        let before = coordinator.clone();
        let rejected = coordinator.retry_cleanup(continuation);
        match state {
            IneligibleState::WrongTombstoneBinding
            | IneligibleState::TerminalDestructivePredecessor => assert!(matches!(
                rejected,
                Err(ViewportCoordinatorError::InvalidCleanupContinuation {
                    effect,
                    predecessor,
                }) if effect == continuation && predecessor == destructive
            )),
            IneligibleState::ExactDestroyed => assert!(matches!(
                rejected,
                Err(ViewportCoordinatorError::CleanupEffectNotRetryable {
                    effect,
                    phase: EffectPhase::Destroyed { .. },
                })
                    if effect == continuation
            )),
        }
        assert_eq!(coordinator, before, "retry mutated {state:?}");
        assert!(coordinator.take_new_effects().is_empty());
    }
}

#[test]
fn repeated_restore_migrates_cleanup_only_after_the_pointer_barrier() {
    for restore_was_indeterminate in [false, true] {
        let (mut coordinator, binding, _, old_restore) = released_unobserved_runtime_child_enable();
        if restore_was_indeterminate {
            assert_eq!(
                report_effect_result(
                    &mut coordinator,
                    binding.epoch(),
                    old_restore,
                    binding.epoch(),
                    EffectDispatchResult::Indeterminate(
                        crate::effect::EffectIndeterminateReason::AcknowledgementLost,
                    ),
                ),
                EffectTransition::Applied
            );
        }

        let first_epoch = WorkspaceEpoch::new(1);
        coordinator
            .reconcile_workspace_epoch(first_epoch, &BTreeSet::new())
            .expect("first workspace replacement must retire the binding");
        assert!(coordinator.take_new_effects().is_empty());

        coordinator
            .publish_snapshot(&routing_snapshot(
                binding,
                2,
                WindowInputState::ReceivesInput,
                WindowPresentationState::Visible,
                PlatformCapability::Supported,
                InputEffectAcknowledgement::known(Some(old_restore)),
            ))
            .expect("exact pointer restoration must open the cleanup barrier");
        assert!(!coordinator.pointer_passthrough.contains_binding(binding));
        let cleanup_requests = coordinator.take_new_effects();
        let cleanup = cleanup_requests
            .iter()
            .find_map(|request| match request.effect() {
                PlatformEffect::ReleaseChild { binding: actual } if *actual == binding => {
                    Some(request.id())
                }
                _ => None,
            })
            .expect("restored runtime child must request cleanup");

        let second_epoch = WorkspaceEpoch::new(2);
        coordinator
            .reconcile_workspace_epoch(second_epoch, &BTreeSet::new())
            .expect("second workspace replacement must migrate cleanup observation");
        let migrated = coordinator.take_new_effects();
        let cleanup_successor = migrated
            .iter()
            .find_map(|request| match request.effect() {
                PlatformEffect::ContinueCleanup {
                    binding: actual,
                    predecessor,
                    ..
                } if *actual == binding && *predecessor == cleanup => Some(request.id()),
                _ => None,
            })
            .expect("emitted cleanup must receive an observation-only successor");
        assert!(migrated.iter().all(|request| !matches!(
            request.effect(),
            PlatformEffect::SetPointerPassthrough { .. }
                | PlatformEffect::ReleaseChild { .. }
                | PlatformEffect::RequestRootClose { .. }
        )));

        assert_eq!(
            report_effect_result(
                &mut coordinator,
                second_epoch,
                cleanup_successor,
                second_epoch,
                EffectDispatchResult::DispatchFailed(
                    crate::effect::DispatchFailureReason::ProviderStopped,
                ),
            ),
            EffectTransition::Applied
        );
        assert_eq!(
            coordinator
                .effects()
                .record(cleanup_successor)
                .map(crate::effect::EffectRecord::phase),
            Some(EffectPhase::ObservationDispatchFailed(
                crate::effect::DispatchFailureReason::ProviderStopped,
            ))
        );

        let observation_retry = coordinator
            .retry_cleanup(cleanup_successor)
            .expect("provider recovery must retry only cleanup observation");
        let retry_requests = coordinator.take_new_effects();
        assert!(matches!(
            retry_requests.as_slice(),
            [request]
                if request.id() == observation_retry
                    && matches!(
                        request.effect(),
                        PlatformEffect::ContinueCleanup {
                            binding: actual,
                            predecessor,
                            after: Some(after),
                        } if *actual == binding
                            && *predecessor == cleanup
                            && *after == cleanup_successor
                    )
        ));

        let failure = EffectDispatchResult::DispatchFailed(
            crate::effect::DispatchFailureReason::ProviderStopped,
        );
        assert_eq!(
            report_effect_result(
                &mut coordinator,
                second_epoch,
                cleanup,
                second_epoch,
                failure,
            ),
            EffectTransition::StaleEpoch
        );
        assert_eq!(
            report_effect_result(
                &mut coordinator,
                second_epoch,
                cleanup,
                first_epoch,
                failure,
            ),
            EffectTransition::Applied
        );
        assert_eq!(
            coordinator
                .effects()
                .record(cleanup)
                .map(EffectRecord::phase),
            Some(EffectPhase::DispatchFailed(
                crate::effect::DispatchFailureReason::ProviderStopped,
            ))
        );
        assert_eq!(
            retirement(&coordinator, binding).status(),
            BindingRetirementStatus::CleanupFailed { effect: cleanup }
        );
    }
}

#[test]
fn retired_observed_window_restores_input_before_cleanup_can_fail() {
    let (mut coordinator, binding) = runtime_child_routing_coordinator();
    let pointer = PointerId::new(1);
    let enable = coordinator
        .begin_drag_routing(pointer, binding.surface())
        .expect("drag routing must begin")
        .expect("receiving input must request passthrough");
    let _ = coordinator.take_new_effects();
    coordinator
        .publish_snapshot(&routing_snapshot(
            binding,
            2,
            WindowInputState::PassThrough,
            WindowPresentationState::Visible,
            PlatformCapability::Supported,
            InputEffectAcknowledgement::known(Some(enable)),
        ))
        .expect("passthrough must be observed before replacement");
    let old_restore = coordinator
        .end_drag_routing(pointer)
        .expect("drag release must preserve restoration")
        .expect("old restore effect must exist");
    let _ = coordinator.take_new_effects();

    let new_epoch = WorkspaceEpoch::new(1);
    let reconciliation = coordinator
        .reconcile_workspace_epoch(new_epoch, &BTreeSet::new())
        .expect("workspace replacement must retire the old binding");
    assert_eq!(reconciliation.retired(), &[binding]);
    assert!(coordinator.take_new_effects().is_empty());
    assert_eq!(
        retirement(&coordinator, binding).status(),
        BindingRetirementStatus::AwaitingInputRestore
    );

    coordinator
        .publish_snapshot(&routing_snapshot(
            binding,
            3,
            WindowInputState::PassThrough,
            WindowPresentationState::Visible,
            PlatformCapability::Supported,
            InputEffectAcknowledgement::known(Some(enable)),
        ))
        .expect("late enable must not cross the restoration barrier");
    assert!(coordinator.pointer_passthrough.contains_binding(binding));
    assert!(coordinator.take_new_effects().is_empty());
    coordinator
        .publish_snapshot(&routing_snapshot(
            binding,
            4,
            WindowInputState::ReceivesInput,
            WindowPresentationState::Visible,
            PlatformCapability::Supported,
            InputEffectAcknowledgement::known(Some(old_restore)),
        ))
        .expect("retired input must settle before cleanup begins");
    assert!(!coordinator.pointer_passthrough.contains_binding(binding));
    let cleanup = coordinator.take_new_effects();
    let close = cleanup
        .iter()
        .find_map(|request| match request.effect() {
            PlatformEffect::ReleaseChild { binding: actual } if *actual == binding => {
                Some(request.id())
            }
            _ => None,
        })
        .expect("restored runtime child must request release cleanup");
    assert_eq!(
        coordinator
            .report_effect(
                new_epoch,
                EffectResult::new(
                    close,
                    new_epoch,
                    EffectDispatchResult::DispatchFailed(
                        crate::effect::DispatchFailureReason::AdapterRejected,
                    ),
                ),
            )
            .expect("effect report must reduce"),
        EffectTransition::Applied
    );
    assert!(matches!(
        coordinator
            .effects()
            .record(old_restore)
            .expect("retired restore must remain queryable")
            .phase(),
        EffectPhase::ObservedApplied { .. }
    ));
    assert_eq!(
        retirement(&coordinator, binding).status(),
        BindingRetirementStatus::CleanupFailed { effect: close }
    );
}

#[test]
fn hidden_and_minimized_windows_cannot_start_pointer_routing() {
    for presentation in [
        WindowPresentationState::Hidden,
        WindowPresentationState::Minimized,
    ] {
        let (mut coordinator, binding) = routing_coordinator(
            WindowInputState::ReceivesInput,
            presentation,
            PlatformCapability::Supported,
        );
        assert_eq!(
            coordinator
                .begin_drag_routing(PointerId::new(1), binding.surface())
                .expect("non-routeable presentation is a supported no-op"),
            None
        );
        assert!(coordinator.take_new_effects().is_empty());
    }
}
