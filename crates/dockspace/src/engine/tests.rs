use super::*;

mod native_staging_resource;

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

#[test]
fn requirement_pipeline_reconciles_popup_before_finalizing_disabled_policy() {
    let mut fixture = tab_strip_reducer_fixture(DockPolicy::default());
    let key = TabStripStateKey::new(fixture.surface, fixture.bar);
    assert!(
        fixture
            .engine
            .presentation_authority
            .tab_strip_states
            .state(key)
            .is_some()
    );
    let (session, _) = fixture
        .engine
        .presentation_authority
        .tab_strip_states
        .open_tab_list_menu(key, Some(fixture.items[2]))
        .expect("fresh engine must open a live strip without a prior revision");
    assert_eq!(session.key(), key);
    let mut disabled_replacement = DockPolicy::default();
    disabled_replacement.set_tab_bar(crate::policy::TabBarPolicy::new(
        crate::policy::TabBarVisibility::Visible,
        TabBarInteraction::Disabled,
    ));
    let next_policy = fixture
        .engine
        .policy
        .revision()
        .checked_next()
        .expect("test policy revision must advance");
    fixture.engine.policy = disabled_replacement.snapshot(next_policy);
    fixture
        .engine
        .advance_revision(InputSequence::new(1))
        .expect("policy reconciliation must rebuild requirements");
    assert!(
        fixture
            .engine
            .presentation_authority
            .tab_strip_states
            .active_menu()
            .is_none()
    );
    assert_eq!(
        fixture
            .engine
            .presentation_authority
            .presentation_requirements
            .popup(),
        fixture
            .engine
            .presentation_authority
            .tab_strip_states
            .popup_requirement()
    );
    assert!(matches!(
        fixture
            .engine
            .presentation_authority
            .presentation_requirements
            .popup(),
        crate::tab_strip::PopupPlaneRequirement::Inactive { .. }
    ));

    let mut disabled_policy = DockPolicy::default();
    disabled_policy.set_tab_bar(crate::policy::TabBarPolicy::new(
        crate::policy::TabBarVisibility::Visible,
        TabBarInteraction::Disabled,
    ));
    let disabled = tab_strip_reducer_fixture(disabled_policy);
    let measurements = tab_strip_reducer_measurements(&disabled, true);
    let plan = compile_tab_strip_reducer_plan(&disabled, &measurements);
    let control = TabStripControlId::TabListMenu(disabled.bar);
    let record = plan
        .tab_strip_control_records()
        .iter()
        .copied()
        .find(|record| record.id() == control)
        .expect("disabled control remains a blocker");
    assert!(!record.enabled());
    let region = compiled_control_region(&disabled, &plan, control);
    let point = LogicalPoint::new(
        record.bounds().x() + record.bounds().width() * 0.5,
        record.bounds().y() + record.bounds().height() * 0.5,
    )
    .expect("control midpoint must be finite");
    assert_eq!(
        disabled.engine.freeze_journal_click_action(
            &plan,
            disabled.surface,
            region,
            point,
            &disabled.engine.policy,
        ),
        Err(InteractionRejection::TabStripControlDisabled { control })
    );
    assert_eq!(
        disabled.engine.interaction.status(),
        InteractionStatus::Idle
    );
}

#[test]
fn prepared_tab_strip_scroll_is_finite_clamped_and_reveals_hidden_members() {
    for delta in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        assert_eq!(
            TabScrollAdjustment::scroll_by(delta),
            Err(TabScrollAdjustmentError::NonFiniteDelta)
        );
    }
    let duplicate = ItemId::new(510);
    assert_eq!(
        TabScrollAdjustment::scroll_by_preserving(1.0, [duplicate, duplicate]),
        Err(TabScrollAdjustmentError::DuplicatePreservedItem { item: duplicate })
    );

    let mut fixture = tab_strip_reducer_fixture_with_selection(DockPolicy::default(), 1);
    let key = TabStripStateKey::new(fixture.surface, fixture.bar);
    fixture
        .engine
        .presentation_authority
        .tab_strip_states
        .state_mut(key)
        .set_scroll_offset(0.0)
        .expect("zero strip offset is valid");
    let host = fixture
        .engine
        .create_presentation_host()
        .expect("test presentation host must mint");
    publish_tab_strip_reducer_projection(&mut fixture, host, true);
    let maximum = fixture
        .engine
        .interaction_projection(fixture.surface)
        .expect("published strip is interactive")
        .plan()
        .tab_bar_records()[0]
        .maximum_scroll_offset();
    assert!(maximum > 24.0);

    let first = submit_prepared_interaction(&mut fixture.engine, host, |view| {
        view.prepare_tab_strip_scroll(
            fixture.surface,
            fixture.bar,
            TabScrollAdjustment::scroll_by(24.0).expect("finite delta is valid"),
        )
        .map(|prepared| EngineInput::AdjustTabStripScroll { prepared })
    });
    assert!(matches!(
        first.reduced_inputs()[0].outcome(),
        InputOutcome::InteractionProcessed {
            outcome: InteractionOutcome::TabStripScrolled {
                offset,
                changed: true,
                ..
            },
            ..
        } if *offset == 24.0
    ));

    publish_tab_strip_reducer_projection(&mut fixture, host, true);
    let clamped = submit_prepared_interaction(&mut fixture.engine, host, |view| {
        view.prepare_tab_strip_scroll(
            fixture.surface,
            fixture.bar,
            TabScrollAdjustment::scroll_by(1_000_000.0).expect("finite delta is valid"),
        )
        .map(|prepared| EngineInput::AdjustTabStripScroll { prepared })
    });
    assert!(matches!(
        clamped.reduced_inputs()[0].outcome(),
        InputOutcome::InteractionProcessed {
            outcome: InteractionOutcome::TabStripScrolled {
                offset,
                changed: true,
                ..
            },
            ..
        } if *offset == maximum
    ));

    let mut boundary = tab_strip_reducer_fixture(DockPolicy::default());
    let boundary_host = boundary
        .engine
        .create_presentation_host()
        .expect("boundary presentation host must mint");
    publish_tab_strip_reducer_projection(&mut boundary, boundary_host, true);
    let boundary = submit_prepared_interaction(&mut boundary.engine, boundary_host, |view| {
        view.prepare_tab_strip_scroll(
            boundary.surface,
            boundary.bar,
            TabScrollAdjustment::scroll_by(10.0).expect("finite delta is valid"),
        )
        .map(|prepared| EngineInput::AdjustTabStripScroll { prepared })
    });
    assert!(matches!(
        boundary.reduced_inputs()[0].outcome(),
        InputOutcome::InteractionProcessed {
            outcome: InteractionOutcome::TabStripScrolled {
                offset,
                changed: false,
                ..
            },
            ..
        } if *offset == maximum
    ));

    let mut reveal = tab_strip_reducer_fixture_with_selection(DockPolicy::default(), 2);
    let reveal_key = TabStripStateKey::new(reveal.surface, reveal.bar);
    reveal
        .engine
        .presentation_authority
        .tab_strip_states
        .state_mut(reveal_key)
        .set_scroll_offset(0.0)
        .expect("zero strip offset is valid");
    let reveal_host = reveal
        .engine
        .create_presentation_host()
        .expect("reveal presentation host must mint");
    publish_tab_strip_reducer_projection(&mut reveal, reveal_host, true);
    let hidden = reveal.items[3];
    let before = reveal
        .engine
        .interaction_projection(reveal.surface)
        .expect("published reveal strip is interactive")
        .plan()
        .tab_bar_records()[0]
        .members()
        .iter()
        .find(|member| member.tab().item == hidden)
        .copied()
        .expect("hidden member remains in the complete roster");
    assert_eq!(before.visibility(), TabStripMemberVisibility::Hidden);
    let revealed = submit_prepared_interaction(&mut reveal.engine, reveal_host, |view| {
        view.prepare_tab_strip_scroll(
            reveal.surface,
            reveal.bar,
            TabScrollAdjustment::reveal_item(hidden),
        )
        .map(|prepared| EngineInput::AdjustTabStripScroll { prepared })
    });
    assert!(matches!(
        revealed.reduced_inputs()[0].outcome(),
        InputOutcome::InteractionProcessed {
            outcome: InteractionOutcome::TabStripScrolled { changed: true, .. },
            ..
        }
    ));
    publish_tab_strip_reducer_projection(&mut reveal, reveal_host, true);
    let plan = reveal
        .engine
        .interaction_projection(reveal.surface)
        .expect("revealed strip is interactive")
        .plan();
    let bar = &plan.tab_bar_records()[0];
    let member = bar
        .members()
        .iter()
        .find(|member| member.tab().item == hidden)
        .expect("revealed item remains in the roster");
    assert!(member.full_bounds().x() >= bar.viewport().x());
    assert!(member.full_bounds().max().x() <= bar.viewport().max().x());
}

#[test]
fn prepared_strip_scroll_preserves_highest_priority_operability() {
    let mut fixture = tab_strip_reducer_fixture_with_selection(DockPolicy::default(), 0);
    let key = TabStripStateKey::new(fixture.surface, fixture.bar);
    fixture
        .engine
        .presentation_authority
        .tab_strip_states
        .state_mut(key)
        .set_scroll_offset(0.0)
        .expect("zero strip offset is valid");
    let host = fixture
        .engine
        .create_presentation_host()
        .expect("test presentation host must mint");
    publish_tab_strip_reducer_projection(&mut fixture, host, true);

    let preserved = fixture.items[0];
    let incompatible = fixture.items[3];
    let transition = submit_prepared_interaction(&mut fixture.engine, host, |view| {
        view.prepare_tab_strip_scroll(
            fixture.surface,
            fixture.bar,
            TabScrollAdjustment::scroll_by_preserving(1_000_000.0, [preserved, incompatible])
                .expect("ordered preserve roster is valid"),
        )
        .map(|prepared| EngineInput::AdjustTabStripScroll { prepared })
    });
    assert!(matches!(
        transition.reduced_inputs()[0].outcome(),
        InputOutcome::InteractionProcessed {
            outcome: InteractionOutcome::TabStripScrolled {
                offset,
                changed: true,
                ..
            },
            ..
        } if *offset > 0.0
    ));
    publish_tab_strip_reducer_projection(&mut fixture, host, true);
    assert!(
        fixture
            .engine
            .interaction_projection(fixture.surface)
            .expect("updated strip is authoritative")
            .plan()
            .tab_records()
            .iter()
            .any(|tab| tab.id().item == preserved),
        "the highest-priority item must retain operable tab chrome",
    );
}

#[test]
fn prepared_strip_scroll_preserves_compatible_operable_tab_ranges() {
    let mut fixture = tab_strip_reducer_fixture_with_selection(DockPolicy::default(), 0);
    let key = TabStripStateKey::new(fixture.surface, fixture.bar);
    fixture
        .engine
        .presentation_authority
        .tab_strip_states
        .state_mut(key)
        .set_scroll_offset(0.0)
        .expect("zero strip offset is valid");
    let host = fixture
        .engine
        .create_presentation_host()
        .expect("test presentation host must mint");
    publish_tab_strip_reducer_projection(&mut fixture, host, true);

    let retained = [fixture.items[1], fixture.items[0]];
    let transition = submit_prepared_interaction(&mut fixture.engine, host, |view| {
        view.prepare_tab_strip_scroll(
            fixture.surface,
            fixture.bar,
            TabScrollAdjustment::scroll_by_preserving(1_000_000.0, retained)
                .expect("ordered preserve roster is valid"),
        )
        .map(|prepared| EngineInput::AdjustTabStripScroll { prepared })
    });
    assert!(matches!(
        transition.reduced_inputs()[0].outcome(),
        InputOutcome::InteractionProcessed {
            outcome: InteractionOutcome::TabStripScrolled {
                offset,
                changed: true,
                ..
            },
            ..
        } if *offset > 0.0
    ));

    publish_tab_strip_reducer_projection(&mut fixture, host, true);
    let plan = fixture
        .engine
        .interaction_projection(fixture.surface)
        .expect("updated strip is authoritative")
        .plan();
    for item in retained {
        assert!(
            plan.tab_records().iter().any(|tab| tab.id().item == item),
            "preserved tab {item} must retain its configured drag/close operability",
        );
    }
}

#[test]
fn direct_selection_reveals_the_new_priority_item_after_explicit_strip_scroll() {
    let mut fixture = tab_strip_reducer_fixture_with_selection(DockPolicy::default(), 0);
    let surface = fixture.surface;
    let bar = fixture.bar;
    let selected = fixture.items[3];
    let key = TabStripStateKey::new(surface, bar);
    fixture
        .engine
        .presentation_authority
        .tab_strip_states
        .state_mut(key)
        .set_scroll_offset(0.0)
        .expect("zero strip offset is valid");
    let host = fixture
        .engine
        .create_presentation_host()
        .expect("test presentation host must mint");
    publish_tab_strip_reducer_projection(&mut fixture, host, true);
    let before = fixture
        .engine
        .interaction_projection(surface)
        .expect("published strip is interactive")
        .plan()
        .tab_bar_records()[0]
        .members()
        .iter()
        .find(|member| member.tab().item == selected)
        .copied()
        .expect("target item remains in the complete roster");
    assert_eq!(before.visibility(), TabStripMemberVisibility::Hidden);

    let source = fixture
        .engine
        .workspace()
        .capture_item_source(bar.root, bar.tabs, selected)
        .expect("target item source must be current");
    let expected = fixture.engine.version();
    submit_test_input(
        &mut fixture.engine,
        host,
        EngineInput::WorkspaceCommand {
            expected,
            command: WorkspaceCommand::Select { source },
        },
    )
    .expect("direct selection must commit");
    assert_eq!(
        fixture
            .engine
            .presentation_authority
            .tab_strip_states
            .state(key)
            .and_then(|state| state.scroll_offset()),
        Some(0.0),
        "selection keeps the last effective offset until the next exact solve"
    );

    publish_tab_strip_reducer_projection(&mut fixture, host, true);
    let plan = fixture
        .engine
        .interaction_projection(surface)
        .expect("republished strip is interactive")
        .plan();
    let bar = &plan.tab_bar_records()[0];
    let member = bar
        .members()
        .iter()
        .find(|member| member.tab().item == selected)
        .expect("selected item remains in the complete roster");
    assert_eq!(member.visibility(), TabStripMemberVisibility::Visible);
    assert!(member.full_bounds().x() >= bar.viewport().x());
    assert!(member.full_bounds().max().x() <= bar.viewport().max().x());
    assert!(bar.scroll_offset() > 0.0);
    assert_eq!(
        fixture
            .engine
            .presentation_authority
            .tab_strip_states
            .state(key)
            .and_then(|state| state.scroll_offset()),
        Some(bar.scroll_offset()),
        "a successfully installed Ready plan adopts its solved offset"
    );
}

#[test]
fn only_an_installed_ready_plan_silently_adopts_its_resolved_scroll_offset() {
    let mut fixture = tab_strip_reducer_fixture(DockPolicy::default());
    let surface = fixture.surface;
    let key = TabStripStateKey::new(surface, fixture.bar);
    fixture
        .engine
        .presentation_authority
        .tab_strip_states
        .set_tab_strip_scroll_offset(key, 0.0)
        .expect("explicit beginning offset is valid");
    let token = fixture
        .engine
        .begin_surface_contribution(surface)
        .expect("surface contribution must begin");
    let measurements = tab_strip_reducer_measurements(&fixture, true);
    let stale_ready = fixture
        .engine
        .prepare_surface_contribution(token, measurements.clone())
        .expect("duplicate ready candidate must prepare against the same base");
    let ready = fixture
        .engine
        .prepare_surface_contribution(token, measurements)
        .expect("ready candidate must prepare");
    let resolved = match ready.paint_candidate() {
        PreparedSurfacePaintCandidate::Ready(candidate) => candidate
            .plan()
            .tab_bar_records()
            .first()
            .expect("fixture plan has one tab bar")
            .scroll_offset(),
        candidate => panic!("fixture must prepare Ready, got {candidate:?}"),
    };
    assert!(resolved > 0.0);
    let requirement_before = fixture
        .engine
        .presentation_authority
        .presentation_requirements
        .revision();
    let scene_before = fixture
        .engine
        .presentation_authority
        .scene
        .surface(surface)
        .expect("surface scene exists")
        .stamp();
    let ledger_before = fixture.engine.presentation_ledger_diagnostics();
    let policy = fixture.engine.policy.clone();

    let installed = fixture
        .engine
        .reduce_surface_contribution(ReducerTickId::new(900), ready, &policy)
        .expect("ready contribution must install");
    assert!(matches!(
        installed,
        SurfaceContributionOutcome::Ready { .. }
    ));
    assert_eq!(
        fixture
            .engine
            .presentation_authority
            .tab_strip_states
            .state(key)
            .and_then(|state| state.scroll_offset()),
        Some(resolved)
    );
    assert_eq!(
        fixture
            .engine
            .presentation_authority
            .presentation_requirements
            .revision(),
        requirement_before,
        "silent adoption cannot mint a new measurement requirement"
    );
    assert_eq!(
        fixture.engine.presentation_ledger_diagnostics(),
        ledger_before,
        "installing a candidate cannot manufacture an emission"
    );
    let scene_after_ready = fixture
        .engine
        .presentation_authority
        .scene
        .surface(surface)
        .expect("ready surface scene exists")
        .stamp();
    assert_eq!(
        scene_after_ready.revision().get(),
        scene_before.revision().get() + 1,
        "Ready installation advances the scene exactly once"
    );
    fixture
        .engine
        .try_rebuild_presentation_requirements(None)
        .expect("adopted solved state already matches the installed plan");
    assert_eq!(
        fixture
            .engine
            .presentation_authority
            .presentation_requirements
            .revision(),
        requirement_before
    );
    assert_eq!(
        fixture
            .engine
            .presentation_authority
            .scene
            .surface(surface)
            .expect("ready scene remains installed")
            .stamp(),
        scene_after_ready,
        "silent adoption cannot trigger a contribution feedback loop"
    );

    fixture
        .engine
        .presentation_authority
        .tab_strip_states
        .set_tab_strip_scroll_offset(key, 1.0)
        .expect("replacement transient offset is valid");
    let stale = fixture
        .engine
        .reduce_surface_contribution(ReducerTickId::new(901), stale_ready, &policy)
        .expect("stale Ready is a typed nonfatal rejection");
    assert!(matches!(
        stale,
        SurfaceContributionOutcome::Rejected {
            reason: SurfaceContributionRejection::StaleBase { .. },
            ..
        }
    ));
    assert_eq!(
        fixture
            .engine
            .presentation_authority
            .tab_strip_states
            .state(key)
            .and_then(|state| state.scroll_offset()),
        Some(1.0),
        "stale Ready cannot overwrite newer transient state"
    );

    let retained = fixture
        .engine
        .prepare_surface_retained_contribution(
            fixture
                .engine
                .begin_surface_contribution(surface)
                .expect("current Ready contribution must begin"),
        )
        .expect("current Ready candidate may be retained");
    fixture
        .engine
        .presentation_authority
        .tab_strip_states
        .set_tab_strip_scroll_offset(key, 2.0)
        .expect("second replacement transient offset is valid");
    let retained = fixture
        .engine
        .reduce_surface_contribution(ReducerTickId::new(902), retained, &policy)
        .expect("retained contribution must reduce");
    assert!(matches!(
        retained,
        SurfaceContributionOutcome::Retained { .. }
    ));
    assert_eq!(
        fixture
            .engine
            .presentation_authority
            .tab_strip_states
            .state(key)
            .and_then(|state| state.scroll_offset()),
        Some(2.0),
        "Retained candidates never carry a replacement solved offset"
    );
    assert_eq!(
        fixture
            .engine
            .presentation_authority
            .scene
            .surface(surface)
            .expect("retention keeps the same Ready scene")
            .stamp(),
        scene_after_ready
    );
}

#[test]
fn prepared_menu_scroll_revalidates_popup_authority_and_dismisses_exactly() {
    let mut fixture = tab_strip_reducer_fixture(DockPolicy::default());
    let host = fixture
        .engine
        .create_presentation_host()
        .expect("test presentation host must mint");
    publish_tab_strip_reducer_projection(&mut fixture, host, true);
    let opened = submit_prepared_interaction(&mut fixture.engine, host, |view| {
        view.prepare_tab_strip_control_activation(
            fixture.surface,
            TabStripControlId::TabListMenu(fixture.bar),
        )
        .map(|prepared| EngineInput::ActivateTabStripControl { prepared })
    });
    let session = match opened.reduced_inputs()[0].outcome() {
        InputOutcome::InteractionProcessed {
            outcome:
                InteractionOutcome::TabStripControlActivated {
                    changed: true,
                    menu: Some(session),
                    ..
                },
            ..
        } => *session,
        outcome => panic!("prepared menu control must open one session: {outcome:?}"),
    };
    publish_tab_strip_reducer_projection(&mut fixture, host, true);

    let stale_dismiss = {
        let frame = begin_test_host_frame(&fixture.engine, host);
        frame
            .view()
            .prepare_tab_list_menu_dismiss(fixture.surface, session)
            .expect("current menu dismissal proof prepares")
    };
    let revision_before_scroll = fixture
        .engine
        .presentation_authority
        .presentation_requirements
        .revision();
    let scrolled = submit_prepared_interaction(&mut fixture.engine, host, |view| {
        view.prepare_tab_list_menu_scroll(
            fixture.surface,
            session,
            TabScrollAdjustment::scroll_by(1_000_000.0).expect("finite delta is valid"),
        )
        .map(|prepared| EngineInput::AdjustTabListMenuScroll { prepared })
    });
    let maximum = match scrolled.reduced_inputs()[0].outcome() {
        InputOutcome::InteractionProcessed {
            outcome:
                InteractionOutcome::TabListMenuScrolled {
                    offset,
                    changed: true,
                    ..
                },
            ..
        } => *offset,
        outcome => panic!("menu scroll must clamp at its maximum: {outcome:?}"),
    };
    assert!(maximum > 0.0);
    assert!(
        fixture
            .engine
            .presentation_authority
            .presentation_requirements
            .revision()
            > revision_before_scroll,
        "menu scrolling invalidates the popup roster as one cross-surface authority"
    );
    assert!(
        fixture
            .engine
            .interaction_projection(fixture.surface)
            .is_none()
    );

    publish_tab_strip_reducer_projection(&mut fixture, host, true);
    let stale = submit_test_input(
        &mut fixture.engine,
        host,
        EngineInput::DismissTabListMenu {
            prepared: stale_dismiss,
        },
    )
    .expect("stale prepared dismissal reduces as an explicit rejection");
    assert!(matches!(
        stale.reduced_inputs()[0].outcome(),
        InputOutcome::InteractionProcessed {
            outcome: InteractionOutcome::Rejected(InteractionRejection::StaleScene),
            ..
        }
    ));
    assert_eq!(
        fixture
            .engine
            .presentation_authority
            .tab_strip_states
            .active_menu()
            .map(|menu| menu.session()),
        Some(session)
    );

    let boundary = submit_prepared_interaction(&mut fixture.engine, host, |view| {
        view.prepare_tab_list_menu_scroll(
            fixture.surface,
            session,
            TabScrollAdjustment::scroll_by(1.0).expect("finite delta is valid"),
        )
        .map(|prepared| EngineInput::AdjustTabListMenuScroll { prepared })
    });
    assert!(matches!(
        boundary.reduced_inputs()[0].outcome(),
        InputOutcome::InteractionProcessed {
            outcome: InteractionOutcome::TabListMenuScrolled {
                offset,
                changed: false,
                ..
            },
            ..
        } if *offset == maximum
    ));
    publish_tab_strip_reducer_projection(&mut fixture, host, true);

    let revealed = submit_prepared_interaction(&mut fixture.engine, host, |view| {
        view.prepare_tab_list_menu_scroll(
            fixture.surface,
            session,
            TabScrollAdjustment::reveal_item(fixture.items[0]),
        )
        .map(|prepared| EngineInput::AdjustTabListMenuScroll { prepared })
    });
    assert!(matches!(
        revealed.reduced_inputs()[0].outcome(),
        InputOutcome::InteractionProcessed {
            outcome: InteractionOutcome::TabListMenuScrolled {
                offset: 0.0,
                changed: true,
                ..
            },
            ..
        }
    ));
    publish_tab_strip_reducer_projection(&mut fixture, host, true);

    let dismissed = submit_prepared_interaction(&mut fixture.engine, host, |view| {
        view.prepare_tab_list_menu_dismiss(fixture.surface, session)
            .map(|prepared| EngineInput::DismissTabListMenu { prepared })
    });
    assert!(matches!(
        dismissed.reduced_inputs()[0].outcome(),
        InputOutcome::InteractionProcessed {
            outcome: InteractionOutcome::TabListMenuDismissed { session: actual },
            ..
        } if *actual == session
    ));
    assert!(
        fixture
            .engine
            .presentation_authority
            .tab_strip_states
            .active_menu()
            .is_none()
    );
}

#[test]
fn prepared_menu_navigation_atomically_moves_focus_reveals_and_clamps() {
    let mut fixture = tab_strip_reducer_fixture(DockPolicy::default());
    let surface = fixture.surface;
    let bar = fixture.bar;
    let items = fixture.items;
    let host = fixture
        .engine
        .create_presentation_host()
        .expect("test presentation host must mint");
    publish_tab_strip_reducer_projection(&mut fixture, host, true);
    let opened = submit_prepared_interaction(&mut fixture.engine, host, |view| {
        view.prepare_tab_strip_control_activation(surface, TabStripControlId::TabListMenu(bar))
            .map(|prepared| EngineInput::ActivateTabStripControl { prepared })
    });
    let session = match opened.reduced_inputs()[0].outcome() {
        InputOutcome::InteractionProcessed {
            outcome:
                InteractionOutcome::TabStripControlActivated {
                    menu: Some(session),
                    ..
                },
            ..
        } => *session,
        outcome => panic!("prepared control must open one menu: {outcome:?}"),
    };
    assert_eq!(
        fixture
            .engine
            .presentation_authority
            .tab_strip_states
            .active_menu()
            .map(|menu| menu.focus()),
        Some(items[3])
    );
    publish_tab_strip_reducer_projection(&mut fixture, host, true);

    let first = submit_prepared_interaction(&mut fixture.engine, host, |view| {
        view.prepare_tab_list_menu_navigation(surface, session, TabListMenuNavigation::First)
            .map(|prepared| EngineInput::NavigateTabListMenu { prepared })
    });
    assert!(matches!(
        first.reduced_inputs()[0].outcome(),
        InputOutcome::InteractionProcessed {
            outcome: InteractionOutcome::TabListMenuFocusMoved {
                item,
                offset: 0.0,
                changed: true,
                ..
            },
            ..
        } if *item == items[0]
    ));

    publish_tab_strip_reducer_projection(&mut fixture, host, true);
    let next = submit_prepared_interaction(&mut fixture.engine, host, |view| {
        view.prepare_tab_list_menu_navigation(surface, session, TabListMenuNavigation::Next)
            .map(|prepared| EngineInput::NavigateTabListMenu { prepared })
    });
    assert!(matches!(
        next.reduced_inputs()[0].outcome(),
        InputOutcome::InteractionProcessed {
            outcome: InteractionOutcome::TabListMenuFocusMoved {
                item,
                changed: true,
                ..
            },
            ..
        } if *item == items[1]
    ));

    publish_tab_strip_reducer_projection(&mut fixture, host, true);
    let revision_before_reveal = fixture
        .engine
        .presentation_authority
        .presentation_requirements
        .revision();
    let revealed = submit_prepared_interaction(&mut fixture.engine, host, |view| {
        view.prepare_tab_list_menu_navigation(
            surface,
            session,
            TabListMenuNavigation::Focus(items[3]),
        )
        .map(|prepared| EngineInput::NavigateTabListMenu { prepared })
    });
    let revealed_offset = match revealed.reduced_inputs()[0].outcome() {
        InputOutcome::InteractionProcessed {
            outcome:
                InteractionOutcome::TabListMenuFocusMoved {
                    item,
                    offset,
                    changed: true,
                    ..
                },
            ..
        } if *item == items[3] => *offset,
        outcome => panic!("focus and reveal must commit atomically: {outcome:?}"),
    };
    assert!(revealed_offset > 0.0);
    assert_eq!(
        fixture
            .engine
            .presentation_authority
            .presentation_requirements
            .revision(),
        revision_before_reveal
            .checked_next()
            .expect("one navigation delta must advance one revision")
    );
    let active = fixture
        .engine
        .presentation_authority
        .tab_strip_states
        .active_menu()
        .expect("navigation keeps the menu open");
    assert_eq!(active.focus(), items[3]);
    assert_eq!(active.scroll_offset(), revealed_offset);
    assert!(fixture.engine.interaction_projection(surface).is_none());

    publish_tab_strip_reducer_projection(&mut fixture, host, true);
    let plan = fixture
        .engine
        .interaction_projection(surface)
        .expect("republished menu is interactive")
        .plan();
    let menu = plan
        .tab_list_menu_records()
        .iter()
        .find(|record| record.session() == session)
        .expect("republished plan carries the same menu session");
    let focused = menu
        .rows()
        .iter()
        .filter(|row| row.focused())
        .collect::<Vec<_>>();
    assert_eq!(focused.len(), 1);
    assert_eq!(focused[0].tab().item, items[3]);

    let boundary = submit_prepared_interaction(&mut fixture.engine, host, |view| {
        view.prepare_tab_list_menu_navigation(surface, session, TabListMenuNavigation::Next)
            .map(|prepared| EngineInput::NavigateTabListMenu { prepared })
    });
    assert!(matches!(
        boundary.reduced_inputs()[0].outcome(),
        InputOutcome::InteractionProcessed {
            outcome: InteractionOutcome::TabListMenuFocusMoved {
                item,
                offset,
                changed: false,
                ..
            },
            ..
        } if *item == items[3] && *offset == revealed_offset
    ));
}

#[test]
fn prepared_menu_navigation_rejects_missing_targets_and_stale_session_aba() {
    let mut fixture = tab_strip_reducer_fixture(DockPolicy::default());
    let surface = fixture.surface;
    let bar = fixture.bar;
    let items = fixture.items;
    let host = fixture
        .engine
        .create_presentation_host()
        .expect("test presentation host must mint");
    publish_tab_strip_reducer_projection(&mut fixture, host, true);
    let opened = submit_prepared_interaction(&mut fixture.engine, host, |view| {
        view.prepare_tab_strip_control_activation(surface, TabStripControlId::TabListMenu(bar))
            .map(|prepared| EngineInput::ActivateTabStripControl { prepared })
    });
    let first_session = match opened.reduced_inputs()[0].outcome() {
        InputOutcome::InteractionProcessed {
            outcome:
                InteractionOutcome::TabStripControlActivated {
                    menu: Some(session),
                    ..
                },
            ..
        } => *session,
        outcome => panic!("prepared control must open one menu: {outcome:?}"),
    };
    publish_tab_strip_reducer_projection(&mut fixture, host, true);

    let missing = ItemId::new(999_999);
    let frame = begin_test_host_frame(&fixture.engine, host);
    assert_eq!(
        frame.view().prepare_tab_list_menu_navigation(
            surface,
            first_session,
            TabListMenuNavigation::Focus(missing),
        ),
        Err(InteractionRejection::TabListMenuFocusItemUnavailable {
            session: first_session,
            item: missing,
        })
    );
    let stale = frame
        .view()
        .prepare_tab_list_menu_navigation(surface, first_session, TabListMenuNavigation::Previous)
        .expect("current navigation proof prepares");

    let dismissed = submit_prepared_interaction(&mut fixture.engine, host, |view| {
        view.prepare_tab_list_menu_dismiss(surface, first_session)
            .map(|prepared| EngineInput::DismissTabListMenu { prepared })
    });
    assert!(matches!(
        dismissed.reduced_inputs()[0].outcome(),
        InputOutcome::InteractionProcessed {
            outcome: InteractionOutcome::TabListMenuDismissed { .. },
            ..
        }
    ));
    publish_tab_strip_reducer_projection(&mut fixture, host, true);
    let reopened = submit_prepared_interaction(&mut fixture.engine, host, |view| {
        view.prepare_tab_strip_control_activation(surface, TabStripControlId::TabListMenu(bar))
            .map(|prepared| EngineInput::ActivateTabStripControl { prepared })
    });
    let successor = match reopened.reduced_inputs()[0].outcome() {
        InputOutcome::InteractionProcessed {
            outcome:
                InteractionOutcome::TabStripControlActivated {
                    menu: Some(session),
                    ..
                },
            ..
        } => *session,
        outcome => panic!("control must reopen a successor menu: {outcome:?}"),
    };
    assert_ne!(successor, first_session);
    publish_tab_strip_reducer_projection(&mut fixture, host, true);

    let rejected = submit_test_input(
        &mut fixture.engine,
        host,
        EngineInput::NavigateTabListMenu { prepared: stale },
    )
    .expect("stale prepared navigation reduces as a rejection");
    assert!(matches!(
        rejected.reduced_inputs()[0].outcome(),
        InputOutcome::InteractionProcessed {
            outcome: InteractionOutcome::Rejected(InteractionRejection::StaleScene),
            ..
        }
    ));
    let active = fixture
        .engine
        .presentation_authority
        .tab_strip_states
        .active_menu()
        .expect("stale predecessor input must not close the successor");
    assert_eq!(active.session(), successor);
    assert_eq!(active.focus(), items[3]);
}

#[test]
fn prepared_menu_end_then_enter_binds_the_exact_workspace_event_cause() {
    let mut fixture = tab_strip_reducer_fixture_with_selection(DockPolicy::default(), 0);
    let surface = fixture.surface;
    let bar = fixture.bar;
    let target = fixture.items[3];
    let host = fixture
        .engine
        .create_presentation_host()
        .expect("test presentation host must mint");
    publish_tab_strip_reducer_projection(&mut fixture, host, true);
    let opened = submit_prepared_interaction(&mut fixture.engine, host, |view| {
        view.prepare_tab_strip_control_activation(surface, TabStripControlId::TabListMenu(bar))
            .map(|prepared| EngineInput::ActivateTabStripControl { prepared })
    });
    let session = match opened.reduced_inputs()[0].outcome() {
        InputOutcome::InteractionProcessed {
            outcome:
                InteractionOutcome::TabStripControlActivated {
                    menu: Some(session),
                    ..
                },
            ..
        } => *session,
        outcome => panic!("prepared control must open one menu: {outcome:?}"),
    };
    publish_tab_strip_reducer_projection(&mut fixture, host, true);

    let navigated = submit_prepared_interaction(&mut fixture.engine, host, |view| {
        view.prepare_tab_list_menu_navigation(surface, session, TabListMenuNavigation::Last)
            .map(|prepared| EngineInput::NavigateTabListMenu { prepared })
    });
    assert!(matches!(
        navigated.reduced_inputs()[0].outcome(),
        InputOutcome::InteractionProcessed {
            outcome: InteractionOutcome::TabListMenuFocusMoved {
                item,
                changed: true,
                ..
            },
            ..
        } if *item == target
    ));
    publish_tab_strip_reducer_projection(&mut fixture, host, true);

    let tab = TabSceneId {
        root: bar.root,
        tabs: bar.tabs,
        item: target,
    };
    let selected = submit_prepared_interaction(&mut fixture.engine, host, |view| {
        view.prepare_tab_list_menu_row_activation(surface, session, tab)
            .map(|prepared| EngineInput::ActivateTabListMenuRow { prepared })
    });
    assert!(matches!(
        selected.reduced_inputs()[0].outcome(),
        InputOutcome::InteractionProcessed {
            outcome: InteractionOutcome::TabListMenuItemSelected {
                tab: actual,
                changed: true,
                ..
            },
            ..
        } if *actual == tab
    ));
    assert_eq!(selected.events().len(), 1);
    let reduced = &selected.reduced_inputs()[0];
    let cause = ReductionCause::Input {
        tick: reduced.tick(),
        ordinal: reduced.causal_ordinal(),
        input: reduced.sequence(),
        source: reduced.source(),
        source_sequence: reduced.source_sequence(),
    };
    assert_eq!(
        selected.events()[0].cause(),
        cause,
        "the shared journal/semantic reducer must retain the exact input cause"
    );
}

#[test]
fn strip_scroll_controls_align_the_adjacent_partial_tab_and_stop_at_boundaries() {
    let mut scroll_policy = DockPolicy::default();
    scroll_policy.set_close_capability(crate::policy::CloseCapability::Disabled);
    let mut backward = tab_strip_reducer_fixture_with_selection(scroll_policy.clone(), 1);
    let key = TabStripStateKey::new(backward.surface, backward.bar);
    backward
        .engine
        .presentation_authority
        .tab_strip_states
        .state_mut(key)
        .set_scroll_offset(36.0)
        .expect("test scroll must be valid");
    let measurements = tab_strip_reducer_measurements(&backward, true);
    let plan = compile_tab_strip_reducer_plan(&backward, &measurements);
    let control = TabStripControlId::ScrollBackward(backward.bar);
    let expected = aligned_tab_strip_scroll_offset(&plan, key, control)
        .expect("one beginning-side partial tab must exist");
    let outcome = backward
        .engine
        .finish_journal_tab_strip_control(
            tab_strip_test_cause(),
            &frozen_control(&plan, key, control),
            &plan,
        )
        .expect("backward control must settle");
    assert!(matches!(
        outcome,
        InteractionOutcome::TabStripControlActivated {
            control: actual,
            changed: true,
            menu: None,
        } if actual == control
    ));
    assert_eq!(
        backward
            .engine
            .presentation_authority
            .tab_strip_states
            .state(key)
            .and_then(crate::tab_strip::TabStripState::scroll_offset),
        Some(expected)
    );

    let mut forward = tab_strip_reducer_fixture_with_selection(scroll_policy.clone(), 1);
    let forward_key = TabStripStateKey::new(forward.surface, forward.bar);
    forward
        .engine
        .presentation_authority
        .tab_strip_states
        .state_mut(forward_key)
        .set_scroll_offset(36.0)
        .expect("test scroll must be valid");
    let measurements = tab_strip_reducer_measurements(&forward, true);
    let plan = compile_tab_strip_reducer_plan(&forward, &measurements);
    let control = TabStripControlId::ScrollForward(forward.bar);
    let expected = aligned_tab_strip_scroll_offset(&plan, forward_key, control)
        .expect("one ending-side partial tab must exist");
    let outcome = forward
        .engine
        .finish_journal_tab_strip_control(
            tab_strip_test_cause(),
            &frozen_control(&plan, forward_key, control),
            &plan,
        )
        .expect("forward control must settle");
    assert!(matches!(
        outcome,
        InteractionOutcome::TabStripControlActivated { changed: true, .. }
    ));
    assert_eq!(
        forward
            .engine
            .presentation_authority
            .tab_strip_states
            .state(forward_key)
            .and_then(crate::tab_strip::TabStripState::scroll_offset),
        Some(expected)
    );

    let mut boundary = tab_strip_reducer_fixture_with_selection(scroll_policy, 0);
    let boundary_key = TabStripStateKey::new(boundary.surface, boundary.bar);
    boundary
        .engine
        .presentation_authority
        .tab_strip_states
        .state_mut(boundary_key)
        .set_scroll_offset(0.0)
        .expect("zero is a valid boundary");
    let measurements = tab_strip_reducer_measurements(&boundary, true);
    let plan = compile_tab_strip_reducer_plan(&boundary, &measurements);
    assert_eq!(
        aligned_tab_strip_scroll_offset(
            &plan,
            boundary_key,
            TabStripControlId::ScrollBackward(boundary.bar),
        ),
        None
    );
}

#[test]
fn menu_control_release_opens_then_toggles_and_invalidates_old_contributions() {
    let mut fixture = tab_strip_reducer_fixture(DockPolicy::default());
    let key = TabStripStateKey::new(fixture.surface, fixture.bar);
    let measurements = tab_strip_reducer_measurements(&fixture, true);
    let prepared = fixture
        .engine
        .prepare_surface_contribution(
            fixture
                .engine
                .begin_surface_contribution(fixture.surface)
                .expect("surface contribution must begin"),
            measurements.clone(),
        )
        .expect("closed-menu contribution must prepare");
    let plan = compile_tab_strip_reducer_plan(&fixture, &measurements);
    let control = TabStripControlId::TabListMenu(fixture.bar);
    let frozen = frozen_control(&plan, key, control);
    let selected_before = match fixture.engine.workspace.node(fixture.bar.tabs) {
        Some(Node::Tabs { selected, .. }) => *selected,
        _ => panic!("fixture bar must remain tabs"),
    };
    let outcome = fixture
        .engine
        .finish_journal_tab_strip_control(tab_strip_test_cause(), &frozen, &plan)
        .expect("menu open must settle");
    let opened = match outcome {
        InteractionOutcome::TabStripControlActivated {
            changed: true,
            menu: Some(session),
            ..
        } => session,
        outcome => panic!("menu control must open one session, got {outcome:?}"),
    };
    assert_eq!(opened.key(), key);
    assert_eq!(
        match fixture.engine.workspace.node(fixture.bar.tabs) {
            Some(Node::Tabs { selected, .. }) => *selected,
            _ => panic!("fixture bar must remain tabs"),
        },
        selected_before,
        "the opening release cannot be reinterpreted as a new menu row"
    );
    let policy = fixture.engine.policy.clone();
    assert!(matches!(
        fixture
            .engine
            .reduce_surface_contribution(ReducerTickId::new(2), prepared, &policy)
            .expect("stale contribution must reduce as a rejection"),
        SurfaceContributionOutcome::Rejected {
            reason: SurfaceContributionRejection::StaleBase { .. },
            ..
        }
    ));

    let outcome = fixture
        .engine
        .finish_journal_tab_strip_control(tab_strip_test_cause(), &frozen, &plan)
        .expect("menu toggle-close must settle");
    assert!(matches!(
        outcome,
        InteractionOutcome::TabStripControlActivated {
            changed: true,
            menu: None,
            ..
        }
    ));
    assert!(
        fixture
            .engine
            .presentation_authority
            .tab_strip_states
            .active_menu()
            .is_none()
    );
}

#[test]
fn owner_popup_geometry_unavailable_closes_only_after_the_contribution_batch() {
    let mut fixture = tab_strip_reducer_fixture(DockPolicy::default());
    let surface = fixture.surface;
    let key = TabStripStateKey::new(surface, fixture.bar);
    let host = fixture
        .engine
        .create_presentation_host()
        .expect("test presentation host must mint");
    publish_tab_strip_reducer_projection(&mut fixture, host, true);
    let closed_plan = fixture
        .engine
        .interaction_projection(surface)
        .expect("closed strip should be interactive")
        .plan()
        .clone();
    let opened = fixture
        .engine
        .finish_journal_tab_strip_control(
            tab_strip_test_cause(),
            &frozen_control(
                &closed_plan,
                key,
                TabStripControlId::TabListMenu(fixture.bar),
            ),
            &closed_plan,
        )
        .expect("menu control should open");
    let session = match opened {
        InteractionOutcome::TabStripControlActivated {
            menu: Some(session),
            changed: true,
            ..
        } => session,
        outcome => panic!("menu control should open one session: {outcome:?}"),
    };
    let popup_plane =
        LogicalRect::new(0.0, 100.0, 260.0, 80.0).expect("positive popup plane should be valid");
    let measurements = tab_strip_reducer_measurements_with_popup_plane(
        &fixture.engine,
        surface,
        true,
        Some(popup_plane),
    );
    let prepared = fixture
        .engine
        .prepare_surface_contribution(
            fixture
                .engine
                .begin_surface_contribution(surface)
                .expect("owner contribution should begin"),
            measurements,
        )
        .expect("proved popup geometry failure should prepare as unavailable");
    assert!(matches!(
        prepared.paint_candidate(),
        PreparedSurfacePaintCandidate::Unavailable {
            reason: SurfaceContributionUnavailableReason::PopupGeometryUnavailable {
                session: actual,
                reason: PopupGeometryUnavailableReason::AnchorOutsidePlane,
            },
        } if actual == session
    ));
    assert_eq!(
        fixture
            .engine
            .presentation_authority
            .tab_strip_states
            .active_menu()
            .map(crate::tab_strip::ActiveTabListMenu::session),
        Some(session),
        "preparation is read-only and cannot close the active session"
    );

    let transition = submit_surface_measurement(&mut fixture.engine, host, prepared);
    assert!(matches!(
        transition.surface_contributions(),
        [SurfaceContributionOutcome::Unavailable {
            surface: actual,
            reason: SurfaceContributionUnavailableReason::PopupGeometryUnavailable {
                session: actual_session,
                reason: PopupGeometryUnavailableReason::AnchorOutsidePlane,
            },
            ..
        }] if *actual == surface && *actual_session == session
    ));
    assert!(
        fixture
            .engine
            .presentation_authority
            .tab_strip_states
            .active_menu()
            .is_none()
    );
    assert!(matches!(
        fixture.engine.presentation_requirements().popup(),
        crate::tab_strip::PopupPlaneRequirement::Inactive { .. }
    ));
    assert!(fixture.engine.interaction_projection(surface).is_none());
    assert!(matches!(
        fixture.engine.scene().surface(surface),
        Some(SurfaceScene::Stale(_) | SurfaceScene::Bootstrap(_))
    ));
}

#[test]
fn menu_control_replaces_cross_surface_session_and_invalidates_both_owners() {
    let first_surface = SurfaceId::new(601);
    let second_surface = SurfaceId::new(602);
    let first_root = RootId::new(603);
    let second_root = RootId::new(604);
    let first_items = [
        ItemId::new(610),
        ItemId::new(611),
        ItemId::new(612),
        ItemId::new(613),
    ];
    let second_items = [
        ItemId::new(620),
        ItemId::new(621),
        ItemId::new(622),
        ItemId::new(623),
    ];
    let mut builder = Workspace::builder();
    let first_tabs =
        builder.insert_node(Node::tabs_with_selection(first_items, Some(first_items[3])));
    let second_tabs = builder.insert_node(Node::tabs_with_selection(
        second_items,
        Some(second_items[2]),
    ));
    builder.set_root(
        first_root,
        RootRecord::new(first_tabs).with_central(first_tabs),
    );
    builder.set_root(
        second_root,
        RootRecord::new(second_tabs).with_central(second_tabs),
    );
    builder.set_surface(first_surface, SurfacePresentation::with_main(first_root));
    builder.set_surface(second_surface, SurfacePresentation::with_main(second_root));
    let mut engine = DockEngine::new(
        builder
            .build()
            .expect("cross-surface tab-strip workspace must validate"),
        DockPolicy::default(),
    )
    .expect("cross-surface tab-strip engine must initialize");
    let first_bar = TabBarSceneId {
        root: first_root,
        tabs: first_tabs,
    };
    let second_bar = TabBarSceneId {
        root: second_root,
        tabs: second_tabs,
    };
    let first_key = TabStripStateKey::new(first_surface, first_bar);
    let second_key = TabStripStateKey::new(second_surface, second_bar);
    let selected_before = (first_items[3], second_items[2]);

    let first_measurements = tab_strip_reducer_measurements_for(&engine, first_surface, true);
    let first_plan =
        compile_tab_strip_reducer_plan_for(&engine, first_surface, &first_measurements);
    let first_control = TabStripControlId::TabListMenu(first_bar);
    let first_session = match engine
        .finish_journal_tab_strip_control(
            tab_strip_test_cause(),
            &frozen_control(&first_plan, first_key, first_control),
            &first_plan,
        )
        .expect("first menu open must settle")
    {
        InteractionOutcome::TabStripControlActivated {
            control,
            changed: true,
            menu: Some(session),
        } if control == first_control => session,
        outcome => panic!("first menu must open one session, got {outcome:?}"),
    };
    let first_stamp_before_replacement = engine
        .presentation_authority
        .scene
        .surface(first_surface)
        .expect("first surface remains rostered")
        .stamp();
    let second_stamp_before_replacement = engine
        .presentation_authority
        .scene
        .surface(second_surface)
        .expect("second surface remains rostered")
        .stamp();

    let second_measurements = tab_strip_reducer_measurements_for(&engine, second_surface, true);
    let second_plan =
        compile_tab_strip_reducer_plan_for(&engine, second_surface, &second_measurements);
    let second_control = TabStripControlId::TabListMenu(second_bar);
    let second_session = match engine
        .finish_journal_tab_strip_control(
            tab_strip_test_cause(),
            &frozen_control(&second_plan, second_key, second_control),
            &second_plan,
        )
        .expect("replacement menu open must settle")
    {
        InteractionOutcome::TabStripControlActivated {
            control,
            changed: true,
            menu: Some(session),
        } if control == second_control => session,
        outcome => panic!("replacement menu must open one session, got {outcome:?}"),
    };

    assert_ne!(second_session, first_session);
    assert_eq!(
        engine
            .presentation_authority
            .tab_strip_states
            .active_menu()
            .map(|menu| menu.session()),
        Some(second_session)
    );
    assert!(
        engine
            .presentation_authority
            .tab_strip_states
            .active_menu_for(first_key)
            .is_none()
    );
    assert_eq!(
        engine
            .presentation_authority
            .tab_strip_states
            .active_menu_for(second_key)
            .map(|menu| menu.session()),
        Some(second_session)
    );
    assert_ne!(
        engine
            .presentation_authority
            .scene
            .surface(first_surface)
            .expect("first surface remains rostered")
            .stamp(),
        first_stamp_before_replacement
    );
    assert_ne!(
        engine
            .presentation_authority
            .scene
            .surface(second_surface)
            .expect("second surface remains rostered")
            .stamp(),
        second_stamp_before_replacement
    );
    assert_eq!(
        match engine.workspace.node(first_tabs) {
            Some(Node::Tabs { selected, .. }) => *selected,
            _ => panic!("first bar must remain tabs"),
        },
        Some(selected_before.0),
        "opening on another surface cannot select a menu row on the old owner"
    );
    assert_eq!(
        match engine.workspace.node(second_tabs) {
            Some(Node::Tabs { selected, .. }) => *selected,
            _ => panic!("second bar must remain tabs"),
        },
        Some(selected_before.1),
        "the replacement opening edge cannot be reinterpreted as a menu row"
    );

    let first_stamp = engine
        .presentation_authority
        .scene
        .surface(first_surface)
        .expect("first surface remains rostered")
        .stamp();
    let mut changed_surfaces = BTreeSet::new();
    engine
        .settle_popup_geometry_unavailability_after_surface_contributions(
            ReducerTickId::new(701),
            &[SurfaceContributionOutcome::Unavailable {
                surface: first_surface,
                stamp: first_stamp,
                reason: SurfaceContributionUnavailableReason::PopupGeometryUnavailable {
                    session: second_session,
                    reason: PopupGeometryUnavailableReason::AnchorOutsidePlane,
                },
            }],
            &mut changed_surfaces,
        )
        .expect("a non-owner unavailable outcome must be ignored");
    assert!(changed_surfaces.is_empty());
    assert_eq!(
        engine
            .presentation_authority
            .tab_strip_states
            .active_menu()
            .map(|menu| menu.session()),
        Some(second_session),
        "a non-owner surface cannot close the current menu"
    );

    engine
        .settle_popup_geometry_unavailability_after_surface_contributions(
            ReducerTickId::new(702),
            &[SurfaceContributionOutcome::Unavailable {
                surface: first_surface,
                stamp: first_stamp,
                reason: SurfaceContributionUnavailableReason::PopupGeometryUnavailable {
                    session: first_session,
                    reason: PopupGeometryUnavailableReason::AnchorOutsidePlane,
                },
            }],
            &mut changed_surfaces,
        )
        .expect("a stale owner session must be ignored");
    assert!(changed_surfaces.is_empty());
    assert_eq!(
        engine
            .presentation_authority
            .tab_strip_states
            .active_menu()
            .map(|menu| menu.session()),
        Some(second_session),
        "an unavailable outcome for a stale session cannot close its successor"
    );

    let host = engine
        .create_presentation_host()
        .expect("cross-surface presentation host must mint");
    let first_measurements = tab_strip_reducer_measurements_for(&engine, first_surface, true);
    publish_surface_projection_with_measurements(
        &mut engine,
        host,
        first_surface,
        first_measurements,
    );
    let second_measurements = tab_strip_reducer_measurements_for(&engine, second_surface, true);
    publish_surface_projection_with_measurements(
        &mut engine,
        host,
        second_surface,
        second_measurements,
    );
    assert!(engine.interaction_projection(first_surface).is_some());
    assert!(engine.interaction_projection(second_surface).is_some());

    let first_stamp = engine
        .presentation_authority
        .scene
        .surface(first_surface)
        .expect("first surface remains rostered")
        .stamp();
    let second_stamp = engine
        .presentation_authority
        .scene
        .surface(second_surface)
        .expect("second surface remains rostered")
        .stamp();
    let stale = SurfaceContributionOutcome::Unavailable {
        surface: first_surface,
        stamp: first_stamp,
        reason: SurfaceContributionUnavailableReason::PopupGeometryUnavailable {
            session: first_session,
            reason: PopupGeometryUnavailableReason::AnchorOutsidePlane,
        },
    };
    let non_owner = SurfaceContributionOutcome::Unavailable {
        surface: first_surface,
        stamp: first_stamp,
        reason: SurfaceContributionUnavailableReason::PopupGeometryUnavailable {
            session: second_session,
            reason: PopupGeometryUnavailableReason::AnchorOutsidePlane,
        },
    };
    let exact = SurfaceContributionOutcome::Unavailable {
        surface: second_surface,
        stamp: second_stamp,
        reason: SurfaceContributionUnavailableReason::PopupGeometryUnavailable {
            session: second_session,
            reason: PopupGeometryUnavailableReason::AnchorOutsidePlane,
        },
    };
    let mut forward = engine.candidate();
    let mut reverse = engine.candidate();
    let mut forward_changed = BTreeSet::new();
    let mut reverse_changed = BTreeSet::new();
    forward
        .settle_popup_geometry_unavailability_after_surface_contributions(
            ReducerTickId::new(703),
            &[stale.clone(), non_owner.clone(), exact.clone()],
            &mut forward_changed,
        )
        .expect("exact owner unavailability must settle after earlier inert outcomes");
    reverse
        .settle_popup_geometry_unavailability_after_surface_contributions(
            ReducerTickId::new(703),
            &[exact, non_owner, stale],
            &mut reverse_changed,
        )
        .expect("exact owner unavailability must settle before later inert outcomes");
    let expected_changed = BTreeSet::from([first_surface, second_surface]);
    assert_eq!(forward_changed, expected_changed);
    assert_eq!(reverse_changed, expected_changed);
    for settled in [&forward, &reverse] {
        assert!(
            settled
                .presentation_authority
                .tab_strip_states
                .active_menu()
                .is_none()
        );
        assert!(matches!(
            settled.presentation_requirements().popup(),
            crate::tab_strip::PopupPlaneRequirement::Inactive { .. }
        ));
        assert!(settled.interaction_projection(first_surface).is_none());
        assert!(settled.interaction_projection(second_surface).is_none());
    }
    assert_eq!(
        forward.presentation_authority.tab_strip_states,
        reverse.presentation_authority.tab_strip_states
    );
    assert_eq!(
        forward.presentation_authority.presentation_requirements,
        reverse.presentation_authority.presentation_requirements
    );
    assert_eq!(
        forward.presentation_authority.scene,
        reverse.presentation_authority.scene
    );
}

#[test]
fn menu_row_selection_and_close_publish_atomically_and_stale_sessions_reject() {
    let mut fixture = tab_strip_reducer_fixture(DockPolicy::default());
    let key = TabStripStateKey::new(fixture.surface, fixture.bar);
    let measurements = tab_strip_reducer_measurements(&fixture, true);
    let automatic_plan = compile_tab_strip_reducer_plan(&fixture, &measurements);
    let maximum = automatic_plan.tab_bar_records()[0].maximum_scroll_offset();
    assert!(maximum > 0.0);
    fixture
        .engine
        .presentation_authority
        .tab_strip_states
        .set_tab_strip_scroll_offset(key, maximum)
        .expect("explicit end offset is valid");
    let closed_plan = compile_tab_strip_reducer_plan(&fixture, &measurements);
    assert_eq!(
        closed_plan.tab_bar_records()[0]
            .members()
            .first()
            .expect("fixture has a first tab")
            .visibility(),
        TabStripMemberVisibility::Hidden
    );
    let menu_control = TabStripControlId::TabListMenu(fixture.bar);
    let open = fixture
        .engine
        .finish_journal_tab_strip_control(
            tab_strip_test_cause(),
            &frozen_control(&closed_plan, key, menu_control),
            &closed_plan,
        )
        .expect("menu open must settle");
    let session = match open {
        InteractionOutcome::TabStripControlActivated {
            menu: Some(session),
            ..
        } => session,
        outcome => panic!("menu must open, got {outcome:?}"),
    };
    let menu_measurements = tab_strip_reducer_measurements(&fixture, true);
    let menu_plan = compile_tab_strip_reducer_plan(&fixture, &menu_measurements);
    let row = menu_plan
        .tab_list_menu_records()
        .iter()
        .find(|menu| menu.session() == session)
        .and_then(|menu| menu.rows().first())
        .copied()
        .expect("open menu must project its first row");
    let frozen = FrozenTabListMenuRowClick {
        session,
        record: row,
        revision: menu_plan.popup().revision(),
    };
    let before = fixture.engine.version();
    let mut events = Vec::new();
    let policy = fixture.engine.policy.clone();
    let outcome = fixture
        .engine
        .finish_journal_tab_list_menu_row(
            tab_strip_test_cause(),
            tab_strip_test_focus_causal(),
            &frozen,
            &menu_plan,
            &policy,
            &mut events,
        )
        .expect("menu row must settle atomically");
    assert!(matches!(
        outcome,
        InteractionOutcome::TabListMenuItemSelected {
            session: actual,
            tab,
            changed: true,
            ..
        } if actual == session && tab == row.tab()
    ));
    assert_ne!(fixture.engine.version(), before);
    assert_eq!(events.len(), 1);
    assert!(
        fixture
            .engine
            .presentation_authority
            .tab_strip_states
            .active_menu()
            .is_none()
    );
    assert_eq!(
        fixture
            .engine
            .presentation_authority
            .tab_strip_states
            .state(key)
            .and_then(|state| state.scroll_offset()),
        Some(maximum),
        "menu selection preserves the current effective offset for minimal reveal"
    );
    assert_eq!(
        match fixture.engine.workspace.node(fixture.bar.tabs) {
            Some(Node::Tabs { selected, .. }) => *selected,
            _ => panic!("fixture bar must remain tabs"),
        },
        Some(row.tab().item)
    );
    let selected_measurements = tab_strip_reducer_measurements(&fixture, true);
    let selected_plan = compile_tab_strip_reducer_plan(&fixture, &selected_measurements);
    assert_eq!(
        selected_plan.tab_bar_records()[0]
            .members()
            .first()
            .expect("selected first tab remains in the roster")
            .visibility(),
        TabStripMemberVisibility::Visible
    );

    let workspace_after = fixture.engine.workspace.clone();
    let outcome = fixture
        .engine
        .finish_journal_tab_list_menu_row(
            tab_strip_test_cause(),
            tab_strip_test_focus_causal(),
            &frozen,
            &menu_plan,
            &policy,
            &mut Vec::new(),
        )
        .expect("stale session must reject without mutation");
    assert_eq!(
        outcome,
        InteractionOutcome::Rejected(InteractionRejection::TabListMenuSessionUnavailable {
            session
        })
    );
    assert_eq!(fixture.engine.workspace, workspace_after);
}

#[test]
fn menu_backdrop_rejects_stale_session_and_revision_without_dismissing_current_menu() {
    let mut fixture = tab_strip_reducer_fixture(DockPolicy::default());
    let key = TabStripStateKey::new(fixture.surface, fixture.bar);
    let measurements = tab_strip_reducer_measurements(&fixture, true);
    let closed_plan = compile_tab_strip_reducer_plan(&fixture, &measurements);
    let menu_control = TabStripControlId::TabListMenu(fixture.bar);
    let first_session = match fixture
        .engine
        .finish_journal_tab_strip_control(
            tab_strip_test_cause(),
            &frozen_control(&closed_plan, key, menu_control),
            &closed_plan,
        )
        .expect("first menu open must settle")
    {
        InteractionOutcome::TabStripControlActivated {
            menu: Some(session),
            ..
        } => session,
        outcome => panic!("first menu must open, got {outcome:?}"),
    };
    let first_measurements = tab_strip_reducer_measurements(&fixture, true);
    let first_plan = compile_tab_strip_reducer_plan(&fixture, &first_measurements);
    let first_backdrop = first_plan
        .tab_list_menu_backdrop_records()
        .iter()
        .copied()
        .find(|record| record.session() == first_session)
        .expect("first menu must compile one backdrop");
    let first_frozen = FrozenTabListMenuBackdropClick {
        session: first_session,
        record: first_backdrop,
        revision: first_plan.popup().revision(),
    };

    let close_delta = fixture
        .engine
        .presentation_authority
        .tab_strip_states
        .close_tab_list_menu(first_session)
        .expect("first exact session must close");
    fixture
        .engine
        .consume_tab_strip_state_delta(tab_strip_test_cause(), &close_delta)
        .expect("first close must invalidate the popup roster");
    let (second_session, open_delta) = fixture
        .engine
        .presentation_authority
        .tab_strip_states
        .open_tab_list_menu(key, Some(fixture.items[1]))
        .expect("the live strip must open a successor session");
    fixture
        .engine
        .consume_tab_strip_state_delta(tab_strip_test_cause(), &open_delta)
        .expect("successor open must invalidate the popup roster");
    assert_ne!(second_session, first_session);
    let current_measurements = tab_strip_reducer_measurements(&fixture, true);
    let current_plan = compile_tab_strip_reducer_plan(&fixture, &current_measurements);

    let stale_session = fixture
        .engine
        .finish_journal_tab_list_menu_backdrop(tab_strip_test_cause(), &first_frozen, &current_plan)
        .expect("stale session rejection must settle");
    assert_eq!(
        stale_session,
        InteractionOutcome::Rejected(InteractionRejection::TabListMenuSessionUnavailable {
            session: first_session,
        })
    );
    assert_eq!(
        fixture
            .engine
            .presentation_authority
            .tab_strip_states
            .active_menu()
            .map(|menu| menu.session()),
        Some(second_session)
    );

    let current_backdrop = current_plan
        .tab_list_menu_backdrop_records()
        .iter()
        .copied()
        .find(|record| record.session() == second_session)
        .expect("successor menu must compile one backdrop");
    let actual_revision = current_plan.popup().revision();
    let stale_revision = PopupRoutingRevision::new_for_test(
        actual_revision
            .get()
            .checked_add(1)
            .expect("test routing revision must not exhaust"),
    );
    let stale_revision_frozen = FrozenTabListMenuBackdropClick {
        session: second_session,
        record: current_backdrop,
        revision: stale_revision,
    };
    let rejected = fixture
        .engine
        .finish_journal_tab_list_menu_backdrop(
            tab_strip_test_cause(),
            &stale_revision_frozen,
            &current_plan,
        )
        .expect("stale routing revision rejection must settle");
    assert_eq!(
        rejected,
        InteractionOutcome::Rejected(InteractionRejection::TabListMenuPopupRevisionChanged {
            session: second_session,
            expected: stale_revision,
            actual: actual_revision,
        })
    );
    assert_eq!(
        fixture
            .engine
            .presentation_authority
            .tab_strip_states
            .active_menu()
            .map(|menu| menu.session()),
        Some(second_session),
        "stale popup authority cannot dismiss the current menu"
    );
}

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
    capabilities.set_authoritative_inventory(PlatformCapability::Supported);
    test_platform_snapshot(
        capabilities,
        FocusObservationEnvelope::new(
            FocusObservationGeneration::new(1),
            Authority::Unknown(AuthorityUnavailableReason::NotReported),
            Authority::Unknown(AuthorityUnavailableReason::NotReported),
        ),
        vec![
            ObservedWindow::new(binding).with_coordinate_observation(
                WindowCoordinateObservation::new(
                    binding,
                    CoordinateObservationGeneration::new(1),
                    Authority::Known(
                        PhysicalRect::new(0.0, 0.0, 640.0, 480.0)
                            .expect("test content bounds must be valid"),
                    ),
                    Authority::Unknown(AuthorityUnavailableReason::NotReported),
                    Authority::Known(ScaleFactor::new(1.0).expect("test scale must be valid")),
                    Authority::Known(ScaleFactor::new(1.0).expect("test scale must be valid")),
                ),
            ),
        ],
        Vec::new(),
        unknown_work_area_observation(1),
    )
    .expect("test platform snapshot must be canonical")
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
    let provider_a = engine
        .create_pointer_provider(
            PointerProviderScope::SurfaceLocal(SurfaceLocalPointerScope::new(
                host,
                SurfaceLocalPointerEndpoint::Native(binding_a),
            )),
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
        Some(provider_a),
        "sealed frame admission must fail closed without mutating the ledger"
    );
}

#[test]
fn surface_local_pointer_host_is_admitted_only_when_the_prelude_is_sealed() {
    let mut engine = single_surface_engine(SOURCE_SURFACE, SOURCE_ROOT, ItemId::new(1));
    let rendering_host = engine
        .create_presentation_host()
        .expect("rendering presentation host must mint");
    let pointer_host = engine
        .create_presentation_host()
        .expect("pointer presentation host must mint");
    let provider = engine
        .create_pointer_provider(
            PointerProviderScope::SurfaceLocal(SurfaceLocalPointerScope::new(
                pointer_host,
                SurfaceLocalPointerEndpoint::Logical(SOURCE_SURFACE),
            )),
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
        }) if actual_provider == provider && actual_host == pointer_host
    ));
    assert_eq!(engine.pointer_provider(), Some(provider));
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
    let lease_a = engine
        .create_pointer_provider(
            PointerProviderScope::SurfaceLocal(SurfaceLocalPointerScope::new(
                host,
                SurfaceLocalPointerEndpoint::Native(binding_a),
            )),
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
        .submit_pointer_journal(lease_a, empty.clone())
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

    engine
        .retire_pointer_provider(lease_a)
        .expect("the stale A1 lease can be retired exactly once");
    assert_eq!(engine.pointer_provider(), None);
    let lease_b = engine
        .create_pointer_provider(
            PointerProviderScope::SurfaceLocal(SurfaceLocalPointerScope::new(
                host,
                SurfaceLocalPointerEndpoint::Native(binding_b),
            )),
            PointerEdgeSequence::new(0),
        )
        .expect("A2 pointer provider must be admitted after A1 retirement");

    let mut successor_frame = begin_test_host_frame(&engine, host);
    successor_frame
        .submit_pointer_journal(lease_b, empty)
        .expect("A2 journal must stage");
    successor_frame
        .submit_pointer_receiver_receipts(
            PointerReceiverReceiptBatch::new(Vec::<PointerReceiverReceipt>::new())
                .expect("A2 empty journal has an empty exact receipt set"),
        )
        .expect("A2 empty receipt set must stage");
    complete_host_frame_with_explicit_surface_roster(&engine, &mut successor_frame);
    successor_frame
        .finish(&mut engine)
        .expect("A2 lease must continue through the host-frame reducer");
    assert_eq!(engine.pointer_provider(), Some(lease_b));
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

fn local_pointer_journal(
    previous: u64,
    edges: impl IntoIterator<Item = (PointerEdgeKind, LogicalPoint)>,
) -> PointerEdgeJournal {
    let previous = PointerEdgeSequence::new(previous);
    let edges = edges
        .into_iter()
        .enumerate()
        .map(|(offset, (kind, position))| {
            let offset = u64::try_from(offset).expect("test edge offset must fit u64");
            let sequence = PointerEdgeSequence::new(previous.get() + offset + 1);
            PointerEdge::new(
                sequence,
                TEST_POINTER,
                kind,
                PointerEdgeLocation::SurfaceLocal {
                    position: Authority::Known(position),
                },
                Authority::Known(PointerCaptureOwner::ProviderEndpoint),
            )
        })
        .collect::<Vec<_>>();
    let committed = edges.last().map_or(previous, PointerEdge::sequence);
    PointerEdgeJournal::new(previous, committed, edges)
        .expect("test pointer journal must be contiguous")
}

fn region_center(region: &crate::presentation_hit::PresentationHitRegion) -> LogicalPoint {
    let rect = region.hit().rect();
    LogicalPoint::new(
        rect.x() + rect.width() * 0.5,
        rect.y() + rect.height() * 0.5,
    )
    .expect("test receiver center must be finite")
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
        .create_pointer_provider(
            PointerProviderScope::SurfaceLocal(SurfaceLocalPointerScope::new(
                host,
                SurfaceLocalPointerEndpoint::Logical(SOURCE_SURFACE),
            )),
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
        .submit_pointer_journal(
            provider,
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
        .submit_pointer_journal(
            provider,
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
        .create_pointer_provider(
            PointerProviderScope::SurfaceLocal(SurfaceLocalPointerScope::new(
                host,
                SurfaceLocalPointerEndpoint::Logical(SOURCE_SURFACE),
            )),
            PointerEdgeSequence::new(0),
        )
        .expect("splitter pointer provider must mint");
    let workspace_volume =
        crate::drop_resolver::structural_work::WorkspaceCloneVolume::capture(engine.workspace());

    crate::drop_resolver::structural_work::reset();
    let mut frame = begin_test_host_frame(&engine, host);
    frame
        .submit_pointer_journal(
            provider,
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
    frame
        .submit_pointer_journal(
            provider,
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
    frame
        .submit_pointer_journal(
            provider,
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
            scene_paint_hit_regions: 30,
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
    assert_eq!(
        work.workspace_deep_clones
            .multi_command_move_baselines
            .calls,
        0
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
        .create_pointer_provider(
            PointerProviderScope::SurfaceLocal(SurfaceLocalPointerScope::new(
                host,
                SurfaceLocalPointerEndpoint::Logical(SOURCE_SURFACE),
            )),
            PointerEdgeSequence::new(0),
        )
        .expect("tab pointer provider must mint");
    let owner = GestureOwner::Stream(PointerStreamId::new(provider, TEST_POINTER, 1));
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
        .create_pointer_provider(
            PointerProviderScope::SurfaceLocal(SurfaceLocalPointerScope::new(
                host,
                SurfaceLocalPointerEndpoint::Logical(SOURCE_SURFACE),
            )),
            PointerEdgeSequence::new(0),
        )
        .expect("contained pointer provider must mint");
    let owner = GestureOwner::Stream(PointerStreamId::new(provider, TEST_POINTER, 1));
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
    let predecessor = engine
        .create_platform_provider()
        .expect("predecessor platform provider must mint");
    let replacement = engine
        .begin_platform_provider_replacement(predecessor)
        .expect("platform provider replacement must begin");
    let frozen_tick = engine.last_reducer_tick();
    let frozen_frontier = engine.viewport.platform_provider_frontier();
    let mut frame = begin_test_host_frame(&engine, rendering_host);
    complete_host_frame_with_explicit_surface_roster(&engine, &mut frame);

    let successor = engine
        .finish_platform_provider_replacement(replacement.ticket())
        .expect("replacement platform provider must activate");
    let current_frontier = engine.viewport.platform_provider_frontier();
    assert!(current_frontier > frozen_frontier);
    assert_eq!(engine.last_reducer_tick(), frozen_tick);
    assert_eq!(engine.platform_provider(), Some(successor));

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
    assert_eq!(engine.platform_provider(), Some(successor));
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
        .commit();

    assert_eq!(transition.tick().get(), before_tick.get() + 1);
    assert_eq!(engine.last_reducer_tick(), transition.tick());
}

#[test]
fn host_frame_prelude_cannot_cross_platform_provider_activation() {
    let mut engine = single_surface_engine(SOURCE_SURFACE, SOURCE_ROOT, ItemId::new(1));
    let rendering_host = engine
        .create_presentation_host()
        .expect("rendering presentation host must mint");
    let predecessor = engine
        .create_platform_provider()
        .expect("predecessor platform provider must mint");
    let replacement = engine
        .begin_platform_provider_replacement(predecessor)
        .expect("platform provider replacement must begin");
    let frozen_frontier = engine.viewport.platform_provider_frontier();
    let mut prelude = engine
        .begin_host_frame(rendering_host)
        .expect("test presentation host frame must begin");
    prelude
        .submit_presentation_observation(HostPresentationObservation::NoUpdate)
        .expect("test host frame must submit an observation");

    let successor = engine
        .finish_platform_provider_replacement(replacement.ticket())
        .expect("replacement platform provider must activate");
    let current_frontier = engine.viewport.platform_provider_frontier();

    assert!(matches!(
        prelude.seal(&engine),
        Err(EngineError::HostFramePlatformProviderFrontierStale {
            submitted,
            current,
        }) if submitted == frozen_frontier.get() && current == current_frontier.get()
    ));
    assert_eq!(engine.platform_provider(), Some(successor));
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
    assert_eq!(
        emission.output().payload(),
        HostPresentationOutputPayload::Paint {
            scene: ticket,
            interaction: HostInteractionPresentation::default(),
        }
    );
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

struct FocusRevealFixture {
    engine: DockEngine,
    presentation_host: PresentationHostLease,
    tabs: crate::ids::NodeId,
    binding: ViewportBinding,
    focus_generation: u64,
}

fn focus_reveal_fixture() -> FocusRevealFixture {
    let mut builder = Workspace::builder();
    let tabs = builder.insert_node(Node::tabs([
        ItemId::new(1),
        ItemId::new(2),
        ItemId::new(3),
        ItemId::new(4),
    ]));
    builder.set_root(SOURCE_ROOT, RootRecord::new(tabs));
    builder.set_surface(SOURCE_SURFACE, SurfacePresentation::with_main(SOURCE_ROOT));
    let workspace = builder
        .build()
        .expect("focus reveal workspace must be valid");
    let mut engine =
        DockEngine::new(workspace, DockPolicy::default()).expect("engine must be valid");
    let presentation_host = engine
        .create_presentation_host()
        .expect("focus presentation host must mint");
    let token = WindowToken::new(1);
    let provider = test_platform_provider(&mut engine);
    let expected = engine.version();
    let registered = submit_test_input(
        &mut engine,
        presentation_host,
        EngineInput::RegisterViewport {
            provider,
            expected,
            surface: SOURCE_SURFACE,
            token,
            role: ViewportRole::Root,
            recovery_target: None,
        },
    )
    .expect("viewport registration must reduce");
    let InputOutcome::ViewportRegistered { binding } = registered.reduced_inputs()[0].outcome()
    else {
        panic!("viewport registration must publish its exact binding");
    };
    let binding = *binding;
    let mut fixture = FocusRevealFixture {
        engine,
        presentation_host,
        tabs,
        binding,
        focus_generation: 0,
    };
    publish_focus_snapshot(
        &mut fixture,
        Authority::Known(GlobalFocusedWindow::Dock(binding)),
    );
    fixture
}

fn publish_focus_snapshot(
    fixture: &mut FocusRevealFixture,
    focused: Authority<GlobalFocusedWindow>,
) -> EngineTransition {
    fixture.focus_generation += 1;
    let mut capabilities = PlatformCapabilities::default();
    capabilities.set_authoritative_inventory(PlatformCapability::Supported);
    capabilities.set_global_focus_observation(PlatformCapability::Supported);
    capabilities.set_window_activation_control(PlatformCapability::Supported);
    let snapshot = test_platform_snapshot(
        capabilities,
        FocusObservationEnvelope::new(
            FocusObservationGeneration::new(fixture.focus_generation),
            focused,
            Authority::Known(None),
        ),
        vec![
            ObservedWindow::new(fixture.binding).with_presentation_observation(
                WindowPresentationObservation::new(
                    fixture.binding,
                    crate::viewport::PresentationObservationGeneration::new(
                        fixture.focus_generation,
                    ),
                    Authority::Known(WindowPresentationState::Visible),
                    PresentationEffectAcknowledgement::known(None),
                ),
            ),
        ],
        Vec::new(),
        unknown_work_area_observation(fixture.focus_generation),
    )
    .expect("focus snapshot must be canonical");
    let provider = test_platform_provider(&mut fixture.engine);
    let expected_epoch = fixture.engine.version().epoch();
    let input = EngineInput::PublishPlatformSnapshot {
        provider,
        expected_epoch,
        snapshot,
    };
    let Some(pointer_provider) = fixture.engine.pointer_provider() else {
        return submit_test_input(&mut fixture.engine, fixture.presentation_host, input)
            .expect("focus snapshot must reduce");
    };
    let watermark = PointerEdgeSequence::new(0);
    let journal = PointerEdgeJournal::new(watermark, watermark, Vec::new())
        .expect("idle focus-frame journal must preserve its watermark");
    let mut stream = TestInputStream::resume(&fixture.engine, ENGINE_TEST_INPUT_SOURCE);
    let mut frame = begin_test_host_frame(&fixture.engine, fixture.presentation_host);
    frame
        .submit_pointer_journal(pointer_provider, journal)
        .expect("active pointer provider must submit every host frame");
    let receipts = frame
        .pointer_receiver_candidates()
        .expect("idle pointer journal must freeze an exact candidate roster")
        .candidates()
        .iter()
        .cloned()
        .map(|candidate| candidate.receipt(PointerReceiverObservation::NotApplicable));
    frame
        .submit_pointer_receiver_receipts(
            PointerReceiverReceiptBatch::new(receipts)
                .expect("idle pointer receipts must be exact"),
        )
        .expect("idle pointer receipts must stage");
    stream
        .append(&mut frame, input)
        .expect("focus snapshot must fit the host-frame semantic phase");
    complete_host_frame_with_explicit_surface_roster(&fixture.engine, &mut frame);
    frame
        .finish(&mut fixture.engine)
        .expect("focus snapshot with pointer authority must reduce")
}

fn establish_known_all_released_pointer_authority(fixture: &mut FocusRevealFixture) {
    let provider = fixture
        .engine
        .create_pointer_provider(
            PointerProviderScope::DesktopGlobal,
            PointerEdgeSequence::new(0),
        )
        .expect("desktop-global pointer provider must mint");
    let checkpoint = PointerAuthorityCheckpoint::known(PointerEdgeSequence::new(0), Vec::new())
        .expect("empty complete pointer checkpoint must be canonical");
    let journal = PointerEdgeJournal::new(
        PointerEdgeSequence::new(0),
        PointerEdgeSequence::new(0),
        Vec::new(),
    )
    .expect("empty pointer journal must preserve its watermark")
    .with_authority_checkpoint(checkpoint)
    .expect("checkpoint must bind the journal predecessor");
    let mut frame = begin_test_host_frame(&fixture.engine, fixture.presentation_host);
    frame
        .submit_pointer_journal(provider, journal)
        .expect("pointer checkpoint must stage");
    let receipts = frame
        .pointer_receiver_candidates()
        .expect("pointer provider must freeze an exact candidate roster")
        .candidates()
        .iter()
        .cloned()
        .map(|candidate| candidate.receipt(PointerReceiverObservation::NotApplicable));
    frame
        .submit_pointer_receiver_receipts(
            PointerReceiverReceiptBatch::new(receipts)
                .expect("empty-journal receipts must be exact"),
        )
        .expect("pointer checkpoint receipts must stage");
    complete_host_frame_with_explicit_surface_roster(&fixture.engine, &mut frame);
    frame
        .finish(&mut fixture.engine)
        .expect("complete pointer checkpoint must reduce");
}

fn selected_item(fixture: &FocusRevealFixture) -> Option<ItemId> {
    let Node::Tabs { selected, .. } = fixture
        .engine
        .workspace()
        .node(fixture.tabs)
        .expect("focus tabs must remain current")
    else {
        panic!("focus fixture node must remain tabs");
    };
    *selected
}

struct FocusEffectFixture {
    engine: DockEngine,
    presentation_host: PresentationHostLease,
    binding_a: ViewportBinding,
    binding_b: ViewportBinding,
    platform_generation: u64,
    focus_generation: u64,
}

fn focus_effect_fixture() -> FocusEffectFixture {
    let mut builder = Workspace::builder();
    let tabs_a = builder.insert_node(Node::tabs([ItemId::new(1)]));
    let tabs_b = builder.insert_node(Node::tabs([ItemId::new(2)]));
    builder.set_root(SOURCE_ROOT, RootRecord::new(tabs_a));
    builder.set_root(TARGET_ROOT, RootRecord::new(tabs_b));
    builder.set_surface(SOURCE_SURFACE, SurfacePresentation::with_main(SOURCE_ROOT));
    builder.set_surface(TARGET_SURFACE, SurfacePresentation::with_main(TARGET_ROOT));
    let workspace = builder
        .build()
        .expect("focus effect workspace must be valid");
    let mut engine =
        DockEngine::new(workspace, DockPolicy::default()).expect("engine must be valid");
    let presentation_host = engine
        .create_presentation_host()
        .expect("focus presentation host must mint");
    let provider = test_platform_provider(&mut engine);
    let expected = engine.version();
    submit_test_batch(
        &mut engine,
        presentation_host,
        [
            EngineInput::RegisterViewport {
                provider,
                expected,
                surface: SOURCE_SURFACE,
                token: WindowToken::new(1),
                role: ViewportRole::Root,
                recovery_target: None,
            },
            EngineInput::RegisterViewport {
                provider,
                expected,
                surface: TARGET_SURFACE,
                token: WindowToken::new(2),
                role: ViewportRole::Root,
                recovery_target: None,
            },
        ],
    )
    .expect("viewport registrations must reduce");
    let binding_a = engine
        .viewport()
        .viewport(SOURCE_SURFACE)
        .expect("first viewport must be current")
        .binding();
    let binding_b = engine
        .viewport()
        .viewport(TARGET_SURFACE)
        .expect("second viewport must be current")
        .binding();
    let mut fixture = FocusEffectFixture {
        engine,
        presentation_host,
        binding_a,
        binding_b,
        platform_generation: 0,
        focus_generation: 0,
    };
    publish_effect_focus_snapshot(
        &mut fixture,
        Authority::Known(GlobalFocusedWindow::Foreign),
        Authority::Known(None),
    );
    fixture
}

fn publish_effect_focus_snapshot(
    fixture: &mut FocusEffectFixture,
    focused: Authority<GlobalFocusedWindow>,
    acknowledged_effect: Authority<Option<crate::effect::EffectId>>,
) -> EngineTransition {
    fixture.focus_generation += 1;
    publish_effect_focus_snapshot_at(
        fixture,
        fixture.focus_generation,
        focused,
        acknowledged_effect,
    )
}

fn publish_effect_focus_snapshot_at(
    fixture: &mut FocusEffectFixture,
    generation: u64,
    focused: Authority<GlobalFocusedWindow>,
    acknowledged_effect: Authority<Option<crate::effect::EffectId>>,
) -> EngineTransition {
    let mut capabilities = PlatformCapabilities::default();
    capabilities.set_authoritative_inventory(PlatformCapability::Supported);
    capabilities.set_global_focus_observation(PlatformCapability::Supported);
    capabilities.set_window_activation_control(PlatformCapability::Supported);
    fixture.platform_generation += 1;
    let provider_generation = fixture.platform_generation;
    let snapshot = test_platform_snapshot_at(
        provider_generation,
        capabilities,
        FocusObservationEnvelope::new(
            FocusObservationGeneration::new(generation),
            focused,
            acknowledged_effect,
        ),
        vec![
            ObservedWindow::new(fixture.binding_a).with_presentation_observation(
                WindowPresentationObservation::new(
                    fixture.binding_a,
                    crate::viewport::PresentationObservationGeneration::new(generation),
                    Authority::Known(WindowPresentationState::Visible),
                    PresentationEffectAcknowledgement::known(None),
                ),
            ),
            ObservedWindow::new(fixture.binding_b).with_presentation_observation(
                WindowPresentationObservation::new(
                    fixture.binding_b,
                    crate::viewport::PresentationObservationGeneration::new(generation),
                    Authority::Known(WindowPresentationState::Visible),
                    PresentationEffectAcknowledgement::known(None),
                ),
            ),
        ],
        Vec::new(),
        unknown_work_area_observation(provider_generation),
    )
    .expect("focus effect snapshot must be canonical");
    let provider = test_platform_provider(&mut fixture.engine);
    let expected_epoch = fixture.engine.version().epoch();
    submit_test_input(
        &mut fixture.engine,
        fixture.presentation_host,
        EngineInput::PublishPlatformSnapshot {
            provider,
            expected_epoch,
            snapshot,
        },
    )
    .expect("focus effect snapshot must reduce")
}

fn request_focus_effect(
    fixture: &mut FocusEffectFixture,
    binding: ViewportBinding,
    focus: PanelFocus,
) -> (
    crate::effect::EffectId,
    crate::viewport_focus::ActivationGeneration,
) {
    let expected = fixture.engine.version();
    let transition = submit_test_input(
        &mut fixture.engine,
        fixture.presentation_host,
        EngineInput::ActivateViewport {
            expected,
            request: ViewportActivationRequest::explicit(binding, focus),
        },
    )
    .expect("activation must reduce");
    let activation = transition
        .reduced_inputs()
        .iter()
        .find_map(|input| match input.outcome() {
            InputOutcome::ViewportActivationRequested { activation } => Some(*activation),
            _ => None,
        })
        .expect("activation input must publish its generation");
    let effect = transition
        .platform_effects()
        .iter()
        .find_map(|request| match request.effect() {
            crate::effect::PlatformEffect::RequestFocus {
                binding: requested, ..
            } if *requested == binding => Some(request.id()),
            _ => None,
        })
        .expect("activation must emit one exact focus effect");
    (effect, activation.generation())
}

fn observed_focus_effect_ids(transition: &EngineTransition) -> Vec<crate::effect::EffectId> {
    transition
        .focus_delta()
        .effects()
        .iter()
        .filter_map(|change| {
            change
                .observed()
                .map(crate::viewport_focus::ObservedPlatformFocusEffect::effect)
        })
        .collect()
}

fn assert_focus_effect_observed(engine: &DockEngine, effect: crate::effect::EffectId) {
    assert!(matches!(
        engine
            .viewport()
            .effects()
            .record(effect)
            .map(crate::effect::EffectRecord::phase),
        Some(crate::effect::EffectPhase::ObservedApplied { .. })
    ));
}

#[test]
fn late_superseded_focus_ack_settles_only_its_effect_before_successor_completion() {
    let mut fixture = focus_effect_fixture();
    let binding_a = fixture.binding_a;
    let binding_b = fixture.binding_b;
    let (effect_a, activation_a) =
        request_focus_effect(&mut fixture, binding_a, PanelFocus::Item(ItemId::new(1)));
    let (effect_b, activation_b) =
        request_focus_effect(&mut fixture, binding_b, PanelFocus::Item(ItemId::new(2)));

    let late_a = publish_effect_focus_snapshot(
        &mut fixture,
        Authority::Known(GlobalFocusedWindow::Foreign),
        Authority::Known(Some(effect_a)),
    );
    assert_eq!(observed_focus_effect_ids(&late_a), vec![effect_a]);
    assert_focus_effect_observed(&fixture.engine, effect_a);
    assert_eq!(
        fixture
            .engine
            .viewport_focus()
            .pending_activation()
            .map(crate::viewport_focus::PendingViewportActivation::generation),
        Some(activation_b),
        "late predecessor acknowledgement must not complete or cancel the successor"
    );
    assert!(
        fixture
            .engine
            .viewport_focus()
            .pending_pane_intent()
            .is_none()
    );

    let completed_b = publish_effect_focus_snapshot(
        &mut fixture,
        Authority::Known(GlobalFocusedWindow::Dock(binding_b)),
        Authority::Known(Some(effect_b)),
    );
    assert_eq!(observed_focus_effect_ids(&completed_b), vec![effect_b]);
    assert_focus_effect_observed(&fixture.engine, effect_b);
    assert!(
        fixture
            .engine
            .viewport_focus()
            .pending_activation()
            .is_none()
    );
    let intent = fixture
        .engine
        .viewport_focus()
        .pending_pane_intent()
        .expect("successor target observation must install its pane intent");
    assert_eq!(intent.activation(), Some(activation_b));
    assert_eq!(intent.target(), binding_b);
    assert_eq!(intent.focus(), PanelFocus::Item(ItemId::new(2)));
    assert_ne!(intent.activation(), Some(activation_a));
}

#[test]
fn one_envelope_can_settle_late_predecessor_and_complete_current_target() {
    let mut fixture = focus_effect_fixture();
    let binding_a = fixture.binding_a;
    let binding_b = fixture.binding_b;
    let (effect_a, _) =
        request_focus_effect(&mut fixture, binding_a, PanelFocus::Item(ItemId::new(1)));
    let (effect_b, activation_b) =
        request_focus_effect(&mut fixture, binding_b, PanelFocus::Item(ItemId::new(2)));

    let transition = publish_effect_focus_snapshot(
        &mut fixture,
        Authority::Known(GlobalFocusedWindow::Dock(binding_b)),
        Authority::Known(Some(effect_a)),
    );
    assert_eq!(
        observed_focus_effect_ids(&transition),
        vec![effect_a, effect_b],
        "FocusDelta must retain both exact-A and target-B settlements in effect order"
    );
    assert_focus_effect_observed(&fixture.engine, effect_a);
    assert_focus_effect_observed(&fixture.engine, effect_b);
    assert_eq!(
        transition.focus_delta().effects()[0]
            .observed()
            .map(crate::viewport_focus::ObservedPlatformFocusEffect::evidence),
        Some(crate::viewport_focus::PlatformFocusEvidence::ExactEffectAcknowledgement)
    );
    assert_eq!(
        transition.focus_delta().effects()[1]
            .observed()
            .map(crate::viewport_focus::ObservedPlatformFocusEffect::evidence),
        Some(crate::viewport_focus::PlatformFocusEvidence::NewerMatchingObservation)
    );
    let intent = fixture
        .engine
        .viewport_focus()
        .pending_pane_intent()
        .expect("current target evidence must complete the successor");
    assert_eq!(intent.activation(), Some(activation_b));
    assert_eq!(intent.target(), binding_b);
}

#[test]
fn wrong_stale_incarnation_and_duplicate_focus_acks_do_not_settle_again() {
    let mut fixture = focus_effect_fixture();
    let binding_a = fixture.binding_a;
    let binding_b = fixture.binding_b;
    let (effect_a, _) = request_focus_effect(&mut fixture, binding_a, PanelFocus::None);
    let (_, activation_b) = request_focus_effect(&mut fixture, binding_b, PanelFocus::None);

    let wrong = publish_effect_focus_snapshot(
        &mut fixture,
        Authority::Known(GlobalFocusedWindow::Foreign),
        Authority::Known(Some(crate::effect::EffectId::new(9_999))),
    );
    assert!(observed_focus_effect_ids(&wrong).is_empty());
    assert_eq!(
        fixture
            .engine
            .viewport()
            .effects()
            .record(effect_a)
            .map(crate::effect::EffectRecord::phase),
        Some(crate::effect::EffectPhase::Requested)
    );

    let stale_generation = fixture.focus_generation - 1;
    let stale = publish_effect_focus_snapshot_at(
        &mut fixture,
        stale_generation,
        Authority::Known(GlobalFocusedWindow::Foreign),
        Authority::Known(Some(effect_a)),
    );
    assert!(observed_focus_effect_ids(&stale).is_empty());
    assert_eq!(
        fixture
            .engine
            .viewport()
            .effects()
            .record(effect_a)
            .map(crate::effect::EffectRecord::phase),
        Some(crate::effect::EffectPhase::Requested)
    );

    let first = publish_effect_focus_snapshot(
        &mut fixture,
        Authority::Known(GlobalFocusedWindow::Foreign),
        Authority::Known(Some(effect_a)),
    );
    assert_eq!(observed_focus_effect_ids(&first), vec![effect_a]);
    let duplicate = publish_effect_focus_snapshot(
        &mut fixture,
        Authority::Known(GlobalFocusedWindow::Foreign),
        Authority::Known(Some(effect_a)),
    );
    assert!(observed_focus_effect_ids(&duplicate).is_empty());
    assert_eq!(
        fixture
            .engine
            .viewport_focus()
            .pending_activation()
            .map(crate::viewport_focus::PendingViewportActivation::generation),
        Some(activation_b)
    );

    let stale_binding = ViewportBinding::new(
        binding_a.authority_domain(),
        binding_a.epoch(),
        binding_a.surface(),
        binding_a.token(),
        WindowIncarnation::new(binding_a.incarnation().get() + 1),
    );
    let stale_effect = fixture
        .engine
        .viewport
        .request_focus_binding(stale_binding)
        .expect("stale-incarnation effect must allocate for the invariant test");
    let stale_incarnation = publish_effect_focus_snapshot(
        &mut fixture,
        Authority::Known(GlobalFocusedWindow::Foreign),
        Authority::Known(Some(stale_effect)),
    );
    assert!(observed_focus_effect_ids(&stale_incarnation).is_empty());
    assert_eq!(
        fixture
            .engine
            .viewport()
            .effects()
            .record(stale_effect)
            .map(crate::effect::EffectRecord::phase),
        Some(crate::effect::EffectPhase::Requested)
    );
}

#[test]
fn explicit_focus_atomically_reveals_hidden_item_without_acknowledging_it() {
    let mut fixture = focus_reveal_fixture();
    assert_eq!(selected_item(&fixture), Some(ItemId::new(1)));

    let expected = fixture.engine.version();
    let transition = submit_test_input(
        &mut fixture.engine,
        fixture.presentation_host,
        EngineInput::ActivateViewport {
            expected,
            request: ViewportActivationRequest::explicit(
                fixture.binding,
                PanelFocus::Item(ItemId::new(2)),
            ),
        },
    )
    .expect("explicit activation must reduce");

    assert_eq!(selected_item(&fixture), Some(ItemId::new(2)));
    assert!(
        fixture
            .engine
            .viewport_focus()
            .pending_pane_intent()
            .is_some()
    );
    assert_eq!(
        fixture.engine.viewport_focus().panel_focus(SOURCE_SURFACE),
        PanelFocusRecord::NoHistory,
        "selection and pane rendering cannot acknowledge focus"
    );
    assert!(transition.focus_delta().pane_intent().is_some());
}

#[test]
fn explicit_focus_minimally_reveals_from_the_current_strip_scroll() {
    let mut fixture = focus_reveal_fixture();
    let bar = TabBarSceneId {
        root: SOURCE_ROOT,
        tabs: fixture.tabs,
    };
    let key = TabStripStateKey::new(SOURCE_SURFACE, bar);
    fixture
        .engine
        .presentation_authority
        .tab_strip_states
        .set_tab_strip_scroll_offset(key, 0.0)
        .expect("explicit beginning offset is valid");
    let measurements = tab_strip_reducer_measurements_for(&fixture.engine, SOURCE_SURFACE, true);
    let before = compile_tab_strip_reducer_plan_for(&fixture.engine, SOURCE_SURFACE, &measurements);
    assert_eq!(
        before.tab_bar_records()[0]
            .members()
            .iter()
            .find(|member| member.tab().item == ItemId::new(4))
            .expect("focus target remains in the complete roster")
            .visibility(),
        TabStripMemberVisibility::Hidden
    );

    let expected = fixture.engine.version();
    let transition = submit_test_input(
        &mut fixture.engine,
        fixture.presentation_host,
        EngineInput::ActivateViewport {
            expected,
            request: ViewportActivationRequest::explicit(
                fixture.binding,
                PanelFocus::Item(ItemId::new(4)),
            ),
        },
    )
    .expect("explicit focus must reduce");
    assert_eq!(selected_item(&fixture), Some(ItemId::new(4)));
    assert!(transition.focus_delta().pane_intent().is_some());
    assert_eq!(
        fixture
            .engine
            .presentation_authority
            .tab_strip_states
            .state(key)
            .and_then(|state| state.scroll_offset()),
        Some(0.0)
    );

    let measurements = tab_strip_reducer_measurements_for(&fixture.engine, SOURCE_SURFACE, true);
    let after = compile_tab_strip_reducer_plan_for(&fixture.engine, SOURCE_SURFACE, &measurements);
    assert_eq!(
        after.tab_bar_records()[0]
            .members()
            .iter()
            .find(|member| member.tab().item == ItemId::new(4))
            .expect("focused target remains in the complete roster")
            .visibility(),
        TabStripMemberVisibility::Visible
    );
}

#[test]
fn platform_restore_reveals_the_exact_hidden_focus_history_item() {
    let mut fixture = focus_reveal_fixture();
    let expected_epoch = fixture.engine.version().epoch();
    submit_test_input(
        &mut fixture.engine,
        fixture.presentation_host,
        EngineInput::PublishPaneFocusObservation {
            expected_epoch,
            observation: PaneFocusObservation::new(
                PaneFocusObservationGeneration::new(1),
                fixture.binding,
                PanelFocus::Item(ItemId::new(2)),
            ),
        },
    )
    .expect("pane focus history must reduce");
    let source = fixture
        .engine
        .workspace()
        .capture_item_source(SOURCE_ROOT, fixture.tabs, ItemId::new(1))
        .expect("first item source must be current");
    let expected = fixture.engine.version();
    submit_test_input(
        &mut fixture.engine,
        fixture.presentation_host,
        EngineInput::WorkspaceCommand {
            expected,
            command: WorkspaceCommand::Select { source },
        },
    )
    .expect("selection must reduce");
    assert_eq!(selected_item(&fixture), Some(ItemId::new(1)));

    publish_focus_snapshot(&mut fixture, Authority::Known(GlobalFocusedWindow::Foreign));
    establish_known_all_released_pointer_authority(&mut fixture);
    let binding = fixture.binding;
    let restored = publish_focus_snapshot(
        &mut fixture,
        Authority::Known(GlobalFocusedWindow::Dock(binding)),
    );

    assert_eq!(selected_item(&fixture), Some(ItemId::new(2)));
    assert!(
        fixture
            .engine
            .viewport_focus()
            .pending_pane_intent()
            .is_some()
    );
    assert!(restored.focus_delta().pane_intent().is_some());
}

struct PayloadFocusFixture {
    engine: DockEngine,
    binding: ViewportBinding,
    item_payload: MovePayload,
    stale_item_payload: MovePayload,
    tabs_payload: MovePayload,
    subtree_payload: MovePayload,
}

fn payload_focus_fixture() -> PayloadFocusFixture {
    let mut builder = Workspace::builder();
    let left_tabs = builder.insert_node(Node::tabs([ItemId::new(1), ItemId::new(3)]));
    let right_tabs = builder.insert_node(Node::tabs([ItemId::new(4)]));
    let source_split = builder.insert_node(
        Node::split(
            crate::graph::Axis::Horizontal,
            [left_tabs, right_tabs],
            [0.5, 0.5],
        )
        .expect("source split must be valid"),
    );
    let target_tabs = builder.insert_node(Node::tabs([ItemId::new(2)]));
    builder.set_root(SOURCE_ROOT, RootRecord::new(source_split));
    builder.set_root(TARGET_ROOT, RootRecord::new(target_tabs));
    builder.set_surface(SOURCE_SURFACE, SurfacePresentation::with_main(SOURCE_ROOT));
    builder.set_surface(TARGET_SURFACE, SurfacePresentation::with_main(TARGET_ROOT));
    let workspace = builder
        .build()
        .expect("payload focus workspace must be valid");
    let item_payload = MovePayload::Item(
        workspace
            .capture_item_source(SOURCE_ROOT, left_tabs, ItemId::new(1))
            .expect("item payload must be current"),
    );
    let tabs_payload = MovePayload::Tabs(
        workspace
            .capture_node_source(SOURCE_ROOT, left_tabs)
            .expect("tabs payload must be current"),
    );
    let subtree_payload = MovePayload::Subtree(
        workspace
            .capture_node_source(SOURCE_ROOT, source_split)
            .expect("subtree payload must be current"),
    );
    let mut stale_item_payload = item_payload.clone();
    let wrong_fingerprint = workspace
        .capture_node_source(TARGET_ROOT, target_tabs)
        .expect("target source must be current")
        .fingerprint()
        .clone();
    let MovePayload::Item(stale_source) = &mut stale_item_payload else {
        unreachable!("fixture creates an item payload");
    };
    stale_source.fingerprint = wrong_fingerprint;

    let engine = DockEngine::new(workspace, DockPolicy::default()).expect("engine must be valid");
    let binding = ViewportBinding::new(
        engine.authority_domain,
        WorkspaceEpoch::new(0),
        SOURCE_SURFACE,
        WindowToken::new(1),
        WindowIncarnation::new(1),
    );
    PayloadFocusFixture {
        engine,
        binding,
        item_payload,
        stale_item_payload,
        tabs_payload,
        subtree_payload,
    }
}

fn record_payload_focus(
    engine: &mut DockEngine,
    binding: ViewportBinding,
    observation_generation: &mut u64,
    focus: PanelFocus,
) {
    *observation_generation += 1;
    assert!(matches!(
        engine.viewport_focus.publish_pane_focus_observation(
            PaneFocusObservation::new(
                PaneFocusObservationGeneration::new(*observation_generation),
                binding,
                focus,
            ),
            |candidate| candidate == binding,
            |surface, item| {
                surface == SOURCE_SURFACE
                    && [ItemId::new(1), ItemId::new(3), ItemId::new(4)].contains(&item)
            },
        ),
        PaneFocusObservationTransition::Applied { .. }
    ));
}

#[test]
fn payload_focus_is_frozen_only_for_an_exact_payload_member() {
    let PayloadFocusFixture {
        mut engine,
        binding,
        item_payload,
        stale_item_payload,
        tabs_payload,
        subtree_payload,
    } = payload_focus_fixture();
    let mut observation_generation = 0_u64;

    assert_eq!(
        engine
            .freeze_payload_focus(&item_payload)
            .expect("current payload must freeze"),
        PaneFocusDisposition::Preserve,
        "missing pane-focus history must remain an explicit no-op"
    );

    record_payload_focus(
        &mut engine,
        binding,
        &mut observation_generation,
        PanelFocus::Item(ItemId::new(1)),
    );
    assert_eq!(
        engine
            .freeze_payload_focus(&item_payload)
            .expect("current payload must freeze"),
        PaneFocusDisposition::Set(ItemId::new(1))
    );
    assert!(
        engine.freeze_payload_focus(&stale_item_payload).is_err(),
        "a stale payload must reject instead of fabricating a clear-focus command"
    );

    record_payload_focus(
        &mut engine,
        binding,
        &mut observation_generation,
        PanelFocus::Item(ItemId::new(3)),
    );
    assert_eq!(
        engine
            .freeze_payload_focus(&item_payload)
            .expect("current payload must freeze"),
        PaneFocusDisposition::Clear,
        "an inactive item drag must not invent pane focus"
    );
    assert_eq!(
        engine
            .freeze_payload_focus(&tabs_payload)
            .expect("current payload must freeze"),
        PaneFocusDisposition::Set(ItemId::new(3))
    );

    record_payload_focus(
        &mut engine,
        binding,
        &mut observation_generation,
        PanelFocus::Item(ItemId::new(4)),
    );
    assert_eq!(
        engine
            .freeze_payload_focus(&tabs_payload)
            .expect("current payload must freeze"),
        PaneFocusDisposition::Clear,
        "a sibling item on the same surface is outside the exact tabs payload"
    );
    assert_eq!(
        engine
            .freeze_payload_focus(&subtree_payload)
            .expect("current payload must freeze"),
        PaneFocusDisposition::Set(ItemId::new(4))
    );

    record_payload_focus(
        &mut engine,
        binding,
        &mut observation_generation,
        PanelFocus::None,
    );
    assert_eq!(
        engine
            .freeze_payload_focus(&subtree_payload)
            .expect("current payload must freeze"),
        PaneFocusDisposition::Clear
    );
}

#[test]
fn private_focus_generations_do_not_publish_state_or_focus_delta() {
    let mut fixture = counter_fixture();
    publish_counter_scene(&mut fixture);
    let stale_binding = ViewportBinding::new(
        fixture.engine.authority_domain,
        WorkspaceEpoch::new(0),
        SOURCE_SURFACE,
        WindowToken::new(99),
        WindowIncarnation::new(99),
    );
    let expected = fixture.engine.version();
    let transition = submit_test_input(
        &mut fixture.engine,
        fixture.presentation_host,
        EngineInput::ActivateViewport {
            expected,
            request: ViewportActivationRequest::explicit(stale_binding, PanelFocus::None),
        },
    )
    .expect("suppressed activation must reduce");
    let InputOutcome::ViewportActivationRequested { activation } =
        transition.reduced_inputs()[0].outcome()
    else {
        panic!("explicit activation must produce an activation outcome");
    };
    assert_eq!(
        activation.outcome(),
        ActivationStartOutcome::Suppressed(
            crate::viewport_focus::ActivationSuppression::StaleBinding
        )
    );
    assert!(transition.focus_delta().is_empty());
    assert!(!transition.published_state_changed());

    let maintenance = submit_test_input(
        &mut fixture.engine,
        fixture.presentation_host,
        EngineInput::ValidateWorkspace,
    )
    .expect("maintenance input must reduce");
    assert!(maintenance.focus_delta().is_empty());
    assert!(!maintenance.published_state_changed());
}

#[test]
fn action_batch_barrier_allows_an_unrelated_target_mutation() {
    let mut fixture = counter_fixture();
    let before = fixture.engine.workspace().clone();
    let roster =
        SurfaceRosterDisposition::capture(fixture.engine.workspace(), SOURCE_SURFACE, None)
            .expect("direct edge roster must freeze without coordinate authority");
    let barrier = BTreeMap::from([(SOURCE_SURFACE, roster)]);
    let target = fixture
        .engine
        .workspace()
        .capture_tab_target(TARGET_ROOT, fixture.target_tabs)
        .expect("target tabs must be current");
    let mut events = Vec::new();

    let application = fixture
        .engine
        .apply_interaction_command_with_barrier(
            InputSequence::new(1),
            &WorkspaceCommand::Open {
                item: ItemId::new(99),
                target: crate::command::DockTarget::Center(target),
            },
            Some(&barrier),
            &mut events,
        )
        .expect("unrelated target mutation must reduce");

    assert!(matches!(
        application,
        CommandApplication::Applied { changed: true, .. }
    ));
    assert_ne!(fixture.engine.workspace(), &before);
    assert!(!events.is_empty());
}

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

#[test]
fn zero_area_ready_surface_is_rejected_and_cannot_authorize_contained_placement() {
    let mut fixture = counter_fixture();
    let contribution = begin_surface_measurement(
        &fixture.engine,
        SOURCE_SURFACE,
        LogicalRect::new(0.0, 0.0, 0.0, 100.0)
            .expect("zero-width bounds remain representable geometry"),
    );
    let transition =
        submit_surface_measurement(&mut fixture.engine, fixture.presentation_host, contribution);
    assert!(matches!(
        transition
            .surface_contributions()
            .iter()
            .find(|outcome| outcome.surface() == SOURCE_SURFACE),
        Some(SurfaceContributionOutcome::Unavailable {
            surface: SOURCE_SURFACE,
            reason: SurfaceContributionUnavailableReason::EmptyBounds,
            ..
        })
    ));

    assert_eq!(
        fixture.engine.contained_placement(
            SOURCE_SURFACE,
            test_rect(),
            LogicalSize::new(0.0, 0.0).expect("minimum size must be valid"),
        ),
        Err(ContainedPlacementUnavailable::BootstrapSurface {
            surface: SOURCE_SURFACE,
        })
    );
}

#[test]
fn contained_clamp_never_authorizes_a_non_positive_durable_rect() {
    let minimum = LogicalSize::new(0.0, 0.0).expect("minimum size must be valid");
    for requested in [
        LogicalRect::new(0.0, 0.0, 0.0, 40.0)
            .expect("zero-width request remains representable geometry"),
        LogicalRect::new(0.0, 0.0, 40.0, 0.0)
            .expect("zero-height request remains representable geometry"),
    ] {
        assert_eq!(
            clamp_contained_rect(SOURCE_SURFACE, test_rect(), requested, minimum),
            Err(ContainedPlacementUnavailable::UnrepresentableGeometry {
                surface: SOURCE_SURFACE,
            })
        );
    }

    let huge_bounds = LogicalRect::from_min_max(
        LogicalPoint::new(f64::MAX / 2.0, 0.0).expect("minimum corner must be finite"),
        LogicalPoint::new(f64::MAX, 100.0).expect("maximum corner must be finite"),
    )
    .expect("huge positive bounds must remain representable");
    assert_eq!(
        clamp_contained_rect(
            SOURCE_SURFACE,
            huge_bounds,
            LogicalRect::new(0.0, 0.0, 1.0, 40.0).expect("positive request must be valid"),
            minimum,
        ),
        Err(ContainedPlacementUnavailable::UnrepresentableGeometry {
            surface: SOURCE_SURFACE,
        })
    );
}

fn background_bounds() -> LogicalRect {
    LogicalRect::new(0.0, 0.0, 400.0, 300.0).expect("background bounds must be valid")
}

fn background_contained_rect() -> LogicalRect {
    LogicalRect::new(220.0, 160.0, 120.0, 120.0).expect("contained rectangle must be valid")
}

fn background_fixture(policy: DockPolicy) -> BackgroundFixture {
    let mut builder = Workspace::builder();
    let source_tabs = builder.insert_node(Node::tabs([ItemId::new(1), ItemId::new(2)]));
    let contained_tabs = builder.insert_node(Node::tabs([ItemId::new(3)]));
    let contained = FloatingPresentationId::new(2);
    builder.set_root(SOURCE_ROOT, RootRecord::new(source_tabs));
    builder.set_root(TARGET_ROOT, RootRecord::new(contained_tabs));
    builder.set_surface(SOURCE_SURFACE, SurfacePresentation::with_main(SOURCE_ROOT));
    builder.set_surface(TARGET_SURFACE, SurfacePresentation::rootless());
    builder.set_contained_floating(
        contained,
        ContainedFloating::new(TARGET_ROOT, background_contained_rect()),
    );
    builder
        .attach_contained(TARGET_SURFACE, contained)
        .expect("rootless target surface must exist");
    let workspace = builder.build().expect("background workspace must be valid");
    let mut engine = DockEngine::new(workspace, policy).expect("background engine must be valid");
    let presentation_host = engine
        .create_presentation_host()
        .expect("background presentation host must mint");
    let mut fixture = BackgroundFixture {
        engine,
        presentation_host,
        source_tabs,
    };
    publish_background_scene(&mut fixture);
    fixture
}

fn publish_background_scene(fixture: &mut BackgroundFixture) {
    publish_surface_projection(
        &mut fixture.engine,
        fixture.presentation_host,
        SOURCE_SURFACE,
        background_bounds(),
    );
    publish_surface_projection(
        &mut fixture.engine,
        fixture.presentation_host,
        TARGET_SURFACE,
        background_bounds(),
    );
}

#[test]
fn prepared_surface_plan_is_compiled_once_and_installed_without_recompilation() {
    let mut fixture = background_fixture(DockPolicy::default());
    crate::scene_compiler::reset_surface_compilation_count();
    let token = fixture
        .engine
        .begin_surface_contribution(SOURCE_SURFACE)
        .expect("source surface contribution begins");
    let measurements = surface_measurements(&fixture.engine, SOURCE_SURFACE, background_bounds());

    let prepared = fixture
        .engine
        .prepare_surface_contribution(token, measurements)
        .expect("complete measurements prepare successfully");

    assert!(matches!(
        prepared.paint_candidate(),
        PreparedSurfacePaintCandidate::Ready(_)
    ));
    assert_eq!(crate::scene_compiler::surface_compilation_count(), 1);
    let transition =
        submit_surface_measurement(&mut fixture.engine, fixture.presentation_host, prepared);
    assert!(matches!(
        transition
            .surface_contributions()
            .iter()
            .find(|outcome| outcome.surface() == SOURCE_SURFACE),
        Some(SurfaceContributionOutcome::Ready {
            surface: SOURCE_SURFACE,
            ..
        })
    ));
    assert_eq!(
        crate::scene_compiler::surface_compilation_count(),
        1,
        "reduction must install the prepared plan rather than compiling it again"
    );
}

fn background_payload(fixture: &BackgroundFixture, item: ItemId) -> MovePayload {
    MovePayload::Item(
        fixture
            .engine
            .workspace()
            .capture_item_source(SOURCE_ROOT, fixture.source_tabs, item)
            .expect("background source item must be current"),
    )
}

#[test]
fn engine_presented_drop_reuses_the_requirement_workspace_index() {
    let fixture = background_fixture(DockPolicy::default());
    let source = background_payload(&fixture, ItemId::new(1));
    let projection = fixture
        .engine
        .interaction_projection(TARGET_SURFACE)
        .expect("rootless target projection must be receiver-authoritative");
    let presentation = JournalSurfacePresentation::from_interaction(projection);
    let point = LogicalPoint::new(10.0, 10.0).expect("background point must be finite");

    crate::drop_resolver::structural_work::reset();
    let query = fixture
        .engine
        .resolve_presented_drop_with_current_index(
            &presentation,
            fixture.engine.policy_snapshot(),
            DragSessionId::new(fixture.engine.version().epoch(), DragGeneration::new(1)),
            source,
            Some(crate::intent::SurfaceBackgroundRootOffer::new(RootId::new(
                1_000,
            ))),
            point,
        )
        .expect("current engine presentation must resolve without rebuilding its index");
    assert!(matches!(
        query.resolution(),
        DropResolution::Resolved(resolved)
            if resolved.target_id()
                == crate::drop_target::DropTargetId::SurfaceBackground {
                    surface: TARGET_SURFACE,
                }
    ));

    let work = crate::drop_resolver::structural_work::snapshot();
    assert_eq!(work.drop_targets_assessed, 1);
    assert_eq!(work.geometric_winners, 1);
    assert_eq!(work.transaction_prepares, 1);
    assert_eq!(work.root_fingerprint_builds, 2);
    assert_eq!(work.root_fingerprint_node_visits, 2);
}

struct DurableRectFixture {
    engine: DockEngine,
    presentation_host: PresentationHostLease,
    surface: SurfaceId,
    first: FloatingPresentationId,
    second: FloatingPresentationId,
}

fn durable_rect_fixture() -> DurableRectFixture {
    let surface = SurfaceId::new(60);
    let first = FloatingPresentationId::new(62);
    let second = FloatingPresentationId::new(61);
    let first_root = RootId::new(62);
    let second_root = RootId::new(61);
    let mut builder = Workspace::builder();
    let first_tabs = builder.insert_node(Node::tabs([ItemId::new(62)]));
    let second_tabs = builder.insert_node(Node::tabs([ItemId::new(61)]));
    builder.set_root(first_root, RootRecord::new(first_tabs));
    builder.set_root(second_root, RootRecord::new(second_tabs));
    builder.set_surface(surface, SurfacePresentation::rootless());
    builder.set_contained_floating(
        first,
        ContainedFloating::new(
            first_root,
            LogicalRect::new(220.0, 160.0, 120.0, 120.0)
                .expect("first contained rectangle must be valid"),
        ),
    );
    builder.set_contained_floating(
        second,
        ContainedFloating::new(
            second_root,
            LogicalRect::new(150.0, 100.0, 120.0, 120.0)
                .expect("second contained rectangle must be valid"),
        ),
    );
    builder
        .attach_contained(surface, first)
        .expect("first contained presentation must attach");
    builder
        .attach_contained(surface, second)
        .expect("second contained presentation must attach");
    let workspace = builder
        .build()
        .expect("durable-rectangle workspace must be valid");
    let mut engine = DockEngine::new(workspace, DockPolicy::default())
        .expect("durable-rectangle engine must be valid");
    let presentation_host = engine
        .create_presentation_host()
        .expect("durable-rectangle presentation host must mint");
    DurableRectFixture {
        engine,
        presentation_host,
        surface,
        first,
        second,
    }
}

fn submit_durable_rect_projection(
    fixture: &mut DurableRectFixture,
    bounds: LogicalRect,
) -> EngineTransition {
    let contribution = begin_surface_measurement(&fixture.engine, fixture.surface, bounds);
    submit_surface_measurement(&mut fixture.engine, fixture.presentation_host, contribution)
}

fn publish_initial_durable_rect_projection(fixture: &mut DurableRectFixture) {
    publish_surface_projection(
        &mut fixture.engine,
        fixture.presentation_host,
        fixture.surface,
        LogicalRect::new(0.0, 0.0, 400.0, 300.0).expect("initial surface bounds must be valid"),
    );
}

#[test]
fn surface_resize_clips_presentation_without_rewriting_durable_contained_rects() {
    let mut fixture = durable_rect_fixture();
    publish_initial_durable_rect_projection(&mut fixture);
    let before_workspace = fixture.engine.workspace().clone();
    let before_version = fixture.engine.version();
    let initial_plan = fixture
        .engine
        .scene()
        .ready_surface(fixture.surface)
        .expect("initial projection must be painted")
        .plan();
    let initial_identity = initial_plan
        .contained_records()
        .iter()
        .map(|record| {
            (
                record.floating(),
                record.root(),
                record.ordinal(),
                record.layer(),
            )
        })
        .collect::<Vec<_>>();
    let initial_outer_bounds = initial_plan
        .contained_records()
        .iter()
        .map(crate::scene::ContainedRecord::outer_bounds)
        .collect::<Vec<_>>();
    assert_eq!(
        initial_outer_bounds,
        [
            fixture
                .engine
                .workspace()
                .contained_floating(fixture.first)
                .expect("first durable floating must exist")
                .rect,
            fixture
                .engine
                .workspace()
                .contained_floating(fixture.second)
                .expect("second durable floating must exist")
                .rect,
        ]
    );

    let shrunken_bounds =
        LogicalRect::new(0.0, 0.0, 200.0, 150.0).expect("shrunken bounds must be valid");
    let shrunken = submit_durable_rect_projection(&mut fixture, shrunken_bounds);
    assert!(matches!(
        shrunken.surface_contributions(),
        [SurfaceContributionOutcome::Ready {
            surface,
            ..
        }] if *surface == fixture.surface
    ));
    assert!(shrunken.events().is_empty());
    assert_eq!(fixture.engine.workspace(), &before_workspace);
    assert_eq!(fixture.engine.version(), before_version);

    let pending_shrunken = fixture
        .engine
        .scene()
        .surface(fixture.surface)
        .and_then(SurfaceScene::paint_projection)
        .expect("shrunken projection must be paintable");
    assert_eq!(pending_shrunken.plan().bounds(), shrunken_bounds);
    assert_eq!(
        pending_shrunken
            .plan()
            .contained_records()
            .iter()
            .map(|record| (record.floating(), record.outer_bounds()))
            .collect::<Vec<_>>(),
        [(
            fixture.second,
            LogicalRect::new(150.0, 100.0, 50.0, 50.0)
                .expect("partially clipped bounds must remain representable"),
        )],
        "the fully hidden floating is omitted while the visible subset is clipped"
    );
    assert_eq!(
        fixture
            .engine
            .scene()
            .ready_surface(fixture.surface)
            .expect("the prior painted projection remains hit authority")
            .plan()
            .bounds(),
        LogicalRect::new(0.0, 0.0, 400.0, 300.0).expect("initial surface bounds must be valid")
    );
    publish_surface_projection(
        &mut fixture.engine,
        fixture.presentation_host,
        fixture.surface,
        shrunken_bounds,
    );
    assert_eq!(
        fixture
            .engine
            .scene()
            .ready_surface(fixture.surface)
            .expect("the painted shrunken projection becomes hit authority")
            .plan()
            .bounds(),
        shrunken_bounds
    );

    let expanded_bounds =
        LogicalRect::new(0.0, 0.0, 400.0, 300.0).expect("expanded surface bounds must be valid");
    let expanded = submit_durable_rect_projection(&mut fixture, expanded_bounds);
    assert!(matches!(
        expanded.surface_contributions(),
        [SurfaceContributionOutcome::Ready {
            surface,
            ..
        }] if *surface == fixture.surface
    ));
    assert!(expanded.events().is_empty());
    assert_eq!(fixture.engine.workspace(), &before_workspace);
    assert_eq!(fixture.engine.version(), before_version);

    let pending_expanded = fixture
        .engine
        .scene()
        .surface(fixture.surface)
        .and_then(SurfaceScene::paint_projection)
        .expect("expanded projection must be paintable");
    assert_eq!(
        pending_expanded
            .plan()
            .contained_records()
            .iter()
            .map(|record| {
                (
                    record.floating(),
                    record.root(),
                    record.ordinal(),
                    record.layer(),
                )
            })
            .collect::<Vec<_>>(),
        initial_identity
    );
    assert_eq!(
        pending_expanded
            .plan()
            .contained_records()
            .iter()
            .map(crate::scene::ContainedRecord::outer_bounds)
            .collect::<Vec<_>>(),
        initial_outer_bounds
    );
    publish_surface_projection(
        &mut fixture.engine,
        fixture.presentation_host,
        fixture.surface,
        expanded_bounds,
    );
    assert_eq!(
        fixture
            .engine
            .scene()
            .ready_surface(fixture.surface)
            .expect("the painted expanded projection becomes hit authority")
            .plan()
            .contained_records()
            .iter()
            .map(crate::scene::ContainedRecord::outer_bounds)
            .collect::<Vec<_>>(),
        initial_outer_bounds
    );
}

fn background_source_binding(fixture: &BackgroundFixture) -> ViewportBinding {
    fixture
        .engine
        .viewport()
        .viewport(SOURCE_SURFACE)
        .expect("native background source viewport must be registered")
        .binding()
}

fn background_platform_snapshot(
    source_binding: ViewportBinding,
    content_bounds: Option<PhysicalRect>,
    focus_generation: u64,
    close_requested: bool,
) -> PlatformSnapshot {
    const WORK_AREA: WorkAreaToken = WorkAreaToken::new(93);
    let mut capabilities = PlatformCapabilities::default();
    capabilities.set_native_window_lifecycle(PlatformCapability::Supported);
    capabilities.set_authoritative_inventory(PlatformCapability::Supported);
    capabilities.set_global_window_placement(PlatformCapability::Supported);
    capabilities.set_work_area(PlatformCapability::Supported);
    capabilities.set_global_focus_observation(PlatformCapability::Supported);
    let observed = content_bounds.into_iter().map(|content_bounds| {
        ObservedWindow::new(source_binding)
            .with_coordinate_observation(WindowCoordinateObservation::new(
                source_binding,
                CoordinateObservationGeneration::new(focus_generation),
                Authority::Known(content_bounds),
                Authority::Known(
                    PhysicalRect::new(-8.0, -30.0, 416.0, 338.0)
                        .expect("source outer bounds must be valid"),
                ),
                Authority::Known(ScaleFactor::new(1.0).expect("source scale must be valid")),
                Authority::Known(ScaleFactor::new(1.0).expect("source scale must be valid")),
            ))
            .with_close_requested(Authority::Known(close_requested))
            .with_presentation_observation(WindowPresentationObservation::new(
                source_binding,
                crate::viewport::PresentationObservationGeneration::new(focus_generation),
                Authority::Known(WindowPresentationState::Visible),
                PresentationEffectAcknowledgement::known(None),
            ))
    });
    test_platform_snapshot(
        capabilities,
        FocusObservationEnvelope::new(
            FocusObservationGeneration::new(focus_generation),
            Authority::Known(GlobalFocusedWindow::Foreign),
            Authority::Known(None),
        ),
        observed.collect(),
        Vec::new(),
        known_work_area_observation(
            focus_generation,
            vec![ObservedWorkArea::new(
                WORK_AREA,
                PhysicalRect::new(0.0, 0.0, 1920.0, 1080.0).expect("work area must be valid"),
                ScaleFactor::new(1.0).expect("work-area scale must be valid"),
            )],
        ),
    )
    .expect("native platform facts must be canonical")
}

fn publish_native_background_platform(fixture: &mut BackgroundFixture) -> WorkAreaToken {
    const SOURCE_WINDOW: WindowToken = WindowToken::new(93);
    const WORK_AREA: WorkAreaToken = WorkAreaToken::new(93);

    let provider = test_platform_provider(&mut fixture.engine);
    let expected = fixture.engine.version();
    submit_test_input(
        &mut fixture.engine,
        fixture.presentation_host,
        EngineInput::RegisterViewport {
            provider,
            expected,
            surface: SOURCE_SURFACE,
            token: SOURCE_WINDOW,
            role: ViewportRole::Root,
            recovery_target: None,
        },
    )
    .expect("source viewport registration must reduce");
    let snapshot = background_platform_snapshot(
        background_source_binding(fixture),
        Some(
            PhysicalRect::new(0.0, 0.0, 400.0, 300.0)
                .expect("source physical bounds must be valid"),
        ),
        1,
        false,
    );
    let expected_epoch = fixture.engine.version().epoch();
    submit_test_input(
        &mut fixture.engine,
        fixture.presentation_host,
        EngineInput::PublishPlatformSnapshot {
            provider,
            expected_epoch,
            snapshot,
        },
    )
    .expect("native platform snapshot must reduce");
    publish_background_scene(fixture);
    WORK_AREA
}

#[test]
fn native_surface_without_coordinate_authority_prepares_only_unavailable_paint() {
    const PENDING_WINDOW: WindowToken = WindowToken::new(94);
    let mut fixture = background_fixture(DockPolicy::default());
    let provider = test_platform_provider(&mut fixture.engine);
    let expected = fixture.engine.version();
    submit_test_input(
        &mut fixture.engine,
        fixture.presentation_host,
        EngineInput::RegisterViewport {
            provider,
            expected,
            surface: SOURCE_SURFACE,
            token: PENDING_WINDOW,
            role: ViewportRole::Root,
            recovery_target: None,
        },
    )
    .expect("pending native viewport registration reduces");
    let token = fixture
        .engine
        .begin_surface_contribution(SOURCE_SURFACE)
        .expect("registered surface contribution begins");
    let measurements = surface_measurements(&fixture.engine, SOURCE_SURFACE, background_bounds());

    let prepared = fixture
        .engine
        .prepare_surface_contribution(token, measurements)
        .expect("valid measurements prepare despite unavailable coordinates");

    assert!(matches!(
        prepared.paint_candidate(),
        PreparedSurfacePaintCandidate::Unavailable {
            reason: SurfaceContributionUnavailableReason::CoordinateAuthorityUnavailable,
        }
    ));
}

#[test]
fn headless_contribution_cannot_publish_after_native_registration() {
    const LATE_WINDOW: WindowToken = WindowToken::new(95);
    let mut fixture = background_fixture(DockPolicy::default());
    let contribution =
        begin_surface_measurement(&fixture.engine, SOURCE_SURFACE, background_bounds());
    let provider = test_platform_provider(&mut fixture.engine);
    let expected = fixture.engine.version();
    submit_test_input(
        &mut fixture.engine,
        fixture.presentation_host,
        EngineInput::RegisterViewport {
            provider,
            expected,
            surface: SOURCE_SURFACE,
            token: LATE_WINDOW,
            role: ViewportRole::Root,
            recovery_target: None,
        },
    )
    .expect("late registration must reduce");
    let before_workspace = fixture.engine.workspace().clone();
    let before_version = fixture.engine.version();
    let before_scene = fixture.engine.scene().clone();
    let before_interaction = fixture.engine.interaction().clone();

    let transition =
        submit_surface_measurement(&mut fixture.engine, fixture.presentation_host, contribution);

    assert!(matches!(
        transition
            .surface_contributions()
            .iter()
            .find(|outcome| outcome.surface() == SOURCE_SURFACE),
        Some(SurfaceContributionOutcome::Rejected {
            surface: SOURCE_SURFACE,
            reason: SurfaceContributionRejection::StaleBase { .. },
        })
    ));
    assert!(transition.events().is_empty());
    assert!(transition.interaction_events().is_empty());
    assert_eq!(fixture.engine.workspace(), &before_workspace);
    assert_eq!(fixture.engine.version(), before_version);
    assert_eq!(fixture.engine.scene(), &before_scene);
    assert_eq!(fixture.engine.interaction(), &before_interaction);
}

#[test]
fn native_contribution_requires_an_explicit_tombstone_before_binding_is_removed() {
    let mut fixture = background_fixture(DockPolicy::default());
    publish_native_background_platform(&mut fixture);
    let stale_contribution = begin_surface_measurement(
        &fixture.engine,
        SOURCE_SURFACE,
        LogicalRect::new(0.0, 0.0, 200.0, 150.0).expect("shrunken bounds must be valid"),
    );
    let expected_epoch = fixture.engine.version().epoch();
    let source_binding = background_source_binding(&fixture);
    let provider = test_platform_provider(&mut fixture.engine);
    submit_test_input(
        &mut fixture.engine,
        fixture.presentation_host,
        EngineInput::PublishPlatformSnapshot {
            provider,
            expected_epoch,
            snapshot: background_platform_snapshot(source_binding, None, 2, false),
        },
    )
    .expect("authoritative inventory absence must reduce");
    assert!(
        fixture.engine.viewport().viewport(SOURCE_SURFACE).is_some(),
        "inventory absence is Missing, not a destruction tombstone"
    );
    let before_workspace = fixture.engine.workspace().clone();
    let before_version = fixture.engine.version();
    let before_scene = fixture.engine.scene().clone();
    let before_interaction = fixture.engine.interaction().clone();

    let transition = submit_surface_measurement(
        &mut fixture.engine,
        fixture.presentation_host,
        stale_contribution,
    );

    assert!(matches!(
        transition
            .surface_contributions()
            .iter()
            .find(|outcome| outcome.surface() == SOURCE_SURFACE),
        Some(SurfaceContributionOutcome::Rejected {
            surface: SOURCE_SURFACE,
            reason: SurfaceContributionRejection::StaleBase { .. },
        })
    ));
    assert!(transition.events().is_empty());
    assert!(transition.interaction_events().is_empty());
    assert_eq!(fixture.engine.workspace(), &before_workspace);
    assert_eq!(fixture.engine.version(), before_version);
    assert_eq!(fixture.engine.scene(), &before_scene);
    assert_eq!(fixture.engine.interaction(), &before_interaction);
}

fn install_pending_native_root_reservation(
    fixture: &mut BackgroundFixture,
    reserved_root: RootId,
) -> crate::frame::NativeCreateRequest {
    let work_area = publish_native_background_platform(fixture);
    install_pending_native_root_reservation_with_work_area(fixture, reserved_root, work_area)
}

fn install_pending_native_root_reservation_with_work_area(
    fixture: &mut BackgroundFixture,
    reserved_root: RootId,
    work_area: WorkAreaToken,
) -> crate::frame::NativeCreateRequest {
    const NATIVE_SURFACE: SurfaceId = SurfaceId::new(93);
    const RECOVERY_FLOATING: FloatingPresentationId = FloatingPresentationId::new(93);

    let placement = fixture
        .engine
        .viewport_placement(
            SOURCE_SURFACE,
            LogicalRect::new(50.0, 50.0, 300.0, 220.0)
                .expect("native logical placement must be valid"),
            work_area,
        )
        .expect("native placement proof must be current");
    let payload = background_payload(fixture, ItemId::new(1));
    let command = WorkspaceCommand::CreateSurfaceRoot {
        surface: NATIVE_SURFACE,
        root: reserved_root,
        content: RootContent::Move(payload.clone()),
    };
    let converted_main = crate::surface_recovery::ConvertedMainRecovery::new(
        reserved_root,
        RECOVERY_FLOATING,
        LogicalSize::new(0.0, 0.0).expect("recovery minimum must be valid"),
    );
    let proposal =
        NativeTearOffProposal::new(NATIVE_SURFACE, reserved_root, placement, converted_main)
            .expect("native recovery contract must be valid");
    let anchor = fixture
        .engine
        .root_recovery_anchor(SOURCE_SURFACE)
        .expect("registered source root must own a recovery anchor");
    let target = SurfaceRecoveryTarget::with_converted_main(anchor, converted_main);
    let mut future = fixture.engine.workspace().clone();
    WorkspaceTransaction::from_commands([command.clone()])
        .apply(&mut future, fixture.engine.policy_snapshot())
        .expect("native command must produce a future workspace");
    let id = fixture
        .engine
        .next_surface_recovery_obligation_id(InputSequence::new(93))
        .expect("test obligation identity must advance");
    let obligation = fixture
        .engine
        .authorize_surface_recovery_obligation(
            InputSequence::new(93),
            id,
            &future,
            NATIVE_SURFACE,
            target,
            fixture.engine.policy_snapshot(),
        )
        .expect("native recovery must be authorized");
    let focus_causal = mint_test_focus_causal(&mut fixture.engine);
    let source_presentation = fixture
        .engine
        .interaction_authority(SOURCE_SURFACE)
        .expect("source surface must have presented interaction authority");
    let prepared = PreparedNativeTearOff::new(
        DragSessionId::new(fixture.engine.version().epoch(), DragGeneration::new(93)),
        source_presentation,
        payload,
        fixture.engine.version(),
        command,
        proposal,
        obligation,
        focus_causal,
        PaneFocusDisposition::Clear,
    );
    let request = fixture
        .engine
        .viewport
        .start_native_create(prepared)
        .expect("native create reservation must start");
    assert!(fixture.engine.viewport_focus.reserve_activation_causal(
        request.saga(),
        focus_causal,
        ViewportActivationRequest::tear_off_committed(
            request.binding(),
            PaneFocusDisposition::Clear,
        ),
    ));
    request
}

#[test]
fn pending_newer_native_reservation_preserves_ready_older_owner() {
    let mut policy = DockPolicy::default();
    policy.set_allow_native_surfaces(true);
    let mut fixture = background_fixture(policy);
    let work_area = publish_native_background_platform(&mut fixture);
    let source_binding = background_source_binding(&fixture);
    let older_owner = NativeCreateSagaId::new(200);
    let older_causal = mint_test_focus_causal(&mut fixture.engine);
    assert!(fixture.engine.viewport_focus.reserve_activation_causal(
        older_owner,
        older_causal,
        ViewportActivationRequest::tear_off_committed(source_binding, PaneFocusDisposition::Clear,),
    ));
    let newer = install_pending_native_root_reservation_with_work_area(
        &mut fixture,
        RootId::new(97),
        work_area,
    );
    assert_eq!(
        fixture
            .engine
            .viewport_focus
            .winning_activation_reservation()
            .map(|(owner, _, _)| owner),
        Some(newer.saga())
    );

    fixture
        .engine
        .settle_native_activation_reservations(&mut Vec::new())
        .expect("a pending newer owner must defer reservation settlement");

    let owners = fixture
        .engine
        .viewport_focus
        .activation_reservations()
        .into_iter()
        .map(|(owner, _, _)| owner)
        .collect::<Vec<_>>();
    assert_eq!(owners, vec![newer.saga(), older_owner]);
    assert!(
        fixture
            .engine
            .viewport_focus
            .cancel_activation_reservation(newer.saga())
    );
    assert_eq!(
        fixture
            .engine
            .viewport_focus
            .winning_activation_reservation()
            .map(|(owner, _, _)| owner),
        Some(older_owner),
        "cancelling the pending winner must reveal the retained older claim"
    );
}

#[test]
fn terminal_missing_newer_reservation_is_pruned_before_older_owner_settlement() {
    let mut policy = DockPolicy::default();
    policy.set_allow_native_surfaces(true);
    let mut fixture = background_fixture(policy);
    publish_native_background_platform(&mut fixture);
    let source_binding = background_source_binding(&fixture);
    let older_owner = NativeCreateSagaId::new(201);
    let older_causal = mint_test_focus_causal(&mut fixture.engine);
    assert!(fixture.engine.viewport_focus.reserve_activation_causal(
        older_owner,
        older_causal,
        ViewportActivationRequest::tear_off_committed(source_binding, PaneFocusDisposition::Clear,),
    ));
    let missing_binding = ViewportBinding::new(
        source_binding.authority_domain(),
        source_binding.epoch(),
        SurfaceId::new(202),
        WindowToken::new(202),
        WindowIncarnation::new(1),
    );
    let terminal_owner = NativeCreateSagaId::new(202);
    let newer_causal = mint_test_focus_causal(&mut fixture.engine);
    assert!(
        fixture.engine.viewport_focus.reserve_activation_causal(
            terminal_owner,
            newer_causal,
            ViewportActivationRequest::tear_off_committed(
                missing_binding,
                PaneFocusDisposition::Clear,
            ),
        )
    );

    fixture
        .engine
        .settle_native_activation_reservations(&mut Vec::new())
        .expect("terminal reservation pruning must preserve the older claim");

    assert_eq!(
        fixture
            .engine
            .viewport_focus
            .activation_reservations()
            .into_iter()
            .map(|(owner, _, _)| owner)
            .collect::<Vec<_>>(),
        vec![older_owner]
    );
    assert_eq!(
        fixture
            .engine
            .viewport_focus
            .winning_activation_reservation()
            .map(|(owner, _, _)| owner),
        Some(older_owner)
    );
}

#[test]
fn direct_surface_close_rejects_a_staging_native_create_without_a_plan() {
    let mut policy = DockPolicy::default();
    policy.set_allow_native_surfaces(true);
    let mut fixture = background_fixture(policy);
    let request = install_pending_native_root_reservation(&mut fixture, RootId::new(96));
    let edge = NativeCloseEdge::from_authoritative_requested(
        fixture.engine.authority_domain,
        request.binding(),
        CloseObservationGeneration::new(1),
        fixture.engine.viewport().registry().inventory_generation(),
    );
    let version = fixture.engine.version();
    let policy = fixture.engine.policy.clone();

    let outcome = fixture
        .engine
        .reduce_surface_close_request(
            InputSequence::new(96),
            version,
            edge,
            SurfaceCloseRequest::RetainLayout,
            version,
            &policy,
        )
        .expect("staging close rejection must be a normal reducer outcome");

    assert!(matches!(
        outcome,
        InputOutcome::SurfaceCloseRejected {
            edge: actual,
            reason: SurfaceCloseRequestRejection::StagingBinding { binding },
            ..
        } if actual == edge && binding == request.binding()
    ));
    assert_eq!(fixture.engine.active_close_plans().count(), 0);
    assert!(
        fixture
            .engine
            .viewport()
            .native_create_saga(request.saga())
            .is_some()
    );
}

fn background_native_window_snapshot(
    fixture: &mut BackgroundFixture,
    request: crate::frame::NativeCreateRequest,
    presentation: WindowPresentationState,
    generation: u64,
    focus_control: bool,
) -> PlatformSnapshot {
    background_native_window_snapshot_with_focus(
        fixture,
        request,
        presentation,
        generation,
        focus_control,
        GlobalFocusedWindow::Foreign,
    )
}

fn background_native_window_snapshot_with_focus(
    fixture: &mut BackgroundFixture,
    request: crate::frame::NativeCreateRequest,
    presentation: WindowPresentationState,
    generation: u64,
    focus_control: bool,
    focused: GlobalFocusedWindow,
) -> PlatformSnapshot {
    const WORK_AREA: WorkAreaToken = WorkAreaToken::new(93);
    let mut capabilities = PlatformCapabilities::default();
    capabilities.set_native_window_lifecycle(PlatformCapability::Supported);
    capabilities.set_authoritative_inventory(PlatformCapability::Supported);
    capabilities.set_global_window_placement(PlatformCapability::Supported);
    capabilities.set_work_area(PlatformCapability::Supported);
    capabilities.set_global_focus_observation(PlatformCapability::Supported);
    if focus_control {
        capabilities.set_window_activation_control(PlatformCapability::Supported);
    }
    let source = ObservedWindow::new(background_source_binding(fixture))
        .with_coordinate_observation(WindowCoordinateObservation::new(
            background_source_binding(fixture),
            CoordinateObservationGeneration::new(generation),
            Authority::Known(
                PhysicalRect::new(0.0, 0.0, 400.0, 300.0)
                    .expect("source physical bounds must be valid"),
            ),
            Authority::Known(
                PhysicalRect::new(-8.0, -30.0, 416.0, 338.0)
                    .expect("source outer bounds must be valid"),
            ),
            Authority::Known(ScaleFactor::new(1.0).expect("source scale must be valid")),
            Authority::Known(ScaleFactor::new(1.0).expect("source scale must be valid")),
        ))
        .with_presentation_observation(WindowPresentationObservation::new(
            background_source_binding(fixture),
            crate::viewport::PresentationObservationGeneration::new(generation),
            Authority::Known(WindowPresentationState::Visible),
            PresentationEffectAcknowledgement::known(None),
        ));
    let acknowledged_effect = fixture
        .engine
        .viewport()
        .native_create_saga(request.saga())
        .map(|saga| saga.phase().acknowledged_effect());
    let native = ObservedWindow::new(request.binding())
        .with_coordinate_observation(WindowCoordinateObservation::new(
            request.binding(),
            CoordinateObservationGeneration::new(generation),
            Authority::Known(
                PhysicalRect::new(500.0, 0.0, 300.0, 220.0)
                    .expect("native physical bounds must be valid"),
            ),
            Authority::Known(
                PhysicalRect::new(492.0, -30.0, 316.0, 258.0)
                    .expect("native outer bounds must be valid"),
            ),
            Authority::Known(ScaleFactor::new(1.0).expect("native scale must be valid")),
            Authority::Known(ScaleFactor::new(1.0).expect("native scale must be valid")),
        ))
        .with_presentation_observation(WindowPresentationObservation::new(
            request.binding(),
            crate::viewport::PresentationObservationGeneration::new(generation),
            Authority::Known(presentation),
            PresentationEffectAcknowledgement::known(acknowledged_effect),
        ));
    test_platform_snapshot(
        capabilities,
        FocusObservationEnvelope::new(
            FocusObservationGeneration::new(generation),
            Authority::Known(focused),
            Authority::Known(None),
        ),
        vec![source, native],
        Vec::new(),
        known_work_area_observation(
            generation,
            vec![ObservedWorkArea::new(
                WORK_AREA,
                PhysicalRect::new(0.0, 0.0, 1920.0, 1080.0).expect("work area must be valid"),
                ScaleFactor::new(1.0).expect("work-area scale must be valid"),
            )],
        ),
    )
    .expect("native create snapshot must be canonical")
}

fn publish_background_native_window(
    fixture: &mut BackgroundFixture,
    request: crate::frame::NativeCreateRequest,
    presentation: WindowPresentationState,
    generation: u64,
) -> EngineTransition {
    let snapshot =
        background_native_window_snapshot(fixture, request, presentation, generation, false);
    let provider = test_platform_provider(&mut fixture.engine);
    let expected_epoch = fixture.engine.version().epoch();
    submit_test_input(
        &mut fixture.engine,
        fixture.presentation_host,
        EngineInput::PublishPlatformSnapshot {
            provider,
            expected_epoch,
            snapshot,
        },
    )
    .expect("native create snapshot must reduce")
}

fn current_native_staging_presentation(
    fixture: &BackgroundFixture,
    request: crate::frame::NativeCreateRequest,
    phase: crate::presentation_observation::NativeStagingPresentationPhase,
) -> NativeStagingPresentation {
    let saga = fixture
        .engine
        .viewport()
        .native_create_saga(request.saga())
        .expect("native create saga must remain pending");
    match saga.phase() {
        crate::frame::NativeCreatePhase::AwaitingPreShowPresentation { presentation, .. }
        | crate::frame::NativeCreatePhase::AwaitingPostShowPresentation { presentation, .. }
            if presentation.phase() == phase =>
        {
            Some(presentation)
        }
        _ => None,
    }
    .unwrap_or_else(|| {
        panic!(
            "native create must request {phase:?}, actual phase was {:?}",
            saga.phase()
        )
    })
}

fn emit_background_native_staging(
    fixture: &mut BackgroundFixture,
    presentation: NativeStagingPresentation,
) -> crate::presentation_observation::HostPresentationOutput {
    let mut frame = begin_test_host_frame(&fixture.engine, fixture.presentation_host);
    assert!(
        frame
            .view()
            .native_staging_presentations()
            .any(|current| current == presentation)
    );
    for obligation in frame
        .issue_presentation_obligations()
        .expect("native staging frame must issue its exact physical roster")
    {
        let disposition = if obligation.slot().native_staging() == Some(presentation) {
            HostPresentationDisposition::Painted(HostInteractionPresentation::default())
        } else {
            HostPresentationDisposition::Unavailable(
                HostPresentationUnavailableReason::OutputNotProduced,
            )
        };
        frame
            .settle_presentation_obligation(obligation, disposition)
            .expect("native staging physical obligation must resolve");
    }
    complete_surface_contribution_roster(&mut frame);
    let emitted = frame
        .finish(&mut fixture.engine)
        .expect("native staging output must emit");
    let outputs = emitted
        .presentation_emissions()
        .iter()
        .filter_map(|emission| {
            let output = emission.output();
            matches!(
                output.payload(),
                HostPresentationOutputPayload::NativeStaging {
                    presentation: actual,
                } if actual == presentation
            )
            .then_some(output)
        })
        .collect::<Vec<_>>();
    let [output] = outputs.as_slice() else {
        panic!("one exact native staging output must emit, got {outputs:?}");
    };
    *output
}

fn observe_background_presentation_output(
    fixture: &mut BackgroundFixture,
    output: crate::presentation_observation::HostPresentationOutput,
) -> EngineTransition {
    let mut prelude = fixture
        .engine
        .begin_host_frame(fixture.presentation_host)
        .expect("native staging observation frame must begin");
    prelude
        .submit_presentation_observation(HostPresentationObservation::Batch(vec![
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
            ),
        ]))
        .expect("native staging observation must submit");
    let mut frame = prelude
        .seal(&fixture.engine)
        .expect("native staging observation frame must seal");
    complete_host_frame_with_explicit_surface_roster(&fixture.engine, &mut frame);
    frame
        .finish(&mut fixture.engine)
        .expect("native staging observation must reduce")
}

fn present_background_native_staging(
    fixture: &mut BackgroundFixture,
    presentation: NativeStagingPresentation,
) -> EngineTransition {
    let output = emit_background_native_staging(fixture, presentation);
    observe_background_presentation_output(fixture, output)
}

fn advance_pending_native_root_to_awaiting_visible(
    fixture: &mut BackgroundFixture,
    request: crate::frame::NativeCreateRequest,
) {
    let hidden =
        publish_background_native_window(fixture, request, WindowPresentationState::Hidden, 2);
    assert!(hidden.platform_effects().iter().all(|effect| {
        !matches!(
            effect.effect(),
            crate::effect::PlatformEffect::ShowWindow { binding, .. }
                if *binding == request.binding()
        )
    }));
    let pre_show = current_native_staging_presentation(
        fixture,
        request,
        crate::presentation_observation::NativeStagingPresentationPhase::PreShow,
    );
    let pre_show_presented = present_background_native_staging(fixture, pre_show);
    assert!(pre_show_presented.platform_effects().iter().any(|effect| {
        matches!(
            effect.effect(),
            crate::effect::PlatformEffect::ShowWindow { binding, .. }
                if *binding == request.binding()
        )
    }));
    let acknowledged =
        publish_background_native_window(fixture, request, WindowPresentationState::Hidden, 3);
    assert!(acknowledged.platform_effects().is_empty());
}

fn advance_pending_native_root_to_ownership_transfer(
    fixture: &mut BackgroundFixture,
    request: crate::frame::NativeCreateRequest,
) -> EngineTransition {
    advance_pending_native_root_to_awaiting_visible(fixture, request);
    let visible =
        publish_background_native_window(fixture, request, WindowPresentationState::Visible, 4);
    assert!(
        fixture
            .engine
            .workspace()
            .surface(request.binding().surface())
            .is_none()
    );
    let post_show = current_native_staging_presentation(
        fixture,
        request,
        crate::presentation_observation::NativeStagingPresentationPhase::PostShow,
    );
    let transferred = present_background_native_staging(fixture, post_show);
    assert!(visible.platform_effects().iter().all(|effect| {
        !matches!(
            effect.effect(),
            PlatformEffect::RequestFocus { binding, .. } if *binding == request.binding()
        )
    }));
    transferred
}

fn advance_pending_native_root_to_first_live(
    fixture: &mut BackgroundFixture,
    request: crate::frame::NativeCreateRequest,
) -> EngineTransition {
    let _ = advance_pending_native_root_to_ownership_transfer(fixture, request);
    let surface = request.binding().surface();
    let bounds =
        LogicalRect::new(0.0, 0.0, 300.0, 220.0).expect("native target test bounds must be valid");
    let measurements = surface_measurements(&fixture.engine, surface, bounds);
    publish_surface_projection_with_transition(
        &mut fixture.engine,
        fixture.presentation_host,
        surface,
        measurements,
    )
    .1
}

#[test]
fn auto_focused_native_window_waits_for_first_live_before_pane_focus_is_admitted() {
    const NATIVE_SURFACE: SurfaceId = SurfaceId::new(93);
    let mut policy = DockPolicy::default();
    policy.set_allow_native_surfaces(true);
    let mut fixture = background_fixture(policy);
    let request = install_pending_native_root_reservation(&mut fixture, RootId::new(94));
    let prepared = fixture
        .engine
        .viewport()
        .native_create_saga(request.saga())
        .expect("pending native create must retain its release-time focus claim")
        .prepared()
        .clone();
    assert!(fixture.engine.viewport_focus.reserve_activation_causal(
        request.saga(),
        prepared.focus_causal(),
        ViewportActivationRequest::tear_off_committed(request.binding(), prepared.pane_focus(),),
    ));

    let _ = fixture.engine.viewport.take_new_effects();
    advance_pending_native_root_to_awaiting_visible(&mut fixture, request);
    let snapshot = background_native_window_snapshot_with_focus(
        &mut fixture,
        request,
        WindowPresentationState::Visible,
        4,
        true,
        GlobalFocusedWindow::Dock(request.binding()),
    );
    let provider = test_platform_provider(&mut fixture.engine);
    let expected_epoch = fixture.engine.version().epoch();
    let visible = submit_test_input(
        &mut fixture.engine,
        fixture.presentation_host,
        EngineInput::PublishPlatformSnapshot {
            provider,
            expected_epoch,
            snapshot,
        },
    )
    .expect("an exact auto-focused staging window must not deadlock admission");
    assert!(fixture.engine.workspace().surface(NATIVE_SURFACE).is_none());
    let post_show = current_native_staging_presentation(
        &fixture,
        request,
        crate::presentation_observation::NativeStagingPresentationPhase::PostShow,
    );
    let _ = present_background_native_staging(&mut fixture, post_show);
    assert_eq!(
        fixture
            .engine
            .viewport()
            .viewport(NATIVE_SURFACE)
            .map(crate::viewport_registry::ViewportRecord::admission),
        Some(crate::viewport_registry::ViewportAdmission::Pending)
    );
    let bounds =
        LogicalRect::new(0.0, 0.0, 300.0, 220.0).expect("native target test bounds must be valid");
    let measurements = surface_measurements(&fixture.engine, NATIVE_SURFACE, bounds);
    let (_, transition) = publish_surface_projection_with_transition(
        &mut fixture.engine,
        fixture.presentation_host,
        NATIVE_SURFACE,
        measurements,
    );

    assert!(fixture.engine.workspace().surface(NATIVE_SURFACE).is_some());
    assert_eq!(
        fixture
            .engine
            .viewport()
            .viewport(NATIVE_SURFACE)
            .map(crate::viewport_registry::ViewportRecord::admission),
        Some(crate::viewport_registry::ViewportAdmission::Admitted)
    );
    let intent = fixture
        .engine
        .viewport_focus()
        .pending_pane_intent()
        .expect("first-live admission must install the release-time pane claim");
    assert_eq!(intent.target(), request.binding());
    assert_eq!(intent.causal(), prepared.focus_causal());
    assert_eq!(intent.focus(), PanelFocus::None);
    assert!(visible.platform_effects().iter().all(|effect| {
        !matches!(
            effect.effect(),
            PlatformEffect::RequestFocus { binding, .. } if *binding == request.binding()
        )
    }));
    assert!(transition.platform_effects().iter().all(|effect| {
        !matches!(
            effect.effect(),
            PlatformEffect::RequestFocus { binding, .. } if *binding == request.binding()
        )
    }));
}

#[test]
fn native_focus_reservation_replays_after_activation_control_becomes_available() {
    let mut policy = DockPolicy::default();
    policy.set_allow_native_surfaces(true);
    let mut fixture = background_fixture(policy);
    let request = install_pending_native_root_reservation(&mut fixture, RootId::new(97));
    let prepared = fixture
        .engine
        .viewport()
        .native_create_saga(request.saga())
        .expect("pending native create must retain its release-time focus claim")
        .prepared()
        .clone();
    assert!(fixture.engine.viewport_focus.reserve_activation_causal(
        request.saga(),
        prepared.focus_causal(),
        ViewportActivationRequest::tear_off_committed(request.binding(), prepared.pane_focus(),),
    ));

    let _ = fixture.engine.viewport.take_new_effects();
    advance_pending_native_root_to_awaiting_visible(&mut fixture, request);
    let visible = publish_background_native_window(
        &mut fixture,
        request,
        WindowPresentationState::Visible,
        4,
    );
    assert!(visible.platform_effects().iter().all(|effect| {
        !matches!(
            effect.effect(),
            PlatformEffect::RequestFocus { binding, .. } if *binding == request.binding()
        )
    }));
    let post_show = current_native_staging_presentation(
        &fixture,
        request,
        crate::presentation_observation::NativeStagingPresentationPhase::PostShow,
    );
    let transferred = present_background_native_staging(&mut fixture, post_show);
    assert!(transferred.platform_effects().iter().all(|effect| {
        !matches!(
            effect.effect(),
            PlatformEffect::RequestFocus { binding, .. } if *binding == request.binding()
        )
    }));
    let bounds =
        LogicalRect::new(0.0, 0.0, 300.0, 220.0).expect("native target test bounds must be valid");
    let measurements = surface_measurements(&fixture.engine, request.binding().surface(), bounds);
    let (_, first_live) = publish_surface_projection_with_transition(
        &mut fixture.engine,
        fixture.presentation_host,
        request.binding().surface(),
        measurements,
    );
    assert!(first_live.platform_effects().iter().all(|effect| {
        !matches!(
            effect.effect(),
            PlatformEffect::RequestFocus { binding, .. } if *binding == request.binding()
        )
    }));
    assert_eq!(
        fixture
            .engine
            .viewport_focus
            .winning_activation_reservation()
            .map(|(owner, _, _)| owner),
        Some(request.saga())
    );

    let snapshot = background_native_window_snapshot(
        &mut fixture,
        request,
        WindowPresentationState::Visible,
        5,
        true,
    );
    let provider = test_platform_provider(&mut fixture.engine);
    let expected_epoch = fixture.engine.version().epoch();
    let replayed = submit_test_input(
        &mut fixture.engine,
        fixture.presentation_host,
        EngineInput::PublishPlatformSnapshot {
            provider,
            expected_epoch,
            snapshot,
        },
    )
    .expect("the authoritative capability update must replay the reservation");
    assert!(replayed.platform_effects().iter().any(|effect| {
        matches!(
            effect.effect(),
            PlatformEffect::RequestFocus { binding, .. } if *binding == request.binding()
        )
    }));
    assert!(
        fixture
            .engine
            .viewport_focus
            .winning_activation_reservation()
            .is_none()
    );
}

#[test]
fn delayed_auto_focus_from_a_superseded_native_create_reasserts_the_current_winner() {
    let mut policy = DockPolicy::default();
    policy.set_allow_native_surfaces(true);
    let mut fixture = background_fixture(policy);
    let request = install_pending_native_root_reservation(&mut fixture, RootId::new(95));
    let prepared = fixture
        .engine
        .viewport()
        .native_create_saga(request.saga())
        .expect("pending native create must retain its release-time focus claim")
        .prepared()
        .clone();
    assert!(fixture.engine.viewport_focus.reserve_activation_causal(
        request.saga(),
        prepared.focus_causal(),
        ViewportActivationRequest::tear_off_committed(request.binding(), prepared.pane_focus(),),
    ));

    let _ = fixture.engine.viewport.take_new_effects();
    advance_pending_native_root_to_awaiting_visible(&mut fixture, request);
    let source_binding = background_source_binding(&fixture);
    let winner_causal = mint_test_focus_causal(&mut fixture.engine);
    let winner = fixture
        .engine
        .start_viewport_activation(
            ViewportActivationRequest::pointer_tab_gesture(
                source_binding,
                PanelFocus::Item(ItemId::new(2)),
            ),
            winner_causal,
            &mut Vec::new(),
        )
        .expect("the newer source-window gesture must become the focus winner");
    assert!(
        matches!(
            winner.outcome(),
            ActivationStartOutcome::ObserveOnlyRecorded { .. }
        ),
        "unexpected winner activation: {:?}",
        winner.outcome()
    );
    let visible = background_native_window_snapshot_with_focus(
        &mut fixture,
        request,
        WindowPresentationState::Visible,
        4,
        true,
        GlobalFocusedWindow::Foreign,
    );
    let provider = test_platform_provider(&mut fixture.engine);
    let expected_epoch = fixture.engine.version().epoch();
    let visible = submit_test_input(
        &mut fixture.engine,
        fixture.presentation_host,
        EngineInput::PublishPlatformSnapshot {
            provider,
            expected_epoch,
            snapshot: visible,
        },
    )
    .expect("the superseded native create must still commit its topology");
    assert!(
        visible
            .platform_effects()
            .iter()
            .all(|effect| { !matches!(effect.effect(), PlatformEffect::RequestFocus { .. }) })
    );
    let post_show = current_native_staging_presentation(
        &fixture,
        request,
        crate::presentation_observation::NativeStagingPresentationPhase::PostShow,
    );
    let _ = present_background_native_staging(&mut fixture, post_show);
    let bounds =
        LogicalRect::new(0.0, 0.0, 300.0, 220.0).expect("native target test bounds must be valid");
    let measurements = surface_measurements(&fixture.engine, request.binding().surface(), bounds);
    let (_, first_live) = publish_surface_projection_with_transition(
        &mut fixture.engine,
        fixture.presentation_host,
        request.binding().surface(),
        measurements,
    );
    assert!(first_live.platform_effects().iter().any(|effect| {
        matches!(
            effect.effect(),
            PlatformEffect::RequestFocus { binding, .. } if *binding == source_binding
        )
    }));
    assert!(
        fixture
            .engine
            .viewport_focus
            .suppressed_tear_off_bindings()
            .contains(&request.binding())
    );

    let delayed_auto_focus = background_native_window_snapshot_with_focus(
        &mut fixture,
        request,
        WindowPresentationState::Visible,
        5,
        true,
        GlobalFocusedWindow::Dock(request.binding()),
    );
    let provider = test_platform_provider(&mut fixture.engine);
    let expected_epoch = fixture.engine.version().epoch();
    let delayed = submit_test_input(
        &mut fixture.engine,
        fixture.presentation_host,
        EngineInput::PublishPlatformSnapshot {
            provider,
            expected_epoch,
            snapshot: delayed_auto_focus,
        },
    )
    .expect("the delayed auto-focus edge must reduce atomically");
    assert!(
        delayed
            .platform_effects()
            .iter()
            .all(|effect| { !matches!(effect.effect(), PlatformEffect::RequestFocus { .. }) })
    );
    assert_eq!(
        fixture
            .engine
            .viewport_focus()
            .pending_activation()
            .map(crate::viewport_focus::PendingViewportActivation::request),
        Some(ViewportActivationRequest::pointer_tab_gesture(
            source_binding,
            PanelFocus::Item(ItemId::new(2)),
        ))
    );
    assert!(
        fixture
            .engine
            .viewport_focus
            .suppressed_tear_off_bindings()
            .contains(&request.binding()),
        "the old binding must remain isolated until the exact focus barrier settles"
    );

    let winner_observed = background_native_window_snapshot_with_focus(
        &mut fixture,
        request,
        WindowPresentationState::Visible,
        6,
        true,
        GlobalFocusedWindow::Dock(source_binding),
    );
    let provider = test_platform_provider(&mut fixture.engine);
    let expected_epoch = fixture.engine.version().epoch();
    submit_test_input(
        &mut fixture.engine,
        fixture.presentation_host,
        EngineInput::PublishPlatformSnapshot {
            provider,
            expected_epoch,
            snapshot: winner_observed,
        },
    )
    .expect("the compensating focus observation must settle the winning activation");
    assert!(
        fixture
            .engine
            .viewport_focus
            .suppressed_tear_off_bindings()
            .is_empty(),
        "the exact winner observation must retire the lifecycle quarantine"
    );

    let later_user_focus = background_native_window_snapshot_with_focus(
        &mut fixture,
        request,
        WindowPresentationState::Visible,
        7,
        true,
        GlobalFocusedWindow::Dock(request.binding()),
    );
    let provider = test_platform_provider(&mut fixture.engine);
    let expected_epoch = fixture.engine.version().epoch();
    submit_test_input(
        &mut fixture.engine,
        fixture.presentation_host,
        EngineInput::PublishPlatformSnapshot {
            provider,
            expected_epoch,
            snapshot: later_user_focus,
        },
    )
    .expect("a later focus edge must not remain permanently quarantined");
    assert_eq!(
        fixture.engine.viewport_focus().winning_focus_target(),
        Some(request.binding())
    );
}

#[test]
fn tick_final_vacancy_suppresses_focus_for_a_newly_visible_native_binding() {
    const NATIVE_SURFACE: SurfaceId = SurfaceId::new(93);
    const DESTINATION_FLOATING: FloatingPresentationId = FloatingPresentationId::new(94);
    const COMMAND_SOURCE: StableInputSourceId = StableInputSourceId::new(97);

    let mut policy = DockPolicy::default();
    policy.set_allow_native_surfaces(true);
    let mut fixture = background_fixture(policy);
    let reserved_root = RootId::new(94);
    let request = install_pending_native_root_reservation(&mut fixture, reserved_root);
    let prepared = fixture
        .engine
        .viewport()
        .native_create_saga(request.saga())
        .expect("pending native create must retain its prepared command")
        .prepared()
        .clone();
    let mut after_native_commit = fixture.engine.workspace().clone();
    WorkspaceTransaction::from_commands([prepared.command().clone()])
        .apply(&mut after_native_commit, fixture.engine.policy_snapshot())
        .expect("prepared native command must remain applicable");
    let source = after_native_commit
        .root(reserved_root)
        .and_then(|root| {
            after_native_commit
                .capture_node_source(reserved_root, root.node)
                .ok()
        })
        .expect("prepared native root must remain rehomeable");
    let expected_after_native_commit = WorkspaceVersion::new(
        fixture.engine.version().epoch(),
        fixture
            .engine
            .version()
            .revision()
            .checked_next()
            .expect("fixture revision can advance once"),
    );

    let _ = fixture.engine.viewport.take_new_effects();
    advance_pending_native_root_to_awaiting_visible(&mut fixture, request);
    let snapshot = background_native_window_snapshot(
        &mut fixture,
        request,
        WindowPresentationState::Visible,
        4,
        true,
    );
    let provider = test_platform_provider(&mut fixture.engine);
    let expected_epoch = fixture.engine.version().epoch();
    let visible = submit_test_input(
        &mut fixture.engine,
        fixture.presentation_host,
        EngineInput::PublishPlatformSnapshot {
            provider,
            expected_epoch,
            snapshot,
        },
    )
    .expect("visible native observation must reduce before post-show presentation");
    assert!(
        visible
            .platform_effects()
            .iter()
            .all(|effect| { !matches!(effect.effect(), PlatformEffect::RequestFocus { .. }) })
    );
    let post_show = current_native_staging_presentation(
        &fixture,
        request,
        crate::presentation_observation::NativeStagingPresentationPhase::PostShow,
    );
    let post_show_output = emit_background_native_staging(&mut fixture, post_show);

    let mut prelude = fixture
        .engine
        .begin_host_frame(fixture.presentation_host)
        .expect("post-show settlement frame must begin");
    prelude
        .submit_presentation_observation(HostPresentationObservation::Batch(vec![
            HostPresentationObservationEntry::new(
                post_show_output.stream(),
                HostPresentationStreamObservation::Captured {
                    generation: HostPresentationCaptureGeneration::new(
                        post_show_output.key().ordinal_for_test(),
                    ),
                    progress: HostPresentationProgress::Retired {
                        settled_through: post_show_output.key(),
                        presented: Authority::Known(Some(post_show_output.key())),
                    },
                },
            ),
        ]))
        .expect("post-show presentation proof must submit");
    let mut frame = prelude
        .seal(&fixture.engine)
        .expect("post-show proof must transfer before semantic input");
    let mut semantic_writer = TestInputStream::resume(&fixture.engine, COMMAND_SOURCE);
    semantic_writer
        .append(
            &mut frame,
            EngineInput::WorkspaceCommand {
                expected: expected_after_native_commit,
                command: WorkspaceCommand::RehomeRoot {
                    source,
                    target: RootPresentationTarget::Contained {
                        surface: TARGET_SURFACE,
                        floating: DESTINATION_FLOATING,
                        rect: background_contained_rect(),
                        position: ContainedPosition::Front,
                    },
                },
            },
        )
        .expect("vacating command belongs to the semantic phase");
    complete_host_frame_with_explicit_surface_roster(&fixture.engine, &mut frame);
    let transition = frame
        .finish(&mut fixture.engine)
        .expect("visible admission and final vacancy must commit atomically");

    assert!(fixture.engine.workspace().surface(NATIVE_SURFACE).is_none());
    assert!(fixture.engine.viewport().viewport(NATIVE_SURFACE).is_none());
    assert!(transition.platform_effects().iter().any(|effect| {
        matches!(
            effect.effect(),
            PlatformEffect::ReleaseChild { binding } if *binding == request.binding()
        )
    }));
    assert!(transition.platform_effects().iter().all(|effect| {
        !matches!(
            effect.effect(),
            PlatformEffect::RequestFocus { binding, .. } if *binding == request.binding()
        )
    }));
    assert!(
        fixture
            .engine
            .viewport_focus()
            .pending_activation()
            .is_none(),
        "the final vacancy must clear the native activation before focus effect emission"
    );
}

#[test]
fn tick_final_runtime_child_vacancy_emits_one_release_before_effect_extraction() {
    const NATIVE_SURFACE: SurfaceId = SurfaceId::new(93);
    const DESTINATION_FLOATING: FloatingPresentationId = FloatingPresentationId::new(94);
    const INPUT_SOURCE: StableInputSourceId = StableInputSourceId::new(94);

    let mut policy = DockPolicy::default();
    policy.set_allow_native_surfaces(true);
    let mut fixture = background_fixture(policy);
    let reserved_root = RootId::new(94);
    let request = install_pending_native_root_reservation(&mut fixture, reserved_root);
    let create_effects = fixture.engine.viewport.take_new_effects();
    assert!(create_effects.iter().any(|effect| {
        effect.id() == request.effect()
            && matches!(
                effect.effect(),
                crate::effect::PlatformEffect::CreateWindow { binding, .. }
                    if *binding == request.binding()
            )
    }));

    let _ = advance_pending_native_root_to_first_live(&mut fixture, request);
    assert!(fixture.engine.workspace().surface(NATIVE_SURFACE).is_some());
    assert!(
        fixture
            .engine
            .viewport()
            .native_create_saga(request.saga())
            .is_none(),
        "commit must consume the active saga"
    );
    assert!(!fixture.engine.native_create_reserves_root(reserved_root));
    assert!(
        !fixture
            .engine
            .native_create_reserves_floating(FloatingPresentationId::new(93))
    );
    assert!(
        fixture
            .engine
            .bound_surface_recoveries
            .get(&NATIVE_SURFACE)
            .is_some_and(|bound| bound.binding == request.binding())
    );

    let root = fixture
        .engine
        .workspace()
        .root(reserved_root)
        .expect("reserved root was installed");
    let source = fixture
        .engine
        .workspace()
        .capture_node_source(reserved_root, root.node)
        .expect("reserved root remains capturable");
    let expected = fixture.engine.version();
    let mut frame = begin_test_host_frame(&fixture.engine, fixture.presentation_host);
    let mut semantic_writer = TestInputStream::resume(&fixture.engine, INPUT_SOURCE);
    semantic_writer
        .append(
            &mut frame,
            EngineInput::WorkspaceCommand {
                expected,
                command: WorkspaceCommand::RehomeRoot {
                    source,
                    target: RootPresentationTarget::Contained {
                        surface: TARGET_SURFACE,
                        floating: DESTINATION_FLOATING,
                        rect: background_contained_rect(),
                        position: ContainedPosition::Front,
                    },
                },
            },
        )
        .expect("test input must fit the semantic host-frame phase");
    complete_host_frame_with_explicit_surface_roster(&fixture.engine, &mut frame);
    let vacated = frame
        .finish(&mut fixture.engine)
        .expect("runtime-owned surface vacancy commits");

    assert!(fixture.engine.workspace().surface(NATIVE_SURFACE).is_none());
    let releases = vacated
        .platform_effects()
        .iter()
        .filter(|effect| {
            matches!(
                effect.effect(),
                crate::effect::PlatformEffect::ReleaseChild { binding }
                    if *binding == request.binding()
            )
        })
        .count();
    assert_eq!(releases, 1);
    assert!(fixture.engine.viewport().viewport(NATIVE_SURFACE).is_none());
    assert!(
        fixture
            .engine
            .viewport()
            .binding_retirements()
            .any(|(binding, retirement)| binding == request.binding()
                && matches!(
                    retirement.status(),
                    crate::frame::BindingRetirementStatus::CleanupRequested { .. }
                ))
    );

    let mut next_frame = begin_test_host_frame(&fixture.engine, fixture.presentation_host);
    complete_host_frame_with_explicit_surface_roster(&fixture.engine, &mut next_frame);
    let next = next_frame
        .finish(&mut fixture.engine)
        .expect("the next empty tick commits");
    assert!(next.platform_effects().iter().all(|effect| {
        !matches!(
            effect.effect(),
            crate::effect::PlatformEffect::ReleaseChild { binding }
                if *binding == request.binding()
        )
    }));
}

#[test]
fn indeterminate_create_survives_inventory_absence_and_its_origin_survives_restore() {
    let mut policy = DockPolicy::default();
    policy.set_allow_native_surfaces(true);
    let mut fixture = background_fixture(policy);
    let reserved_root = RootId::new(94);
    let request = install_pending_native_root_reservation(&mut fixture, reserved_root);
    assert!(
        fixture
            .engine
            .viewport
            .take_new_effects()
            .iter()
            .any(|effect| {
                effect.id() == request.effect()
                    && matches!(
                        effect.effect(),
                        PlatformEffect::CreateWindow { binding, .. }
                            if *binding == request.binding()
                    )
            })
    );
    assert_eq!(
        fixture
            .engine
            .viewport
            .report_effect(
                fixture.engine.version().epoch(),
                EffectResult::new(
                    request.effect(),
                    request.binding().epoch(),
                    EffectDispatchResult::Indeterminate(
                        crate::effect::EffectIndeterminateReason::AcknowledgementLost,
                    ),
                ),
            )
            .expect("indeterminate create report must reduce"),
        EffectTransition::Applied
    );

    let expected_epoch = fixture.engine.version().epoch();
    let source_binding = background_source_binding(&fixture);
    let provider = test_platform_provider(&mut fixture.engine);
    submit_test_input(
        &mut fixture.engine,
        fixture.presentation_host,
        EngineInput::PublishPlatformSnapshot {
            provider,
            expected_epoch,
            snapshot: background_platform_snapshot(
                source_binding,
                Some(
                    PhysicalRect::new(0.0, 0.0, 400.0, 300.0).expect("source bounds must be valid"),
                ),
                2,
                false,
            ),
        },
    )
    .expect("authoritative absence must not terminate an active create");
    assert!(matches!(
        fixture
            .engine
            .viewport()
            .native_create_saga(request.saga())
            .map(crate::frame::NativeCreateSaga::phase),
        Some(crate::frame::NativeCreatePhase::AwaitingHidden { create })
            if create == request.effect()
    ));

    assert_eq!(
        fixture
            .engine
            .viewport
            .cancel_native_create(request.saga())
            .expect("indeterminate create cancellation must reduce"),
        None
    );
    let retirement = fixture
        .engine
        .viewport()
        .binding_retirements()
        .find_map(|(binding, retirement)| (binding == request.binding()).then_some(retirement))
        .expect("an emitted create must retain exact compensation ownership");
    let origin = retirement.origin();
    assert_eq!(
        origin,
        crate::frame::BindingRetirementOrigin::NativeCreateAborted {
            create: request.effect(),
        }
    );
    assert!(retirement.may_reappear());

    fixture
        .engine
        .viewport
        .reconcile_workspace_epoch(WorkspaceEpoch::new(1), &BTreeSet::new())
        .expect("workspace restore must preserve terminal create compensation ownership");
    let restored = fixture
        .engine
        .viewport()
        .binding_retirements()
        .find_map(|(binding, retirement)| (binding == request.binding()).then_some(retirement))
        .expect("restore must retain exact compensation origin");
    assert_eq!(restored.origin(), origin);
    assert_eq!(
        restored.status(),
        crate::frame::BindingRetirementStatus::AwaitingAppearance
    );
}

#[test]
fn definitive_create_failure_releases_the_reserved_surface_immediately() {
    let mut policy = DockPolicy::default();
    policy.set_allow_native_surfaces(true);
    let mut fixture = background_fixture(policy);
    let reserved_root = RootId::new(94);
    let request = install_pending_native_root_reservation(&mut fixture, reserved_root);
    let _ = fixture.engine.viewport.take_new_effects();

    assert_eq!(
        fixture
            .engine
            .viewport
            .report_effect(
                fixture.engine.version().epoch(),
                EffectResult::new(
                    request.effect(),
                    request.binding().epoch(),
                    EffectDispatchResult::DispatchFailed(
                        crate::effect::DispatchFailureReason::WindowUnavailable,
                    ),
                ),
            )
            .expect("definitive create failure must reduce"),
        EffectTransition::Applied
    );
    assert!(
        fixture
            .engine
            .viewport()
            .native_create_saga(request.saga())
            .is_none()
    );
    assert!(
        fixture
            .engine
            .viewport()
            .viewport(request.binding().surface())
            .is_none()
    );
    assert!(
        fixture
            .engine
            .viewport()
            .binding_retirements()
            .all(|(binding, _)| binding != request.binding())
    );
    assert!(!fixture.engine.native_create_reserves_root(reserved_root));
    assert!(fixture.engine.viewport.take_new_effects().is_empty());
}

#[test]
fn explicit_tick_fatal_reduction_rolls_back_provenance_and_complete_state() {
    let root = RootId::new(1);
    let surface = SurfaceId::new(1);
    let mut builder = Workspace::builder();
    let tabs = builder.insert_node(Node::tabs([ItemId::new(1), ItemId::new(2)]));
    builder.set_root(root, RootRecord::new(tabs));
    builder.set_surface(surface, SurfacePresentation::with_main(root));
    let workspace = builder.build().expect("test workspace must be valid");
    let command = WorkspaceCommand::Select {
        source: workspace
            .capture_item_source(root, tabs, ItemId::new(2))
            .expect("source must be capturable"),
    };
    let mut engine =
        DockEngine::new(workspace, DockPolicy::default()).expect("engine must be valid");
    let presentation_host = engine
        .create_presentation_host()
        .expect("fatal reduction presentation host must mint");
    engine.version = WorkspaceVersion::new(
        WorkspaceEpoch::default(),
        WorkspaceRevision::new(u64::MAX - 1),
    );
    engine
        .try_rebuild_presentation_requirements(None)
        .expect("the exhausted-revision fixture must retain a current manifest");
    let expected = engine.version;
    let mut policy = engine.policy.to_policy();
    policy.set_allow_native_surfaces(true);
    let source = StableInputSourceId::new(41);
    let mut frame = begin_test_host_frame(&engine, presentation_host);
    frame
        .append_input(
            source,
            SourceSequence::new(1),
            EngineInput::WorkspaceCommand { expected, command },
        )
        .expect("workspace command belongs to the semantic phase");
    frame
        .append_configuration(
            source,
            SourceSequence::new(2),
            EngineInput::ReplacePolicy { expected, policy },
        )
        .expect("policy replacement belongs to the terminal configuration phase");
    complete_host_frame_with_explicit_surface_roster(&engine, &mut frame);
    let before = engine.candidate();

    assert!(matches!(
        frame.finish(&mut engine),
        Err(EngineError::WorkspaceRevisionExhausted { .. })
    ));
    assert_eq!(engine, before);
    assert_eq!(engine.last_reducer_tick(), ReducerTickId::default());
    assert_eq!(engine.last_input_sequence(), InputSequence::default());
    assert_eq!(engine.semantic_input_watermark(), None);
}

#[test]
fn explicit_tick_counter_exhaustion_is_atomic() {
    let mut fixture = counter_fixture();
    fixture.engine.last_reducer_tick = ReducerTickId::new(u64::MAX);
    let before_tick_exhaustion = fixture.engine.candidate();

    let mut prelude = fixture
        .engine
        .begin_host_frame(fixture.presentation_host)
        .expect("tick-exhaustion prelude must begin");
    prelude
        .submit_presentation_observation(HostPresentationObservation::NoUpdate)
        .expect("tick-exhaustion observation must stage");
    assert!(matches!(
        prelude.seal(&fixture.engine),
        Err(EngineError::ReducerTickExhausted)
    ));
    assert_eq!(fixture.engine, before_tick_exhaustion);

    fixture.engine.last_reducer_tick = ReducerTickId::default();
    fixture.engine.last_input = InputSequence::new(u64::MAX);
    let source = StableInputSourceId::new(42);
    let before_input_exhaustion = fixture.engine.candidate();
    let mut frame = begin_test_host_frame(&fixture.engine, fixture.presentation_host);
    assert_eq!(
        frame.append_input(
            source,
            SourceSequence::new(1),
            EngineInput::ValidateWorkspace,
        ),
        Err(CoreHostFrameError::InputPrefixReductionFailed),
        "input sequence exhaustion fails while reducing the input prefix",
    );
    assert_eq!(
        frame.finish(&mut fixture.engine),
        Err(EngineError::InputSequenceExhausted)
    );
    assert_eq!(fixture.engine, before_input_exhaustion);
    assert_eq!(fixture.engine.semantic_input_watermark(), None);
}

#[test]
fn surface_contribution_keeps_its_exact_authority_across_delayed_submission() {
    let mut fixture = counter_fixture();
    publish_counter_scene(&mut fixture);
    let stale_contribution =
        begin_surface_measurement(&fixture.engine, TARGET_SURFACE, test_rect());
    let submitted_base = stale_contribution.token().base();

    let expected = fixture.engine.version();
    let mut policy = fixture.engine.policy().clone();
    policy.set_allow_native_surfaces(true);
    submit_test_input(
        &mut fixture.engine,
        fixture.presentation_host,
        EngineInput::ReplacePolicy { expected, policy },
    )
    .expect("policy change must advance the workspace version");
    publish_counter_scene(&mut fixture);

    let before_workspace = fixture.engine.workspace().clone();
    let before_target_scene = fixture
        .engine
        .scene()
        .surface(TARGET_SURFACE)
        .cloned()
        .expect("target surface remains rostered before delayed submission");
    let before_interaction = fixture.engine.interaction().clone();

    let transition = submit_surface_measurement(
        &mut fixture.engine,
        fixture.presentation_host,
        stale_contribution,
    );

    assert!(matches!(
        transition.surface_contributions().iter().find(|outcome| outcome.surface() == TARGET_SURFACE),
        Some(SurfaceContributionOutcome::Rejected {
            surface: TARGET_SURFACE,
            reason: SurfaceContributionRejection::StaleBase {
                submitted,
                current: Some(current),
            },
        }) if *submitted == submitted_base && *current != submitted_base
    ));
    assert!(transition.events().is_empty());
    assert!(transition.interaction_events().is_empty());
    assert_eq!(fixture.engine.workspace(), &before_workspace);
    assert_eq!(
        fixture.engine.scene().surface(TARGET_SURFACE),
        Some(&before_target_scene),
        "the rejected delayed contribution cannot mutate its own target surface"
    );
    assert_eq!(fixture.engine.interaction(), &before_interaction);
}

#[test]
fn rootless_contained_surface_remains_focusable_routeable_and_registerable() {
    let rootless_surface = SurfaceId::new(20);
    let target_surface = SurfaceId::new(21);
    let contained_root = RootId::new(20);
    let target_root = RootId::new(21);
    let floating = FloatingPresentationId::new(20);
    let item = ItemId::new(20);
    let mut builder = Workspace::builder();
    let contained_tabs = builder.insert_node(Node::tabs([item]));
    let target_tabs = builder.insert_node(Node::tabs([ItemId::new(21)]));
    builder.set_root(contained_root, RootRecord::new(contained_tabs));
    builder.set_root(target_root, RootRecord::new(target_tabs));
    builder.set_surface(rootless_surface, SurfacePresentation::rootless());
    builder.set_surface(target_surface, SurfacePresentation::with_main(target_root));
    builder.set_contained_floating(
        floating,
        ContainedFloating::new(contained_root, test_rect()),
    );
    builder
        .attach_contained(rootless_surface, floating)
        .expect("rootless surface draft must exist");
    let workspace = builder.build().expect("rootless workspace must be valid");
    let mut engine =
        DockEngine::new(workspace, DockPolicy::default()).expect("rootless engine must be valid");
    let presentation_host = engine
        .create_presentation_host()
        .expect("rootless presentation host must mint");
    let provider = test_platform_provider(&mut engine);

    assert!(engine.surface_items(rootless_surface).contains(&item));
    assert!(
        DockEngine::capture_surface_item_source(engine.workspace(), rootless_surface, item,)
            .is_some()
    );
    assert_eq!(
        engine.payload_surface(&MovePayload::Item(
            engine
                .workspace()
                .capture_item_source(contained_root, contained_tabs, item)
                .expect("contained item must be current"),
        )),
        Some(rootless_surface)
    );

    let expected = engine.version();
    submit_test_input(
        &mut engine,
        presentation_host,
        EngineInput::RegisterViewport {
            provider,
            expected,
            surface: target_surface,
            token: WindowToken::new(21),
            role: ViewportRole::Root,
            recovery_target: None,
        },
    )
    .expect("target root anchor registration must reduce");
    let anchor = engine
        .root_recovery_anchor(target_surface)
        .expect("target root must own a recovery anchor");
    let expected = engine.version();
    let registered = submit_test_input(
        &mut engine,
        presentation_host,
        EngineInput::RegisterViewport {
            provider,
            expected,
            surface: rootless_surface,
            token: WindowToken::new(20),
            role: ViewportRole::Child,
            recovery_target: Some(SurfaceRecoveryTarget::forest_only(anchor)),
        },
    )
    .expect("rootless child registration must reduce");
    assert!(matches!(
        registered.reduced_inputs()[0].outcome(),
        InputOutcome::ViewportRegistered { binding }
            if binding.surface() == rootless_surface
    ));
}

#[test]
fn child_viewport_bootstrap_mints_its_recovery_identity_in_core() {
    let host_surface = SurfaceId::new(22);
    let child_surface = SurfaceId::new(23);
    let host_root = RootId::new(22);
    let child_root = RootId::new(23);
    let mut builder = Workspace::builder();
    let host_tabs = builder.insert_node(Node::tabs([ItemId::new(22)]));
    let child_tabs = builder.insert_node(Node::tabs([ItemId::new(23)]));
    builder.set_root(host_root, RootRecord::new(host_tabs));
    builder.set_root(child_root, RootRecord::new(child_tabs));
    builder.set_surface(host_surface, SurfacePresentation::with_main(host_root));
    builder.set_surface(child_surface, SurfacePresentation::with_main(child_root));
    let workspace = builder.build().expect("bootstrap workspace must be valid");
    let mut engine =
        DockEngine::new(workspace, DockPolicy::default()).expect("bootstrap engine must be valid");
    let presentation_host = engine
        .create_presentation_host()
        .expect("bootstrap presentation host must mint");
    let provider = test_platform_provider(&mut engine);

    let expected = engine.version();
    submit_test_input(
        &mut engine,
        presentation_host,
        EngineInput::RegisterViewport {
            provider,
            expected,
            surface: host_surface,
            token: WindowToken::new(22),
            role: ViewportRole::Root,
            recovery_target: None,
        },
    )
    .expect("recovery host registration must reduce");
    let frontier_before = engine.presentation_identity_frontier();
    let minimum_size = engine.presentation_config().minimum_floating_size();
    let expected = engine.version();
    let registered = submit_test_input(
        &mut engine,
        presentation_host,
        EngineInput::BootstrapChildViewport {
            provider,
            expected,
            surface: child_surface,
            token: WindowToken::new(23),
            recovery: SurfaceRecoveryBootstrap::new(host_surface),
        },
    )
    .expect("child bootstrap registration must reduce");

    assert!(matches!(
        registered.reduced_inputs()[0].outcome(),
        InputOutcome::ViewportRegistered { binding }
            if binding.surface() == child_surface
    ));
    let target = engine
        .bound_surface_recoveries
        .get(&child_surface)
        .expect("registered child must retain a recovery obligation")
        .obligation
        .target();
    assert_eq!(target.host_surface(), host_surface);
    let converted = target
        .converted_main()
        .expect("rooted child must reserve a converted-main presentation");
    assert_eq!(converted.source_root(), child_root);
    assert_eq!(converted.minimum_size(), minimum_size);
    assert!(converted.floating().get() > frontier_before.last_floating());
    assert_eq!(
        engine.presentation_identity_frontier().last_floating(),
        converted.floating().get()
    );
}

#[test]
fn rejected_child_viewport_bootstrap_does_not_consume_a_floating_identity() {
    let host_surface = SurfaceId::new(24);
    let child_surface = SurfaceId::new(25);
    let host_root = RootId::new(24);
    let child_root = RootId::new(25);
    let mut builder = Workspace::builder();
    let host_tabs = builder.insert_node(Node::tabs([ItemId::new(24)]));
    let child_tabs = builder.insert_node(Node::tabs([ItemId::new(25)]));
    builder.set_root(host_root, RootRecord::new(host_tabs));
    builder.set_root(child_root, RootRecord::new(child_tabs));
    builder.set_surface(host_surface, SurfacePresentation::with_main(host_root));
    builder.set_surface(child_surface, SurfacePresentation::with_main(child_root));
    let workspace = builder.build().expect("bootstrap workspace must be valid");
    let mut policy = DockPolicy::default();
    policy.set_allow_contained_floating(false);
    let mut engine = DockEngine::new(workspace, policy).expect("bootstrap engine must be valid");
    let presentation_host = engine
        .create_presentation_host()
        .expect("bootstrap presentation host must mint");
    let provider = test_platform_provider(&mut engine);

    let expected = engine.version();
    submit_test_input(
        &mut engine,
        presentation_host,
        EngineInput::RegisterViewport {
            provider,
            expected,
            surface: host_surface,
            token: WindowToken::new(24),
            role: ViewportRole::Root,
            recovery_target: None,
        },
    )
    .expect("recovery host registration must reduce");
    let frontier_before = engine.presentation_identity_frontier();
    let expected = engine.version();
    let rejected = submit_test_input(
        &mut engine,
        presentation_host,
        EngineInput::BootstrapChildViewport {
            provider,
            expected,
            surface: child_surface,
            token: WindowToken::new(25),
            recovery: SurfaceRecoveryBootstrap::new(host_surface),
        },
    )
    .expect("policy rejection must still reduce the input");

    assert!(matches!(
        rejected.reduced_inputs()[0].outcome(),
        InputOutcome::ViewportRegistrationRejected { surface } if *surface == child_surface
    ));
    assert_eq!(engine.presentation_identity_frontier(), frontier_before);
    assert!(engine.viewport().viewport(child_surface).is_none());
}

#[test]
fn main_departure_is_not_vacancy_until_the_last_contained_root_leaves() {
    let source_surface = SurfaceId::new(30);
    let target_surface = SurfaceId::new(31);
    let main_root = RootId::new(30);
    let sibling_root = RootId::new(31);
    let target_root = RootId::new(32);
    let sibling = FloatingPresentationId::new(30);
    let moved_main = FloatingPresentationId::new(31);
    let mut builder = Workspace::builder();
    let main_tabs = builder.insert_node(Node::tabs([ItemId::new(30)]));
    let sibling_tabs = builder.insert_node(Node::tabs([ItemId::new(31)]));
    let target_tabs = builder.insert_node(Node::tabs([ItemId::new(32)]));
    builder.set_root(main_root, RootRecord::new(main_tabs));
    builder.set_root(sibling_root, RootRecord::new(sibling_tabs));
    builder.set_root(target_root, RootRecord::new(target_tabs));
    builder.set_surface(source_surface, SurfacePresentation::with_main(main_root));
    builder.set_surface(target_surface, SurfacePresentation::with_main(target_root));
    builder.set_contained_floating(sibling, ContainedFloating::new(sibling_root, test_rect()));
    builder
        .attach_contained(source_surface, sibling)
        .expect("source surface must exist");
    let workspace = builder.build().expect("workspace must be valid");
    let mut engine =
        DockEngine::new(workspace, DockPolicy::default()).expect("engine must be valid");
    let presentation_host = engine
        .create_presentation_host()
        .expect("vacancy presentation host must mint");
    let provider = test_platform_provider(&mut engine);
    let expected = engine.version();
    submit_test_input(
        &mut engine,
        presentation_host,
        EngineInput::RegisterViewport {
            provider,
            expected,
            surface: source_surface,
            token: WindowToken::new(30),
            role: ViewportRole::Root,
            recovery_target: None,
        },
    )
    .expect("source viewport registration must reduce");
    let main_source = engine
        .workspace()
        .capture_node_source(main_root, main_tabs)
        .expect("main root must be current");
    let mut events = Vec::new();
    let application = engine
        .apply_interaction_command(
            InputSequence::new(30),
            &WorkspaceCommand::RehomeRoot {
                source: main_source,
                target: RootPresentationTarget::Contained {
                    surface: target_surface,
                    floating: moved_main,
                    rect: test_rect(),
                    position: ContainedPosition::Front,
                },
            },
            &mut events,
        )
        .expect("main root rehome must reduce");
    assert!(matches!(
        application,
        CommandApplication::Applied { changed: true, .. }
    ));
    let source = engine
        .workspace()
        .surface(source_surface)
        .expect("contained sibling must keep the source surface alive");
    assert_eq!(source.main_root, None);
    assert_eq!(source.contained, vec![sibling]);
    assert_no_pointer_passthrough_effects(&engine);

    let sibling_source = engine
        .workspace()
        .capture_node_source(sibling_root, sibling_tabs)
        .expect("contained sibling must remain current");
    engine
        .apply_interaction_command(
            InputSequence::new(31),
            &WorkspaceCommand::RehomeRoot {
                source: sibling_source,
                target: RootPresentationTarget::Contained {
                    surface: target_surface,
                    floating: sibling,
                    rect: test_rect(),
                    position: ContainedPosition::Front,
                },
            },
            &mut events,
        )
        .expect("last contained rehome must reduce");
    assert!(engine.workspace().surface(source_surface).is_none());
    assert_eq!(
        engine
            .workspace()
            .surface(target_surface)
            .expect("target surface must remain")
            .contained,
        vec![moved_main, sibling]
    );
}

#[test]
fn stale_roster_precondition_is_a_nonpublishing_recovery_rejection() {
    let source_surface = SurfaceId::new(40);
    let target_surface = SurfaceId::new(41);
    let source_root = RootId::new(40);
    let target_root = RootId::new(41);
    let floating = FloatingPresentationId::new(40);
    let mut builder = Workspace::builder();
    let source_tabs = builder.insert_node(Node::tabs([ItemId::new(40), ItemId::new(42)]));
    let target_tabs = builder.insert_node(Node::tabs([ItemId::new(41)]));
    builder.set_root(source_root, RootRecord::new(source_tabs));
    builder.set_root(target_root, RootRecord::new(target_tabs));
    builder.set_surface(source_surface, SurfacePresentation::with_main(source_root));
    builder.set_surface(target_surface, SurfacePresentation::with_main(target_root));
    let workspace = builder.build().expect("workspace must be valid");
    let mut engine =
        DockEngine::new(workspace, DockPolicy::default()).expect("engine must be valid");
    let roster = SurfaceRosterDisposition::capture(engine.workspace(), source_surface, None)
        .expect("source roster must freeze");
    let placement = SurfaceRehomePlacement::new(
        target_surface,
        Some(SurfaceMainRehome::Present(
            RootPresentationTarget::Contained {
                surface: target_surface,
                floating,
                rect: test_rect(),
                position: ContainedPosition::Front,
            },
        )),
        Vec::new(),
    );
    let transaction = roster
        .compile_rehome_transaction(engine.workspace(), &placement)
        .expect("current roster must compile");
    let Some(Node::Tabs { selected, .. }) = engine.workspace.nodes.get_mut(source_tabs) else {
        panic!("source root must remain tabs");
    };
    *selected = Some(ItemId::new(42));
    engine
        .workspace
        .tab_mru
        .insert(source_tabs, vec![ItemId::new(42), ItemId::new(40)]);
    engine
        .workspace
        .validate()
        .expect("stale source mutation must remain valid");
    let before = engine.workspace().clone();
    let version = engine.version();
    let mut events = Vec::new();

    let applied = engine
        .apply_surface_roster_transaction(
            InputSequence::new(40),
            &roster,
            &transaction,
            None,
            &BTreeMap::new(),
            &mut events,
            WorkspacePublicationAuthority::Ordinary,
        )
        .expect("stale roster is an expected rejection");

    assert!(!applied);
    assert_eq!(engine.workspace(), &before);
    assert_eq!(engine.version(), version);
    assert!(events.is_empty());
}
