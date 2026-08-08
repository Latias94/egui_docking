use super::route_lifecycle::{configured_child_accepts_predecessor, existing_restored_core_route};
use super::*;
use dockspace::effect::{DispatchFailureReason, EffectDispatchResult, EffectId};
use dockspace::engine::BackendIngressProgress;
use dockspace::graph::{Node, RootRecord, SurfacePresentation, Workspace};
use dockspace::ids::{ItemId, RootId, SurfaceId, WorkspaceEpoch};
use dockspace::pointer_receiver::PointerReceiverReceiptBatch;
use dockspace::policy::DockPolicy;
use dockspace::scene_manifest::MeasurementUnavailableReason;
use dockspace::surface_recovery::SurfaceRecoveryBootstrap;
use dockspace::viewport::{ViewportRole, WindowToken};
use egui_dockspace::{EguiFrameScheduleKey, EguiPresentationResult, NativeCoreRoute};

const HOST_SURFACE: SurfaceId = SurfaceId::new(1);
const CHILD_SURFACE: SurfaceId = SurfaceId::new(2);

fn test_dockspace() -> Dockspace {
    let root = RootId::new(1);
    let mut builder = Workspace::builder();
    let tabs = builder.insert_node(Node::tabs([ItemId::new(1)]));
    builder.set_root(root, RootRecord::new(tabs));
    builder.set_surface(HOST_SURFACE, SurfacePresentation::with_main(root));
    Dockspace::builder(
        "native-ingress-transaction",
        builder.build().expect("the test workspace is valid"),
    )
    .policy(DockPolicy::default())
    .build()
    .expect("the test dockspace is valid")
}

fn restored_test_dockspace() -> Dockspace {
    let host_root = RootId::new(1);
    let child_root = RootId::new(2);
    let mut builder = Workspace::builder();
    let host_tabs = builder.insert_node(Node::tabs([ItemId::new(1)]));
    let child_tabs = builder.insert_node(Node::tabs([ItemId::new(2)]));
    builder.set_root(host_root, RootRecord::new(host_tabs));
    builder.set_root(child_root, RootRecord::new(child_tabs));
    builder.set_surface(HOST_SURFACE, SurfacePresentation::with_main(host_root));
    builder.set_surface(CHILD_SURFACE, SurfacePresentation::with_main(child_root));
    Dockspace::builder(
        "native-pending-registration-retirement",
        builder.build().expect("the test workspace is valid"),
    )
    .policy(DockPolicy::default())
    .build()
    .expect("the test dockspace is valid")
}

fn record_idle_pointer(recorder: &mut BackendIngressRecorder) {
    let watermark = PointerEdgeSequence::new(0);
    recorder
        .record_pointer_segment(
            PointerEdgeJournal::new(watermark, watermark, Vec::new())
                .expect("an idle pointer interval is canonical"),
        )
        .expect("the idle pointer interval enters the backend order");
}

fn commit_backend_batch(
    dockspace: &mut Dockspace,
    recorder: &BackendIngressRecorder,
    routes: impl IntoIterator<Item = NativeCoreRoute>,
    frame: u64,
) {
    commit_backend_batch_with_roster(
        dockspace,
        recorder,
        NativeBindingRoster::new(routes, []),
        frame,
    );
}

fn commit_backend_batch_with_roster(
    dockspace: &mut Dockspace,
    recorder: &BackendIngressRecorder,
    roster: NativeBindingRoster,
    frame: u64,
) {
    let mut input = dockspace
        .begin_native_cycle(EguiFrameScheduleKey::new(frame, 0), roster)
        .expect("the native frame begins");
    let progress = input
        .submit_ingress(
            recorder
                .pending_batch()
                .expect("the backend suffix freezes"),
        )
        .expect("the backend suffix reduces");
    let progress = if progress == BackendIngressProgress::ReceiverReceiptsRequired {
        input
            .submit_pointer_receiver_receipts(
                PointerReceiverReceiptBatch::new([])
                    .expect("an empty receiver answer is canonical"),
            )
            .expect("the empty pointer interval resumes")
    } else {
        progress
    };
    assert_eq!(progress, BackendIngressProgress::Complete);
    let mut presentation = input
        .into_presentation()
        .expect("complete backend input enters presentation");
    let expected = presentation.expected_surfaces().collect::<Vec<_>>();
    for surface in expected {
        presentation
            .mark_surface_unavailable(dockspace, surface, MeasurementUnavailableReason::Deferred)
            .expect("the headless test explicitly completes every surface");
    }
    let commit = presentation
        .finish(dockspace)
        .expect("the backend frame commits atomically");
    for output in commit.into_parts().1 {
        output.settle_with(|_, _| EguiPresentationResult::Dropped);
    }
}

fn test_capabilities() -> PlatformCapabilities {
    let mut capabilities = PlatformCapabilities::default();
    capabilities.set_native_window_lifecycle(PlatformCapability::Supported);
    capabilities.set_authoritative_inventory(PlatformCapability::Supported);
    capabilities.set_global_window_placement(PlatformCapability::Supported);
    capabilities.set_work_area(PlatformCapability::Supported);
    capabilities.set_global_focus_observation(PlatformCapability::Supported);
    capabilities
}

fn observed_host_window(binding: ViewportBinding) -> ObservedWindow {
    ObservedWindow::new(binding)
        .with_coordinate_observation(WindowCoordinateObservation::new(
            binding,
            CoordinateObservationGeneration::new(1),
            Authority::Known(
                PhysicalRect::new(0.0, 0.0, 640.0, 480.0).expect("the client bounds are valid"),
            ),
            Authority::Known(
                PhysicalRect::new(-8.0, -30.0, 656.0, 518.0).expect("the outer bounds are valid"),
            ),
            Authority::Known(ScaleFactor::new(1.0).expect("the scale is valid")),
            Authority::Known(ScaleFactor::new(1.0).expect("the scale is valid")),
        ))
        .with_input_state(Authority::Known(WindowInputState::ReceivesInput))
        .with_presentation_observation(WindowPresentationObservation::new(
            binding,
            PresentationObservationGeneration::new(1),
            Authority::Known(WindowPresentationState::Visible),
            PresentationEffectAcknowledgement::known(None),
        ))
        .with_close_requested(Authority::Known(false))
}

fn destroyed_child_snapshot(
    generation: u64,
    host: ViewportBinding,
    child: ViewportBinding,
) -> PlatformSnapshot {
    let host_window = observed_host_window(host);
    PlatformSnapshot::new(
        PlatformSnapshotGeneration::new(generation),
        CapabilityRosterObservation::new(
            CapabilityObservationGeneration::new(generation),
            Authority::Known(test_capabilities()),
        ),
        FocusObservationEnvelope::new(
            FocusObservationGeneration::new(generation),
            Authority::Known(GlobalFocusedWindow::Dock(host)),
            Authority::Known(None),
        ),
        WindowInventoryObservation::new(
            InventoryObservationGeneration::new(generation),
            Authority::Known(vec![host]),
        )
        .expect("the live host inventory is canonical"),
        vec![host_window],
        vec![WindowCloseObservation::new(
            child,
            CloseObservationGeneration::new(generation),
            Authority::Known(WindowCloseState::Destroyed),
            CloseEffectAcknowledgement::known(None),
        )],
        WorkAreaRosterObservation::new(
            WorkAreaObservationGeneration::new(generation),
            Authority::Known(vec![ObservedWorkArea::new(
                WorkAreaToken::new(1),
                PhysicalRect::new(0.0, 0.0, 1920.0, 1080.0).expect("the work area is valid"),
                ScaleFactor::new(1.0).expect("the work-area scale is valid"),
            )]),
        )
        .expect("the work-area roster is canonical"),
    )
    .expect("the destroyed-child snapshot is canonical")
}

fn live_child_snapshot(
    generation: u64,
    host: ViewportBinding,
    child: ViewportBinding,
) -> PlatformSnapshot {
    let mut inventory = vec![host, child];
    inventory.sort_unstable();
    PlatformSnapshot::new(
        PlatformSnapshotGeneration::new(generation),
        CapabilityRosterObservation::new(
            CapabilityObservationGeneration::new(generation),
            Authority::Known(test_capabilities()),
        ),
        FocusObservationEnvelope::new(
            FocusObservationGeneration::new(generation),
            Authority::Known(GlobalFocusedWindow::Dock(host)),
            Authority::Known(None),
        ),
        WindowInventoryObservation::new(
            InventoryObservationGeneration::new(generation),
            Authority::Known(inventory),
        )
        .expect("the live child inventory is canonical"),
        vec![observed_host_window(host), observed_host_window(child)],
        Vec::new(),
        WorkAreaRosterObservation::new(
            WorkAreaObservationGeneration::new(generation),
            Authority::Known(vec![ObservedWorkArea::new(
                WorkAreaToken::new(1),
                PhysicalRect::new(0.0, 0.0, 1920.0, 1080.0).expect("the work area is valid"),
                ScaleFactor::new(1.0).expect("the work-area scale is valid"),
            )]),
        )
        .expect("the work-area roster is canonical"),
    )
    .expect("the live-child snapshot is canonical")
}

struct PendingRestoredFixture {
    dockspace: Dockspace,
    bridge: NativeIngressBridge,
    presentations: NativePresentationLedger,
    host_route: NativeCoreRoute,
    child_exact: ExactNativeViewport,
    child_binding: ViewportBinding,
}

fn committed_pending_restored_registration() -> PendingRestoredFixture {
    let mut dockspace = restored_test_dockspace();
    let mut bridge = NativeIngressBridge::new().expect("runtime identity is available");
    bridge.recorder = Some(
        dockspace
            .create_backend_ingress_provider(PointerEdgeSequence::new(0))
            .expect("the test backend provider is available"),
    );

    dockspace
        .record_backend_viewport_registration(
            bridge.recorder.as_mut().expect("the recorder is enrolled"),
            HOST_SURFACE,
            WindowToken::new(1),
            ViewportRole::Root,
            None,
        )
        .expect("the host registration is staged");
    record_idle_pointer(bridge.recorder.as_mut().expect("the recorder is enrolled"));
    commit_backend_batch(
        &mut dockspace,
        bridge.recorder.as_ref().expect("the recorder is enrolled"),
        [],
        1,
    );
    bridge
        .reclaim_committed_prefix(&mut dockspace)
        .expect("the committed host prefix is reclaimed");
    let host_binding = dockspace
        .native_viewport_binding(HOST_SURFACE)
        .expect("the host registration minted a binding");
    let host_route = NativeCoreRoute::new(
        ExactNativeViewport::new(ViewportId::ROOT, NativeViewportIncarnation::new(1)),
        HOST_SURFACE,
        host_binding,
    );

    let child_viewport = ViewportId::from_hash_of("pending-restored-child");
    let child_exact = ExactNativeViewport::new(child_viewport, NativeViewportIncarnation::new(1));
    let ordinal = dockspace
        .record_backend_child_viewport_bootstrap(
            bridge.recorder.as_mut().expect("the recorder is enrolled"),
            CHILD_SURFACE,
            WindowToken::new(2),
            SurfaceRecoveryBootstrap::new(HOST_SURFACE),
        )
        .expect("the restored child registration is staged");
    bridge
        .insert_pending_registration(PendingNativeRegistration::staged(
            child_exact,
            CHILD_SURFACE,
            ordinal,
        ))
        .expect("the restored child sidecar is staged");
    record_idle_pointer(bridge.recorder.as_mut().expect("the recorder is enrolled"));
    commit_backend_batch(
        &mut dockspace,
        bridge.recorder.as_ref().expect("the recorder is enrolled"),
        [host_route],
        2,
    );
    bridge
        .reclaim_committed_prefix(&mut dockspace)
        .expect("the committed child prefix is reclaimed");
    let child_binding = dockspace
        .native_viewport_binding(CHILD_SURFACE)
        .expect("the restored child registration minted a binding");
    assert_eq!(
        bridge
            .pending_registrations
            .get(&child_exact)
            .and_then(|pending| pending.committed_binding()),
        Some(child_binding),
    );

    PendingRestoredFixture {
        dockspace,
        bridge,
        presentations: NativePresentationLedger::new().expect("presentation identity is available"),
        host_route,
        child_exact,
        child_binding,
    }
}

#[test]
fn an_idle_native_cycle_still_submits_its_pointer_watermark() {
    let watermark = PointerEdgeSequence::new(41);
    let interval = empty_pointer_interval(false, watermark)
        .expect("an unchanged watermark is structurally valid")
        .expect("an idle provider cycle requires an explicit interval");

    assert_eq!(interval.previous(), watermark);
    assert_eq!(interval.through(), watermark);
    assert!(interval.edges().is_empty());
    assert!(
        empty_pointer_interval(true, watermark)
            .expect("a prior segment is structurally valid")
            .is_none()
    );
}

#[test]
fn unavailable_delivery_authority_is_never_inferred_from_the_event_source() {
    assert_eq!(
        translate_delivery_owner_fact(
            None,
            Some(NativeUnavailableReason::StaleSource),
            &BTreeMap::new(),
        ),
        Authority::Unknown(AuthorityUnavailableReason::SurfaceUnavailable)
    );
    assert_eq!(
        translate_delivery_owner_fact(
            Some(&NativePointerDeliveryOwner::Foreign),
            None,
            &BTreeMap::new(),
        ),
        Authority::Known(PointerEventDeliveryOwner::Foreign)
    );
}

#[test]
fn native_scroll_delta_reaches_core_without_f32_normalization() {
    let raw = NativeScrollDelta::Lines(
        eframe::NativeFiniteScrollVector::new(16_777_217.25, -16_777_216.0)
            .expect("the native vector is finite"),
    );
    let translated = translate_scroll_delta(
        raw,
        Authority::Unknown(AuthorityUnavailableReason::NotReported),
    )
    .expect("line deltas do not require coordinate conversion");

    assert_eq!(translated.vector().x(), 16_777_217.25);
    assert_eq!(translated.vector().y(), -16_777_216.0);
}

#[test]
fn unknown_physical_scroll_coordinates_preserve_the_journal_sample() {
    let raw = NativeScrollDelta::PhysicalPixels(
        eframe::NativeFiniteScrollVector::new(2.5, -7.25).expect("the native vector is finite"),
    );
    let translated = translate_scroll_delta(
        raw,
        Authority::Unknown(AuthorityUnavailableReason::NotReported),
    )
    .expect("missing coordinate authority is a typed fact, not a cycle failure");

    assert!(matches!(
        translated,
        ScrollDelta::PhysicalPixels {
            delta,
            coordinates: Authority::Unknown(AuthorityUnavailableReason::NotReported),
        } if delta.x() == 2.5 && delta.y() == -7.25
    ));
}

#[test]
fn unsupported_visibility_fails_the_combined_native_lifecycle_closed() {
    let capability = translate_backend_capability(
        intersect_backend_capability(
            NativeBackendCapability::Supported,
            NativeBackendCapability::Unsupported,
        ),
        PlatformRequirement::NativeWindowLifecycle,
    );

    assert!(matches!(
        capability,
        PlatformCapability::Unsupported(issue)
            if issue.requirement() == PlatformRequirement::NativeWindowLifecycle
                && issue.reason() == PlatformCapabilityReason::BackendUnsupported
    ));
}

#[test]
fn unknown_backend_placement_never_becomes_supported() {
    let capability = translate_backend_capability(
        NativeBackendCapability::Unknown,
        PlatformRequirement::GlobalWindowPlacement,
    );

    assert!(matches!(
        capability,
        PlatformCapability::Unknown(issue)
            if issue.requirement() == PlatformRequirement::GlobalWindowPlacement
                && issue.reason() == PlatformCapabilityReason::EnvironmentUnavailable
    ));
}

#[test]
fn complete_backend_capability_roster_is_translated_without_implicit_support() {
    let capabilities = translate_capability_roster(
        NativeBackendCapabilities::default(),
        PlatformCapability::Supported,
    );
    let translated = [
        (
            capabilities.native_window_lifecycle(),
            PlatformRequirement::NativeWindowLifecycle,
        ),
        (
            capabilities.authoritative_inventory(),
            PlatformRequirement::AuthoritativeInventory,
        ),
        (
            capabilities.hovered_window(),
            PlatformRequirement::HoveredWindow,
        ),
        (
            capabilities.desktop_pointer_position(),
            PlatformRequirement::DesktopPointerPosition,
        ),
        (
            capabilities.authoritative_button_state(),
            PlatformRequirement::AuthoritativeButtonState,
        ),
        (
            capabilities.global_window_placement(),
            PlatformRequirement::GlobalWindowPlacement,
        ),
        (
            capabilities.pointer_hit_test_observation(),
            PlatformRequirement::PointerHitTestObservation,
        ),
        (
            capabilities.pointer_hit_test_control(),
            PlatformRequirement::PointerHitTestControl,
        ),
        (
            capabilities.global_focus_observation(),
            PlatformRequirement::GlobalFocusObservation,
        ),
        (
            capabilities.window_activation_control(),
            PlatformRequirement::WindowActivationControl,
        ),
        (
            capabilities.close_cancellation(),
            PlatformRequirement::CloseCancellation,
        ),
    ];

    for (capability, requirement) in translated {
        assert!(matches!(
            capability,
            PlatformCapability::Unknown(issue)
                if issue.requirement() == requirement
                    && issue.reason() == PlatformCapabilityReason::EnvironmentUnavailable
        ));
    }
    assert_eq!(capabilities.work_area(), PlatformCapability::Supported);
}

#[test]
fn backend_sidecars_survive_retry_and_retire_only_after_commit() {
    let mut ledger = BackendSidecarLedger::default();
    assert_eq!(ledger.insert(3, "edge-three"), None);
    assert_eq!(ledger.insert(5, "edge-five"), None);

    assert_eq!(ledger.get(3), Some(&"edge-three"));
    assert_eq!(ledger.get(5), Some(&"edge-five"));
    ledger.retire_through(2);
    assert_eq!(ledger.len(), 2, "an aborted prefix is retained for replay");

    ledger.retire_through(3);
    assert_eq!(ledger.get(3), None);
    assert_eq!(ledger.get(5), Some(&"edge-five"));
    assert_eq!(ledger.len(), 1);
}

#[test]
fn post_commit_outbox_is_restored_with_the_native_prepare_snapshot() {
    let mut bridge = NativeIngressBridge::new().expect("runtime identity is available");
    let effect = EffectResult::new(
        EffectId::new(7),
        WorkspaceEpoch::new(3),
        EffectDispatchResult::DispatchFailed(DispatchFailureReason::AdapterRejected),
    );
    bridge
        .post_commit_records
        .push(PostCommitRecord::Effect(DeferredEffectResult::Ordinary(
            effect,
        )));
    let snapshot = bridge.state_snapshot();

    bridge.post_commit_records.clear();
    bridge.restore_state(snapshot);

    assert!(matches!(
        bridge.post_commit_records.as_slice(),
        [PostCommitRecord::Effect(DeferredEffectResult::Ordinary(restored))]
            if *restored == effect
    ));
}

#[test]
fn post_commit_outbox_only_requires_follow_up_when_it_contains_work() {
    let mut bridge = NativeIngressBridge::new().expect("runtime identity is available");
    assert!(!bridge.has_post_commit_records());

    bridge
        .post_commit_records
        .push(PostCommitRecord::Effect(DeferredEffectResult::Ordinary(
            EffectResult::new(
                EffectId::new(7),
                WorkspaceEpoch::new(3),
                EffectDispatchResult::DispatchFailed(DispatchFailureReason::AdapterRejected),
            ),
        )));

    assert!(bridge.has_post_commit_records());
}

#[test]
fn local_destructive_result_waits_for_cleanup_observer_after_backend_handoff() {
    let PendingRestoredFixture {
        mut dockspace,
        mut bridge,
        child_binding,
        ..
    } = committed_pending_restored_registration();
    let current_epoch = dockspace.engine().version().epoch();
    let predecessor = EffectId::new(77);
    let old_provider = bridge
        .recorder
        .as_ref()
        .expect("the predecessor provider is enrolled")
        .lease()
        .platform_incarnation();
    bridge.post_commit_records.push(PostCommitRecord::Effect(
        DeferredEffectResult::local_destructive_for_test(
            predecessor,
            current_epoch,
            old_provider,
            child_binding,
            EffectDispatchResult::DispatchFailed(DispatchFailureReason::WindowUnavailable),
        ),
    ));

    let recorder = bridge
        .recorder
        .take()
        .expect("the predecessor recorder is enrolled");
    let mut drained = recorder.drain();
    let replacement = dockspace
        .begin_backend_ingress_provider_replacement(&mut drained)
        .expect("the predecessor provider can be retired");
    let (mut ticket, _) = replacement.into_parts();
    let successor = dockspace
        .finish_backend_ingress_provider_replacement(&mut ticket)
        .expect("the successor provider can be activated");
    assert_ne!(
        successor.lease().platform_incarnation(),
        old_provider,
        "the test must cross a real provider incarnation boundary",
    );
    bridge.recorder = Some(successor);

    bridge
        .flush_post_commit_records(current_epoch)
        .expect("the committed outbox remains replayable after handoff");
    assert!(
        bridge
            .recorder
            .as_ref()
            .expect("the successor recorder remains enrolled")
            .pending_batch()
            .expect("the successor recorder remains valid")
            .is_empty(),
        "a predecessor result must not be reported as an ordinary successor-provider fact",
    );
    assert_eq!(bridge.effects.cleanup_retained_counts(), (0, 1, 0, 0));
}

#[test]
fn committed_bootstrap_can_retire_before_its_adapter_route_and_fully_quiesce() {
    let PendingRestoredFixture {
        mut dockspace,
        mut bridge,
        mut presentations,
        host_route,
        child_exact,
        child_binding,
    } = committed_pending_restored_registration();
    let transaction = NativeIngressTransaction {
        recorder: bridge
            .recorder
            .as_ref()
            .expect("the recorder is enrolled")
            .savepoint(),
        state: bridge.state_snapshot(),
        presentation: presentations.prepare_savepoint(),
    };
    let mut unpublished_routes = BTreeMap::new();

    let retirement = bridge
        .retire_native_binding(
            &mut dockspace,
            &mut presentations,
            &mut unpublished_routes,
            &mut BTreeMap::new(),
            child_exact,
            CloseEffectAcknowledgement::known(None),
        )
        .expect("the committed bootstrap has exact retirement authority")
        .expect("the committed bootstrap is docking-owned");
    assert_eq!(retirement.surface(), CHILD_SURFACE);
    assert_eq!(retirement.core, child_binding);
    assert_eq!(retirement.adapter_retirement(), None);
    assert!(!bridge.pending_registrations.contains_key(&child_exact));
    assert_eq!(
        bridge.retired_routes.get(&child_exact),
        Some(&child_binding)
    );

    bridge
        .record_retirement_quiescence(&mut presentations, child_exact)
        .expect("full native quiescence follows the exact retirement");
    bridge
        .recorder
        .as_mut()
        .expect("the recorder is enrolled")
        .record_platform_snapshot(
            dockspace.engine().version().epoch(),
            destroyed_child_snapshot(1, host_route.core(), child_binding),
        )
        .expect("the ordered destroyed observation is recorded");
    record_idle_pointer(bridge.recorder.as_mut().expect("the recorder is enrolled"));
    commit_backend_batch(
        &mut dockspace,
        bridge.recorder.as_ref().expect("the recorder is enrolled"),
        [host_route],
        3,
    );
    bridge.commit_transaction(&presentations, transaction);
    presentations.accept_commit();
    assert!(dockspace.native_viewport_binding(CHILD_SURFACE).is_none());
    assert_eq!(
        dockspace
            .engine()
            .runtime_retention_manifest()
            .bindings()
            .destroyed_binding_guards(),
        1,
        "the committed destruction guard remains until exact backend quiescence settles",
    );

    bridge
        .reclaim_committed_prefix(&mut dockspace)
        .expect("the committed full-quiescence prefix reclaims its core guard");
    presentations
        .reclaim_committed_quiescence(&mut dockspace)
        .expect("the adapter presentation tombstone also reaches quiescence");
    assert!(bridge.retired_routes.is_empty());
    assert_eq!(
        dockspace
            .engine()
            .runtime_retention_manifest()
            .bindings()
            .destroyed_binding_guards(),
        0,
    );
}

#[test]
fn bootstrap_before_route_retirement_rolls_back_to_the_committed_registration() {
    let PendingRestoredFixture {
        mut dockspace,
        mut bridge,
        mut presentations,
        child_exact,
        child_binding,
        ..
    } = committed_pending_restored_registration();
    let transaction = NativeIngressTransaction {
        recorder: bridge
            .recorder
            .as_ref()
            .expect("the recorder is enrolled")
            .savepoint(),
        state: bridge.state_snapshot(),
        presentation: presentations.prepare_savepoint(),
    };
    let mut unpublished_routes = BTreeMap::new();

    bridge
        .retire_native_binding(
            &mut dockspace,
            &mut presentations,
            &mut unpublished_routes,
            &mut BTreeMap::new(),
            child_exact,
            CloseEffectAcknowledgement::known(None),
        )
        .expect("the committed bootstrap may stage retirement")
        .expect("the child is docking-owned");
    bridge
        .record_retirement_quiescence(&mut presentations, child_exact)
        .expect("the staged retirement may stage exact quiescence");
    assert!(!bridge.pending_registrations.contains_key(&child_exact));
    assert!(bridge.retired_routes.contains_key(&child_exact));

    bridge.rollback_transaction(&mut dockspace, &mut presentations, transaction);

    assert_eq!(
        bridge
            .pending_registrations
            .get(&child_exact)
            .and_then(|pending| pending.committed_binding()),
        Some(child_binding),
    );
    assert!(!bridge.retired_routes.contains_key(&child_exact));
    assert!(
        bridge
            .recorder
            .as_ref()
            .expect("the recorder remains enrolled")
            .pending_batch()
            .expect("the rolled-back recorder remains valid")
            .is_empty(),
    );
    assert!(!presentations.has_committed_quiescence_work());

    assert!(
        bridge
            .retire_native_binding(
                &mut dockspace,
                &mut presentations,
                &mut unpublished_routes,
                &mut BTreeMap::new(),
                child_exact,
                CloseEffectAcknowledgement::known(None),
            )
            .expect("the exact retirement remains retryable after rollback")
            .is_some(),
    );
}

#[test]
fn uncommitted_bootstrap_retirement_fails_closed_without_losing_registration() {
    let mut dockspace = test_dockspace();
    let mut bridge = NativeIngressBridge::new().expect("runtime identity is available");
    bridge.recorder = Some(
        dockspace
            .create_backend_ingress_provider(PointerEdgeSequence::new(0))
            .expect("the test backend provider is available"),
    );
    let exact = ExactNativeViewport::new(
        ViewportId::from_hash_of("uncommitted-bootstrap"),
        NativeViewportIncarnation::new(1),
    );
    let ordinal = dockspace
        .record_backend_viewport_registration(
            bridge.recorder.as_mut().expect("the recorder is enrolled"),
            HOST_SURFACE,
            WindowToken::new(7),
            ViewportRole::Root,
            None,
        )
        .expect("the registration is staged");
    bridge
        .insert_pending_registration(PendingNativeRegistration::staged(
            exact,
            HOST_SURFACE,
            ordinal,
        ))
        .expect("the registration sidecar is staged");
    let mut presentations =
        NativePresentationLedger::new().expect("presentation identity is available");

    let error = bridge
        .retire_native_binding(
            &mut dockspace,
            &mut presentations,
            &mut BTreeMap::new(),
            &mut BTreeMap::new(),
            exact,
            CloseEffectAcknowledgement::known(None),
        )
        .expect_err("an uncommitted registration cannot authorize core retirement");
    assert!(matches!(error, NativeRuntimeError::IngressUnavailable(_)));
    assert!(bridge.pending_registrations.contains_key(&exact));
    assert!(!bridge.retired_routes.contains_key(&exact));
}

#[test]
fn replacement_incarnation_waits_until_same_surface_retirement_commits() {
    let PendingRestoredFixture {
        dockspace,
        child_exact,
        child_binding,
        ..
    } = committed_pending_restored_registration();
    assert_eq!(
        existing_restored_core_route(&dockspace, CHILD_SURFACE, &[]),
        Some((CHILD_SURFACE, child_binding)),
    );
    let retirement = RetiredNativeRoute {
        exact: child_exact,
        surface: CHILD_SURFACE,
        core: child_binding,
        adapter_route_was_published: true,
        close_acknowledgement: CloseEffectAcknowledgement::known(None),
    };

    assert_eq!(
        existing_restored_core_route(&dockspace, CHILD_SURFACE, &[retirement]),
        None,
        "A2 must not route to C1 while the same batch retires A1/C1",
    );
}

#[test]
fn runtime_and_restored_children_authorize_only_their_exact_replacement_predecessor() {
    let PendingRestoredFixture {
        child_exact,
        child_binding,
        ..
    } = committed_pending_restored_registration();
    let runtime_child = crate::NativeSurfaceSpec::child(
        child_exact.viewport(),
        CHILD_SURFACE,
        egui::ViewportBuilder::default(),
    );
    assert!(configured_child_accepts_predecessor(
        &runtime_child,
        CHILD_SURFACE,
        child_binding,
    ));

    let restored_child = crate::NativeSurfaceSpec::restored_child(
        child_exact.viewport(),
        CHILD_SURFACE,
        child_binding.token(),
        SurfaceRecoveryBootstrap::new(HOST_SURFACE),
        egui::ViewportBuilder::default(),
    );
    assert!(configured_child_accepts_predecessor(
        &restored_child,
        CHILD_SURFACE,
        child_binding,
    ));

    let mismatched_restored_child = crate::NativeSurfaceSpec::restored_child(
        child_exact.viewport(),
        CHILD_SURFACE,
        WindowToken::new(child_binding.token().get() + 1),
        SurfaceRecoveryBootstrap::new(HOST_SURFACE),
        egui::ViewportBuilder::default(),
    );
    assert!(!configured_child_accepts_predecessor(
        &mismatched_restored_child,
        CHILD_SURFACE,
        child_binding,
    ));
}

fn runtime_child_roster(viewport: ViewportId) -> NativeViewportRoster {
    let mut roster = NativeViewportRoster::new(crate::NativeSurfaceSpec::root(
        HOST_SURFACE,
        WindowToken::new(900),
    ))
    .expect("the root native viewport is valid");
    roster
        .insert(crate::NativeSurfaceSpec::child(
            viewport,
            CHILD_SURFACE,
            egui::ViewportBuilder::default(),
        ))
        .expect("the runtime child native viewport is valid");
    roster
}

fn restored_child_roster(viewport: ViewportId) -> NativeViewportRoster {
    let mut roster = NativeViewportRoster::new(crate::NativeSurfaceSpec::root(
        HOST_SURFACE,
        WindowToken::new(900),
    ))
    .expect("the root native viewport is valid");
    roster
        .insert(crate::NativeSurfaceSpec::restored_child(
            viewport,
            CHILD_SURFACE,
            WindowToken::new(901),
            SurfaceRecoveryBootstrap::new(HOST_SURFACE),
            egui::ViewportBuilder::default(),
        ))
        .expect("the restored child native viewport is valid");
    roster
}

#[test]
fn replacement_lineage_routes_only_the_final_live_incarnation() {
    let PendingRestoredFixture {
        bridge,
        child_exact,
        child_binding,
        ..
    } = committed_pending_restored_registration();
    let intermediate = ExactNativeViewport::new(
        child_exact.viewport(),
        NativeViewportIncarnation::new(child_exact.incarnation().get() + 1),
    );
    let final_live = ExactNativeViewport::new(
        child_exact.viewport(),
        NativeViewportIncarnation::new(child_exact.incarnation().get() + 2),
    );
    let mut plan = bridge
        .plan_deferred_replacement_lineages_from_exacts(
            BTreeMap::from([(child_exact.viewport(), vec![child_exact, intermediate])]),
            &BTreeSet::from([final_live]),
            &runtime_child_roster(child_exact.viewport()),
        )
        .expect("the ordered replacement lineage is valid");

    assert_eq!(
        plan.successor_after_retirement.get(&child_exact),
        Some(&PendingNativeRegistration::awaiting_predecessor(
            final_live,
            CHILD_SURFACE,
            child_binding,
        )),
    );
    assert_eq!(
        plan.intermediate_retirements,
        BTreeSet::from([intermediate]),
    );
    assert_eq!(
        plan.route_less_lifetimes,
        BTreeSet::from([intermediate, final_live]),
    );
    assert_eq!(
        plan.live_keepalives,
        BTreeMap::from([(
            final_live.viewport(),
            RouteLessNativeKeepalive::new(final_live, CHILD_SURFACE),
        )]),
    );
    assert!(
        !bridge.routes.contains_key(&final_live.viewport()),
        "a physical keepalive must not become an authoritative route",
    );
    assert!(plan.terminal_viewports.is_empty());
    assert!(plan.take_intermediate(intermediate));
    assert!(plan.take_successor(child_exact).is_some());
    let (keepalives, terminal_viewports) = plan
        .finish()
        .expect("the ordered lineage is completely consumed");
    assert_eq!(
        keepalives,
        BTreeMap::from([(
            final_live.viewport(),
            RouteLessNativeKeepalive::new(final_live, CHILD_SURFACE),
        )]),
    );
    assert!(terminal_viewports.is_empty());
}

#[test]
fn replacement_lineage_without_a_live_successor_keeps_only_tombstones() {
    let PendingRestoredFixture {
        bridge,
        child_exact,
        ..
    } = committed_pending_restored_registration();
    let intermediate = ExactNativeViewport::new(
        child_exact.viewport(),
        NativeViewportIncarnation::new(child_exact.incarnation().get() + 1),
    );
    let mut plan = bridge
        .plan_deferred_replacement_lineages_from_exacts(
            BTreeMap::from([(child_exact.viewport(), vec![child_exact, intermediate])]),
            &BTreeSet::new(),
            &runtime_child_roster(child_exact.viewport()),
        )
        .expect("a fully retired replacement lineage is valid");

    assert!(plan.successor_after_retirement.is_empty());
    assert_eq!(
        plan.intermediate_retirements,
        BTreeSet::from([intermediate]),
    );
    assert_eq!(plan.route_less_lifetimes, BTreeSet::from([intermediate]),);
    assert!(plan.live_keepalives.is_empty());
    assert_eq!(
        plan.terminal_viewports,
        BTreeSet::from([child_exact.viewport()]),
    );
    assert!(plan.take_intermediate(intermediate));
    let (keepalives, terminal_viewports) = plan
        .finish()
        .expect("the terminal lineage is completely consumed");
    assert!(keepalives.is_empty());
    assert_eq!(terminal_viewports, BTreeSet::from([child_exact.viewport()]),);
}

#[test]
fn deferred_replacement_adoption_is_transactional_and_fast_retirement_keeps_core_authority() {
    let PendingRestoredFixture {
        mut dockspace,
        mut bridge,
        mut presentations,
        host_route,
        child_exact,
        child_binding,
    } = committed_pending_restored_registration();
    let replacement = ExactNativeViewport::new(
        child_exact.viewport(),
        NativeViewportIncarnation::new(child_exact.incarnation().get() + 1),
    );
    bridge.pending_registrations.insert(
        replacement,
        PendingNativeRegistration::awaiting_predecessor(replacement, CHILD_SURFACE, child_binding),
    );
    assert!(matches!(
        bridge.pending_registrations[&replacement].phase,
        PendingNativeRegistrationPhase::AwaitingPredecessorRetirement(binding)
            if binding == child_binding
    ));

    let child_route = NativeCoreRoute::new(child_exact, CHILD_SURFACE, child_binding);
    bridge
        .recorder
        .as_mut()
        .expect("the recorder is enrolled")
        .record_platform_snapshot(
            dockspace.engine().version().epoch(),
            live_child_snapshot(1, host_route.core(), child_binding),
        )
        .expect("the live child coordinates enter the ordered prefix");
    record_idle_pointer(bridge.recorder.as_mut().expect("the recorder is enrolled"));
    commit_backend_batch(
        &mut dockspace,
        bridge.recorder.as_ref().expect("the recorder is enrolled"),
        [host_route, child_route],
        3,
    );
    bridge
        .reclaim_committed_prefix(&mut dockspace)
        .expect("the live child facts are reclaimed after commit");

    let retirement_transaction = NativeIngressTransaction {
        recorder: bridge
            .recorder
            .as_ref()
            .expect("the recorder is enrolled")
            .savepoint(),
        state: bridge.state_snapshot(),
        presentation: presentations.prepare_savepoint(),
    };
    bridge
        .retire_native_binding(
            &mut dockspace,
            &mut presentations,
            &mut BTreeMap::new(),
            &mut BTreeMap::new(),
            child_exact,
            CloseEffectAcknowledgement::known(None),
        )
        .expect("the predecessor retirement is valid")
        .expect("the committed predecessor has core authority");
    bridge
        .record_retirement_quiescence(&mut presentations, child_exact)
        .expect("the predecessor ingress is fully quiescent");
    bridge
        .recorder
        .as_mut()
        .expect("the recorder is enrolled")
        .record_platform_snapshot(
            dockspace.engine().version().epoch(),
            destroyed_child_snapshot(2, host_route.core(), child_binding),
        )
        .expect("the predecessor destruction enters the ordered prefix");
    record_idle_pointer(bridge.recorder.as_mut().expect("the recorder is enrolled"));
    commit_backend_batch_with_roster(
        &mut dockspace,
        bridge.recorder.as_ref().expect("the recorder is enrolled"),
        NativeBindingRoster::new([host_route], [child_exact]),
        4,
    );
    bridge.commit_transaction(&presentations, retirement_transaction);
    presentations.accept_commit();
    bridge
        .reclaim_committed_prefix(&mut dockspace)
        .expect("the predecessor retirement boundary is reclaimed");
    let replacement_binding = dockspace
        .engine()
        .viewport()
        .recovery_pending(CHILD_SURFACE)
        .and_then(dockspace::frame::RecoveryPending::replacement_binding)
        .expect("core destruction mints one exact replacement binding");
    assert_ne!(replacement_binding, child_binding);

    let replay_transaction = NativeIngressTransaction {
        recorder: bridge
            .recorder
            .as_ref()
            .expect("the recorder is enrolled")
            .savepoint(),
        state: bridge.state_snapshot(),
        presentation: presentations.prepare_savepoint(),
    };
    let mut effect_cycle = bridge.begin_effect_cycle();
    assert_eq!(
        effect_cycle.deferred_replacements.get(&CHILD_SURFACE),
        Some(&replacement),
    );
    effect_cycle
        .adopted_replacements
        .insert(replacement, replacement_binding);
    bridge
        .prepare_effect_cycle_adoptions(&effect_cycle)
        .expect("the existing native lifetime adopts the core-minted replacement");
    assert!(matches!(
        bridge.pending_registrations[&replacement].phase,
        PendingNativeRegistrationPhase::AdoptedReplacement {
            predecessor,
            replacement,
        } if predecessor == child_binding && replacement == replacement_binding
    ));
    bridge.rollback_transaction(&mut dockspace, &mut presentations, replay_transaction);
    assert!(matches!(
        bridge.pending_registrations[&replacement].phase,
        PendingNativeRegistrationPhase::AwaitingPredecessorRetirement(binding)
            if binding == child_binding
    ));

    bridge
        .prepare_effect_cycle_adoptions(&effect_cycle)
        .expect("the aborted adoption can be replayed exactly");
    assert!(matches!(
        bridge.pending_registrations[&replacement].phase,
        PendingNativeRegistrationPhase::AdoptedReplacement {
            predecessor,
            replacement,
        } if predecessor == child_binding && replacement == replacement_binding
    ));

    let retirement = bridge
        .retire_native_binding(
            &mut dockspace,
            &mut presentations,
            &mut BTreeMap::new(),
            &mut BTreeMap::new(),
            replacement,
            CloseEffectAcknowledgement::known(None),
        )
        .expect("an adopted replacement has exact core retirement authority")
        .expect("the adopted replacement remains docking-owned before route publication");
    assert_eq!(retirement.core, replacement_binding);
    assert_eq!(retirement.adapter_retirement(), None);
    assert_eq!(
        bridge.retired_routes.get(&replacement),
        Some(&replacement_binding),
    );
    assert!(
        !bridge
            .deferred_replacement_retirements
            .contains(&replacement),
        "an adopted replacement must not fall back to the authority-free retirement lane",
    );
}

#[test]
fn deferred_replacement_can_coexist_with_an_unpublished_committed_predecessor() {
    let PendingRestoredFixture {
        mut bridge,
        child_exact,
        child_binding,
        ..
    } = committed_pending_restored_registration();
    let replacement = ExactNativeViewport::new(
        child_exact.viewport(),
        NativeViewportIncarnation::new(child_exact.incarnation().get() + 1),
    );
    let registration =
        PendingNativeRegistration::awaiting_predecessor(replacement, CHILD_SURFACE, child_binding);

    bridge
        .insert_deferred_replacement_registration(registration, child_exact)
        .expect("the unpublished committed predecessor remains exact retirement authority");

    assert_eq!(
        bridge.pending_registrations[&child_exact].committed_binding(),
        Some(child_binding),
    );
    assert_eq!(
        bridge.pending_registrations[&replacement].predecessor_binding(),
        Some(child_binding),
    );
}

#[test]
fn deferred_replacement_fast_retirement_is_exact_and_rollback_safe() {
    let PendingRestoredFixture {
        mut bridge,
        child_exact,
        child_binding,
        ..
    } = committed_pending_restored_registration();
    let replacement = ExactNativeViewport::new(
        child_exact.viewport(),
        NativeViewportIncarnation::new(child_exact.incarnation().get() + 1),
    );
    bridge.pending_registrations.insert(
        replacement,
        PendingNativeRegistration::awaiting_predecessor(replacement, CHILD_SURFACE, child_binding),
    );
    let snapshot = bridge.state_snapshot();

    assert!(
        bridge
            .retire_deferred_replacement(replacement)
            .expect("the route-less replacement has a typed retirement lane"),
    );
    assert!(!bridge.pending_registrations.contains_key(&replacement));
    assert!(bridge.route_less_replacement(replacement));

    bridge.restore_state(snapshot);
    assert!(bridge.pending_registrations.contains_key(&replacement));
    assert!(
        !bridge
            .deferred_replacement_retirements
            .contains(&replacement)
    );

    bridge
        .retire_deferred_replacement(replacement)
        .expect("the replayed retirement remains exact");
    let mut presentations =
        NativePresentationLedger::new().expect("presentation identity is available");
    bridge
        .record_retirement_quiescence(&mut presentations, replacement)
        .expect("route-less replacement quiescence needs no forged core receipt");
    assert!(!bridge.route_less_replacement(replacement));
    assert!(matches!(
        bridge.record_retirement_quiescence(&mut presentations, replacement),
        Err(NativeRuntimeError::IngressUnavailable(_))
    ));
}

#[test]
fn adopted_provisional_retirement_requires_matching_core_authorities() {
    let PendingRestoredFixture {
        mut dockspace,
        mut bridge,
        mut presentations,
        child_exact,
        child_binding,
        ..
    } = committed_pending_restored_registration();
    bridge
        .pending_registrations
        .get_mut(&child_exact)
        .expect("the replacement retains its pending registration")
        .phase = PendingNativeRegistrationPhase::AdoptedReplacement {
        predecessor: child_binding,
        replacement: child_binding,
    };
    bridge
        .effects
        .install_provisional_native_for_test(child_exact, child_binding, false);

    let retirement = bridge
        .retire_native_binding(
            &mut dockspace,
            &mut presentations,
            &mut BTreeMap::new(),
            &mut BTreeMap::new(),
            child_exact,
            CloseEffectAcknowledgement::known(None),
        )
        .expect("the provisional lifetime has exact retirement authority")
        .expect("the provisional core binding produces one retirement");

    assert_eq!(retirement.surface, CHILD_SURFACE);
    assert_eq!(retirement.core, child_binding);
    assert_eq!(retirement.adapter_retirement(), None);
    assert_eq!(
        bridge.retired_routes.get(&child_exact),
        Some(&child_binding)
    );
    assert!(!bridge.pending_registrations.contains_key(&child_exact));
    assert!(!bridge.effects.has_provisional_native(child_exact));
    assert!(
        bridge
            .effects
            .has_initialization_receipt_tombstone(child_exact)
    );
}

#[test]
fn fresh_provisional_retirement_uses_effect_core_authority() {
    let PendingRestoredFixture {
        mut dockspace,
        mut bridge,
        mut presentations,
        child_exact,
        child_binding,
        ..
    } = committed_pending_restored_registration();
    bridge.pending_registrations.remove(&child_exact);
    bridge
        .effects
        .install_provisional_native_for_test(child_exact, child_binding, false);

    let retirement = bridge
        .retire_native_binding(
            &mut dockspace,
            &mut presentations,
            &mut BTreeMap::new(),
            &mut BTreeMap::new(),
            child_exact,
            CloseEffectAcknowledgement::known(None),
        )
        .expect("the fresh provisional lifetime has exact retirement authority")
        .expect("the fresh create core binding produces one retirement");

    assert_eq!(retirement.surface, CHILD_SURFACE);
    assert_eq!(retirement.core, child_binding);
    assert_eq!(retirement.adapter_retirement(), None);
    assert!(!bridge.effects.has_provisional_native(child_exact));
    assert!(
        bridge
            .effects
            .has_initialization_receipt_tombstone(child_exact)
    );
}

#[test]
fn failed_native_initialization_is_quarantined_and_rollback_safe() {
    let PendingRestoredFixture {
        mut bridge,
        child_exact,
        child_binding,
        ..
    } = committed_pending_restored_registration();
    bridge
        .pending_registrations
        .get_mut(&child_exact)
        .expect("the replacement retains its pending registration")
        .phase = PendingNativeRegistrationPhase::AdoptedReplacement {
        predecessor: child_binding,
        replacement: child_binding,
    };
    bridge
        .effects
        .install_provisional_native_for_test(child_exact, child_binding, true);
    let snapshot = bridge.state_snapshot();

    assert!(
        bridge
            .quarantine_failed_native_lifetime(child_exact, &mut BTreeMap::new())
            .expect("the failed initialization enters quarantine")
    );
    assert!(bridge.quarantined_native_lifetimes.contains(&child_exact));
    assert!(!bridge.pending_registrations.contains_key(&child_exact));
    assert!(!bridge.effects.has_provisional_native(child_exact));
    assert!(
        bridge
            .effects
            .has_initialization_receipt_tombstone(child_exact)
    );
    assert!(bridge.route_less_replacement(child_exact));

    bridge.restore_state(snapshot);
    assert!(!bridge.quarantined_native_lifetimes.contains(&child_exact));
    assert!(bridge.pending_registrations.contains_key(&child_exact));
    assert!(bridge.effects.has_provisional_native(child_exact));
    assert!(
        !bridge
            .effects
            .has_initialization_receipt_tombstone(child_exact)
    );

    bridge
        .quarantine_failed_native_lifetime(child_exact, &mut BTreeMap::new())
        .expect("the replayed failure enters the same quarantine");
    assert!(
        bridge
            .retire_quarantined_native_lifetime(child_exact)
            .expect("the quarantined lifetime has a route-less retirement lane")
    );
    assert!(!bridge.quarantined_native_lifetimes.contains(&child_exact));
    assert!(
        bridge
            .deferred_replacement_retirements
            .contains(&child_exact)
    );
    assert!(
        bridge
            .effects
            .has_initialization_receipt_tombstone(child_exact)
    );
    let mut presentations =
        NativePresentationLedger::new().expect("presentation identity is available");
    bridge
        .record_retirement_quiescence(&mut presentations, child_exact)
        .expect("quiescence retires the initialization receipt tombstone");
    assert!(
        !bridge
            .effects
            .has_initialization_receipt_tombstone(child_exact)
    );
}

#[test]
fn materialized_restored_bootstrap_can_retire_before_its_first_snapshot() {
    let viewport = ViewportId::from_hash_of("materialized-restored-retirement");
    let exact = ExactNativeViewport::new(viewport, NativeViewportIncarnation::new(1));
    let roster = restored_child_roster(viewport);
    let mut bridge = NativeIngressBridge::new().expect("runtime identity is available");
    bridge
        .effects
        .install_materialized_restored_viewport_for_test(viewport, CHILD_SURFACE);
    let snapshot = bridge.state_snapshot();

    assert!(bridge.route_less_replacement(exact));
    assert!(
        bridge
            .retire_materialized_restored_bootstrap(&roster, exact)
            .expect("the materialized restored lifetime has a route-less retirement lane")
    );
    assert!(bridge.retiring_restored_bootstraps.contains(&exact));
    assert!(!bridge.effects.restored_create_is_materialized(viewport));

    bridge.restore_state(snapshot);
    assert!(!bridge.retiring_restored_bootstraps.contains(&exact));
    assert!(bridge.effects.restored_create_is_materialized(viewport));

    bridge
        .retire_materialized_restored_bootstrap(&roster, exact)
        .expect("the replayed retirement remains exact");
    let mut presentations =
        NativePresentationLedger::new().expect("presentation identity is available");
    bridge
        .record_retirement_quiescence(&mut presentations, exact)
        .expect("exact quiescence releases the route-less bootstrap retirement");
    assert!(!bridge.route_less_replacement(exact));
    assert!(
        !bridge.effects.restored_create_is_materialized(viewport),
        "the viewport is eligible for restored create rescheduling",
    );
}

#[test]
fn route_less_external_retirement_consumes_quiescence_without_core_authority() {
    let mut bridge = NativeIngressBridge::new().expect("runtime identity is available");
    let mut presentations =
        NativePresentationLedger::new().expect("presentation identity is available");
    let exact = ExactNativeViewport::new(
        ViewportId::from_hash_of("external-retirement"),
        NativeViewportIncarnation::new(1),
    );

    bridge
        .record_external_retirement(exact)
        .expect("an unknown physical viewport has an explicit external lane");
    assert!(bridge.external_retirements.contains(&exact));
    bridge
        .record_retirement_quiescence(&mut presentations, exact)
        .expect("fork quiescence terminates the external lane without a core receipt");
    assert!(bridge.external_retirements.is_empty());
    assert!(matches!(
        bridge.record_retirement_quiescence(&mut presentations, exact),
        Err(NativeRuntimeError::IngressUnavailable(_))
    ));
}

#[test]
fn aborted_outer_transaction_restores_registration_and_recorder_state() {
    let surface = SurfaceId::new(1);
    let mut dockspace = test_dockspace();
    let mut bridge = NativeIngressBridge::new().expect("runtime identity is available");
    bridge.recorder = Some(
        dockspace
            .create_backend_ingress_provider(PointerEdgeSequence::new(0))
            .expect("the test backend provider is available"),
    );
    let mut presentations =
        NativePresentationLedger::new().expect("presentation identity is available");
    let transaction = NativeIngressTransaction {
        recorder: bridge
            .recorder
            .as_ref()
            .expect("the recorder is enrolled")
            .savepoint(),
        state: bridge.state_snapshot(),
        presentation: presentations.prepare_savepoint(),
    };

    let ordinal = dockspace
        .record_backend_viewport_registration(
            bridge.recorder.as_mut().expect("the recorder is enrolled"),
            surface,
            WindowToken::new(7),
            ViewportRole::Root,
            None,
        )
        .expect("the registration is staged");
    let exact = ExactNativeViewport::new(ViewportId::ROOT, NativeViewportIncarnation::new(1));
    bridge
        .insert_pending_registration(PendingNativeRegistration::staged(exact, surface, ordinal))
        .expect("the registration sidecar is staged");
    bridge.observation_generation = 19;

    bridge.rollback_transaction(&mut dockspace, &mut presentations, transaction);

    assert!(bridge.pending_registrations.is_empty());
    assert_eq!(bridge.observation_generation, 0);
    assert!(
        bridge
            .recorder
            .as_ref()
            .expect("the recorder remains enrolled")
            .pending_batch()
            .expect("the rolled-back recorder remains valid")
            .records()
            .is_empty()
    );
}

#[test]
fn outside_all_route_requires_the_exact_committed_roster_generation_and_token() {
    let selected = WorkAreaToken::new(7);
    let accepted = BTreeSet::from([selected]);

    assert!(native_work_area_route_is_current(4, selected, 4, &accepted));
    assert!(!native_work_area_route_is_current(
        3, selected, 4, &accepted
    ));
    assert!(!native_work_area_route_is_current(
        4,
        WorkAreaToken::new(8),
        4,
        &accepted,
    ));
}

#[test]
fn native_pointer_cancel_reasons_preserve_terminal_cause() {
    assert_eq!(
        translate_pointer_cancel_reason(NativePointerStreamCancelReason::PlatformCancelled),
        PointerStreamCancelReason::ExplicitPlatformCancellation,
    );
    assert_eq!(
        translate_pointer_cancel_reason(NativePointerStreamCancelReason::DeviceRemoved),
        PointerStreamCancelReason::DeviceRemoved,
    );
    assert_eq!(
        translate_pointer_cancel_reason(NativePointerStreamCancelReason::BindingRetired),
        PointerStreamCancelReason::BindingRetired,
    );
}
