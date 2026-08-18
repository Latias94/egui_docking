//! Host-frame, provider, identity, and presentation authority tests.

use super::*;
use crate::model::DockspaceActionOutcome;
use crate::pointer_journal::{
    FiniteScrollVector, ScrollModifiers, ScrollMomentum, SurfaceLocalPointerRetirementDisposition,
};
use crate::viewport::{CoordinateGeneration, PresentationObservationGeneration};

fn register_test_root_viewport(
    engine: &mut DockEngine,
    host: PresentationHostLease,
    surface: SurfaceId,
    token: WindowToken,
) -> ViewportBinding {
    let provider = test_platform_provider(engine);
    let expected = engine.version();
    let transition = submit_test_input(
        engine,
        host,
        EngineInput::RegisterViewport {
            provider,
            expected,
            surface,
            token,
            role: ViewportRole::Root,
            recovery_target: None,
        },
    )
    .expect("test root viewport must register");
    match transition.reduced_inputs()[0].outcome() {
        InputOutcome::ViewportRegistered { binding } => *binding,
        outcome => panic!("unexpected viewport registration outcome: {outcome:?}"),
    }
}

fn platform_snapshot_for_binding(binding: ViewportBinding) -> PlatformSnapshot {
    let mut capabilities = PlatformCapabilities::default();
    capabilities.set_native_window_lifecycle(PlatformCapability::Supported);
    capabilities.set_authoritative_inventory(PlatformCapability::Supported);
    test_platform_snapshot(
        capabilities,
        FocusObservationEnvelope::new(
            FocusObservationGeneration::new(1),
            Authority::Unknown(AuthorityUnavailableReason::NotReported),
            Authority::Unknown(AuthorityUnavailableReason::NotReported),
        ),
        vec![
            ObservedWindow::new(binding)
                .with_coordinate_observation(WindowCoordinateObservation::new(
                    binding,
                    CoordinateObservationGeneration::new(1),
                    Authority::Known(
                        PhysicalRect::new(0.0, 0.0, 640.0, 480.0)
                            .expect("test content bounds must be valid"),
                    ),
                    Authority::Known(
                        PhysicalRect::new(-8.0, -30.0, 656.0, 518.0)
                            .expect("test outer bounds must be valid"),
                    ),
                    Authority::Known(ScaleFactor::new(1.0).expect("test scale must be valid")),
                    Authority::Known(ScaleFactor::new(1.0).expect("test scale must be valid")),
                ))
                .with_presentation_observation(WindowPresentationObservation::new(
                    binding,
                    PresentationObservationGeneration::new(1),
                    Authority::Known(WindowPresentationState::Visible),
                    PresentationEffectAcknowledgement::known(None),
                )),
        ],
        Vec::new(),
        unknown_work_area_observation(1),
    )
    .expect("test platform snapshot must be canonical")
}

fn establish_native_surface_stream(
    engine: &mut DockEngine,
    host: PresentationHostLease,
    binding: ViewportBinding,
) {
    let provider = test_platform_provider(engine);
    let expected_epoch = engine.version().epoch();
    submit_test_input(
        engine,
        host,
        EngineInput::PublishPlatformSnapshot {
            provider,
            expected_epoch,
            snapshot: platform_snapshot_for_binding(binding),
        },
    )
    .expect("native endpoint coordinates must publish");
    publish_surface_projection(engine, host, binding.surface(), test_rect());
}

#[test]
fn native_surface_local_lease_cannot_freeze_a_reincarnated_binding_frame() {
    let mut engine = single_surface_engine(SOURCE_SURFACE, SOURCE_ROOT, ItemId::new(1));
    let host = engine
        .create_presentation_host()
        .expect("test presentation host must mint");
    let binding_a = engine
        .viewport
        .register_existing(
            engine.version().epoch(),
            SOURCE_SURFACE,
            WindowToken::new(709),
            ViewportRole::Root,
            None,
        )
        .expect("first native binding must register");
    establish_native_surface_stream(&mut engine, host, binding_a);
    let provider_a = engine
        .create_surface_local_pointer_provider(
            SurfaceLocalPointerScope::new(host, SurfaceLocalPointerEndpoint::Native(binding_a)),
            PointerEdgeSequence::new(0),
        )
        .expect("A1 pointer provider must be admitted");

    let rebound_epoch = engine
        .version()
        .epoch()
        .checked_next()
        .expect("test workspace epoch must advance");
    engine
        .viewport
        .reconcile_workspace_epoch(
            rebound_epoch,
            &std::collections::BTreeSet::from([SOURCE_SURFACE]),
        )
        .expect("workspace reconciliation must rebind the native viewport");
    let binding_b = engine
        .viewport
        .viewport(SOURCE_SURFACE)
        .expect("rebound viewport must remain registered")
        .binding();
    assert_ne!(binding_a, binding_b);

    let mut prelude = engine
        .begin_host_frame(host)
        .expect("observation-only prelude must not inspect pointer authority");
    prelude
        .submit_presentation_observation(HostPresentationObservation::NoUpdate)
        .expect("prelude observation must submit");
    assert!(matches!(
        prelude.seal(&engine),
        Err(EngineError::PointerProviderScope { .. })
    ));
    assert_eq!(
        engine.pointer_provider(),
        Some(provider_a.lease()),
        "sealed frame admission must fail closed without mutating the ledger"
    );
}

#[test]
fn native_surface_local_enrollment_rejects_a_stale_coordinate_generation() {
    let mut engine = single_surface_engine(SOURCE_SURFACE, SOURCE_ROOT, ItemId::new(1));
    let host = engine
        .create_presentation_host()
        .expect("test presentation host must mint");
    let binding =
        register_test_root_viewport(&mut engine, host, SOURCE_SURFACE, WindowToken::new(710));
    establish_native_surface_stream(&mut engine, host, binding);
    let current = engine
        .viewport()
        .viewport(SOURCE_SURFACE)
        .expect("the native viewport remains current")
        .coordinate_generation();
    let stale = CoordinateGeneration::new(current.get().saturating_sub(1));
    assert_ne!(stale, current);
    assert!(matches!(
        engine.create_surface_local_pointer_provider(
            SurfaceLocalPointerScope::new_native(host, binding, stale),
            PointerEdgeSequence::new(0),
        ),
        Err(EngineError::PointerProviderScope { .. })
    ));

    let provider = engine
        .create_surface_local_pointer_provider(
            SurfaceLocalPointerScope::new(host, SurfaceLocalPointerEndpoint::Native(binding)),
            PointerEdgeSequence::new(0),
        )
        .expect("the convenience constructor freezes the current generation");
    assert_eq!(
        engine.pointer_provider(),
        Some(provider.lease()),
        "rejected stale enrollment leaves the provider lane available"
    );
}

#[test]
fn surface_local_pointer_host_must_be_the_rendering_host_at_seal() {
    let mut engine = single_surface_engine(SOURCE_SURFACE, SOURCE_ROOT, ItemId::new(1));
    let rendering_host = engine
        .create_presentation_host()
        .expect("rendering presentation host must mint");
    let pointer_host = engine
        .create_presentation_host()
        .expect("pointer presentation host must mint");
    publish_surface_projection(&mut engine, pointer_host, SOURCE_SURFACE, test_rect());
    let provider = engine
        .create_surface_local_pointer_provider(
            SurfaceLocalPointerScope::new(
                pointer_host,
                SurfaceLocalPointerEndpoint::Logical(SOURCE_SURFACE),
            ),
            PointerEdgeSequence::new(0),
        )
        .expect("surface-local pointer provider must be admitted");

    let mut prelude = engine
        .begin_host_frame(rendering_host)
        .expect("prelude freezes presentation observation scope only");
    prelude
        .submit_presentation_observation(HostPresentationObservation::NoUpdate)
        .expect("rendering observation must submit");

    assert!(matches!(
        prelude.seal(&engine),
        Err(EngineError::PointerProviderHostOutsideFrameScope {
            provider: actual_provider,
            host: actual_host,
        }) if actual_provider == provider.lease() && actual_host == pointer_host
    ));
    assert_eq!(engine.pointer_provider(), Some(provider.lease()));
}

#[test]
fn surface_local_pointer_host_cannot_borrow_another_hosts_active_surface() {
    let mut engine = single_surface_engine(SOURCE_SURFACE, SOURCE_ROOT, ItemId::new(1));
    let rendering_host = engine
        .create_presentation_host()
        .expect("rendering presentation host must mint");
    let pointer_host = engine
        .create_presentation_host()
        .expect("foreign pointer presentation host must mint");
    publish_surface_projection(&mut engine, rendering_host, SOURCE_SURFACE, test_rect());

    assert!(matches!(
        engine.create_surface_local_pointer_provider(
            SurfaceLocalPointerScope::new(
                pointer_host,
                SurfaceLocalPointerEndpoint::Logical(SOURCE_SURFACE),
            ),
            PointerEdgeSequence::new(0),
        ),
        Err(EngineError::PointerProviderSurfaceHostMismatch {
            host,
            surface,
            owner,
        }) if host == pointer_host && surface == SOURCE_SURFACE && owner == rendering_host
    ));
    assert_eq!(engine.pointer_provider(), None);
}

#[test]
fn surface_local_pointer_lifecycle_rejects_every_raw_lease_bypass() {
    let mut engine = single_surface_engine(SOURCE_SURFACE, SOURCE_ROOT, ItemId::new(1));
    let host = engine
        .create_presentation_host()
        .expect("test presentation host must mint");
    publish_surface_projection(&mut engine, host, SOURCE_SURFACE, test_rect());
    let scope =
        SurfaceLocalPointerScope::new(host, SurfaceLocalPointerEndpoint::Logical(SOURCE_SURFACE));

    assert!(matches!(
        engine.create_pointer_provider(
            PointerProviderScope::SurfaceLocal(scope),
            PointerEdgeSequence::new(0),
        ),
        Err(EngineError::SurfaceLocalPointerProducerRequired)
    ));

    let provider = engine
        .create_surface_local_pointer_provider(scope, PointerEdgeSequence::new(0))
        .expect("affine surface-local producer must mint");
    let lease = provider.lease();
    let empty = PointerEdgeJournal::new(
        PointerEdgeSequence::new(0),
        PointerEdgeSequence::new(0),
        Vec::new(),
    )
    .expect("empty journal must preserve its watermark");
    let mut frame = begin_test_host_frame(&engine, host);
    assert!(matches!(
        frame.submit_pointer_journal(lease, empty),
        Err(CoreHostFrameError::SurfaceLocalPointerProducerRequired {
            provider: rejected,
        }) if rejected == lease
    ));
    drop(frame);

    let mut receipt = provider
        .drain()
        .expect("raw-lease rejection leaves no affine frame attempt in flight");
    let empty = PointerEdgeJournal::new(
        PointerEdgeSequence::new(0),
        PointerEdgeSequence::new(0),
        Vec::new(),
    )
    .expect("drained raw retry journal preserves its watermark");
    let mut drained_frame = begin_test_host_frame(&engine, host);
    assert!(matches!(
        drained_frame.submit_pointer_journal(lease, empty),
        Err(CoreHostFrameError::SurfaceLocalPointerProducerRequired {
            provider: rejected,
        }) if rejected == lease
    ));
    drop(drained_frame);
    assert!(matches!(
        engine.retire_pointer_provider(lease),
        Err(EngineError::SurfaceLocalPointerProducerRequired)
    ));
    assert_eq!(engine.pointer_provider(), Some(lease));
    let retirement = engine
        .retire_quiesced_surface_local_pointer_provider(&mut receipt)
        .expect("the exact drained producer remains retireable");
    assert!(retirement.repaint_required());
    assert!(!retirement.interaction_changed());
    assert_eq!(engine.pointer_provider(), None);
}

#[test]
fn surface_local_pointer_drain_waits_for_the_staged_host_frame_to_finish() {
    let mut engine = single_surface_engine(SOURCE_SURFACE, SOURCE_ROOT, ItemId::new(1));
    let host = engine
        .create_presentation_host()
        .expect("test presentation host must mint");
    publish_surface_projection(&mut engine, host, SOURCE_SURFACE, test_rect());
    let provider = engine
        .create_surface_local_pointer_provider(
            SurfaceLocalPointerScope::new(
                host,
                SurfaceLocalPointerEndpoint::Logical(SOURCE_SURFACE),
            ),
            PointerEdgeSequence::new(0),
        )
        .expect("surface-local pointer provider must mint");
    let lease = provider.lease();
    let empty = PointerEdgeJournal::new(
        PointerEdgeSequence::new(0),
        PointerEdgeSequence::new(0),
        Vec::new(),
    )
    .expect("empty journal must preserve its watermark");
    let mut frame = begin_test_host_frame(&engine, host);
    frame
        .submit_surface_pointer_journal(&provider, empty)
        .expect("surface-local journal must stage through the affine producer");

    let provider = provider
        .drain()
        .expect_err("an uncommitted host frame must retain the producer lane")
        .into_provider();
    assert_eq!(provider.lease(), lease);
    assert_eq!(
        provider.committed_through(),
        PointerEdgeSequence::new(0),
        "staging alone must not advance the committed watermark"
    );

    drop(frame);
    let mut receipt = provider
        .drain()
        .expect("dropping the staged frame releases the producer lane");
    assert_eq!(receipt.committed_through(), PointerEdgeSequence::new(0));
    let retirement = engine
        .retire_quiesced_surface_local_pointer_provider(&mut receipt)
        .expect("the recovered provider must remain exactly retireable");
    assert!(retirement.repaint_required());
    assert!(!retirement.interaction_changed());
    assert_eq!(engine.pointer_provider(), None);
}

#[test]
fn surface_local_frame_guard_defers_abandonment_reaping_until_the_frame_drops() {
    let mut engine = single_surface_engine(SOURCE_SURFACE, SOURCE_ROOT, ItemId::new(1));
    let host = engine
        .create_presentation_host()
        .expect("test presentation host must mint");
    publish_surface_projection(&mut engine, host, SOURCE_SURFACE, test_rect());
    let provider = engine
        .create_surface_local_pointer_provider(
            SurfaceLocalPointerScope::new(
                host,
                SurfaceLocalPointerEndpoint::Logical(SOURCE_SURFACE),
            ),
            PointerEdgeSequence::new(0),
        )
        .expect("surface-local pointer provider must mint");
    let lease = provider.lease();
    let mut frame = begin_test_host_frame(&engine, host);
    frame
        .submit_surface_pointer_journal(
            &provider,
            PointerEdgeJournal::new(
                PointerEdgeSequence::new(0),
                PointerEdgeSequence::new(0),
                Vec::new(),
            )
            .expect("empty journal preserves its watermark"),
        )
        .expect("the frame retains the producer lane");

    drop(provider);
    assert_eq!(
        engine
            .reap_abandoned_surface_local_pointer_provider()
            .expect("a guarded producer query is valid"),
        None,
        "the in-flight frame guard is still a live producer-side capability",
    );
    assert_eq!(engine.pointer_provider(), Some(lease));

    drop(frame);
    assert!(matches!(
        engine.begin_host_frame(host),
        Err(EngineError::SurfaceLocalPointerProviderAbandoned { provider })
            if provider == lease
    ));
    let outcome = engine
        .reap_abandoned_surface_local_pointer_provider()
        .expect("dropping the final producer-side capability permits reclamation")
        .expect("the abandoned provider is reclaimed");
    assert_eq!(
        outcome.disposition(),
        SurfaceLocalPointerRetirementDisposition::RetiredActive
    );
    assert!(outcome.repaint_required());
    assert!(!outcome.interaction_changed());
    assert_eq!(engine.pointer_provider(), None);
}

#[test]
fn dropped_surface_local_drain_receipt_can_be_reaped_without_sticking_the_lane() {
    let mut engine = single_surface_engine(SOURCE_SURFACE, SOURCE_ROOT, ItemId::new(1));
    let host = engine
        .create_presentation_host()
        .expect("test presentation host must mint");
    publish_surface_projection(&mut engine, host, SOURCE_SURFACE, test_rect());
    let provider = engine
        .create_surface_local_pointer_provider(
            SurfaceLocalPointerScope::new(
                host,
                SurfaceLocalPointerEndpoint::Logical(SOURCE_SURFACE),
            ),
            PointerEdgeSequence::new(0),
        )
        .expect("surface-local pointer provider must mint");
    let receipt = provider
        .drain()
        .expect("an idle producer drains immediately");

    assert_eq!(
        engine
            .reap_abandoned_surface_local_pointer_provider()
            .expect("a retained receipt query is valid"),
        None,
        "the affine drain receipt remains a live producer-side capability",
    );
    drop(receipt);

    assert!(matches!(
        engine.create_surface_local_pointer_provider(
            SurfaceLocalPointerScope::new(
                host,
                SurfaceLocalPointerEndpoint::Logical(SOURCE_SURFACE),
            ),
            PointerEdgeSequence::new(0),
        ),
        Err(EngineError::SurfaceLocalPointerProviderAbandoned { .. })
    ));

    let outcome = engine
        .reap_abandoned_surface_local_pointer_provider()
        .expect("a lost receipt is recoverable")
        .expect("the abandoned active lane is reclaimed");
    assert_eq!(
        outcome.disposition(),
        SurfaceLocalPointerRetirementDisposition::RetiredActive
    );
    assert!(outcome.repaint_required());
    assert_eq!(engine.pointer_provider(), None);
    assert_eq!(
        engine
            .reap_abandoned_surface_local_pointer_provider()
            .expect("repeated reclamation is inert"),
        None,
    );
}

#[test]
fn dropped_producer_of_an_implicitly_retired_lease_compacts_without_a_new_tick() {
    let mut engine = single_surface_engine(SOURCE_SURFACE, SOURCE_ROOT, ItemId::new(1));
    let host = engine
        .create_presentation_host()
        .expect("test presentation host must mint");
    publish_surface_projection(&mut engine, host, SOURCE_SURFACE, test_rect());
    let provider = engine
        .create_surface_local_pointer_provider(
            SurfaceLocalPointerScope::new(
                host,
                SurfaceLocalPointerEndpoint::Logical(SOURCE_SURFACE),
            ),
            PointerEdgeSequence::new(0),
        )
        .expect("surface-local pointer provider must mint");

    let retirement = engine
        .retire_presentation_host(host, PresentationHostRetirementReason::RuntimeDestroyed)
        .expect("presentation host retirement is atomic");
    assert!(matches!(
        retirement,
        PresentationHostRetirementOutcome::Retired { .. }
    ));
    assert_eq!(engine.pointer_provider(), None);
    assert_eq!(
        engine
            .runtime_retention_manifest()
            .pointer()
            .retired_lease_guards(),
        1,
    );
    let retirement_tick = engine.last_reducer_tick();

    drop(provider);
    let outcome = engine
        .reap_abandoned_surface_local_pointer_provider()
        .expect("an abandoned retired producer can be compacted")
        .expect("the retired monitor remains discoverable");
    assert_eq!(
        outcome.disposition(),
        SurfaceLocalPointerRetirementDisposition::CompactedPreviouslyRetired
    );
    assert!(!outcome.repaint_required());
    assert!(!outcome.interaction_changed());
    assert_eq!(engine.last_reducer_tick(), retirement_tick);
    assert_eq!(
        engine
            .runtime_retention_manifest()
            .pointer()
            .retired_lease_guards(),
        0,
    );
}

#[test]
fn native_surface_local_lease_cannot_cross_a_workspace_binding_reincarnation() {
    let mut engine = single_surface_engine(SOURCE_SURFACE, SOURCE_ROOT, ItemId::new(1));
    let host = engine
        .create_presentation_host()
        .expect("test presentation host must mint");
    let binding_a = engine
        .viewport
        .register_existing(
            engine.version().epoch(),
            SOURCE_SURFACE,
            WindowToken::new(710),
            ViewportRole::Root,
            None,
        )
        .expect("first native binding must register");
    establish_native_surface_stream(&mut engine, host, binding_a);
    let provider_a = engine
        .create_surface_local_pointer_provider(
            SurfaceLocalPointerScope::new(host, SurfaceLocalPointerEndpoint::Native(binding_a)),
            PointerEdgeSequence::new(0),
        )
        .expect("A1 pointer provider must be admitted");

    let empty = PointerEdgeJournal::new(
        PointerEdgeSequence::new(0),
        PointerEdgeSequence::new(0),
        Vec::new(),
    )
    .expect("empty journal must preserve its watermark");
    let mut stale_frame = begin_test_host_frame(&engine, host);
    stale_frame
        .submit_surface_pointer_journal(&provider_a, empty.clone())
        .expect("the A1 provider can stage a structurally valid journal");
    stale_frame
        .submit_pointer_receiver_receipts(
            PointerReceiverReceiptBatch::new(Vec::<PointerReceiverReceipt>::new())
                .expect("empty journal has an empty exact receipt set"),
        )
        .expect("empty receipt set must stage");
    complete_host_frame_with_explicit_surface_roster(&engine, &mut stale_frame);

    let rebound_epoch = engine
        .version()
        .epoch()
        .checked_next()
        .expect("test workspace epoch must advance");
    let roster = std::collections::BTreeSet::from([SOURCE_SURFACE]);
    engine
        .viewport
        .reconcile_workspace_epoch(rebound_epoch, &roster)
        .expect("workspace reconciliation must rebind the native viewport");
    let binding_b = engine
        .viewport
        .viewport(SOURCE_SURFACE)
        .expect("rebound viewport must remain registered")
        .binding();
    assert_ne!(binding_a, binding_b);

    let before = engine.candidate();
    assert!(matches!(
        stale_frame.finish(&mut engine),
        Err(EngineError::PointerProviderScope { .. })
    ));
    assert_eq!(
        engine, before,
        "a stale A1 journal must not mutate A2 state"
    );

    let mut receipt_a = provider_a
        .drain()
        .expect("rejected stale frame releases the producer lane");
    let retirement = engine
        .retire_quiesced_surface_local_pointer_provider(&mut receipt_a)
        .expect("the stale A1 provider can be retired exactly once");
    assert!(retirement.repaint_required());
    assert!(!retirement.interaction_changed());
    assert_eq!(engine.pointer_provider(), None);
    assert!(matches!(
        engine.create_surface_local_pointer_provider(
            SurfaceLocalPointerScope::new(host, SurfaceLocalPointerEndpoint::Native(binding_b)),
            PointerEdgeSequence::new(0),
        ),
        Err(EngineError::PointerProviderSurfaceEndpointMismatch {
            submitted: SurfaceLocalPointerEndpoint::Native(submitted),
            active: HostPresentationEndpoint::Native(active),
            ..
        }) if submitted == binding_b && active == binding_a
    ));
}

#[test]
fn host_frame_records_one_whole_engine_candidate_clone() {
    let mut fixture = counter_fixture();
    let expected_workspace_volume =
        crate::drop_resolver::structural_work::WorkspaceCloneVolume::capture(
            fixture.engine.workspace(),
        );
    crate::drop_resolver::structural_work::reset();

    let mut frame = begin_test_host_frame(&fixture.engine, fixture.presentation_host);
    complete_host_frame_with_explicit_surface_roster(&fixture.engine, &mut frame);
    frame
        .finish(&mut fixture.engine)
        .expect("empty authoritative host frame must reduce");

    let work = crate::drop_resolver::structural_work::snapshot();
    assert_eq!(work.engine_deep_clones.atomic_candidates.calls, 1);
    assert_eq!(
        work.engine_deep_clones.atomic_candidates.volume,
        expected_workspace_volume
    );
}

#[test]
fn surface_contribution_authority_diff_is_linear_in_surface_count() {
    for surface_count in [16_usize, 128, 1_024] {
        let mut builder = Workspace::builder();
        for index in 0..surface_count {
            let identity = u64::try_from(index + 1).expect("surface fixture identity fits u64");
            let item = ItemId::new(identity);
            let root = RootId::new(identity);
            let surface = SurfaceId::new(identity);
            let tabs = builder.insert_node(Node::tabs([item]));
            builder.set_root(root, RootRecord::new(tabs).with_central(tabs));
            builder.set_surface(surface, SurfacePresentation::with_main(root));
        }
        let mut engine = DockEngine::new(
            builder.build().expect("scale fixture workspace validates"),
            DockPolicy::default(),
        )
        .expect("scale fixture engine initializes");
        let host = engine
            .create_presentation_host()
            .expect("scale fixture presentation host mints");
        let mut frame = begin_test_host_frame(&engine, host);
        complete_host_frame_with_explicit_surface_roster(&engine, &mut frame);

        crate::scene::interaction_authority_work::reset();
        frame
            .finish(&mut engine)
            .expect("complete scale fixture frame reduces");
        let work = crate::scene::interaction_authority_work::snapshot();

        assert_eq!(work.scans, 2, "surface count {surface_count}");
        assert_eq!(
            work.surface_visits,
            surface_count * 2,
            "surface count {surface_count}"
        );
    }
}

#[test]
fn surface_contribution_validation_reuses_the_manifest_workspace_index() {
    for surface_count in [16_usize, 128, 1_024] {
        let mut builder = Workspace::builder();
        for index in 0..surface_count {
            let identity = u64::try_from(index + 1).expect("surface fixture identity fits u64");
            let item = ItemId::new(identity);
            let root = RootId::new(identity);
            let surface = SurfaceId::new(identity);
            let tabs = builder.insert_node(Node::tabs([item]));
            builder.set_root(root, RootRecord::new(tabs).with_central(tabs));
            builder.set_surface(surface, SurfacePresentation::with_main(root));
        }
        let mut engine = DockEngine::new(
            builder.build().expect("scale fixture workspace validates"),
            DockPolicy::default(),
        )
        .expect("scale fixture engine initializes");
        let host = engine
            .create_presentation_host()
            .expect("scale fixture presentation host mints");
        let mut frame = begin_test_host_frame(&engine, host);

        crate::drop_resolver::structural_work::reset();
        for identity in 1..=surface_count {
            let surface =
                SurfaceId::new(u64::try_from(identity).expect("surface fixture identity fits u64"));
            let token = frame
                .view()
                .begin_surface_contribution(surface)
                .expect("rostered surface has one contribution token");
            let measurements = tab_strip_reducer_measurements_for(&engine, surface, false);
            let contribution = frame
                .view()
                .prepare_surface_contribution(token, measurements)
                .expect("current surface measurements prepare");
            frame
                .push_surface_contribution(contribution)
                .expect("each surface contributes once");
        }

        let work = crate::drop_resolver::structural_work::snapshot();
        assert!(
            work.root_fingerprint_builds <= surface_count,
            "surface validation rebuilt more than one root fingerprint per surface: {work:?}"
        );
        assert!(
            work.root_fingerprint_node_visits <= surface_count,
            "surface validation revisited more than one root node per surface: {work:?}"
        );
    }
}

#[test]
fn reverse_surface_contributions_use_indexed_canonical_storage() {
    for surface_count in [16_usize, 128, 1_024] {
        let mut builder = Workspace::builder();
        for index in 0..surface_count {
            let identity = u64::try_from(index + 1).expect("surface fixture identity fits u64");
            let item = ItemId::new(identity);
            let root = RootId::new(identity);
            let surface = SurfaceId::new(identity);
            let tabs = builder.insert_node(Node::tabs([item]));
            builder.set_root(root, RootRecord::new(tabs).with_central(tabs));
            builder.set_surface(surface, SurfacePresentation::with_main(root));
        }
        let mut engine = DockEngine::new(
            builder.build().expect("scale fixture workspace validates"),
            DockPolicy::default(),
        )
        .expect("scale fixture engine initializes");
        let host = engine
            .create_presentation_host()
            .expect("scale fixture presentation host mints");
        let mut frame = begin_test_host_frame(&engine, host);

        let _: &std::collections::BTreeMap<SurfaceId, PreparedSurfaceContribution> =
            &frame.surface_contributions;
        for index in (0..surface_count).rev() {
            let identity = u64::try_from(index + 1).expect("surface fixture identity fits u64");
            let surface = SurfaceId::new(identity);
            let token = frame
                .view()
                .begin_surface_contribution(surface)
                .expect("rostered surface has one contribution token");
            let contribution = frame
                .view()
                .prepare_surface_unavailable_contribution(
                    token,
                    MeasurementUnavailableReason::Deferred,
                )
                .expect("deferred surface contribution prepares");
            frame
                .push_surface_contribution(contribution)
                .expect("each surface contributes once");
        }

        assert_eq!(
            frame
                .surface_contributions()
                .map(PreparedSurfaceContribution::surface)
                .collect::<Vec<_>>(),
            (1..=surface_count)
                .map(|identity| SurfaceId::new(
                    u64::try_from(identity).expect("surface fixture identity fits u64")
                ))
                .collect::<Vec<_>>(),
            "contributions remain canonical for {surface_count} surfaces"
        );
    }
}

#[test]
fn surface_local_scroll_batch_refreshes_only_the_changed_surface() {
    for surface_count in [16_usize, 128, 1_024] {
        let mut builder = Workspace::builder();
        for index in 0..surface_count {
            let identity = u64::try_from(index + 1).expect("surface fixture identity fits u64");
            let surface = SurfaceId::new(identity);
            let root = RootId::new(identity);
            let items = if index == 0 {
                vec![
                    ItemId::new(10_001),
                    ItemId::new(10_002),
                    ItemId::new(10_003),
                    ItemId::new(10_004),
                ]
            } else {
                vec![ItemId::new(20_000 + identity)]
            };
            let tabs = builder.insert_node(Node::tabs(items));
            builder.set_root(root, RootRecord::new(tabs).with_central(tabs));
            builder.set_surface(surface, SurfacePresentation::with_main(root));
        }
        let surface = SurfaceId::new(1);
        let mut engine = DockEngine::new(
            builder.build().expect("scroll scale fixture validates"),
            DockPolicy::default(),
        )
        .expect("scroll scale fixture engine initializes");
        let host = engine
            .create_presentation_host()
            .expect("scroll scale fixture presentation host mints");
        let measurements = tab_strip_reducer_measurements_for(&engine, surface, false);
        publish_surface_projection_with_measurements(&mut engine, host, surface, measurements);
        let projection = engine
            .interaction_projection(surface)
            .expect("overflowing tab strip is interactive");
        let receiver = projection
            .hit_manifest()
            .regions()
            .iter()
            .find(|region| {
                matches!(
                    region.id().kind(),
                    PresentationHitRegionKind::TabStripScroll(_)
                )
            })
            .expect("overflowing tab strip publishes one scroll receiver");
        let receiver_id = receiver.id();
        let point = region_center(receiver);
        let delivery = PointerReceiverDelivery::new(
            projection,
            PointerReceiverDeliveryDisposition::Dock(receiver_id),
        )
        .expect("scroll receiver binds the exact presented output");
        let endpoint = ScrollDeliveryEndpoint::new(
            host,
            surface,
            None,
            projection.authority().coordinate_generation(),
        )
        .expect("logical scroll endpoint matches the presentation host");
        let provider = engine
            .create_surface_local_pointer_provider(
                SurfaceLocalPointerScope::new(host, SurfaceLocalPointerEndpoint::Logical(surface)),
                PointerEdgeSequence::new(0),
            )
            .expect("scroll scale fixture pointer provider mints");
        let scroll = ScrollEdge::new(
            ScrollDeviceId::new(1),
            None,
            ScrollPhase::Discrete,
            Some(ScrollDelta::LogicalPoints(
                FiniteScrollVector::new(-10.0, 0.0).expect("scroll delta is finite"),
            )),
            Authority::Known(ScrollMomentum::Direct),
            Authority::Known(ScrollModifiers::default()),
            Authority::Known(endpoint),
        )
        .expect("discrete scroll edge is structurally valid");
        let mut frame = begin_test_host_frame(&engine, host);

        crate::drop_resolver::structural_work::reset();
        for previous in 0..4_u64 {
            frame
                .submit_surface_pointer_journal(
                    &provider,
                    local_pointer_journal(previous, [(PointerEdgeKind::Scrolled(scroll), point)]),
                )
                .expect("scroll segment stages");
            let candidate = frame
                .pointer_receiver_candidates()
                .expect("scroll segment requests receiver evidence")
                .candidates()[0]
                .clone();
            frame
                .submit_pointer_receiver_receipts(
                    PointerReceiverReceiptBatch::new([candidate.receipt(
                        PointerReceiverObservation::Presented(
                            PresentedPointerReceiverObservation::new([
                                PointerReceiverProbeReceipt::Delivery(delivery),
                            ])
                            .expect("scroll segment answers its delivery probe"),
                        ),
                    )])
                    .expect("scroll segment receipt roster is exact"),
                )
                .expect("scroll segment reduces");
        }

        let work = crate::drop_resolver::structural_work::snapshot();
        assert_eq!(
            work.presentation_roster_full_captures, 0,
            "scroll segments must not rebuild the complete roster for {surface_count} surfaces"
        );
        assert_eq!(
            work.presentation_roster_surface_freezes, 4,
            "four applied scroll segments freeze only their changed surface for {surface_count} surfaces"
        );

        complete_host_frame_with_explicit_surface_roster(&engine, &mut frame);
        let transition = frame
            .finish(&mut engine)
            .expect("scroll scale fixture host frame reduces");
        assert_eq!(transition.reduced_pointer_edges().len(), 4);
        assert!(transition.reduced_pointer_edges().iter().all(|edge| {
            matches!(
                edge.interaction_outcomes(),
                [InteractionOutcome::Scroll(ScrollReductionOutcome::Applied(application))]
                    if application.receiver() == receiver_id && application.applied_delta() != 0.0
            )
        }));
    }
}

#[test]
fn real_host_frame_tab_press_and_release_have_bounded_clone_work() {
    let mut builder = Workspace::builder();
    let tabs = builder.insert_node(Node::tabs([ItemId::new(1), ItemId::new(2)]));
    builder.set_root(SOURCE_ROOT, RootRecord::new(tabs).with_central(tabs));
    builder.set_surface(SOURCE_SURFACE, SurfacePresentation::with_main(SOURCE_ROOT));
    let mut engine = DockEngine::new(
        builder.build().expect("tab workspace must validate"),
        DockPolicy::default(),
    )
    .expect("tab engine must initialize");
    let host = engine
        .create_presentation_host()
        .expect("tab presentation host must mint");
    publish_surface_projection(&mut engine, host, SOURCE_SURFACE, test_rect());
    let projection = engine
        .interaction_projection(SOURCE_SURFACE)
        .expect("tab projection must be receiver-authoritative");
    let tab_region = projection
        .hit_manifest()
        .regions()
        .iter()
        .find(|region| {
            matches!(
                region.id().kind(),
                PresentationHitRegionKind::TabBody(tab) if tab.item == ItemId::new(1)
            )
        })
        .expect("selected tab must have one drag receiver")
        .id();
    let point = region_center(
        projection
            .hit_manifest()
            .region(tab_region)
            .expect("selected tab receiver remains present"),
    );
    let provider = engine
        .create_surface_local_pointer_provider(
            SurfaceLocalPointerScope::new(
                host,
                SurfaceLocalPointerEndpoint::Logical(SOURCE_SURFACE),
            ),
            PointerEdgeSequence::new(0),
        )
        .expect("tab pointer provider must mint");
    let workspace_volume =
        crate::drop_resolver::structural_work::WorkspaceCloneVolume::capture(engine.workspace());
    let engine_state = crate::drop_resolver::structural_work::EngineCloneVolume {
        scene_surfaces: 1,
        scene_retained_plans: 1,
        scene_paint_hit_regions: 14,
        presentation_hosts: 1,
        presentation_streams: 1,
        presentation_pending_outputs: 0,
        live_pointer_providers: 1,
        semantic_input_watermark_guards: 0,
    };

    crate::drop_resolver::structural_work::reset();
    let mut press = begin_test_host_frame(&engine, host);
    press
        .submit_surface_pointer_journal(
            &provider,
            local_pointer_journal(
                0,
                [(
                    PointerEdgeKind::ButtonPressed(PointerButton::Primary),
                    point,
                )],
            ),
        )
        .expect("tab press journal must stage");
    let projection = press
        .view()
        .interaction_projection(SOURCE_SURFACE)
        .expect("sealed press retains tab receiver authority");
    let candidate = press
        .pointer_receiver_candidates()
        .expect("tab press freezes a receiver candidate")
        .candidates()[0]
        .clone();
    let delivery = PointerReceiverDelivery::new(
        projection,
        PointerReceiverDeliveryDisposition::Dock(tab_region),
    )
    .expect("tab receiver supports drag delivery");
    press
        .submit_pointer_receiver_receipts(
            PointerReceiverReceiptBatch::new([candidate.receipt(
                PointerReceiverObservation::Presented(
                    PresentedPointerReceiverObservation::new([
                        PointerReceiverProbeReceipt::Delivery(delivery),
                    ])
                    .expect("press receipt answers its delivery probe"),
                ),
            )])
            .expect("press receipt batch is exact"),
        )
        .expect("press receipt must stage");
    complete_host_frame_with_explicit_surface_roster(&engine, &mut press);
    let press_transition = press.finish(&mut engine).expect("tab press must reduce");
    assert_eq!(
        provider.committed_through(),
        PointerEdgeSequence::new(1),
        "host-frame commit advances the affine producer watermark"
    );
    assert!(matches!(
        press_transition.reduced_pointer_edges()[0].interaction_outcomes(),
        [InteractionOutcome::DragArmed { .. }]
    ));
    let press_work = crate::drop_resolver::structural_work::snapshot();
    assert_eq!(press_work.engine_deep_clones.atomic_candidates.calls, 1);
    assert_eq!(
        press_work.engine_deep_clones.atomic_candidates.volume,
        workspace_volume
    );
    assert_eq!(
        press_work.engine_deep_clones.atomic_candidate_state,
        engine_state
    );
    assert_eq!(press_work.workspace_deep_clones.engine_candidates.calls, 1);
    assert_eq!(
        press_work.workspace_deep_clones.engine_candidates.volume,
        workspace_volume
    );
    assert_eq!(
        press_work
            .workspace_deep_clones
            .transaction_candidates
            .calls,
        1
    );
    assert_eq!(
        press_work
            .workspace_deep_clones
            .transaction_candidates
            .volume,
        workspace_volume
    );
    assert_eq!(press_work.transaction_prepares, 1);
    assert_eq!(press_work.transaction_commands, 1);
    assert_eq!(press_work.root_fingerprint_builds, 6);
    assert_eq!(press_work.root_fingerprint_node_visits, 6);

    crate::drop_resolver::structural_work::reset();
    let mut release = begin_test_host_frame(&engine, host);
    release
        .submit_surface_pointer_journal(
            &provider,
            local_pointer_journal(
                1,
                [(
                    PointerEdgeKind::ButtonReleased(PointerButton::Primary),
                    point,
                )],
            ),
        )
        .expect("tab release journal must stage");
    let candidate = release
        .pointer_receiver_candidates()
        .expect("tab release freezes an exact candidate")
        .candidates()[0]
        .clone();
    release
        .submit_pointer_receiver_receipts(
            PointerReceiverReceiptBatch::new([
                candidate.receipt(PointerReceiverObservation::NotApplicable)
            ])
            .expect("release receipt batch is exact"),
        )
        .expect("release receipt must stage");
    complete_host_frame_with_explicit_surface_roster(&engine, &mut release);
    let release_transition = release
        .finish(&mut engine)
        .expect("tab release must reduce");
    assert_eq!(
        provider.committed_through(),
        PointerEdgeSequence::new(2),
        "the next committed host frame advances the producer watermark again"
    );
    assert!(matches!(
        release_transition.reduced_pointer_edges()[0].interaction_outcomes(),
        [InteractionOutcome::Cancelled {
            reason: InteractionCancelReason::ReleasedBeforeDrag,
            ..
        }]
    ));
    assert_eq!(engine.interaction().status(), InteractionStatus::Idle);
    let release_work = crate::drop_resolver::structural_work::snapshot();
    assert_eq!(release_work.engine_deep_clones.atomic_candidates.calls, 1);
    assert_eq!(
        release_work.engine_deep_clones.atomic_candidates.volume,
        workspace_volume
    );
    assert_eq!(
        release_work.engine_deep_clones.atomic_candidate_state,
        engine_state
    );
    assert_eq!(
        release_work.workspace_deep_clones.engine_candidates.calls,
        0
    );
    assert_eq!(
        release_work
            .workspace_deep_clones
            .transaction_candidates
            .calls,
        0
    );
    assert_eq!(release_work.transaction_prepares, 0);
    assert_eq!(release_work.root_fingerprint_builds, 0);
}

#[test]
fn real_host_frame_contained_close_docks_back_without_opening_content_close() {
    let floating = FloatingPresentationId::new(9);
    let mut builder = Workspace::builder();
    let main_tabs = builder.insert_node(Node::tabs([ItemId::new(1)]));
    let contained_tabs = builder.insert_node(Node::tabs([ItemId::new(2)]));
    builder.set_root(
        SOURCE_ROOT,
        RootRecord::new(main_tabs).with_central(main_tabs),
    );
    builder.set_root(TARGET_ROOT, RootRecord::new(contained_tabs));
    builder.set_surface(SOURCE_SURFACE, SurfacePresentation::with_main(SOURCE_ROOT));
    builder.set_contained_floating(
        floating,
        ContainedFloating::new(
            TARGET_ROOT,
            LogicalRect::new(180.0, 120.0, 260.0, 180.0)
                .expect("contained close fixture bounds validate"),
        ),
    );
    builder
        .attach_contained(SOURCE_SURFACE, floating)
        .expect("contained close fixture attaches");
    let mut engine = DockEngine::new(
        builder
            .build()
            .expect("contained close workspace validates"),
        DockPolicy::default(),
    )
    .expect("contained close engine initializes");
    let host = engine
        .create_presentation_host()
        .expect("contained close presentation host mints");
    publish_surface_projection(
        &mut engine,
        host,
        SOURCE_SURFACE,
        LogicalRect::new(0.0, 0.0, 800.0, 600.0).expect("contained close surface bounds validate"),
    );
    let projection = engine
        .interaction_projection(SOURCE_SURFACE)
        .expect("contained close projection is receiver-authoritative");
    let close_region = projection
        .hit_manifest()
        .regions()
        .iter()
        .find(|region| region.id().kind() == PresentationHitRegionKind::ContainedClose(floating))
        .expect("contained chrome exposes one close receiver")
        .id();
    let point = region_center(
        projection
            .hit_manifest()
            .region(close_region)
            .expect("contained close receiver remains present"),
    );
    let provider = engine
        .create_surface_local_pointer_provider(
            SurfaceLocalPointerScope::new(
                host,
                SurfaceLocalPointerEndpoint::Logical(SOURCE_SURFACE),
            ),
            PointerEdgeSequence::new(0),
        )
        .expect("contained close pointer provider mints");

    let mut frame = begin_test_host_frame(&engine, host);
    frame
        .submit_surface_pointer_journal(
            &provider,
            local_pointer_journal(
                0,
                [(
                    PointerEdgeKind::ButtonPressed(PointerButton::Primary),
                    point,
                )],
            ),
        )
        .expect("contained close press stages");
    let press_candidate = frame
        .pointer_receiver_candidates()
        .expect("contained close press requests a receiver")
        .candidates()[0]
        .clone();
    let press_delivery = PointerReceiverDelivery::new(
        frame
            .view()
            .interaction_projection(SOURCE_SURFACE)
            .expect("press retains contained close authority"),
        PointerReceiverDeliveryDisposition::Dock(close_region),
    )
    .expect("contained close supports click delivery");
    frame
        .submit_pointer_receiver_receipts(
            PointerReceiverReceiptBatch::new([press_candidate.receipt(
                PointerReceiverObservation::Presented(
                    PresentedPointerReceiverObservation::new([
                        PointerReceiverProbeReceipt::Delivery(press_delivery),
                    ])
                    .expect("contained close press answers delivery"),
                ),
            )])
            .expect("contained close press receipt batch is exact"),
        )
        .expect("contained close press receipt stages");
    frame
        .submit_surface_pointer_journal(
            &provider,
            local_pointer_journal(
                1,
                [(
                    PointerEdgeKind::ButtonReleased(PointerButton::Primary),
                    point,
                )],
            ),
        )
        .expect("contained close release stages");
    let release_candidate = frame
        .pointer_receiver_candidates()
        .expect("contained close release requests a receiver")
        .candidates()[0]
        .clone();
    let release_delivery = PointerReceiverDelivery::new(
        frame
            .view()
            .interaction_projection(SOURCE_SURFACE)
            .expect("release retains contained close authority"),
        PointerReceiverDeliveryDisposition::Dock(close_region),
    )
    .expect("contained close supports matching release delivery");
    frame
        .submit_pointer_receiver_receipts(
            PointerReceiverReceiptBatch::new([release_candidate.receipt(
                PointerReceiverObservation::Presented(
                    PresentedPointerReceiverObservation::new([
                        PointerReceiverProbeReceipt::Delivery(release_delivery),
                    ])
                    .expect("contained close release answers delivery"),
                ),
            )])
            .expect("contained close release receipt batch is exact"),
        )
        .expect("contained close release receipt stages");
    complete_host_frame_with_explicit_surface_roster(&engine, &mut frame);
    let transition = frame
        .finish(&mut engine)
        .expect("contained close journal frame commits");

    assert!(transition.reduced_pointer_edges().iter().any(|edge| {
        matches!(
            edge.interaction_outcomes(),
            [InteractionOutcome::ProductActionApplied(
                DockspaceActionOutcome::RootDocked {
                    root: TARGET_ROOT,
                    items,
                    changed: true,
                    ..
                }
            )] if items == &[ItemId::new(2)]
        )
    }));
    assert!(engine.workspace().contained_floating(floating).is_none());
    assert!(
        engine
            .workspace()
            .item_multiset()
            .contains_key(&ItemId::new(2))
    );
    assert_eq!(engine.close.active_plans().count(), 0);
}

#[test]
fn real_host_frame_splitter_gesture_has_bounded_clone_work() {
    let mut builder = Workspace::builder();
    let left = builder.insert_node(Node::tabs([ItemId::new(1)]));
    let right = builder.insert_node(Node::tabs([ItemId::new(2)]));
    let split = builder.insert_node(
        Node::equal_split(Axis::Horizontal, [left, right])
            .expect("splitter workload must validate"),
    );
    builder.set_root(SOURCE_ROOT, RootRecord::new(split).with_central(left));
    builder.set_surface(SOURCE_SURFACE, SurfacePresentation::with_main(SOURCE_ROOT));
    let mut engine = DockEngine::new(
        builder.build().expect("splitter workspace must validate"),
        DockPolicy::default(),
    )
    .expect("splitter engine must initialize");
    let host = engine
        .create_presentation_host()
        .expect("splitter presentation host must mint");
    let bounds =
        LogicalRect::new(0.0, 0.0, 640.0, 480.0).expect("splitter surface bounds must be finite");
    publish_surface_projection(&mut engine, host, SOURCE_SURFACE, bounds);
    let projection = engine
        .interaction_projection(SOURCE_SURFACE)
        .expect("splitter projection must be receiver-authoritative");
    let splitter_region = projection
        .hit_manifest()
        .regions()
        .iter()
        .find(|region| {
            matches!(
                region.id().kind(),
                PresentationHitRegionKind::SplitterHandle(splitter)
                    if splitter.split == split
            )
        })
        .expect("split must expose one pointer handle")
        .id();
    let press = region_center(
        projection
            .hit_manifest()
            .region(splitter_region)
            .expect("splitter receiver remains present"),
    );
    let moved =
        LogicalPoint::new(press.x() + 24.0, press.y()).expect("splitter move point must be finite");
    let released = LogicalPoint::new(press.x() + 48.0, press.y())
        .expect("splitter release point must be finite");
    let provider = engine
        .create_surface_local_pointer_provider(
            SurfaceLocalPointerScope::new(
                host,
                SurfaceLocalPointerEndpoint::Logical(SOURCE_SURFACE),
            ),
            PointerEdgeSequence::new(0),
        )
        .expect("splitter pointer provider must mint");
    let workspace_volume =
        crate::drop_resolver::structural_work::WorkspaceCloneVolume::capture(engine.workspace());

    crate::drop_resolver::structural_work::reset();
    let mut frame = begin_test_host_frame(&engine, host);
    frame
        .submit_surface_pointer_journal(
            &provider,
            local_pointer_journal(
                0,
                [(
                    PointerEdgeKind::ButtonPressed(PointerButton::Primary),
                    press,
                )],
            ),
        )
        .expect("splitter press journal must stage");
    let projection = frame
        .view()
        .interaction_projection(SOURCE_SURFACE)
        .expect("sealed splitter frame retains receiver authority");
    let press_candidate = frame
        .pointer_receiver_candidates()
        .expect("splitter press freezes an exact candidate")
        .candidates()[0]
        .clone();
    let press_delivery = PointerReceiverDelivery::new(
        projection,
        PointerReceiverDeliveryDisposition::Dock(splitter_region),
    )
    .expect("splitter supports drag delivery");
    frame
        .submit_pointer_receiver_receipts(
            PointerReceiverReceiptBatch::new([press_candidate.receipt(
                PointerReceiverObservation::Presented(
                    PresentedPointerReceiverObservation::new([
                        PointerReceiverProbeReceipt::Delivery(press_delivery),
                    ])
                    .expect("splitter press answers delivery"),
                ),
            )])
            .expect("splitter receipt batch is exact"),
        )
        .expect("splitter press receipt must stage");
    let work_before_move = crate::drop_resolver::structural_work::snapshot();
    frame
        .submit_surface_pointer_journal(
            &provider,
            local_pointer_journal(1, [(PointerEdgeKind::Moved, moved)]),
        )
        .expect("splitter move journal must stage");
    let move_candidate = frame
        .pointer_receiver_candidates()
        .expect("splitter move freezes an exact candidate")
        .candidates()[0]
        .clone();
    frame
        .submit_pointer_receiver_receipts(
            PointerReceiverReceiptBatch::new([
                move_candidate.receipt(PointerReceiverObservation::NotApplicable)
            ])
            .expect("splitter move receipt batch is exact"),
        )
        .expect("splitter move receipt must stage");
    let work_after_move = crate::drop_resolver::structural_work::snapshot();
    assert_eq!(
        work_after_move.presentation_roster_full_captures,
        work_before_move.presentation_roster_full_captures,
        "one pointer move must not rebuild the complete presentation roster"
    );
    assert_eq!(
        work_after_move.presentation_roster_surface_freezes,
        work_before_move.presentation_roster_surface_freezes,
        "a splitter move without a preview-token change needs no surface freeze"
    );
    frame
        .submit_surface_pointer_journal(
            &provider,
            local_pointer_journal(
                2,
                [(
                    PointerEdgeKind::ButtonReleased(PointerButton::Primary),
                    released,
                )],
            ),
        )
        .expect("splitter release journal must stage");
    let release_candidate = frame
        .pointer_receiver_candidates()
        .expect("splitter release freezes an exact candidate")
        .candidates()[0]
        .clone();
    frame
        .submit_pointer_receiver_receipts(
            PointerReceiverReceiptBatch::new([
                release_candidate.receipt(PointerReceiverObservation::NotApplicable)
            ])
            .expect("splitter release receipt batch is exact"),
        )
        .expect("splitter release receipt must stage");
    complete_host_frame_with_explicit_surface_roster(&engine, &mut frame);
    let transition = frame
        .finish(&mut engine)
        .expect("splitter frame must reduce");
    assert_eq!(
        provider.committed_through(),
        PointerEdgeSequence::new(3),
        "all staged splitter segments commit through the final edge"
    );
    assert!(matches!(
        transition.reduced_pointer_edges()[0].interaction_outcomes(),
        [InteractionOutcome::ResizeBegan { .. }]
    ));
    assert!(matches!(
        transition.reduced_pointer_edges()[1].interaction_outcomes(),
        [InteractionOutcome::ResizeUpdated { .. }]
    ));
    assert!(matches!(
        transition.reduced_pointer_edges()[2].interaction_outcomes(),
        [InteractionOutcome::ResizeDelivered { changed: true, .. }]
    ));
    assert_eq!(engine.interaction().status(), InteractionStatus::Idle);

    let work = crate::drop_resolver::structural_work::snapshot();
    assert_eq!(
        work.presentation_roster_full_captures,
        work_after_move.presentation_roster_full_captures + 1,
        "the committing release must retain the conservative full-roster path"
    );
    assert_eq!(work.engine_deep_clones.atomic_candidates.calls, 1);
    assert_eq!(
        work.engine_deep_clones.atomic_candidates.volume,
        workspace_volume
    );
    assert_eq!(
        work.engine_deep_clones.atomic_candidate_state,
        crate::drop_resolver::structural_work::EngineCloneVolume {
            scene_surfaces: 1,
            scene_retained_plans: 1,
            scene_paint_hit_regions: 32,
            presentation_hosts: 1,
            presentation_streams: 1,
            presentation_pending_outputs: 0,
            live_pointer_providers: 1,
            semantic_input_watermark_guards: 0,
        }
    );
    assert_eq!(work.workspace_deep_clones.engine_candidates.calls, 1);
    assert_eq!(
        work.workspace_deep_clones.engine_candidates.volume,
        workspace_volume
    );
    assert_eq!(work.workspace_deep_clones.transaction_candidates.calls, 1);
    assert_eq!(
        work.workspace_deep_clones.transaction_candidates.volume,
        workspace_volume
    );
    assert_eq!(work.transaction_prepares, 1);
    assert_eq!(work.transaction_commands, 1);
    assert_eq!(work.root_fingerprint_builds, 8);
    assert_eq!(work.root_fingerprint_node_visits, 24);
}

#[test]
fn journal_tab_and_contained_activation_keep_one_outer_engine_clone() {
    let mut builder = Workspace::builder();
    let tabs = builder.insert_node(Node::tabs([ItemId::new(1), ItemId::new(2)]));
    builder.set_root(SOURCE_ROOT, RootRecord::new(tabs).with_central(tabs));
    builder.set_surface(SOURCE_SURFACE, SurfacePresentation::with_main(SOURCE_ROOT));
    let mut engine = DockEngine::new(
        builder.build().expect("tab workspace must validate"),
        DockPolicy::default(),
    )
    .expect("tab engine must initialize");
    let host = engine
        .create_presentation_host()
        .expect("tab presentation host must mint");
    publish_surface_projection(&mut engine, host, SOURCE_SURFACE, test_rect());
    let projection = engine
        .interaction_projection(SOURCE_SURFACE)
        .expect("tab projection must be receiver-authoritative");
    let presentation = JournalSurfacePresentation::from_interaction(projection);
    let tab = presentation
        .plan()
        .tab_records()
        .first()
        .expect("tab projection must contain a tab");
    let tab_bounds = tab.drag_hit().rect();
    let tab_point = LogicalPoint::new(
        tab_bounds.x() + tab_bounds.width() * 0.25,
        tab_bounds.y() + tab_bounds.height() * 0.5,
    )
    .expect("tab point must be finite");
    let provider = engine
        .create_surface_local_pointer_provider(
            SurfaceLocalPointerScope::new(
                host,
                SurfaceLocalPointerEndpoint::Logical(SOURCE_SURFACE),
            ),
            PointerEdgeSequence::new(0),
        )
        .expect("tab pointer provider must mint");
    let owner = GestureOwner::Stream(PointerStreamId::new(provider.lease(), TEST_POINTER, 1));
    let prepared = engine
        .prepare_journal_tab_gesture(&presentation, TabGestureSource::Item(*tab.id()), tab_point)
        .expect("real tab press must prepare");
    let policy = engine.policy.clone();
    let expected_volume =
        crate::drop_resolver::structural_work::WorkspaceCloneVolume::capture(engine.workspace());
    crate::drop_resolver::structural_work::reset();
    let mut outer = engine.candidate();
    let outcome = outer
        .activate_prepared_journal_tab_gesture(
            tab_strip_test_cause(),
            tab_strip_test_focus_causal(),
            owner,
            Authority::Known(PointerCaptureOwner::ProviderEndpoint),
            JournalDragThresholdOrigin::SurfaceLocal(tab_point),
            prepared,
            &policy,
            &mut Vec::new(),
        )
        .expect("real tab press must activate");
    assert!(matches!(outcome, InteractionOutcome::DragArmed { .. }));
    let work = crate::drop_resolver::structural_work::snapshot();
    assert_eq!(work.engine_deep_clones.atomic_candidates.calls, 1);
    assert_eq!(
        work.engine_deep_clones.atomic_candidates.volume,
        expected_volume
    );

    let floating = FloatingPresentationId::new(1);
    let mut builder = Workspace::builder();
    let main_tabs = builder.insert_node(Node::tabs([ItemId::new(10)]));
    let contained_tabs = builder.insert_node(Node::tabs([ItemId::new(11)]));
    builder.set_root(
        SOURCE_ROOT,
        RootRecord::new(main_tabs).with_central(main_tabs),
    );
    builder.set_root(TARGET_ROOT, RootRecord::new(contained_tabs));
    builder.set_surface(SOURCE_SURFACE, SurfacePresentation::with_main(SOURCE_ROOT));
    builder.set_contained_floating(
        floating,
        ContainedFloating::new(
            TARGET_ROOT,
            LogicalRect::new(10.0, 10.0, 80.0, 80.0).expect("contained rect must be valid"),
        ),
    );
    builder
        .attach_contained(SOURCE_SURFACE, floating)
        .expect("contained root must attach");
    let mut engine = DockEngine::new(
        builder.build().expect("contained workspace must validate"),
        DockPolicy::default(),
    )
    .expect("contained engine must initialize");
    let host = engine
        .create_presentation_host()
        .expect("contained presentation host must mint");
    publish_surface_projection(&mut engine, host, SOURCE_SURFACE, test_rect());
    let projection = engine
        .interaction_projection(SOURCE_SURFACE)
        .expect("contained projection must be receiver-authoritative");
    let presentation = JournalSurfacePresentation::from_interaction(projection);
    let contained = presentation
        .plan()
        .contained_record(floating)
        .expect("contained projection must contain the floating root");
    let title_bounds = contained.title_drag_hit().rect();
    let title_point = LogicalPoint::new(
        title_bounds.x() + title_bounds.width() * 0.5,
        title_bounds.y() + title_bounds.height() * 0.5,
    )
    .expect("contained title point must be finite");
    let provider = engine
        .create_surface_local_pointer_provider(
            SurfaceLocalPointerScope::new(
                host,
                SurfaceLocalPointerEndpoint::Logical(SOURCE_SURFACE),
            ),
            PointerEdgeSequence::new(0),
        )
        .expect("contained pointer provider must mint");
    let owner = GestureOwner::Stream(PointerStreamId::new(provider.lease(), TEST_POINTER, 1));
    let prepared = engine
        .prepare_journal_contained_gesture(
            &presentation,
            floating,
            ContainedGestureKind::TitleDrag,
            title_point,
        )
        .expect("real contained title press must prepare");
    let policy = engine.policy.clone();
    let expected_volume =
        crate::drop_resolver::structural_work::WorkspaceCloneVolume::capture(engine.workspace());
    crate::drop_resolver::structural_work::reset();
    let mut outer = engine.candidate();
    let outcome = outer
        .activate_prepared_journal_contained_gesture(
            tab_strip_test_cause(),
            owner,
            Authority::Known(PointerCaptureOwner::ProviderEndpoint),
            Some(JournalDragThresholdOrigin::SurfaceLocal(title_point)),
            prepared,
            &policy,
            &mut Vec::new(),
        )
        .expect("real contained title press must activate");
    assert!(matches!(outcome, InteractionOutcome::DragArmed { .. }));
    let work = crate::drop_resolver::structural_work::snapshot();
    assert_eq!(work.engine_deep_clones.atomic_candidates.calls, 1);
    assert_eq!(
        work.engine_deep_clones.atomic_candidates.volume,
        expected_volume
    );
}

#[test]
fn sealed_host_frame_cannot_overwrite_a_later_presentation_host_lease() {
    let mut engine = single_surface_engine(SOURCE_SURFACE, SOURCE_ROOT, ItemId::new(1));
    let rendering_host = engine
        .create_presentation_host()
        .expect("rendering presentation host must mint");
    let frozen_frontier = engine.presentation_authority.presentation.host_frontier();
    let frozen_tick = engine.last_reducer_tick();
    let mut frame = begin_test_host_frame(&engine, rendering_host);
    complete_host_frame_with_explicit_surface_roster(&engine, &mut frame);

    let later_host = engine
        .create_presentation_host()
        .expect("later presentation host must mint outside the sealed candidate");
    let current_frontier = engine.presentation_authority.presentation.host_frontier();
    assert!(current_frontier > frozen_frontier);
    assert_eq!(engine.presentation_ledger_diagnostics().live_hosts(), 2);

    assert!(matches!(
        frame.finish(&mut engine),
        Err(EngineError::HostFramePresentationHostFrontierStale {
            submitted,
            current,
        }) if submitted == frozen_frontier && current == current_frontier
    ));
    assert_eq!(engine.last_reducer_tick(), frozen_tick);
    assert_eq!(
        engine.presentation_authority.presentation.host_frontier(),
        current_frontier
    );
    engine
        .begin_host_frame(later_host)
        .expect("rejected stale publish must retain the later host lease");

    let successor_host = engine
        .create_presentation_host()
        .expect("successor presentation host must advance beyond the retained lease");
    assert_ne!(successor_host, later_host);
    assert!(engine.presentation_authority.presentation.host_frontier() > current_frontier);
    assert_eq!(engine.presentation_ledger_diagnostics().live_hosts(), 3);
}

#[test]
fn sealed_host_frame_cannot_overwrite_a_replacement_platform_provider() {
    let mut engine = single_surface_engine(SOURCE_SURFACE, SOURCE_ROOT, ItemId::new(1));
    let rendering_host = engine
        .create_presentation_host()
        .expect("rendering presentation host must mint");
    let recorder = engine
        .create_backend_ingress_provider(rendering_host, PointerEdgeSequence::new(0))
        .expect("joined predecessor must mint");
    let mut drained = recorder.drain();
    let replacement = engine
        .begin_backend_ingress_provider_replacement(&mut drained)
        .expect("joined provider replacement must begin");
    let (mut ticket, _) = replacement.into_parts();
    let frozen_tick = engine.last_reducer_tick();
    let frozen_frontier = engine.viewport.platform_provider_frontier();
    let mut frame = begin_test_host_frame(&engine, rendering_host);
    complete_host_frame_with_explicit_surface_roster(&engine, &mut frame);

    let successor = engine
        .finish_backend_ingress_provider_replacement(&mut ticket, rendering_host)
        .expect("replacement joined provider must activate");
    let current_frontier = engine.viewport.platform_provider_frontier();
    assert!(current_frontier > frozen_frontier);
    assert_eq!(engine.last_reducer_tick(), frozen_tick);
    assert_eq!(
        engine.platform_provider(),
        Some(successor.lease().platform_provider())
    );

    assert!(matches!(
        frame.finish(&mut engine),
        Err(EngineError::HostFramePlatformProviderFrontierStale {
            submitted,
            current,
        }) if submitted == frozen_frontier.get() && current == current_frontier.get()
    ));
    assert_eq!(engine.last_reducer_tick(), frozen_tick);
    assert_eq!(
        engine.viewport.platform_provider_frontier(),
        current_frontier
    );
    assert_eq!(
        engine.platform_provider(),
        Some(successor.lease().platform_provider())
    );
}

#[test]
fn sealed_host_frame_cannot_overwrite_initial_platform_provider_creation() {
    let mut engine = single_surface_engine(SOURCE_SURFACE, SOURCE_ROOT, ItemId::new(1));
    let rendering_host = engine
        .create_presentation_host()
        .expect("rendering presentation host must mint");
    let frozen_tick = engine.last_reducer_tick();
    let frozen_frontier = engine.viewport.platform_provider_frontier();
    let mut frame = begin_test_host_frame(&engine, rendering_host);
    complete_host_frame_with_explicit_surface_roster(&engine, &mut frame);

    let provider = engine
        .create_platform_provider()
        .expect("initial platform provider must mint");
    let current_frontier = engine.viewport.platform_provider_frontier();

    assert!(matches!(
        frame.finish(&mut engine),
        Err(EngineError::HostFramePlatformProviderFrontierStale {
            submitted,
            current,
        }) if submitted == frozen_frontier.get() && current == current_frontier.get()
    ));
    assert_eq!(engine.last_reducer_tick(), frozen_tick);
    assert_eq!(engine.platform_provider(), Some(provider));
    assert_eq!(
        engine.viewport.platform_provider_frontier(),
        current_frontier
    );
}

#[test]
fn rejected_platform_provider_change_does_not_invalidate_a_sealed_frame() {
    let mut engine = single_surface_engine(SOURCE_SURFACE, SOURCE_ROOT, ItemId::new(1));
    let rendering_host = engine
        .create_presentation_host()
        .expect("rendering presentation host must mint");
    let provider = engine
        .create_platform_provider()
        .expect("initial platform provider must mint");
    let frozen_frontier = engine.viewport.platform_provider_frontier();
    let mut frame = begin_test_host_frame(&engine, rendering_host);
    complete_host_frame_with_explicit_surface_roster(&engine, &mut frame);

    assert!(matches!(
        engine.create_platform_provider(),
        Err(EngineError::PlatformProvider {
            source: PlatformObservationAuthorityError::ProviderAlreadyActive { active },
        }) if active == provider
    ));
    assert_eq!(
        engine.viewport.platform_provider_frontier(),
        frozen_frontier
    );

    frame
        .finish(&mut engine)
        .expect("a rejected provider change must leave the frame current");
    assert_eq!(engine.platform_provider(), Some(provider));
}

#[test]
fn prepared_host_frame_rolls_back_on_drop_and_publishes_once_on_commit() {
    let mut engine = single_surface_engine(SOURCE_SURFACE, SOURCE_ROOT, ItemId::new(1));
    let rendering_host = engine
        .create_presentation_host()
        .expect("rendering presentation host must mint");
    let before_tick = engine.last_reducer_tick();
    let before_workspace = engine.workspace().clone();
    let mut rolled_back_frame = begin_test_host_frame(&engine, rendering_host);
    complete_host_frame_with_explicit_surface_roster(&engine, &mut rolled_back_frame);

    let prepared = rolled_back_frame
        .prepare(&mut engine)
        .expect("complete host frame must prepare");
    assert_eq!(prepared.transition().tick().get(), before_tick.get() + 1);
    drop(prepared);

    assert_eq!(engine.last_reducer_tick(), before_tick);
    assert_eq!(engine.workspace(), &before_workspace);

    let mut committed_frame = begin_test_host_frame(&engine, rendering_host);
    complete_host_frame_with_explicit_surface_roster(&engine, &mut committed_frame);
    let transition = committed_frame
        .prepare(&mut engine)
        .expect("replacement host frame must prepare")
        .commit()
        .expect("replacement host frame must commit");

    assert_eq!(transition.tick().get(), before_tick.get() + 1);
    assert_eq!(engine.last_reducer_tick(), transition.tick());
}

#[test]
fn host_frame_prelude_cannot_cross_platform_provider_activation() {
    let mut engine = single_surface_engine(SOURCE_SURFACE, SOURCE_ROOT, ItemId::new(1));
    let rendering_host = engine
        .create_presentation_host()
        .expect("rendering presentation host must mint");
    let recorder = engine
        .create_backend_ingress_provider(rendering_host, PointerEdgeSequence::new(0))
        .expect("joined predecessor must mint");
    let mut drained = recorder.drain();
    let replacement = engine
        .begin_backend_ingress_provider_replacement(&mut drained)
        .expect("joined provider replacement must begin");
    let (mut ticket, _) = replacement.into_parts();
    let frozen_frontier = engine.viewport.platform_provider_frontier();
    let mut prelude = engine
        .begin_host_frame(rendering_host)
        .expect("test presentation host frame must begin");
    prelude
        .submit_presentation_observation(HostPresentationObservation::NoUpdate)
        .expect("test host frame must submit an observation");

    let successor = engine
        .finish_backend_ingress_provider_replacement(&mut ticket, rendering_host)
        .expect("replacement joined provider must activate");
    let current_frontier = engine.viewport.platform_provider_frontier();

    assert!(matches!(
        prelude.seal(&engine),
        Err(EngineError::HostFramePlatformProviderFrontierStale {
            submitted,
            current,
        }) if submitted == frozen_frontier.get() && current == current_frontier.get()
    ));
    assert_eq!(
        engine.platform_provider(),
        Some(successor.lease().platform_provider())
    );
}

#[test]
fn presentation_identity_reservations_commit_or_rollback_with_the_engine_candidate() {
    let mut engine = single_surface_engine(SOURCE_SURFACE, SOURCE_ROOT, ItemId::new(1));
    let baseline = engine.presentation_identity_frontier();
    assert_eq!(baseline.last_surface(), SOURCE_SURFACE.get());
    assert_eq!(baseline.last_root(), SOURCE_ROOT.get());
    assert_eq!(baseline.last_floating(), 0);

    let mut rejected_candidate = engine.candidate();
    assert_eq!(
        rejected_candidate
            .reserve_presentation_surface_identity()
            .expect("candidate surface identity must fit"),
        SurfaceId::new(2)
    );
    assert_eq!(
        rejected_candidate
            .reserve_presentation_root_identity()
            .expect("candidate root identity must fit"),
        RootId::new(2)
    );
    assert_eq!(
        rejected_candidate
            .reserve_presentation_floating_identity()
            .expect("candidate floating identity must fit"),
        FloatingPresentationId::new(1)
    );
    drop(rejected_candidate);
    assert_eq!(engine.presentation_identity_frontier(), baseline);

    assert_eq!(
        engine
            .reserve_presentation_surface_identity()
            .expect("published surface identity must fit"),
        SurfaceId::new(2)
    );
    assert_eq!(
        engine
            .reserve_presentation_root_identity()
            .expect("published root identity must fit"),
        RootId::new(2)
    );
    assert_eq!(
        engine
            .reserve_presentation_floating_identity()
            .expect("published floating identity must fit"),
        FloatingPresentationId::new(1)
    );
    assert_eq!(
        engine.presentation_identity_frontier(),
        PresentationIdentityFrontier::from_counters(2, 2, 1),
        "committed reservations remain tombstones even before graph publication"
    );
}

#[cfg(feature = "serde")]
#[test]
fn workspace_replacement_never_regresses_any_identity_frontier() {
    let mut engine = single_surface_engine(SOURCE_SURFACE, SOURCE_ROOT, ItemId::new(1));
    let host = engine
        .create_presentation_host()
        .expect("test presentation host must mint");
    engine
        .reserve_presentation_surface_identity()
        .expect("surface tombstone must reserve");
    engine
        .reserve_presentation_root_identity()
        .expect("root tombstone must reserve");
    engine
        .reserve_presentation_floating_identity()
        .expect("floating tombstone must reserve");
    let tombstones = engine.presentation_identity_frontier();
    let ordinary_replacement = engine.workspace().clone();

    submit_test_input(
        &mut engine,
        host,
        EngineInput::ReplaceWorkspace(ordinary_replacement),
    )
    .expect("ordinary workspace replacement must publish");
    assert_eq!(engine.presentation_identity_frontier(), tombstones);

    let restored = PresentationIdentityFrontier::from_counters(100, 200, 300);
    let identity_aware_replacement = engine.workspace().clone();
    submit_test_input(
        &mut engine,
        host,
        EngineInput::RestoreWorkspace(ValidatedWorkspaceRestore::new(
            identity_aware_replacement,
            restored,
        )),
    )
    .expect("identity-aware workspace replacement must publish");
    assert_eq!(engine.presentation_identity_frontier(), restored);
    assert_eq!(
        engine
            .reserve_presentation_surface_identity()
            .expect("restored surface frontier must advance"),
        SurfaceId::new(101)
    );
    assert_eq!(
        engine
            .reserve_presentation_root_identity()
            .expect("restored root frontier must advance"),
        RootId::new(201)
    );
    assert_eq!(
        engine
            .reserve_presentation_floating_identity()
            .expect("restored floating frontier must advance"),
        FloatingPresentationId::new(301)
    );
}

#[test]
fn application_workspace_commands_cannot_reuse_retired_presentation_identities() {
    let mut policy = DockPolicy::default();
    policy.set_allow_native_surfaces(true);
    let mut engine =
        single_surface_engine_with_policy(SOURCE_SURFACE, SOURCE_ROOT, ItemId::new(1), policy);
    let host = engine
        .create_presentation_host()
        .expect("test presentation host must mint");
    let original = engine.workspace().clone();
    let command = WorkspaceCommand::CreateSurfaceRoot {
        surface: SurfaceId::new(2),
        root: RootId::new(2),
        content: RootContent::OpenItem(ItemId::new(2)),
    };
    let contained_command = WorkspaceCommand::CreateContainedRoot {
        surface: SOURCE_SURFACE,
        root: RootId::new(3),
        floating: FloatingPresentationId::new(1),
        rect: test_rect(),
        position: ContainedPosition::Front,
        content: RootContent::OpenItem(ItemId::new(3)),
    };

    let expected = engine.version();
    let first = submit_test_input(
        &mut engine,
        host,
        EngineInput::WorkspaceCommand {
            expected,
            command: command.clone(),
        },
    )
    .expect("fresh presentation identities must be accepted");
    assert!(matches!(
        first.reduced_inputs()[0].outcome(),
        InputOutcome::CommandProcessed { changed: true, .. }
    ));
    assert_eq!(engine.presentation_identity_frontier().last_surface(), 2);
    assert_eq!(engine.presentation_identity_frontier().last_root(), 2);

    let expected = engine.version();
    let second = submit_test_input(
        &mut engine,
        host,
        EngineInput::WorkspaceCommand {
            expected,
            command: contained_command,
        },
    )
    .expect("fresh contained identities must be accepted");
    assert!(matches!(
        second.reduced_inputs()[0].outcome(),
        InputOutcome::CommandProcessed { changed: true, .. }
    ));
    assert_eq!(engine.presentation_identity_frontier().last_root(), 3);
    assert_eq!(engine.presentation_identity_frontier().last_floating(), 1);

    submit_test_input(&mut engine, host, EngineInput::ReplaceWorkspace(original))
        .expect("removing the created surface must preserve its tombstone");
    let expected = engine.version();
    let rejected = submit_test_input(
        &mut engine,
        host,
        EngineInput::WorkspaceCommand { expected, command },
    )
    .expect("retired identity rejection is an ordinary input outcome");
    assert!(matches!(
        rejected.reduced_inputs()[0].outcome(),
        InputOutcome::CommandRejected {
            error: CommandError::RetiredSurfaceId {
                surface,
                frontier: 2,
            },
            ..
        } if *surface == SurfaceId::new(2)
    ));

    let expected = engine.version();
    let retired_root = submit_test_input(
        &mut engine,
        host,
        EngineInput::WorkspaceCommand {
            expected,
            command: WorkspaceCommand::CreateContainedRoot {
                surface: SOURCE_SURFACE,
                root: RootId::new(2),
                floating: FloatingPresentationId::new(2),
                rect: test_rect(),
                position: ContainedPosition::Front,
                content: RootContent::OpenItem(ItemId::new(4)),
            },
        },
    )
    .expect("retired root rejection is an ordinary input outcome");
    assert!(matches!(
        retired_root.reduced_inputs()[0].outcome(),
        InputOutcome::CommandRejected {
            error: CommandError::RetiredRootId { root, frontier: 3 },
            ..
        } if *root == RootId::new(2)
    ));

    let expected = engine.version();
    let retired_floating = submit_test_input(
        &mut engine,
        host,
        EngineInput::WorkspaceCommand {
            expected,
            command: WorkspaceCommand::CreateContainedRoot {
                surface: SOURCE_SURFACE,
                root: RootId::new(4),
                floating: FloatingPresentationId::new(1),
                rect: test_rect(),
                position: ContainedPosition::Front,
                content: RootContent::OpenItem(ItemId::new(5)),
            },
        },
    )
    .expect("retired floating rejection is an ordinary input outcome");
    assert!(matches!(
        retired_floating.reduced_inputs()[0].outcome(),
        InputOutcome::CommandRejected {
            error: CommandError::RetiredFloatingId {
                floating,
                frontier: 1,
            },
            ..
        } if *floating == FloatingPresentationId::new(1)
    ));
    assert_eq!(engine.presentation_identity_frontier().last_root(), 3);
    assert_eq!(
        engine
            .reserve_presentation_root_identity()
            .expect("rejected floating offer must not consume its fresh root"),
        RootId::new(4)
    );
}

#[test]
fn workspace_replacement_cannot_revive_a_retired_presentation_identity() {
    let mut engine = single_surface_engine(SOURCE_SURFACE, SOURCE_ROOT, ItemId::new(1));
    let host = engine
        .create_presentation_host()
        .expect("test presentation host must mint");
    engine
        .reserve_presentation_surface_identity()
        .expect("surface tombstone must reserve");
    let mut builder = Workspace::builder();
    let tabs = builder.insert_node(Node::tabs([ItemId::new(2)]));
    builder.set_root(RootId::new(2), RootRecord::new(tabs));
    builder.set_surface(
        SurfaceId::new(2),
        SurfacePresentation::with_main(RootId::new(2)),
    );
    let replacement = builder
        .build()
        .expect("replacement workspace must be valid");

    let before = engine.candidate();
    let mut frame = begin_test_host_frame(&engine, host);
    let mut stream = TestInputStream::resume(&engine, ENGINE_TEST_INPUT_SOURCE);
    assert_eq!(
        stream.append(&mut frame, EngineInput::ReplaceWorkspace(replacement)),
        Err(CoreHostFrameError::InputPrefixReductionFailed),
        "a retired presentation identity fails while reducing the input prefix",
    );
    let error = frame
        .finish(&mut engine)
        .expect_err("replacement must not revive a retired surface identity");
    assert!(matches!(
        error,
        EngineError::WorkspaceReplacementIdentityRetired {
            source: CommandError::RetiredSurfaceId { surface, frontier: 2 },
            ..
        } if surface == SurfaceId::new(2)
    ));
    assert_eq!(engine, before);
}

#[test]
fn foreign_engine_viewport_observation_is_rejected_atomically() {
    let mut first = single_surface_engine(SOURCE_SURFACE, SOURCE_ROOT, ItemId::new(1));
    let mut second = single_surface_engine(SOURCE_SURFACE, SOURCE_ROOT, ItemId::new(1));
    let first_host = first
        .create_presentation_host()
        .expect("first presentation host must mint");
    let second_host = second
        .create_presentation_host()
        .expect("second presentation host must mint");
    let token = WindowToken::new(17);
    let first_binding = register_test_root_viewport(&mut first, first_host, SOURCE_SURFACE, token);
    let second_binding =
        register_test_root_viewport(&mut second, second_host, SOURCE_SURFACE, token);

    assert_ne!(
        first_binding.authority_domain(),
        second_binding.authority_domain()
    );
    assert_eq!(first_binding.epoch(), second_binding.epoch());
    assert_eq!(first_binding.surface(), second_binding.surface());
    assert_eq!(first_binding.token(), second_binding.token());
    assert_eq!(first_binding.incarnation(), second_binding.incarnation());
    assert_ne!(first_binding, second_binding);

    let before = second.candidate();
    let provider = test_platform_provider(&mut second);
    let expected_epoch = second.version().epoch();
    let mut frame = begin_test_host_frame(&second, second_host);
    let mut stream = TestInputStream::resume(&second, ENGINE_TEST_INPUT_SOURCE);
    assert_eq!(
        stream.append(
            &mut frame,
            EngineInput::PublishPlatformSnapshot {
                provider,
                expected_epoch,
                snapshot: platform_snapshot_for_binding(first_binding),
            },
        ),
        Err(CoreHostFrameError::InputPrefixReductionFailed),
        "a foreign viewport binding fails while reducing the input prefix",
    );
    let result = frame.finish(&mut second);

    assert!(matches!(
        result,
        Err(EngineError::Viewport {
            source: ViewportCoordinatorError::Registry(
                crate::viewport_registry::ViewportRegistryError::StaleObservedBinding {
                    observed,
                    current,
                }
            ),
            ..
        }) if observed == first_binding && current == second_binding
    ));
    assert_eq!(second, before);
}

#[test]
fn stale_inventory_envelope_rolls_back_the_complete_host_frame() {
    let mut engine = single_surface_engine(SOURCE_SURFACE, SOURCE_ROOT, ItemId::new(1));
    let host = engine
        .create_presentation_host()
        .expect("test presentation host must mint");
    let binding =
        register_test_root_viewport(&mut engine, host, SOURCE_SURFACE, WindowToken::new(18));
    let provider = test_platform_provider(&mut engine);
    let expected_epoch = engine.version().epoch();
    let mut capabilities = PlatformCapabilities::default();
    capabilities.set_authoritative_inventory(PlatformCapability::Supported);
    let focus = |generation| {
        FocusObservationEnvelope::new(
            FocusObservationGeneration::new(generation),
            Authority::Unknown(AuthorityUnavailableReason::NotReported),
            Authority::Unknown(AuthorityUnavailableReason::NotReported),
        )
    };
    let current = test_platform_snapshot_at(
        2,
        capabilities.clone(),
        focus(2),
        vec![ObservedWindow::new(binding)],
        Vec::new(),
        unknown_work_area_observation(2),
    )
    .expect("current platform snapshot must be canonical");
    submit_test_input(
        &mut engine,
        host,
        EngineInput::PublishPlatformSnapshot {
            provider,
            expected_epoch,
            snapshot: current,
        },
    )
    .expect("current inventory envelope must reduce");

    let before = engine.candidate();
    let before_version = engine.version();
    let before_tick = engine.last_reducer_tick();
    let before_registry = engine.viewport.clone();
    let before_input = engine.last_input_sequence();
    let before_source = engine.semantic_input_watermark();
    let stale = test_platform_snapshot_at(
        1,
        capabilities,
        focus(1),
        Vec::new(),
        Vec::new(),
        unknown_work_area_observation(1),
    )
    .expect("stale platform snapshot must remain structurally canonical");

    let mut frame = begin_test_host_frame(&engine, host);
    let mut stream = TestInputStream::resume(&engine, ENGINE_TEST_INPUT_SOURCE);
    assert_eq!(
        stream.append(
            &mut frame,
            EngineInput::PublishPlatformSnapshot {
                provider,
                expected_epoch,
                snapshot: stale,
            },
        ),
        Err(CoreHostFrameError::InputPrefixReductionFailed),
        "a stale inventory envelope fails while reducing the input prefix",
    );
    let result = frame.finish(&mut engine);

    assert!(matches!(
        result,
        Err(EngineError::Viewport {
            source: ViewportCoordinatorError::StaleInventoryObservation {
                submitted,
                current,
            },
            ..
        }) if submitted == InventoryObservationGeneration::new(1)
            && current == InventoryObservationGeneration::new(2)
    ));
    assert_eq!(engine, before);
    assert_eq!(engine.version(), before_version);
    assert_eq!(engine.last_reducer_tick(), before_tick);
    assert_eq!(engine.viewport, before_registry);
    assert_eq!(engine.last_input_sequence(), before_input);
    assert_eq!(engine.semantic_input_watermark(), before_source);
}

fn install_test_output(
    engine: &mut DockEngine,
    host: PresentationHostLease,
    surface: SurfaceId,
) -> (SurfaceSceneStamp, SurfacePresentationOutputTicket) {
    let transition = submit_surface_measurement(
        engine,
        host,
        begin_surface_measurement(engine, surface, test_rect()),
    );
    transition
        .surface_contributions()
        .iter()
        .find_map(|outcome| match outcome {
            SurfaceContributionOutcome::Ready {
                surface: actual,
                stamp,
                ticket,
            } if *actual == surface => Some((*stamp, *ticket)),
            _ => None,
        })
        .unwrap_or_else(|| {
            panic!(
                "expected a ready output for {surface}, got {:?}",
                transition.surface_contributions()
            )
        })
}

#[test]
fn presentation_obligation_cannot_cross_host_frame_attempts() {
    let mut source = single_surface_engine(SOURCE_SURFACE, SOURCE_ROOT, ItemId::new(1));
    let source_host = source
        .create_presentation_host()
        .expect("source presentation host must mint");
    install_test_output(&mut source, source_host, SOURCE_SURFACE);
    let mut source_frame = begin_test_host_frame(&source, source_host);
    let [source_obligation] = source_frame
        .issue_presentation_obligations()
        .expect("source frame must issue one obligation")
        .try_into()
        .expect("source workspace has one physical slot");
    let mut target = single_surface_engine(TARGET_SURFACE, TARGET_ROOT, ItemId::new(2));
    let target_host = target
        .create_presentation_host()
        .expect("target presentation host must mint");

    let mut frame = begin_test_host_frame(&target, target_host);
    let [target_obligation] = frame
        .issue_presentation_obligations()
        .expect("target frame must issue one obligation")
        .try_into()
        .expect("target workspace has one physical slot");
    let expected = target_obligation.attempt();
    let submitted = source_obligation.attempt();
    assert_eq!(
        frame.settle_presentation_obligation(
            source_obligation,
            HostPresentationDisposition::Painted(HostInteractionPresentation::default()),
        ),
        Err(CoreHostFrameError::PresentationObligationAttemptMismatch {
            expected,
            submitted,
        })
    );
    assert!(matches!(
        frame.finish(&mut target),
        Err(EngineError::HostFramePoisoned {
            source: CoreHostFrameError::PresentationObligationAttemptMismatch {
                expected: actual_expected,
                submitted: actual_submitted,
            },
        }) if actual_expected == expected && actual_submitted == submitted
    ));
}

#[test]
fn paired_retention_records_one_actual_paint_and_one_matching_emission() {
    let mut engine = single_surface_engine(SOURCE_SURFACE, SOURCE_ROOT, ItemId::new(1));
    let host = engine
        .create_presentation_host()
        .expect("test presentation host must mint");
    let (stamp, ticket) = install_test_output(&mut engine, host, SOURCE_SURFACE);
    let token = engine
        .begin_surface_contribution(SOURCE_SURFACE)
        .expect("ready surface remains in the frozen roster");
    let mut frame = begin_test_host_frame(&engine, host);
    let [obligation] = frame
        .issue_presentation_obligations()
        .expect("paired frame must issue one obligation")
        .try_into()
        .expect("single surface has one physical slot");

    let request = frame
        .record_painted_surface_contribution(
            obligation,
            token,
            HostInteractionPresentation::default(),
        )
        .expect("same-frame actual paint must pair with retained contribution");
    let transition = frame
        .finish(&mut engine)
        .expect("paired retained contribution must commit");

    assert!(matches!(
        transition.surface_contributions(),
        [SurfaceContributionOutcome::Retained {
            surface: SOURCE_SURFACE,
            stamp: actual_stamp,
            ticket: actual_ticket,
        }] if *actual_stamp == stamp && *actual_ticket == ticket
    ));
    let [emission] = transition.presentation_emissions() else {
        panic!("one paired actual paint must produce one emission");
    };
    assert_eq!(emission.request(), request);
    assert_eq!(emission.output().surface(), SOURCE_SURFACE);
    assert!(matches!(
        emission.output().payload(),
        HostPresentationOutputPayload::Paint {
            scene,
            interaction,
            ..
        } if scene == ticket && interaction == HostInteractionPresentation::default()
    ));
}

#[test]
fn presentation_obligations_can_only_be_issued_once() {
    let mut engine = single_surface_engine(SOURCE_SURFACE, SOURCE_ROOT, ItemId::new(1));
    let host = engine
        .create_presentation_host()
        .expect("test presentation host must mint");
    install_test_output(&mut engine, host, SOURCE_SURFACE);
    let mut frame = begin_test_host_frame(&engine, host);

    let obligations = frame
        .issue_presentation_obligations()
        .expect("the first exact obligation issue is accepted");
    assert_eq!(
        frame.issue_presentation_obligations(),
        Err(CoreHostFrameError::PresentationObligationsAlreadyIssued)
    );
    drop(obligations);
    assert!(matches!(
        frame.finish(&mut engine),
        Err(EngineError::HostFramePoisoned {
            source: CoreHostFrameError::PresentationObligationsAlreadyIssued,
        })
    ));
}

#[test]
fn retained_contribution_does_not_implicitly_satisfy_presentation_obligation() {
    let mut engine = single_surface_engine(SOURCE_SURFACE, SOURCE_ROOT, ItemId::new(1));
    let host = engine
        .create_presentation_host()
        .expect("test presentation host must mint");
    install_test_output(&mut engine, host, SOURCE_SURFACE);
    let retained = engine
        .begin_surface_contribution(SOURCE_SURFACE)
        .expect("ready surface remains in the frozen roster");
    let mut frame = begin_test_host_frame(&engine, host);
    let [_obligation] = frame
        .issue_presentation_obligations()
        .expect("frame must issue its one physical obligation")
        .try_into()
        .expect("single surface has one physical slot");
    let contribution = frame
        .view()
        .prepare_surface_retained_contribution(retained)
        .expect("retained semantic contribution remains valid");
    frame
        .push_surface_contribution(contribution)
        .expect("semantic contribution is independent from physical presentation");
    assert!(matches!(
        frame.finish(&mut engine),
        Err(EngineError::HostFramePresentationObligationRosterIncomplete {
            expected,
            resolved,
        }) if expected == vec![HostPresentationSlot::Surface { surface: SOURCE_SURFACE }]
            && resolved.is_empty()
    ));
}

#[test]
fn paired_retention_rejects_a_bootstrap_surface_before_any_emission() {
    let mut engine = single_surface_engine(SOURCE_SURFACE, SOURCE_ROOT, ItemId::new(1));
    let host = engine
        .create_presentation_host()
        .expect("test presentation host must mint");
    let token = engine
        .begin_surface_contribution(SOURCE_SURFACE)
        .expect("bootstrap surface remains in the frozen roster");
    let mut frame = begin_test_host_frame(&engine, host);
    let [obligation] = frame
        .issue_presentation_obligations()
        .expect("bootstrap frame must issue one obligation")
        .try_into()
        .expect("single surface has one physical slot");

    assert_eq!(
        frame.record_painted_surface_contribution(
            obligation,
            token,
            HostInteractionPresentation::default(),
        ),
        Err(CoreHostFrameError::RetainedContributionNotPaintable {
            surface: SOURCE_SURFACE,
        })
    );
    assert!(matches!(
        frame.finish(&mut engine),
        Err(EngineError::HostFramePoisoned {
            source: CoreHostFrameError::RetainedContributionNotPaintable {
                surface: SOURCE_SURFACE,
            },
        })
    ));
    assert_eq!(
        engine
            .presentation_authority
            .last_presentation_output_serial
            .get(),
        0
    );
}

#[test]
fn failed_host_frame_does_not_consume_presentation_output_serial() {
    let mut engine = single_surface_engine(SOURCE_SURFACE, SOURCE_ROOT, ItemId::new(1));
    let host = engine
        .create_presentation_host()
        .expect("test presentation host must mint");
    let contribution = begin_surface_measurement(&engine, SOURCE_SURFACE, test_rect());
    let mut stale_frame = begin_test_host_frame(&engine, host);
    stale_frame
        .push_surface_contribution(contribution)
        .expect("future-stale frame accepts its complete contribution");

    let mut intervening = begin_test_host_frame(&engine, host);
    complete_host_frame_with_explicit_surface_roster(&engine, &mut intervening);
    intervening
        .finish(&mut engine)
        .expect("intervening tick commits without a Ready output");
    assert_eq!(
        engine
            .presentation_authority
            .last_presentation_output_serial
            .get(),
        0
    );

    assert!(matches!(
        stale_frame.finish(&mut engine),
        Err(EngineError::HostFramePredecessorStale { .. })
    ));
    assert_eq!(
        engine
            .presentation_authority
            .last_presentation_output_serial
            .get(),
        0
    );

    let (_, ticket) = install_test_output(&mut engine, host, SOURCE_SURFACE);
    assert_eq!(ticket.serial().get(), 1);
}

#[test]
fn stale_cleanup_retry_version_is_rejected_without_effects_or_state_change() {
    let mut fixture = counter_fixture();
    let stale = fixture.engine.version();
    let replacement = fixture.engine.workspace().clone();
    submit_test_input(
        &mut fixture.engine,
        fixture.presentation_host,
        EngineInput::ReplaceWorkspace(replacement),
    )
    .expect("workspace replacement must advance the epoch");
    assert_ne!(fixture.engine.version(), stale);

    let before = fixture.engine.candidate();
    let effect_count = fixture.engine.viewport.effects().records().count();
    let outcome = fixture
        .engine
        .reduce_viewport_cleanup_retry(
            InputSequence::new(900),
            stale,
            crate::effect::EffectId::new(901),
        )
        .expect("stale retry must be a typed nonfatal rejection");

    assert_eq!(
        outcome,
        InputOutcome::StaleRejected {
            expected: stale,
            accepted_base: fixture.engine.version(),
        }
    );
    assert_eq!(fixture.engine, before);
    assert_eq!(
        fixture.engine.viewport.effects().records().count(),
        effect_count
    );
}
