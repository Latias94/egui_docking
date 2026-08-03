use super::*;

use crate::geometry::{LogicalPoint, LogicalRect, LogicalSize, PhysicalRect, ScaleFactor};
use crate::graph::{Axis, ContainedFloating, Node, RootRecord, SurfacePresentation};
use crate::ids::{FloatingPresentationId, ItemId, RootId, SurfaceId, WorkspaceEpoch};
use crate::intent::{
    Authority, AuthorityUnavailableReason, NativeTearOffProposal, PointerButton, PointerId,
};
use crate::interaction::{
    DragGeneration, DragSessionId, InteractionOutcome, InteractionRejection, InteractionStatus,
};
use crate::platform::{
    CapabilityRosterObservation, CloseEffectAcknowledgement, ObservedWindow, ObservedWorkArea,
    PlatformCapabilities, PlatformCapability, PresentationEffectAcknowledgement,
    WindowCloseObservation, WindowCloseState, WindowCoordinateObservation,
    WindowInventoryObservation, WindowPresentationObservation, WindowPresentationState,
    WorkAreaRosterObservation,
};
use crate::pointer_journal::{
    PointerAuthorityCheckpoint, PointerCaptureOwner, PointerEdgeJournal, PointerEdgeSequence,
    PointerProviderScope, SurfaceLocalPointerEndpoint, SurfaceLocalPointerScope,
};
use crate::pointer_receiver::{
    PointerReceiverDelivery, PointerReceiverReceipt, PointerReceiverReceiptBatch,
    PresentedPointerReceiverObservation,
};
use crate::presentation_hit::{PresentationHitManifest, PresentationHitRegionKind};
use crate::presentation_observation::{
    HostPresentationCaptureGeneration, HostPresentationObservation,
    HostPresentationObservationEntry, HostPresentationOutputPayload, HostPresentationProgress,
    HostPresentationStreamObservation, PresentationHostLease, PresentationHostRetirementReason,
};
use crate::presentation_observation::{PresentationOutputSerial, SurfacePresentationOutputTicket};
use crate::scene::{SurfaceScene, TabBarSceneId, TabSceneId, TabStripMemberVisibility};
use crate::scene_manifest::{
    Measurement, MeasurementUnavailableReason, SurfaceMeasurements, SurfaceSceneRevision,
    TabIntrinsic, TabListMenuMetrics, TabStripControlMetric, TabStripControlMetrics,
    TabStripControlPlacement, TabStripMetrics,
};
use crate::transition::InputOutcome;
use crate::viewport::{
    CapabilityObservationGeneration, CloseObservationGeneration, CoordinateObservationGeneration,
    InventoryObservationGeneration, ViewportBinding, ViewportRole, WindowIncarnation, WindowToken,
    WorkAreaObservationGeneration, WorkAreaToken,
};
use crate::viewport_focus::{
    FocusObservationEnvelope, FocusObservationGeneration, GlobalFocusedWindow,
    PaneFocusObservationGeneration, PaneFocusObservationTransition, PanelFocusRecord,
};

const SOURCE_ROOT: RootId = RootId::new(1);
const TARGET_ROOT: RootId = RootId::new(2);
const SOURCE_SURFACE: SurfaceId = SurfaceId::new(1);
const TARGET_SURFACE: SurfaceId = SurfaceId::new(2);
const TEST_POINTER: PointerId = PointerId::new(1);
const ENGINE_TEST_INPUT_SOURCE: StableInputSourceId =
    StableInputSourceId::new(0xe0ff_0000_0000_0001);

fn test_platform_provider(engine: &mut DockEngine) -> PlatformObservationLease {
    match engine.platform_provider() {
        Some(provider) => provider,
        None => engine
            .create_platform_provider()
            .expect("test platform provider must mint once per engine"),
    }
}

fn test_platform_snapshot(
    capabilities: PlatformCapabilities,
    focus: FocusObservationEnvelope,
    window_observations: Vec<ObservedWindow>,
    close_observations: Vec<crate::platform::WindowCloseObservation>,
    work_area_observation: WorkAreaRosterObservation,
) -> Result<PlatformSnapshot, crate::platform::PlatformSnapshotError> {
    test_platform_snapshot_at(
        focus.generation().get(),
        capabilities,
        focus,
        window_observations,
        close_observations,
        work_area_observation,
    )
}

fn test_platform_snapshot_at(
    provider_generation: u64,
    capabilities: PlatformCapabilities,
    focus: FocusObservationEnvelope,
    window_observations: Vec<ObservedWindow>,
    close_observations: Vec<crate::platform::WindowCloseObservation>,
    work_area_observation: WorkAreaRosterObservation,
) -> Result<PlatformSnapshot, crate::platform::PlatformSnapshotError> {
    let inventory_supported = capabilities.authoritative_inventory().is_supported();
    let capability_observation = CapabilityRosterObservation::new(
        CapabilityObservationGeneration::new(provider_generation),
        Authority::Known(capabilities),
    );
    let inventory_observation = if inventory_supported {
        WindowInventoryObservation::new(
            InventoryObservationGeneration::new(provider_generation),
            Authority::Known(
                window_observations
                    .iter()
                    .map(ObservedWindow::binding)
                    .collect(),
            ),
        )?
    } else {
        WindowInventoryObservation::unknown(
            InventoryObservationGeneration::new(provider_generation),
            AuthorityUnavailableReason::NotReported,
        )
    };
    PlatformSnapshot::new(
        crate::viewport::PlatformSnapshotGeneration::new(provider_generation),
        capability_observation,
        focus,
        inventory_observation,
        window_observations,
        close_observations,
        work_area_observation,
    )
}

fn unknown_work_area_observation(generation: u64) -> WorkAreaRosterObservation {
    WorkAreaRosterObservation::new(
        WorkAreaObservationGeneration::new(generation),
        Authority::Unknown(AuthorityUnavailableReason::NotReported),
    )
    .expect("test work-area tombstone must be canonical")
}

fn known_work_area_observation(
    generation: u64,
    work_areas: Vec<ObservedWorkArea>,
) -> WorkAreaRosterObservation {
    WorkAreaRosterObservation::new(
        WorkAreaObservationGeneration::new(generation),
        Authority::Known(work_areas),
    )
    .expect("test work-area roster must be canonical")
}

struct TestInputStream {
    source: StableInputSourceId,
    next_sequence: u64,
}

impl TestInputStream {
    fn resume(engine: &DockEngine, source: StableInputSourceId) -> Self {
        Self {
            source,
            next_sequence: engine
                .semantic_input_watermark()
                .map_or(0, SourceSequence::get),
        }
    }

    fn append(
        &mut self,
        frame: &mut CoreHostFrame,
        input: EngineInput,
    ) -> Result<(), CoreHostFrameError> {
        self.next_sequence = self
            .next_sequence
            .checked_add(1)
            .expect("test input source sequence must not exhaust");
        let sequence = SourceSequence::new(self.next_sequence);
        if input.is_configuration_commit() {
            frame.append_configuration(self.source, sequence, input)
        } else {
            frame.append_input(self.source, sequence, input)
        }
    }
}

fn begin_test_host_frame(engine: &DockEngine, host: PresentationHostLease) -> CoreHostFrame {
    let mut prelude = engine
        .begin_host_frame(host)
        .expect("test presentation host frame must begin");
    prelude
        .submit_presentation_observation(HostPresentationObservation::NoUpdate)
        .expect("test host frame must submit an observation");
    prelude
        .seal(engine)
        .expect("test presentation host frame must seal")
}

fn complete_host_frame_with_explicit_surface_roster(
    _engine: &DockEngine,
    frame: &mut CoreHostFrame,
) {
    for obligation in frame
        .issue_presentation_obligations()
        .expect("test frame must issue its exact physical presentation roster")
    {
        frame
            .settle_presentation_obligation(
                obligation,
                HostPresentationDisposition::Unavailable(
                    HostPresentationUnavailableReason::OutputNotProduced,
                ),
            )
            .expect("unpainted test slot must settle explicitly");
    }
    complete_surface_contribution_roster(frame);
}

fn complete_surface_contribution_roster(frame: &mut CoreHostFrame) {
    let surfaces = frame.surfaces().collect::<Vec<_>>();
    for surface in surfaces {
        if frame
            .surface_contributions()
            .iter()
            .any(|contribution| contribution.surface() == surface)
        {
            continue;
        }
        let token = frame
            .view()
            .begin_surface_contribution(surface)
            .expect("frozen test surface remains in the presentation roster");
        match frame.view().scene().surface(surface) {
            Some(SurfaceScene::Ready(_)) => {
                let contribution = frame
                    .view()
                    .prepare_surface_retained_contribution(token)
                    .expect("test Ready candidate remains retained without repainting");
                frame
                    .push_surface_contribution(contribution)
                    .expect("each frozen test surface contributes exactly once");
            }
            Some(SurfaceScene::Bootstrap(_) | SurfaceScene::Stale(_)) => {
                let contribution = frame
                    .view()
                    .prepare_surface_unavailable_contribution(
                        token,
                        MeasurementUnavailableReason::Deferred,
                    )
                    .expect("unpainted test surface reports an explicit unavailable contribution");
                frame
                    .push_surface_contribution(contribution)
                    .expect("each frozen test surface contributes exactly once");
            }
            None => unreachable!("frozen test surface remains rostered"),
        }
    }
}

fn complete_host_frame_with_current_outputs(_engine: &DockEngine, frame: &mut CoreHostFrame) {
    let mut obligations = frame
        .issue_presentation_obligations()
        .expect("test frame must issue its exact physical presentation roster")
        .into_iter()
        .map(|obligation| (obligation.slot().surface(), obligation))
        .collect::<BTreeMap<_, _>>();
    let surfaces = frame.surfaces().collect::<Vec<_>>();
    for surface in surfaces {
        if frame
            .surface_contributions()
            .iter()
            .any(|contribution| contribution.surface() == surface)
        {
            continue;
        }
        let token = frame
            .view()
            .begin_surface_contribution(surface)
            .expect("frozen test surface remains in the presentation roster");
        match frame.view().scene().surface(surface) {
            Some(SurfaceScene::Ready(_)) => {
                let obligation = obligations
                    .remove(&surface)
                    .expect("each test surface has one physical presentation obligation");
                let interaction = frame
                    .view()
                    .presentation_interaction(surface)
                    .unwrap_or_default();
                frame
                    .record_painted_surface_contribution(obligation, token, interaction)
                    .expect("current test output pairs one actual paint with its contribution");
            }
            Some(SurfaceScene::Bootstrap(_) | SurfaceScene::Stale(_)) => {
                let contribution = frame
                    .view()
                    .prepare_surface_unavailable_contribution(
                        token,
                        MeasurementUnavailableReason::Deferred,
                    )
                    .expect("unavailable test surface remains structurally valid");
                frame
                    .push_surface_contribution(contribution)
                    .expect("each frozen test surface contributes exactly once");
            }
            None => unreachable!("frozen test surface remains rostered"),
        }
    }
    for (_, obligation) in obligations {
        frame
            .settle_presentation_obligation(
                obligation,
                HostPresentationDisposition::Unavailable(
                    HostPresentationUnavailableReason::OutputNotProduced,
                ),
            )
            .expect("unpainted physical test slot must settle explicitly");
    }
}

fn submit_test_input(
    engine: &mut DockEngine,
    host: PresentationHostLease,
    input: EngineInput,
) -> Result<EngineTransition, EngineError> {
    let mut stream = TestInputStream::resume(engine, ENGINE_TEST_INPUT_SOURCE);
    let mut frame = begin_test_host_frame(engine, host);
    stream
        .append(&mut frame, input)
        .expect("test input must fit the host-frame phase");
    complete_host_frame_with_explicit_surface_roster(engine, &mut frame);
    frame.finish(engine)
}

fn submit_test_batch(
    engine: &mut DockEngine,
    host: PresentationHostLease,
    inputs: impl IntoIterator<Item = EngineInput>,
) -> Result<EngineTransition, EngineError> {
    let mut stream = TestInputStream::resume(engine, ENGINE_TEST_INPUT_SOURCE);
    let mut frame = begin_test_host_frame(engine, host);
    for input in inputs {
        stream
            .append(&mut frame, input)
            .expect("test input must fit the host-frame phase");
    }
    complete_host_frame_with_explicit_surface_roster(engine, &mut frame);
    frame.finish(engine)
}

fn record_empty_backend_checkpoint(recorder: &mut BackendIngressRecorder) -> BackendIngressBatch {
    recorder
        .record_pointer_segment(
            PointerEdgeJournal::new(
                PointerEdgeSequence::new(0),
                PointerEdgeSequence::new(0),
                Vec::new(),
            )
            .expect("empty pointer checkpoint must be canonical"),
        )
        .expect("backend recorder must accept its current pointer checkpoint");
    recorder
        .batch_after(BackendIngressOrdinal::ORIGIN)
        .expect("initial backend suffix must be available")
}

fn submit_empty_backend_batch(frame: &mut CoreHostFrame, batch: BackendIngressBatch) {
    assert_eq!(
        frame
            .submit_backend_ingress(batch)
            .expect("joined backend batch must validate"),
        BackendIngressProgress::ReceiverReceiptsRequired,
    );
    assert!(
        frame
            .pointer_receiver_candidates()
            .is_some_and(|roster| roster.candidates().is_empty())
    );
    assert_eq!(
        frame
            .submit_backend_pointer_receiver_receipts(
                PointerReceiverReceiptBatch::new([])
                    .expect("empty receipt roster must be canonical"),
            )
            .expect("empty pointer checkpoint must reduce"),
        BackendIngressProgress::Complete,
    );
}

#[test]
fn backend_ingress_watermark_commits_only_with_the_complete_host_frame() {
    let mut engine = single_surface_engine(SOURCE_SURFACE, SOURCE_ROOT, ItemId::new(1));
    let host = engine
        .create_presentation_host()
        .expect("test presentation host must mint");
    let mut recorder = engine
        .create_backend_ingress_provider(host, PointerEdgeSequence::new(0))
        .expect("joined backend provider must enroll atomically");
    let batch = record_empty_backend_checkpoint(&mut recorder);

    let mut failed = begin_test_host_frame(&engine, host);
    submit_empty_backend_batch(&mut failed, batch.clone());
    let duplicate = {
        let token = failed
            .view()
            .begin_surface_contribution(SOURCE_SURFACE)
            .expect("test surface remains rostered");
        failed
            .view()
            .prepare_surface_unavailable_contribution(token, MeasurementUnavailableReason::Deferred)
            .expect("test contribution must prepare")
    };
    complete_host_frame_with_explicit_surface_roster(&engine, &mut failed);
    assert_eq!(
        failed.push_surface_contribution(duplicate),
        Err(CoreHostFrameError::DuplicateSurfaceContribution {
            surface: SOURCE_SURFACE,
        })
    );
    assert!(failed.finish(&mut engine).is_err());
    assert_eq!(engine.backend_ingress_committed_through().get(), 0);

    let mut retry = begin_test_host_frame(&engine, host);
    submit_empty_backend_batch(&mut retry, batch);
    complete_host_frame_with_explicit_surface_roster(&engine, &mut retry);
    retry
        .finish(&mut engine)
        .expect("the identical batch must remain retryable after rollback");
    assert_eq!(
        engine.backend_ingress_committed_through(),
        recorder.recorded_through()
    );
    assert_eq!(engine.semantic_input_watermark(), None);
}

#[test]
fn backend_provider_replacement_rejects_the_predecessor_batch() {
    let mut engine = single_surface_engine(SOURCE_SURFACE, SOURCE_ROOT, ItemId::new(1));
    let host = engine
        .create_presentation_host()
        .expect("test presentation host must mint");
    let mut predecessor = engine
        .create_backend_ingress_provider(host, PointerEdgeSequence::new(0))
        .expect("predecessor backend provider must enroll");
    let predecessor_pointer = predecessor.lease().pointer_provider();
    let predecessor_batch = record_empty_backend_checkpoint(&mut predecessor);
    let replacement = engine
        .begin_backend_ingress_provider_replacement(predecessor.drain())
        .expect("predecessor replacement must revoke both lanes");
    let retained = engine.runtime_retention_manifest().pointer();
    assert_eq!(retained.retired_lease_guards(), 1);
    assert_eq!(retained.compacted_retirement_ranges(), 0);
    assert_eq!(
        engine
            .runtime_retention_manifest()
            .effects()
            .revoked_provider_guards(),
        1
    );
    let (mut replacement, _) = replacement.into_parts();
    let mut successor = engine
        .finish_backend_ingress_provider_replacement(&mut replacement, host)
        .expect("successor lanes must activate atomically");
    assert!(replacement.is_consumed());
    let compacted = engine.runtime_retention_manifest().pointer();
    assert_eq!(compacted.retired_lease_guards(), 0);
    assert_eq!(compacted.compacted_retirement_ranges(), 1);
    assert_eq!(compacted.logical_compacted_leases(), 1);
    assert_eq!(
        engine
            .runtime_retention_manifest()
            .effects()
            .revoked_provider_guards(),
        0
    );
    assert!(matches!(
        engine.retire_pointer_provider(predecessor_pointer),
        Err(EngineError::PointerJournal {
            source: PointerJournalLedgerError::CompactedLease { lease },
        }) if lease == predecessor_pointer
    ));
    assert_eq!(engine.backend_ingress_provider(), Some(successor.lease()));
    assert_eq!(
        engine.platform_provider(),
        Some(successor.lease().platform_provider())
    );
    assert_eq!(
        engine.pointer_provider(),
        Some(successor.lease().pointer_provider())
    );
    let successor_batch = record_empty_backend_checkpoint(&mut successor);

    let mut stale = begin_test_host_frame(&engine, host);
    assert!(matches!(
        stale.submit_backend_ingress(predecessor_batch),
        Err(CoreHostFrameError::BackendIngressRejected {
            source: BackendIngressError::BatchLeaseMismatch { .. },
        })
    ));
    assert!(stale.finish(&mut engine).is_err());

    let mut current = begin_test_host_frame(&engine, host);
    submit_empty_backend_batch(&mut current, successor_batch);
    complete_host_frame_with_explicit_surface_roster(&engine, &mut current);
    current
        .finish(&mut engine)
        .expect("successor batch must own the new namespace");
    assert_eq!(
        engine.backend_ingress_committed_through(),
        successor.recorded_through()
    );
}

#[test]
fn runtime_retention_manifest_bounds_semantic_replay_authority_to_one_writer() {
    let mut engine = single_surface_engine(SOURCE_SURFACE, SOURCE_ROOT, ItemId::new(1));
    assert_eq!(
        engine
            .runtime_retention_manifest()
            .input_sources()
            .watermark_guards(),
        0
    );

    engine
        .validate_and_advance_semantic_input_watermark((1..=10_000).map(|sequence| {
            (
                StableInputSourceId::new(sequence),
                SourceSequence::new(sequence),
            )
        }))
        .expect("one semantic writer accepts globally ordered diagnostic sources");
    let retained = engine.runtime_retention_manifest();
    assert_eq!(retained.input_sources().watermark_guards(), 1);
    assert_eq!(retained.input_sources().retained_structure_count(), 1);
}

#[test]
fn stream_quiescence_defers_until_core_references_are_settled() {
    let mut engine = single_surface_engine(SOURCE_SURFACE, SOURCE_ROOT, ItemId::new(1));
    let host = engine
        .create_presentation_host()
        .expect("test presentation host must mint");
    let first_binding = ViewportBinding::new(
        engine.authority_domain,
        WorkspaceEpoch::new(0),
        SOURCE_SURFACE,
        WindowToken::new(1),
        WindowIncarnation::new(1),
    );
    let second_binding = ViewportBinding::new(
        engine.authority_domain,
        WorkspaceEpoch::new(0),
        SOURCE_SURFACE,
        WindowToken::new(2),
        WindowIncarnation::new(1),
    );
    let retiring = engine
        .presentation_authority
        .presentation
        .emit(
            host,
            SOURCE_SURFACE,
            HostPresentationEndpoint::Native(first_binding),
            HostPresentationOutputPayload::Bootstrap,
        )
        .expect("first endpoint must emit");
    let active = engine
        .presentation_authority
        .presentation
        .emit(
            host,
            SOURCE_SURFACE,
            HostPresentationEndpoint::Native(second_binding),
            HostPresentationOutputPayload::Bootstrap,
        )
        .expect("successor endpoint must emit");

    assert!(
        engine
            .try_prepare_presentation_stream_quiescence(host, retiring.stream())
            .expect("a local stream must remain retryable")
            .is_none()
    );

    let frozen_scope = BTreeSet::from([retiring.stream(), active.stream()]);
    engine
        .presentation_authority
        .presentation
        .reduce_observation(
            host,
            &frozen_scope,
            HostPresentationObservation::Batch(
                vec![retiring, active]
                    .into_iter()
                    .map(|output| {
                        HostPresentationObservationEntry::new(
                            output.stream(),
                            HostPresentationStreamObservation::Captured {
                                generation: HostPresentationCaptureGeneration::new(1),
                                progress: HostPresentationProgress::Retired {
                                    settled_through: output.key(),
                                    presented: Authority::Known(None),
                                },
                            },
                        )
                    })
                    .collect(),
            ),
        )
        .expect("terminal observations must settle both endpoint streams");

    let quiescence = engine
        .try_prepare_presentation_stream_quiescence(host, retiring.stream())
        .expect("settled local stream must validate")
        .expect("settled retiring stream must become reclaimable");
    engine
        .confirm_presentation_stream_quiescence(quiescence)
        .expect("affine stream proof must compact the exact retiring stream");
    assert!(
        !engine
            .presentation_retention_manifest()
            .retains_stream(retiring.stream())
    );
    assert!(
        engine
            .presentation_retention_manifest()
            .retains_stream(active.stream())
    );
}

#[test]
fn stream_quiescence_batch_rolls_back_when_any_proof_is_stale() {
    let mut engine = single_surface_engine(SOURCE_SURFACE, SOURCE_ROOT, ItemId::new(1));
    let host = engine
        .create_presentation_host()
        .expect("test presentation host must mint");
    let bindings = [1, 2, 3].map(|token| {
        ViewportBinding::new(
            engine.authority_domain,
            WorkspaceEpoch::new(0),
            SOURCE_SURFACE,
            WindowToken::new(token),
            WindowIncarnation::new(1),
        )
    });
    let outputs = bindings.map(|binding| {
        engine
            .presentation_authority
            .presentation
            .emit(
                host,
                SOURCE_SURFACE,
                HostPresentationEndpoint::Native(binding),
                HostPresentationOutputPayload::Bootstrap,
            )
            .expect("each exact endpoint must emit")
    });
    let frozen_scope = outputs
        .iter()
        .map(|output| output.stream())
        .collect::<BTreeSet<_>>();
    engine
        .presentation_authority
        .presentation
        .reduce_observation(
            host,
            &frozen_scope,
            HostPresentationObservation::Batch(
                outputs
                    .iter()
                    .map(|output| {
                        HostPresentationObservationEntry::new(
                            output.stream(),
                            HostPresentationStreamObservation::Captured {
                                generation: HostPresentationCaptureGeneration::new(1),
                                progress: HostPresentationProgress::Retired {
                                    settled_through: output.key(),
                                    presented: Authority::Known(None),
                                },
                            },
                        )
                    })
                    .collect(),
            ),
        )
        .expect("terminal observations must settle every endpoint stream");

    let first = engine
        .prepare_presentation_stream_quiescence(host, outputs[0].stream())
        .expect("first retiring stream must prepare");
    let second = engine
        .prepare_presentation_stream_quiescence(host, outputs[1].stream())
        .expect("second retiring stream must prepare");
    let stale_second = engine
        .prepare_presentation_stream_quiescence(host, outputs[1].stream())
        .expect("preparation itself does not consume external renderer proof");
    engine
        .confirm_presentation_stream_quiescence(second)
        .expect("the second stream must compact independently");

    assert!(
        engine
            .confirm_presentation_stream_quiescence_batch([first, stale_second])
            .is_err(),
        "a stale member must reject the whole batch",
    );
    assert!(
        engine
            .presentation_retention_manifest()
            .retains_stream(outputs[0].stream()),
        "the valid first member must not publish before the stale member rejects",
    );
}

#[test]
fn backend_provider_replacement_retries_with_a_live_presentation_host() {
    let mut engine = single_surface_engine(SOURCE_SURFACE, SOURCE_ROOT, ItemId::new(1));
    let predecessor_host = engine
        .create_presentation_host()
        .expect("predecessor presentation host must mint");
    let predecessor = engine
        .create_backend_ingress_provider(predecessor_host, PointerEdgeSequence::new(0))
        .expect("predecessor backend provider must enroll");
    let replacement = engine
        .begin_backend_ingress_provider_replacement(predecessor.drain())
        .expect("joined replacement must revoke every predecessor lane");
    let (mut ticket, _) = replacement.into_parts();

    engine
        .retire_presentation_host(
            predecessor_host,
            PresentationHostRetirementReason::RuntimeDestroyed,
        )
        .expect("the predecessor presentation host may retire during handoff");
    let failure = engine
        .finish_backend_ingress_provider_replacement(&mut ticket, predecessor_host)
        .expect_err("a retired presentation host cannot bind the successor backend");
    assert!(
        matches!(
            failure,
            EngineError::PresentationHostRetired { host, .. }
                | EngineError::PresentationHostRetiredCompacted { host }
                if host == predecessor_host
        ),
        "unexpected replacement failure: {failure:?}",
    );
    assert!(!ticket.is_consumed());
    assert_eq!(engine.backend_ingress_provider(), None);
    assert_eq!(engine.platform_provider(), None);
    assert_eq!(engine.pointer_provider(), None);

    let successor_host = engine
        .create_presentation_host()
        .expect("replacement presentation host must mint");
    let successor = engine
        .finish_backend_ingress_provider_replacement(&mut ticket, successor_host)
        .expect("the unchanged affine ticket must remain retryable with a live host");
    assert!(ticket.is_consumed());
    assert_eq!(
        successor.lease().presentation_host(),
        successor_host,
        "the successor backend must bind the replacement presentation host",
    );
    assert_eq!(engine.backend_ingress_provider(), Some(successor.lease()));
}

#[test]
fn platform_only_replacement_cannot_revoke_a_joined_backend_provider() {
    let mut engine = single_surface_engine(SOURCE_SURFACE, SOURCE_ROOT, ItemId::new(1));
    let host = engine
        .create_presentation_host()
        .expect("test presentation host must mint");
    publish_surface_projection(&mut engine, host, SOURCE_SURFACE, test_rect());
    let mut recorder = engine
        .create_backend_ingress_provider(host, PointerEdgeSequence::new(0))
        .expect("joined backend provider must enroll");
    let joined = recorder.lease();

    let projection = engine
        .interaction_projection(SOURCE_SURFACE)
        .expect("published surface must retain interaction authority");
    let presentation = JournalSurfacePresentation::from_interaction(projection);
    let tab = presentation
        .plan()
        .tab_records()
        .first()
        .expect("single-item workspace must expose a tab");
    let tab_bounds = tab.drag_hit().rect();
    let tab_point = LogicalPoint::new(
        tab_bounds.x() + tab_bounds.width() * 0.5,
        tab_bounds.y() + tab_bounds.height() * 0.5,
    )
    .expect("tab midpoint must be finite");
    let prepared = engine
        .prepare_journal_tab_gesture(&presentation, TabGestureSource::Item(*tab.id()), tab_point)
        .expect("published tab gesture must prepare");
    let owner = GestureOwner::Stream(PointerStreamId::new(
        joined.pointer_provider(),
        TEST_POINTER,
        1,
    ));
    let policy = engine.policy.clone();
    let outcome = engine
        .activate_prepared_journal_tab_gesture(
            tab_strip_test_cause(),
            tab_strip_test_focus_causal(),
            owner,
            Authority::Unknown(AuthorityUnavailableReason::NotReported),
            JournalDragThresholdOrigin::DesktopPhysical {
                position: crate::geometry::PhysicalPoint::new(tab_point.x(), tab_point.y())
                    .expect("desktop threshold point must be finite"),
                source_scale: ScaleFactor::new(1.0).expect("test source scale must be valid"),
            },
            prepared,
            &policy,
            &mut Vec::new(),
        )
        .expect("joined pointer stream must own the armed test gesture");
    assert!(matches!(outcome, InteractionOutcome::DragArmed { .. }));

    let before_version = engine.version;
    let before_tick = engine.last_reducer_tick;
    let before_backend = engine.backend_ingress.clone();
    let before_pointer = engine.pointer_journal.clone();
    let before_interaction = engine.interaction.clone();
    let before_viewport = engine.viewport.clone();
    let before_focus = engine.viewport_focus.clone();
    assert!(matches!(
        engine.begin_platform_provider_replacement(joined.platform_provider()),
        Err(EngineError::BackendIngress {
            source: BackendIngressError::PlatformReplacementRequiresDrain { active },
        }) if active == joined
    ));
    assert_eq!(engine.version, before_version);
    assert_eq!(engine.last_reducer_tick, before_tick);
    assert_eq!(engine.backend_ingress, before_backend);
    assert_eq!(engine.pointer_journal, before_pointer);
    assert_eq!(engine.interaction, before_interaction);
    assert_eq!(engine.viewport, before_viewport);
    assert_eq!(engine.viewport_focus, before_focus);
    assert_eq!(engine.backend_ingress_provider(), Some(joined));
    assert_eq!(engine.platform_provider(), Some(joined.platform_provider()));
    assert_eq!(engine.pointer_provider(), Some(joined.pointer_provider()));
    assert!(matches!(
        engine.interaction().status(),
        InteractionStatus::Armed { .. }
    ));

    let _ = record_empty_backend_checkpoint(&mut recorder);
    let replacement = engine
        .begin_backend_ingress_provider_replacement(recorder.drain())
        .expect("the affine joined drain starts the replacement");
    assert_eq!(engine.backend_ingress_provider(), None);
    assert_eq!(engine.platform_provider(), None);
    assert_eq!(engine.pointer_provider(), None);
    assert_eq!(engine.interaction().status(), InteractionStatus::Idle);
    let (mut ticket, transition) = replacement.into_parts();
    assert!(matches!(
        transition.interaction_events(),
        [event]
            if matches!(
                event.kind(),
                InteractionEventKind::Cancelled {
                    status: InteractionStatus::Armed { .. },
                    reason: InteractionCancelReason::PointerProviderRetired,
                }
            )
    ));
    let successor = engine
        .finish_backend_ingress_provider_replacement(&mut ticket, host)
        .expect("joined successor lanes must activate atomically");
    assert!(ticket.is_consumed());
    assert_eq!(engine.backend_ingress_provider(), Some(successor.lease()));
}

#[test]
fn backend_ingress_interleaves_semantic_input_between_pointer_checkpoints() {
    let mut builder = Workspace::builder();
    let tabs = builder.insert_node(Node::tabs([ItemId::new(1), ItemId::new(2)]));
    builder.set_root(SOURCE_ROOT, RootRecord::new(tabs));
    builder.set_surface(SOURCE_SURFACE, SurfacePresentation::with_main(SOURCE_ROOT));
    let workspace = builder.build().expect("test workspace must be valid");
    let source = workspace
        .capture_item_source(SOURCE_ROOT, tabs, ItemId::new(2))
        .expect("second tab source must be current");
    let mut engine =
        DockEngine::new(workspace, DockPolicy::default()).expect("engine must be valid");
    let host = engine
        .create_presentation_host()
        .expect("test presentation host must mint");
    let expected = engine.version();
    let mut recorder = engine
        .create_backend_ingress_provider(host, PointerEdgeSequence::new(0))
        .expect("joined backend provider must enroll");
    let checkpoint = || {
        PointerEdgeJournal::new(
            PointerEdgeSequence::new(0),
            PointerEdgeSequence::new(0),
            Vec::new(),
        )
        .expect("empty pointer checkpoint must be canonical")
    };
    recorder
        .record_pointer_segment(checkpoint())
        .expect("first pointer checkpoint must record");
    let semantic_ordinal = recorder
        .record_semantic_input(EngineInput::WorkspaceCommand {
            expected,
            command: WorkspaceCommand::Select { source },
        })
        .expect("semantic command must record between pointer checkpoints");
    recorder
        .record_pointer_segment(checkpoint())
        .expect("second pointer checkpoint must record");
    let batch = recorder
        .batch_after(engine.backend_ingress_committed_through())
        .expect("complete ingress suffix must validate");

    let mut frame = begin_test_host_frame(&engine, host);
    assert_eq!(
        frame
            .submit_backend_ingress(batch)
            .expect("first checkpoint must pause replay"),
        BackendIngressProgress::ReceiverReceiptsRequired,
    );
    assert_eq!(
        frame
            .submit_backend_pointer_receiver_receipts(
                PointerReceiverReceiptBatch::new([])
                    .expect("first empty receipt batch must be canonical"),
            )
            .expect("semantic command must reduce before the second checkpoint"),
        BackendIngressProgress::ReceiverReceiptsRequired,
    );
    assert!(matches!(
        frame.view().workspace().node(tabs),
        Some(Node::Tabs {
            selected: Some(item),
            ..
        }) if *item == ItemId::new(2)
    ));
    assert_eq!(
        frame
            .submit_backend_pointer_receiver_receipts(
                PointerReceiverReceiptBatch::new([])
                    .expect("second empty receipt batch must be canonical"),
            )
            .expect("second checkpoint must complete replay"),
        BackendIngressProgress::Complete,
    );
    complete_host_frame_with_explicit_surface_roster(&engine, &mut frame);
    let transition = frame
        .finish(&mut engine)
        .expect("interleaved backend batch must commit atomically");
    let reduced = transition
        .reduced_inputs()
        .iter()
        .find(|input| input.source_sequence().get() == semantic_ordinal.get())
        .expect("semantic ingress ordinal must survive as causal provenance");
    assert_eq!(reduced.causal_ordinal().get(), 0);
}

#[test]
fn backend_ingress_reduces_ordered_native_close_observations_into_one_edge() {
    let mut engine = single_surface_engine(SOURCE_SURFACE, SOURCE_ROOT, ItemId::new(1));
    let host = engine
        .create_presentation_host()
        .expect("test presentation host must mint");
    let mut recorder = engine
        .create_backend_ingress_provider(host, PointerEdgeSequence::new(0))
        .expect("joined backend provider must enroll");
    recorder
        .record_viewport_registration(
            engine.version(),
            SOURCE_SURFACE,
            WindowToken::new(9),
            ViewportRole::Root,
            None,
        )
        .expect("registration must join the backend order");
    let registration_batch = record_empty_backend_checkpoint(&mut recorder);
    let mut registration = begin_test_host_frame(&engine, host);
    submit_empty_backend_batch(&mut registration, registration_batch);
    complete_host_frame_with_explicit_surface_roster(&engine, &mut registration);
    registration
        .finish(&mut engine)
        .expect("ordered registration must commit");
    let binding = engine
        .viewport()
        .viewport(SOURCE_SURFACE)
        .expect("registered surface must have a binding")
        .binding();

    for (generation, state) in [
        (1, WindowCloseState::LiveClear),
        (2, WindowCloseState::LiveRequested),
    ] {
        recorder
            .record_native_close_observation(
                engine.version().epoch(),
                WindowCloseObservation::new(
                    binding,
                    CloseObservationGeneration::new(generation),
                    Authority::Known(state),
                    CloseEffectAcknowledgement::known(None),
                ),
            )
            .expect("close observation must join the backend order");
    }
    recorder
        .record_pointer_segment(
            PointerEdgeJournal::new(
                PointerEdgeSequence::new(0),
                PointerEdgeSequence::new(0),
                Vec::new(),
            )
            .expect("empty pointer checkpoint must be canonical"),
        )
        .expect("close batch must carry its pointer checkpoint");
    let batch = recorder
        .batch_after(engine.backend_ingress_committed_through())
        .expect("close suffix must be contiguous");
    let mut frame = begin_test_host_frame(&engine, host);
    submit_empty_backend_batch(&mut frame, batch);
    complete_host_frame_with_explicit_surface_roster(&engine, &mut frame);
    let transition = frame
        .finish(&mut engine)
        .expect("ordered close observations must commit");
    let edges = transition
        .reduced_inputs()
        .iter()
        .flat_map(|input| match input.outcome() {
            InputOutcome::NativeCloseObservationPublished {
                native_close_edges, ..
            } => native_close_edges.as_slice(),
            _ => &[],
        })
        .copied()
        .collect::<Vec<_>>();
    assert_eq!(edges.len(), 1);
    assert_eq!(edges[0].binding(), binding);
    assert_eq!(edges[0].observed_at(), CloseObservationGeneration::new(2));
}

struct CounterFixture {
    engine: DockEngine,
    presentation_host: PresentationHostLease,
    target_tabs: crate::ids::NodeId,
}

struct BackgroundFixture {
    engine: DockEngine,
    presentation_host: PresentationHostLease,
    source_tabs: crate::ids::NodeId,
}

fn test_rect() -> LogicalRect {
    LogicalRect::new(0.0, 0.0, 100.0, 100.0).expect("test rectangle must be valid")
}

fn surface_measurements(
    engine: &DockEngine,
    surface: SurfaceId,
    bounds: LogicalRect,
) -> SurfaceMeasurements {
    surface_measurements_with_bounds(engine, surface, Measurement::Measured(bounds))
}

fn surface_measurements_with_bounds(
    engine: &DockEngine,
    surface: SurfaceId,
    bounds: Measurement<LogicalRect>,
) -> SurfaceMeasurements {
    let requirements = engine
        .presentation_requirements()
        .surface(surface)
        .expect("test surface requirements must exist");
    let mut measurements = SurfaceMeasurements::new(requirements.ticket());
    measurements
        .set_bounds(requirements.bounds(), bounds)
        .expect("surface bounds answer must be unique");
    if let Some(key) = requirements.popup_plane_bounds() {
        measurements
            .set_popup_plane_bounds(key, bounds)
            .expect("popup-plane bounds answer must be unique");
    }
    let minimum = LogicalSize::new(0.0, 0.0).expect("test minimum must be valid");
    for key in requirements.pane_minimums() {
        measurements
            .insert_pane_minimum(key, Measurement::Measured(minimum))
            .expect("pane minimum answer must be unique");
    }
    for key in requirements.tab_intrinsics() {
        measurements
            .insert_tab_intrinsic(
                key,
                Measurement::Measured(
                    TabIntrinsic::new(56.0).expect("tab intrinsic must be valid"),
                ),
            )
            .expect("tab intrinsic answer must be unique");
    }
    for key in requirements.tab_strips() {
        measurements
            .insert_tab_strip(
                key,
                Measurement::Measured(
                    TabStripMetrics::new(0.0, 0.0).expect("tab strip metrics must be valid"),
                ),
            )
            .expect("tab strip answer must be unique");
    }
    measurements
}

fn begin_surface_measurement(
    engine: &DockEngine,
    surface: SurfaceId,
    bounds: LogicalRect,
) -> PreparedSurfaceContribution {
    let token = engine
        .begin_surface_contribution(surface)
        .expect("test surface must be in the presentation roster");
    engine
        .prepare_surface_contribution(token, surface_measurements(engine, surface, bounds))
        .expect("complete test measurements prepare successfully")
}

fn submit_surface_measurement(
    engine: &mut DockEngine,
    host: PresentationHostLease,
    contribution: PreparedSurfaceContribution,
) -> EngineTransition {
    let mut frame = begin_test_host_frame(engine, host);
    frame
        .push_surface_contribution(contribution)
        .expect("a test tick carries one surface contribution");
    complete_host_frame_with_explicit_surface_roster(engine, &mut frame);
    frame
        .finish(engine)
        .expect("surface contribution must reduce atomically")
}

fn publish_surface_projection(
    engine: &mut DockEngine,
    host: PresentationHostLease,
    surface: SurfaceId,
    bounds: LogicalRect,
) -> SurfaceSceneStamp {
    let measurements = surface_measurements(engine, surface, bounds);
    publish_surface_projection_with_measurements(engine, host, surface, measurements)
}

fn publish_surface_projection_with_measurements(
    engine: &mut DockEngine,
    host: PresentationHostLease,
    surface: SurfaceId,
    measurements: SurfaceMeasurements,
) -> SurfaceSceneStamp {
    publish_surface_projection_with_transition(engine, host, surface, measurements).0
}

fn publish_surface_projection_with_transition(
    engine: &mut DockEngine,
    host: PresentationHostLease,
    surface: SurfaceId,
    measurements: SurfaceMeasurements,
) -> (SurfaceSceneStamp, EngineTransition) {
    let contribution = engine
        .prepare_surface_contribution(
            engine
                .begin_surface_contribution(surface)
                .expect("test surface must remain in the presentation roster"),
            measurements,
        )
        .expect("complete test measurements prepare successfully");
    let transition = submit_surface_measurement(engine, host, contribution);
    let (stamp, _) = transition
        .surface_contributions()
        .iter()
        .find_map(|outcome| match outcome.clone() {
            SurfaceContributionOutcome::Ready {
                surface: actual,
                stamp,
                ticket,
            } if actual == surface => Some((stamp, ticket)),
            _ => None,
        })
        .unwrap_or_else(|| {
            panic!(
                "expected a ready contribution for {surface}, got {:?}",
                transition.surface_contributions()
            )
        });
    let mut emit_prelude = engine
        .begin_host_frame(host)
        .expect("test emit frame must begin");
    emit_prelude
        .submit_presentation_observation(HostPresentationObservation::NoUpdate)
        .expect("test emit observation must submit");
    let mut emit = emit_prelude
        .seal(engine)
        .expect("test emit frame must seal");
    complete_host_frame_with_current_outputs(engine, &mut emit);
    let emitted = emit.finish(engine).expect("test output must emit");
    let outputs = emitted
        .presentation_emissions()
        .iter()
        .map(|emission| emission.output())
        .collect::<Vec<_>>();
    assert_eq!(
        outputs
            .iter()
            .filter(|candidate| candidate.surface() == surface)
            .count(),
        1,
        "one host frame may emit exactly one output for its explicitly painted surface"
    );

    let mut observe_prelude = engine
        .begin_host_frame(host)
        .expect("test observation frame must begin");
    observe_prelude
        .submit_presentation_observation(HostPresentationObservation::Batch(
            outputs
                .into_iter()
                .map(|output| {
                    HostPresentationObservationEntry::new(
                        output.stream(),
                        HostPresentationStreamObservation::Captured {
                            generation: HostPresentationCaptureGeneration::new(
                                output.key().ordinal_for_test(),
                            ),
                            progress: HostPresentationProgress::Retired {
                                settled_through: output.key(),
                                presented: Authority::Known(Some(output.key())),
                            },
                        },
                    )
                })
                .collect(),
        ))
        .expect("test final-presentation observation must submit");
    let mut observe = observe_prelude
        .seal(engine)
        .expect("test observation frame must seal");
    complete_host_frame_with_explicit_surface_roster(engine, &mut observe);
    let observed = observe
        .finish(engine)
        .expect("test observation must reduce");
    assert!(observed.presentation_observations().iter().any(|outcome| {
        matches!(
            outcome,
            crate::presentation_observation::HostPresentationObservationOutcome::Retired {
                promotion_eligible: true,
                ..
            }
        )
    }));
    (stamp, observed)
}

fn assert_no_pointer_passthrough_effects(engine: &DockEngine) {
    assert!(!engine.viewport().effects().records().any(|(_, record)| {
        matches!(
            record.request().effect(),
            crate::effect::PlatformEffect::SetPointerPassthrough { .. }
        )
    }));
}

fn counter_fixture() -> CounterFixture {
    let mut builder = Workspace::builder();
    let source_tabs = builder.insert_node(Node::tabs([ItemId::new(1)]));
    let target_tabs = builder.insert_node(Node::tabs([ItemId::new(2)]));
    builder.set_root(SOURCE_ROOT, RootRecord::new(source_tabs));
    builder.set_root(TARGET_ROOT, RootRecord::new(target_tabs));
    builder.set_surface(SOURCE_SURFACE, SurfacePresentation::with_main(SOURCE_ROOT));
    builder.set_surface(TARGET_SURFACE, SurfacePresentation::with_main(TARGET_ROOT));
    let workspace = builder.build().expect("counter workspace must be valid");
    let mut engine =
        DockEngine::new(workspace, DockPolicy::default()).expect("counter engine must be valid");
    let presentation_host = engine
        .create_presentation_host()
        .expect("counter presentation host must mint");
    CounterFixture {
        engine,
        presentation_host,
        target_tabs,
    }
}

fn single_surface_engine_with_policy(
    surface: SurfaceId,
    root: RootId,
    item: ItemId,
    policy: DockPolicy,
) -> DockEngine {
    let mut builder = Workspace::builder();
    let tabs = builder.insert_node(Node::tabs([item]));
    builder.set_root(root, RootRecord::new(tabs));
    builder.set_surface(surface, SurfacePresentation::with_main(root));
    DockEngine::new(
        builder
            .build()
            .expect("single-surface workspace must be valid"),
        policy,
    )
    .expect("single-surface engine must be valid")
}

fn single_surface_engine(surface: SurfaceId, root: RootId, item: ItemId) -> DockEngine {
    single_surface_engine_with_policy(surface, root, item, DockPolicy::default())
}

struct TabStripReducerFixture {
    engine: DockEngine,
    surface: SurfaceId,
    bar: TabBarSceneId,
    items: [ItemId; 4],
}

fn tab_strip_reducer_fixture(policy: DockPolicy) -> TabStripReducerFixture {
    tab_strip_reducer_fixture_with_selection(policy, 3)
}

fn tab_strip_reducer_fixture_with_selection(
    policy: DockPolicy,
    selected_index: usize,
) -> TabStripReducerFixture {
    let surface = SurfaceId::new(501);
    let root = RootId::new(502);
    let items = [
        ItemId::new(510),
        ItemId::new(511),
        ItemId::new(512),
        ItemId::new(513),
    ];
    let mut builder = Workspace::builder();
    let tabs = builder.insert_node(Node::tabs_with_selection(
        items,
        Some(items[selected_index]),
    ));
    builder.set_root(root, RootRecord::new(tabs).with_central(tabs));
    builder.set_surface(surface, SurfacePresentation::with_main(root));
    let engine = DockEngine::new(
        builder.build().expect("tab-strip workspace must validate"),
        policy,
    )
    .expect("tab-strip engine must initialize");
    TabStripReducerFixture {
        engine,
        surface,
        bar: TabBarSceneId { root, tabs },
        items,
    }
}

fn tab_strip_reducer_measurements(
    fixture: &TabStripReducerFixture,
    with_menu: bool,
) -> SurfaceMeasurements {
    tab_strip_reducer_measurements_for(&fixture.engine, fixture.surface, with_menu)
}

fn tab_strip_reducer_measurements_for(
    engine: &DockEngine,
    surface: SurfaceId,
    with_menu: bool,
) -> SurfaceMeasurements {
    tab_strip_reducer_measurements_with_popup_plane(engine, surface, with_menu, None)
}

fn tab_strip_reducer_measurements_with_popup_plane(
    engine: &DockEngine,
    surface: SurfaceId,
    with_menu: bool,
    popup_plane: Option<LogicalRect>,
) -> SurfaceMeasurements {
    let requirements = engine
        .presentation_requirements()
        .surface(surface)
        .expect("tab-strip surface requirements must exist");
    let mut measurements = SurfaceMeasurements::new(requirements.ticket());
    let bounds = LogicalRect::new(0.0, 0.0, 260.0, 180.0).expect("tab-strip bounds must be valid");
    measurements
        .set_bounds(requirements.bounds(), Measurement::Measured(bounds))
        .expect("bounds answer must be unique");
    if let Some(key) = requirements.popup_plane_bounds() {
        measurements
            .set_popup_plane_bounds(key, Measurement::Measured(popup_plane.unwrap_or(bounds)))
            .expect("popup-plane bounds answer must be unique");
    }
    let minimum = LogicalSize::new(0.0, 0.0).expect("minimum must be valid");
    for key in requirements.pane_minimums() {
        measurements
            .insert_pane_minimum(key, Measurement::Measured(minimum))
            .expect("pane answer must be unique");
    }
    for key in requirements.tab_intrinsics() {
        measurements
            .insert_tab_intrinsic(
                key,
                Measurement::Measured(
                    TabIntrinsic::new(88.0).expect("tab intrinsic must be valid"),
                ),
            )
            .expect("tab answer must be unique");
    }
    let controls = TabStripControlMetrics::new(4.0)
        .expect("control metrics must be valid")
        .with_scroll_backward(
            TabStripControlMetric::new(18.0, TabStripControlPlacement::OverlayLeading)
                .expect("backward control metric must be valid"),
        )
        .with_scroll_forward(
            TabStripControlMetric::new(18.0, TabStripControlPlacement::OverlayTrailing)
                .expect("forward control metric must be valid"),
        )
        .with_tab_list_menu(
            TabStripControlMetric::new(20.0, TabStripControlPlacement::ReservedTrailing)
                .expect("menu control metric must be valid"),
        );
    let menu = TabListMenuMetrics::new(24.0, 8.0, 8.0, 2.0, 92.0, 10.0)
        .expect("menu metrics must be valid");
    for key in requirements.tab_strips() {
        let strip = TabStripMetrics::new(0.0, 0.0)
            .expect("strip metrics must be valid")
            .with_controls(controls);
        measurements
            .insert_tab_strip(
                key,
                Measurement::Measured(if with_menu {
                    strip.with_tab_list_menu(menu)
                } else {
                    strip
                }),
            )
            .expect("strip answer must be unique");
    }
    measurements
}

fn compile_tab_strip_reducer_plan(
    fixture: &TabStripReducerFixture,
    measurements: &SurfaceMeasurements,
) -> PresentationPlan {
    compile_tab_strip_reducer_plan_for(&fixture.engine, fixture.surface, measurements)
}

fn compile_tab_strip_reducer_plan_for(
    engine: &DockEngine,
    surface: SurfaceId,
    measurements: &SurfaceMeasurements,
) -> PresentationPlan {
    compile_surface_measurements(
        engine.workspace(),
        engine.version(),
        &engine.policy,
        &engine.presentation_authority.presentation_config,
        engine.presentation_requirements(),
        measurements,
        &engine.presentation_authority.tab_strip_states,
        &[],
    )
    .unwrap_or_else(|source| panic!("tab-strip measurements for {surface} must compile: {source}"))
}

fn tab_strip_test_cause() -> ReductionCause {
    ReductionCause::Input {
        tick: ReducerTickId::new(1),
        ordinal: ReducerCausalOrdinal::new(1),
        input: InputSequence::new(1),
        source: ENGINE_TEST_INPUT_SOURCE,
        source_sequence: SourceSequence::new(1),
    }
}

fn tab_strip_test_focus_causal() -> FocusCausalStamp {
    FocusCausalStamp::new(PaneFocusIntentGeneration::new(1), tab_strip_test_cause())
}

fn mint_test_focus_causal(engine: &mut DockEngine) -> FocusCausalStamp {
    let generation = engine
        .last_focus_reducer_generation
        .checked_next()
        .expect("the test focus generation must remain available");
    engine.last_focus_reducer_generation = generation;
    FocusCausalStamp::new(generation, tab_strip_test_cause())
}

fn submit_prepared_interaction(
    engine: &mut DockEngine,
    host: PresentationHostLease,
    prepare: impl FnOnce(HostFrameView<'_>) -> Result<EngineInput, InteractionRejection>,
) -> EngineTransition {
    let mut stream = TestInputStream::resume(engine, ENGINE_TEST_INPUT_SOURCE);
    let mut frame = begin_test_host_frame(engine, host);
    let input = prepare(frame.view()).expect("sealed interaction proof must prepare");
    stream
        .append(&mut frame, input)
        .expect("prepared interaction must fit the semantic phase");
    complete_host_frame_with_explicit_surface_roster(engine, &mut frame);
    frame
        .finish(engine)
        .expect("prepared interaction must reduce atomically")
}

fn publish_tab_strip_reducer_projection(
    fixture: &mut TabStripReducerFixture,
    host: PresentationHostLease,
    with_menu: bool,
) {
    let measurements = tab_strip_reducer_measurements(fixture, with_menu);
    publish_surface_projection_with_measurements(
        &mut fixture.engine,
        host,
        fixture.surface,
        measurements,
    );
}

fn frozen_control(
    plan: &PresentationPlan,
    key: TabStripStateKey,
    control: TabStripControlId,
) -> FrozenTabStripControlClick {
    FrozenTabStripControlClick {
        key,
        record: plan
            .tab_strip_control_records()
            .iter()
            .copied()
            .find(|record| record.id() == control)
            .expect("compiled plan must contain the requested control"),
    }
}

fn compiled_control_region(
    fixture: &TabStripReducerFixture,
    plan: &PresentationPlan,
    control: TabStripControlId,
) -> PresentationHitRegionId {
    let ticket = plan
        .measurement_ticket()
        .expect("test reducer plan is measurement-bound");
    let stamp = SurfaceSceneStamp::new(ticket, SurfaceSceneRevision::new(1));
    let output = SurfacePresentationOutputTicket::mint(
        fixture.engine.authority_domain,
        PresentationOutputSerial::new_for_test(1),
        stamp,
    );
    PresentationHitManifest::compile(output, plan)
        .regions()
        .iter()
        .find(|region| region.id().kind() == PresentationHitRegionKind::TabStripControl(control))
        .expect("compiled control must publish one click receiver")
        .id()
}

mod tab_strip;

mod host_authority;

mod focus;

fn publish_counter_scene(fixture: &mut CounterFixture) {
    publish_surface_projection(
        &mut fixture.engine,
        fixture.presentation_host,
        SOURCE_SURFACE,
        test_rect(),
    );
    publish_surface_projection(
        &mut fixture.engine,
        fixture.presentation_host,
        TARGET_SURFACE,
        test_rect(),
    );
}

mod native_lifecycle;
