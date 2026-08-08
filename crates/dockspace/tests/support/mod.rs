#![allow(dead_code)]

use std::collections::{BTreeMap, BTreeSet};

use crate::platform_provider::PlatformObservationLease;
use dockspace::backend_ingress::BackendIngressRecorder;
use dockspace::engine::{
    BackendIngressProgress, CoreHostFrame, CoreHostFrameError, CoreHostFramePrelude,
    CoreHostPresentationFrame, DockEngine, EngineError, EngineInput, HostPresentationDisposition,
    HostPresentationSlot, HostPresentationUnavailableReason,
};
use dockspace::geometry::{LogicalRect, LogicalSize};
use dockspace::ids::{SourceSequence, StableInputSourceId, SurfaceId};
use dockspace::intent::{Authority, AuthorityUnavailableReason};
use dockspace::platform::{
    CapabilityRosterObservation, ObservedWindow, ObservedWorkArea, PlatformCapabilities,
    WindowInventoryObservation, WorkAreaRosterObservation,
};
use dockspace::pointer_journal::{PointerEdgeJournal, PointerEdgeSequence};
use dockspace::pointer_receiver::PointerReceiverReceiptBatch;
use dockspace::presentation_observation::NativeStagingPresentationPhase;
use dockspace::presentation_observation::{
    HostInteractionPresentation, HostPresentationCaptureGeneration, HostPresentationEmission,
    HostPresentationObservation, HostPresentationObservationEntry,
    HostPresentationObservationOutcome, HostPresentationOutput, HostPresentationProgress,
    HostPresentationStreamId, HostPresentationStreamObservation, NativeStagingPresentation,
    PresentationHostLease, PresentationHostRetirementReason,
};
use dockspace::scene::{PresentationPlan, SurfaceScene, SurfaceSceneStamp};
use dockspace::scene_manifest::SceneRequirementManifest;
use dockspace::scene_manifest::{
    Measurement, MeasurementUnavailableReason, SurfaceMeasurements, TabIntrinsic,
    TabListMenuMetrics, TabStripControlMetrics, TabStripMetrics,
};
use dockspace::transition::{EngineTransition, PresentationHostRetirementOutcome};
use dockspace::transition::{InputPriority, SurfaceContributionOutcome};
use dockspace::viewport::ViewportBinding;
use dockspace::viewport::{
    CapabilityObservationGeneration, InventoryObservationGeneration, WorkAreaObservationGeneration,
};

pub fn known_capability_observation(
    generation: u64,
    capabilities: PlatformCapabilities,
) -> CapabilityRosterObservation {
    CapabilityRosterObservation::new(
        CapabilityObservationGeneration::new(generation),
        Authority::Known(capabilities),
    )
}

pub fn known_inventory_observation(
    generation: u64,
    windows: &[ObservedWindow],
) -> WindowInventoryObservation {
    WindowInventoryObservation::new(
        InventoryObservationGeneration::new(generation),
        Authority::Known(windows.iter().map(ObservedWindow::binding).collect()),
    )
    .expect("test inventory roster must be canonical")
}

pub fn unknown_work_area_observation(generation: u64) -> WorkAreaRosterObservation {
    WorkAreaRosterObservation::new(
        WorkAreaObservationGeneration::new(generation),
        Authority::Unknown(AuthorityUnavailableReason::NotReported),
    )
    .expect("test work-area tombstone must be canonical")
}

pub fn known_work_area_observation(
    generation: u64,
    work_areas: Vec<ObservedWorkArea>,
) -> WorkAreaRosterObservation {
    WorkAreaRosterObservation::new(
        WorkAreaObservationGeneration::new(generation),
        Authority::Known(work_areas),
    )
    .expect("test work-area roster must be canonical")
}

/// Stateful in-memory presentation provider for tests that model real output.
///
/// The provider retains core-issued emissions and reports an exact, terminal
/// observation batch before its next host frame. It is intentionally explicit:
/// a test must record a paint result after it has actually selected output to
/// paint, then advance one more frame before interaction authority can exist.
#[derive(Debug)]
pub struct TestPresentationHost {
    lease: PresentationHostLease,
    platform_provider: PlatformObservationLease,
    backend_ingress: Option<BackendIngressRecorder>,
    backend_pointer_through: PointerEdgeSequence,
    last_platform_observation_generation: u64,
    pending: BTreeMap<HostPresentationStreamId, Vec<HostPresentationOutput>>,
    capture_generations: BTreeMap<HostPresentationStreamId, HostPresentationCaptureGeneration>,
    last_observed: BTreeSet<HostPresentationStreamId>,
}

impl TestPresentationHost {
    /// Creates one persistent host lease for a single test adapter instance.
    pub fn new(engine: &mut DockEngine) -> Self {
        let platform_provider = engine.platform_provider().unwrap_or_else(|| {
            engine
                .create_platform_provider()
                .expect("test platform provider lease must mint")
        });
        Self {
            lease: engine
                .create_presentation_host()
                .expect("test presentation host lease must mint"),
            platform_provider,
            backend_ingress: None,
            backend_pointer_through: PointerEdgeSequence::new(0),
            last_platform_observation_generation: 0,
            pending: BTreeMap::new(),
            capture_generations: BTreeMap::new(),
            last_observed: BTreeSet::new(),
        }
    }

    /// Creates one persistent host with a joined platform, pointer, and causal ingress owner.
    pub fn new_joined(engine: &mut DockEngine) -> Self {
        let lease = engine
            .create_presentation_host()
            .expect("test presentation host lease must mint");
        let backend_ingress = engine
            .create_backend_ingress_provider(lease, PointerEdgeSequence::new(0))
            .expect("test joined backend provider must mint");
        let platform_provider = engine
            .platform_provider()
            .expect("joined backend enrollment must publish its platform provider");
        Self {
            lease,
            platform_provider,
            backend_ingress: Some(backend_ingress),
            backend_pointer_through: PointerEdgeSequence::new(0),
            last_platform_observation_generation: 0,
            pending: BTreeMap::new(),
            capture_generations: BTreeMap::new(),
            last_observed: BTreeSet::new(),
        }
    }

    /// Returns this persistent test provider's exact core-minted lease.
    #[must_use]
    pub const fn lease(&self) -> PresentationHostLease {
        self.lease
    }

    /// Returns the persistent adapter's exact core-minted platform lease.
    #[must_use]
    pub const fn platform_provider(&self) -> PlatformObservationLease {
        self.platform_provider
    }

    /// Returns whether this host owns the joined platform, pointer, and causal ingress lane.
    #[must_use]
    pub const fn has_joined_backend(&self) -> bool {
        self.backend_ingress.is_some()
    }

    /// Takes the affine joined recorder before replacing its complete authority bundle.
    pub fn take_backend_ingress(&mut self) -> BackendIngressRecorder {
        self.backend_ingress
            .take()
            .expect("test host must own a joined backend recorder")
    }

    /// Adopts the exact joined successor returned by a completed provider handoff.
    pub fn adopt_backend_ingress(
        &mut self,
        engine: &DockEngine,
        successor: BackendIngressRecorder,
        pointer_through: PointerEdgeSequence,
    ) {
        self.platform_provider = engine
            .platform_provider()
            .expect("joined backend replacement must publish its platform successor");
        self.backend_ingress = Some(successor);
        self.backend_pointer_through = pointer_through;
        self.last_platform_observation_generation = 0;
    }

    fn stage_joined_input(&mut self, input: EngineInput) -> Option<EngineInput> {
        let Some(recorder) = self.backend_ingress.as_mut() else {
            return Some(input);
        };
        match input {
            EngineInput::PublishPlatformSnapshot {
                provider,
                expected_epoch,
                snapshot,
            } => {
                assert_eq!(provider, self.platform_provider);
                recorder
                    .record_platform_snapshot(expected_epoch, snapshot)
                    .expect("test platform snapshot must enter the joined ingress journal");
            }
            EngineInput::PublishNativeCloseObservation {
                provider,
                expected_epoch,
                observation,
            } => {
                assert_eq!(provider, self.platform_provider);
                recorder
                    .record_native_close_observation(expected_epoch, observation)
                    .expect("test native-close fact must enter the joined ingress journal");
            }
            EngineInput::ReportPlatformEffect {
                provider,
                expected_epoch,
                result,
            } => {
                assert_eq!(provider, self.platform_provider);
                assert_eq!(expected_epoch, result.receipt_epoch());
                recorder
                    .record_platform_effect_result(result)
                    .expect("test effect result must enter the joined ingress journal");
            }
            input if input.priority() == InputPriority::ConfigurationCommit => return Some(input),
            input => {
                recorder
                    .record_semantic_input(input)
                    .expect("test semantic input must enter the joined ingress journal");
            }
        }
        None
    }

    /// Allocates the next provider-owned generation without consulting core state.
    pub fn next_platform_observation_generation(&mut self) -> u64 {
        self.last_platform_observation_generation = self
            .last_platform_observation_generation
            .checked_add(1)
            .expect("test platform observation generation must not exhaust");
        self.last_platform_observation_generation
    }

    /// Begins a frame and submits a complete terminal observation for every
    /// emission still pending from this host.
    pub fn begin(&mut self, engine: &DockEngine) -> CoreHostFrame {
        let mut prelude = engine
            .begin_host_frame(self.lease)
            .expect("persistent test host frame must begin");
        self.submit_observation(&mut prelude);
        let mut frame = prelude
            .seal(engine)
            .expect("persistent test host frame must seal");
        if let Some(recorder) = &mut self.backend_ingress {
            let pointer = self.backend_pointer_through;
            recorder
                .record_pointer_segment(
                    PointerEdgeJournal::new(pointer, pointer, Vec::new())
                        .expect("empty joined pointer checkpoint must preserve its watermark"),
                )
                .expect("test joined backend must record one pointer checkpoint per frame");
            let batch = recorder
                .batch_after(engine.backend_ingress_committed_through())
                .expect("test joined ingress suffix must freeze");
            let mut progress = frame
                .submit_backend_ingress(batch)
                .expect("test joined ingress suffix must reduce");
            while progress == BackendIngressProgress::ReceiverReceiptsRequired {
                assert!(
                    frame
                        .pointer_receiver_candidates()
                        .is_some_and(|roster| roster.candidates().is_empty()),
                    "joined recovery fixtures submit only empty pointer checkpoints",
                );
                progress = frame
                    .submit_backend_pointer_receiver_receipts(
                        PointerReceiverReceiptBatch::new([])
                            .expect("empty pointer checkpoint has no receiver probes"),
                    )
                    .expect("empty pointer checkpoint must resume joined replay");
            }
            assert_eq!(progress, BackendIngressProgress::Complete);
        }
        frame
    }

    /// Begins a joined frame whose next causal record is one exact pointer segment.
    ///
    /// The returned progress may require the caller to inspect the frozen receiver
    /// roster and submit matching receipts before completing the frame.
    pub fn begin_joined_pointer_frame(
        &mut self,
        engine: &DockEngine,
        semantic_before_pointer: impl IntoIterator<Item = EngineInput>,
        segment: PointerEdgeJournal,
    ) -> (CoreHostFrame, BackendIngressProgress) {
        for input in semantic_before_pointer {
            assert!(
                self.stage_joined_input(input).is_none(),
                "joined pointer frames require every semantic input to enter the backend journal"
            );
        }
        let through = segment.through();
        self.backend_ingress
            .as_mut()
            .expect("joined pointer frame requires a backend recorder")
            .record_pointer_segment(segment)
            .expect("joined pointer segment must continue its recorder watermark");
        self.backend_pointer_through = through;

        let mut prelude = engine
            .begin_host_frame(self.lease)
            .expect("joined pointer host frame must begin");
        self.submit_observation(&mut prelude);
        let mut frame = prelude
            .seal(engine)
            .expect("joined pointer host frame must seal");
        let batch = self
            .backend_ingress
            .as_ref()
            .expect("joined pointer frame retains its recorder")
            .batch_after(engine.backend_ingress_committed_through())
            .expect("joined pointer suffix must freeze");
        let progress = frame
            .submit_backend_ingress(batch)
            .expect("joined pointer suffix must reduce");
        (frame, progress)
    }

    /// Submits this provider's complete pending observation into an already
    /// frozen host frame. The frame must have enrolled this host as either its
    /// rendering host or an observation-only participant.
    pub fn submit_observation(&mut self, frame: &mut CoreHostFramePrelude) {
        let scope = frame
            .pending_presentation_streams_for(self.lease)
            .expect("test provider must be enrolled in the host frame")
            .into_iter()
            .collect::<BTreeSet<_>>();
        assert_eq!(
            scope,
            self.pending.keys().copied().collect(),
            "test provider must retain every core-issued pending stream"
        );
        if scope.is_empty() {
            frame
                .submit_presentation_observation_for(
                    self.lease,
                    HostPresentationObservation::NoUpdate,
                )
                .expect("empty test host observation must submit");
        } else {
            let mut entries = Vec::with_capacity(scope.len());
            for stream in &scope {
                let outputs = self
                    .pending
                    .get(stream)
                    .expect("pending stream has retained test output");
                let settled_through = outputs
                    .last()
                    .expect("pending stream has at least one output")
                    .key();
                let generation = self
                    .capture_generations
                    .get(stream)
                    .copied()
                    .unwrap_or_default()
                    .checked_next()
                    .expect("test capture generation must not exhaust");
                self.capture_generations.insert(*stream, generation);
                entries.push(HostPresentationObservationEntry::new(
                    *stream,
                    HostPresentationStreamObservation::Captured {
                        generation,
                        progress: HostPresentationProgress::Retired {
                            settled_through,
                            presented: Authority::Known(Some(settled_through)),
                        },
                    },
                ));
            }
            frame
                .submit_presentation_observation_for(
                    self.lease,
                    HostPresentationObservation::Batch(entries),
                )
                .expect("complete test host observation must submit");
        }
        self.last_observed = scope;
    }

    /// Submits a newer capture which cannot settle any pending output.
    pub fn submit_unknown_observation(&mut self, frame: &mut CoreHostFramePrelude) {
        let scope = frame
            .pending_presentation_streams_for(self.lease)
            .expect("test provider must be enrolled in the host frame")
            .into_iter()
            .collect::<BTreeSet<_>>();
        assert!(!scope.is_empty(), "unknown capture requires pending output");
        assert_eq!(scope, self.pending.keys().copied().collect());
        let entries = scope
            .iter()
            .map(|stream| {
                let generation = self
                    .capture_generations
                    .get(stream)
                    .copied()
                    .unwrap_or_default()
                    .checked_next()
                    .expect("test capture generation must not exhaust");
                self.capture_generations.insert(*stream, generation);
                HostPresentationObservationEntry::new(
                    *stream,
                    HostPresentationStreamObservation::Captured {
                        generation,
                        progress: HostPresentationProgress::Unknown(
                            AuthorityUnavailableReason::NotReported,
                        ),
                    },
                )
            })
            .collect();
        frame
            .submit_presentation_observation_for(
                self.lease,
                HostPresentationObservation::Batch(entries),
            )
            .expect("unknown test observation must submit");
        self.last_observed = scope;
    }

    /// Submits a terminal capture proving that none of the pending outputs was
    /// finally presented.
    pub fn submit_not_presented_observation(&mut self, frame: &mut CoreHostFramePrelude) {
        let scope = frame
            .pending_presentation_streams_for(self.lease)
            .expect("test provider must be enrolled in the host frame")
            .into_iter()
            .collect::<BTreeSet<_>>();
        assert!(
            !scope.is_empty(),
            "not-presented capture requires pending output"
        );
        assert_eq!(scope, self.pending.keys().copied().collect());
        let entries = scope
            .iter()
            .map(|stream| {
                let settled_through = self
                    .pending
                    .get(stream)
                    .and_then(|outputs| outputs.last())
                    .expect("pending stream has at least one output")
                    .key();
                HostPresentationObservationEntry::new(
                    *stream,
                    HostPresentationStreamObservation::Captured {
                        generation: self.next_capture_generation(*stream),
                        progress: HostPresentationProgress::Retired {
                            settled_through,
                            presented: Authority::Known(None),
                        },
                    },
                )
            })
            .collect();
        frame
            .submit_presentation_observation_for(
                self.lease,
                HostPresentationObservation::Batch(entries),
            )
            .expect("not-presented test observation must submit");
        self.last_observed = scope;
    }

    /// Submits one exact stream batch with per-surface Presented, Unknown, or
    /// NoUpdate facts. Surfaces absent from both sets explicitly report NoUpdate.
    pub fn submit_partitioned_observation(
        &mut self,
        frame: &mut CoreHostFramePrelude,
        presented: &BTreeSet<SurfaceId>,
        unknown: &BTreeSet<SurfaceId>,
    ) {
        assert!(
            presented.is_disjoint(unknown),
            "one surface cannot be both presented and unknown"
        );
        let scope = frame
            .pending_presentation_streams_for(self.lease)
            .expect("test provider must be enrolled in the host frame")
            .into_iter()
            .collect::<BTreeSet<_>>();
        assert_eq!(scope, self.pending.keys().copied().collect());
        let entries = scope
            .iter()
            .map(|stream| {
                let outputs = self.pending.get(stream).expect("pending stream exists");
                let output = *outputs.last().expect("pending output exists");
                let observation = if presented.contains(&output.surface()) {
                    let generation = self.next_capture_generation(*stream);
                    HostPresentationStreamObservation::Captured {
                        generation,
                        progress: HostPresentationProgress::Retired {
                            settled_through: output.key(),
                            presented: Authority::Known(Some(output.key())),
                        },
                    }
                } else if unknown.contains(&output.surface()) {
                    HostPresentationStreamObservation::Captured {
                        generation: self.next_capture_generation(*stream),
                        progress: HostPresentationProgress::Unknown(
                            AuthorityUnavailableReason::NotReported,
                        ),
                    }
                } else {
                    HostPresentationStreamObservation::NoUpdate
                };
                HostPresentationObservationEntry::new(*stream, observation)
            })
            .collect();
        frame
            .submit_presentation_observation_for(
                self.lease,
                HostPresentationObservation::Batch(entries),
            )
            .expect("partitioned test observation must submit");
        self.last_observed = scope;
    }

    fn next_capture_generation(
        &mut self,
        stream: HostPresentationStreamId,
    ) -> HostPresentationCaptureGeneration {
        let generation = self
            .capture_generations
            .get(&stream)
            .copied()
            .unwrap_or_default()
            .checked_next()
            .expect("test capture generation must not exhaust");
        self.capture_generations.insert(stream, generation);
        generation
    }

    /// Replays the last accepted capture generation with a terminal claim.
    ///
    /// Core must reject every entry and the provider sidecar must keep all
    /// outputs pending for a later valid observation.
    pub fn submit_replayed_retirement_observation(&mut self, frame: &mut CoreHostFramePrelude) {
        let scope = frame
            .pending_presentation_streams_for(self.lease)
            .expect("test provider must be enrolled in the host frame")
            .into_iter()
            .collect::<BTreeSet<_>>();
        assert!(
            !scope.is_empty(),
            "replayed capture requires pending output"
        );
        assert_eq!(scope, self.pending.keys().copied().collect());
        let entries = scope
            .iter()
            .map(|stream| {
                let outputs = self.pending.get(stream).expect("pending stream exists");
                let settled_through = outputs.last().expect("pending output exists").key();
                let generation = *self
                    .capture_generations
                    .get(stream)
                    .expect("an earlier capture generation exists");
                HostPresentationObservationEntry::new(
                    *stream,
                    HostPresentationStreamObservation::Captured {
                        generation,
                        progress: HostPresentationProgress::Retired {
                            settled_through,
                            presented: Authority::Known(Some(settled_through)),
                        },
                    },
                )
            })
            .collect();
        frame
            .submit_presentation_observation_for(
                self.lease,
                HostPresentationObservation::Batch(entries),
            )
            .expect("replayed test observation must submit structurally");
        self.last_observed = scope;
    }

    /// Finishes one provider frame and retains its newly core-issued output.
    pub fn finish(&mut self, frame: CoreHostFrame, engine: &mut DockEngine) -> EngineTransition {
        let frame = frame
            .into_presentation()
            .expect("test host frame must enter its presentation phase");
        self.finish_presentation(frame, engine)
    }

    /// Finishes an already presentation-typed provider frame.
    pub fn finish_presentation(
        &mut self,
        mut frame: CoreHostPresentationFrame,
        engine: &mut DockEngine,
    ) -> EngineTransition {
        frame
            .resolve_all_presentation_obligations_unavailable(
                HostPresentationUnavailableReason::OutputNotProduced,
            )
            .expect("test host must explicitly settle every unpainted physical output");
        let transition = frame.finish(engine).expect("test host frame must commit");
        self.confirm_submitted_observation(&transition);
        self.retain_emissions(transition.presentation_emissions());
        transition
    }

    /// Clears only streams whose terminal observation was actually accepted.
    ///
    /// A successfully committed frame may still contain an independently
    /// rejected stream fact. Such output remains pending in both the core and
    /// this provider and must be reported again with a newer valid capture.
    pub fn confirm_submitted_observation(&mut self, transition: &EngineTransition) {
        let submitted = std::mem::take(&mut self.last_observed);
        for outcome in transition.presentation_observations() {
            let HostPresentationObservationOutcome::Retired { stream, .. } = outcome else {
                continue;
            };
            if submitted.contains(stream) {
                self.pending.remove(stream);
            }
        }
    }

    /// Returns the number of concrete outputs retained by this provider.
    #[must_use]
    pub fn pending_output_count(&self) -> usize {
        self.pending.values().map(Vec::len).sum()
    }

    /// Explicitly terminates this test runtime's presentation host.
    pub fn close(
        self,
        engine: &mut DockEngine,
    ) -> Result<PresentationHostRetirementOutcome, EngineError> {
        engine.retire_presentation_host(
            self.lease,
            PresentationHostRetirementReason::ExplicitShutdown,
        )
    }

    fn retain_emissions(&mut self, emissions: &[HostPresentationEmission]) {
        for emission in emissions {
            let output = emission.output();
            self.pending
                .entry(output.stream())
                .or_default()
                .push(output);
        }
    }
}

/// Appends one explicit producer event to a core-minted host frame.
///
/// Tests own the producer identity and sequence stream just as an adapter
/// would, but the engine owns frame scope and causal append order. This helper
/// deliberately has no batch or causal-order construction API.
pub fn append_host_input(
    frame: &mut CoreHostFrame,
    source: StableInputSourceId,
    sequence: SourceSequence,
    input: EngineInput,
) -> Result<(), CoreHostFrameError> {
    if input.priority() == InputPriority::ConfigurationCommit {
        frame.append_configuration(source, sequence, input)
    } else {
        frame.append_input(source, sequence, input)
    }
}

/// Completes a test host frame's exact core-frozen roster with explicit
/// deferred measurements for surfaces not otherwise rendered by the fixture.
///
/// Tests must never model a missing callback as an implicit negative fact. A
/// real adapter makes this same declaration when a frozen surface cannot be
/// measured in the current host pass.
pub fn complete_host_frame_with_unavailable(_engine: &DockEngine, frame: &mut CoreHostFrame) {
    let submitted = frame
        .surface_contributions()
        .iter()
        .map(|contribution| contribution.surface())
        .collect::<BTreeSet<_>>();
    let missing = frame
        .surfaces()
        .filter(|surface| !submitted.contains(surface))
        .collect::<Vec<_>>();
    for surface in missing {
        let token = frame
            .view()
            .begin_surface_contribution(surface)
            .expect("test surface remains in the current roster");
        let contribution = frame
            .view()
            .prepare_surface_unavailable_contribution(token, MeasurementUnavailableReason::Deferred)
            .expect("test unavailable contribution remains structurally valid");
        frame
            .push_surface_contribution(contribution)
            .expect("each missing test surface receives one unavailable contribution");
    }
}

/// Completes a test host frame without replacing an output already proven painted.
///
/// Ordinary semantic inputs commonly run between paint passes. The fixture
/// retains only a current candidate with an already accepted presentation
/// observation; a fresh Ready candidate is deliberately reported Deferred rather
/// than being treated as painted merely because it exists.
pub fn complete_host_frame_with_retained_or_unavailable(
    _engine: &DockEngine,
    frame: &mut CoreHostFrame,
) {
    complete_retained_or_unavailable_contributions(frame);
}

fn complete_retained_or_unavailable_contributions(frame: &mut CoreHostFrame) {
    let submitted = frame
        .surface_contributions()
        .iter()
        .map(|contribution| contribution.surface())
        .collect::<BTreeSet<_>>();
    let missing = frame
        .surfaces()
        .filter(|surface| !submitted.contains(surface))
        .collect::<Vec<_>>();
    for surface in missing {
        let token = frame
            .view()
            .begin_surface_contribution(surface)
            .expect("test surface remains in the current roster");
        if matches!(
            frame.view().scene().surface(surface),
            Some(SurfaceScene::Ready(_))
        ) {
            let contribution = frame
                .view()
                .prepare_surface_retained_contribution(token)
                .expect("test input frame retains the exact current candidate without painting");
            frame
                .push_surface_contribution(contribution)
                .expect("each retained test surface receives one explicit contribution");
        } else {
            let contribution = frame
                .view()
                .prepare_surface_unavailable_contribution(
                    token,
                    MeasurementUnavailableReason::Deferred,
                )
                .expect("test unavailable contribution remains structurally valid");
            frame
                .push_surface_contribution(contribution)
                .expect("each missing test surface receives one explicit contribution");
        }
    }
}

/// Completes a host frame by pairing every currently compiled Ready candidate
/// with one actual paint in this same host frame. The resulting emission is
/// observed only by a later host frame.
pub fn complete_host_frame_with_current_outputs(
    _engine: &DockEngine,
    frame: CoreHostFrame,
) -> CoreHostPresentationFrame {
    let mut frame = frame
        .into_presentation()
        .expect("test output frame must enter its presentation phase");
    let mut obligations = frame
        .take_presentation_obligations()
        .expect("test frame must issue its exact physical presentation roster")
        .into_iter()
        .map(|obligation| (obligation.slot().surface(), obligation))
        .collect::<BTreeMap<_, _>>();
    let submitted = frame
        .surface_contributions()
        .iter()
        .map(|contribution| contribution.surface())
        .collect::<BTreeSet<_>>();
    let missing = frame
        .surfaces()
        .filter(|surface| !submitted.contains(surface))
        .collect::<Vec<_>>();
    for surface in missing {
        let token = frame
            .view()
            .begin_surface_contribution(surface)
            .expect("test surface remains in the current roster");
        match frame
            .view()
            .scene()
            .surface(surface)
            .and_then(SurfaceScene::ready)
        {
            Some(_) => {
                let obligation = obligations
                    .remove(&surface)
                    .expect("each live test surface has one presentation obligation");
                let interaction = frame
                    .view()
                    .presentation_interaction(surface)
                    .unwrap_or_default();
                frame
                    .record_painted_surface_contribution(obligation, token, interaction)
                    .expect("current test output pairs one actual paint with its contribution");
            }
            None => {
                let contribution = frame
                    .view()
                    .prepare_surface_unavailable_contribution(
                        token,
                        MeasurementUnavailableReason::Deferred,
                    )
                    .expect("test unavailable contribution remains structurally valid");
                frame
                    .push_surface_contribution(contribution)
                    .expect("each missing test surface receives one explicit contribution");
            }
        }
    }
    for (_, obligation) in obligations {
        frame
            .resolve_presentation_obligation(
                obligation,
                HostPresentationDisposition::Unavailable(
                    HostPresentationUnavailableReason::OutputNotProduced,
                ),
            )
            .expect("unpainted test presentation slot must settle explicitly");
    }
    frame
}

/// Completes a test host frame from observed retained outputs or fresh
/// measurements where no prior painted output exists.
///
/// An observed current candidate is retained with its exact ticket. A
/// bootstrap/stale or fresh unacknowledged candidate receives a new measurement
/// only when this fixture explicitly models a fresh render pass.
pub fn complete_host_frame_with_current_measurements(
    _engine: &DockEngine,
    frame: &mut CoreHostFrame,
) {
    let submitted = frame
        .surface_contributions()
        .iter()
        .map(|contribution| contribution.surface())
        .collect::<BTreeSet<_>>();
    let missing = frame
        .surfaces()
        .filter(|surface| !submitted.contains(surface))
        .collect::<Vec<_>>();
    let fallback_bounds =
        LogicalRect::new(0.0, 0.0, 240.0, 160.0).expect("test fallback surface bounds are valid");
    for surface in missing {
        let token = frame
            .view()
            .begin_surface_contribution(surface)
            .expect("test surface remains in the current roster");
        let observed_ready = frame
            .view()
            .interaction_projection(surface)
            .zip(
                frame
                    .view()
                    .scene()
                    .surface(surface)
                    .and_then(SurfaceScene::ready),
            )
            .is_some_and(|(interaction, ready)| {
                interaction.output_ticket() == ready.output_ticket()
            });
        if observed_ready {
            let contribution = frame
                .view()
                .prepare_surface_retained_contribution(token)
                .expect("observed test output remains retained without an implicit repaint");
            frame
                .push_surface_contribution(contribution)
                .expect("each retained test surface receives one explicit contribution");
            continue;
        }
        let bounds = frame
            .view()
            .scene()
            .surface(surface)
            .and_then(SurfaceScene::paint_projection)
            .map_or(fallback_bounds, |projection| projection.plan().bounds());
        let measurements = measurements_from_requirements(
            frame.view().presentation_requirements(),
            surface,
            bounds,
            MeasurementProfile::default(),
        );
        let contribution = frame
            .view()
            .prepare_surface_contribution(token, measurements)
            .expect("test current surface measurements remain structurally valid");
        frame
            .push_surface_contribution(contribution)
            .expect("each missing test surface receives one current contribution");
    }
}

/// One explicit session writer for host-frame inputs.
#[derive(Debug, Clone)]
pub struct TestInputStream {
    source: StableInputSourceId,
    next_sequence: u64,
}

impl TestInputStream {
    /// Creates one semantic writer with a default diagnostic producer label.
    #[must_use]
    pub const fn new(source: StableInputSourceId) -> Self {
        Self {
            source,
            next_sequence: 0,
        }
    }

    /// Returns this writer's default diagnostic producer label.
    #[must_use]
    pub const fn source(&self) -> StableInputSourceId {
        self.source
    }

    /// Resumes this test producer after previously committed semantic input.
    /// This reads only the public session writer watermark; the source remains
    /// a diagnostic label and does not own a separate replay namespace.
    #[must_use]
    pub fn resume(engine: &DockEngine, source: StableInputSourceId) -> Self {
        Self {
            source,
            next_sequence: engine
                .semantic_input_watermark()
                .map_or(0, SourceSequence::get),
        }
    }

    /// Appends one input with this writer's next sequence and default source label.
    pub fn append(
        &mut self,
        frame: &mut CoreHostFrame,
        input: EngineInput,
    ) -> Result<(), CoreHostFrameError> {
        self.append_as(frame, self.source, input)
    }

    /// Appends one input with this writer's next sequence and an explicit diagnostic source.
    pub fn append_as(
        &mut self,
        frame: &mut CoreHostFrame,
        source: StableInputSourceId,
        input: EngineInput,
    ) -> Result<(), CoreHostFrameError> {
        self.next_sequence = self
            .next_sequence
            .checked_add(1)
            .expect("test semantic writer sequence must not exhaust");
        append_host_input(
            frame,
            source,
            SourceSequence::new(self.next_sequence),
            input,
        )
    }

    /// Submits one input at its own explicit reducer boundary.
    pub fn submit(
        &mut self,
        engine: &mut DockEngine,
        host: &mut TestPresentationHost,
        input: EngineInput,
    ) -> Result<EngineTransition, EngineError> {
        let direct_input = host.stage_joined_input(input);
        let mut frame = host.begin(engine);
        if let Some(input) = direct_input {
            if let Err(error) = self.append(&mut frame, input) {
                panic!(
                    "test host-frame input must be structurally valid: {error:?}; reduction: {:?}",
                    frame.input_prefix_error()
                );
            }
        }
        complete_host_frame_with_retained_or_unavailable(engine, &mut frame);
        Ok(host.finish(frame, engine))
    }
}

/// Submits one test input from an explicit stable producer.
pub fn submit_input(
    engine: &mut DockEngine,
    host: &mut TestPresentationHost,
    source: StableInputSourceId,
    input: EngineInput,
) -> Result<EngineTransition, EngineError> {
    TestInputStream::resume(engine, source).submit(engine, host, input)
}

/// Submits an explicitly ordered same-source test batch.
pub fn submit_inputs(
    engine: &mut DockEngine,
    host: &mut TestPresentationHost,
    source: StableInputSourceId,
    inputs: impl IntoIterator<Item = EngineInput>,
) -> Result<EngineTransition, EngineError> {
    let mut stream = TestInputStream::resume(engine, source);
    let direct_inputs = inputs
        .into_iter()
        .filter_map(|input| host.stage_joined_input(input))
        .collect::<Vec<_>>();
    let mut frame = host.begin(engine);
    for input in direct_inputs {
        stream
            .append(&mut frame, input)
            .expect("test host-frame input must be structurally valid");
    }
    complete_host_frame_with_retained_or_unavailable(engine, &mut frame);
    Ok(host.finish(frame, engine))
}

#[derive(Debug, Clone, Copy)]
pub struct MeasurementProfile {
    pub pane_minimum: LogicalSize,
    pub tab_content_width: f64,
    pub leading_reserved: f64,
    pub trailing_reserved: f64,
    pub scroll_offset: Option<f64>,
    pub tab_strip_controls: Option<TabStripControlMetrics>,
    pub tab_list_menu: Option<TabListMenuMetrics>,
}

impl Default for MeasurementProfile {
    fn default() -> Self {
        Self {
            pane_minimum: LogicalSize::new(0.0, 0.0).expect("default minimum is valid"),
            tab_content_width: 56.0,
            leading_reserved: 0.0,
            trailing_reserved: 0.0,
            scroll_offset: None,
            tab_strip_controls: None,
            tab_list_menu: None,
        }
    }
}

pub fn measurements(
    engine: &DockEngine,
    surface: SurfaceId,
    bounds: LogicalRect,
    profile: MeasurementProfile,
) -> SurfaceMeasurements {
    measurements_from_requirements(engine.presentation_requirements(), surface, bounds, profile)
}

fn measurements_from_requirements(
    manifest: &SceneRequirementManifest,
    surface: SurfaceId,
    bounds: LogicalRect,
    profile: MeasurementProfile,
) -> SurfaceMeasurements {
    let requirements = manifest
        .surface(surface)
        .expect("surface requirements exist");
    let mut contribution = SurfaceMeasurements::new(requirements.ticket());
    contribution
        .set_bounds(requirements.bounds(), Measurement::Measured(bounds))
        .expect("surface bounds answer is unique");
    if let Some(key) = requirements.popup_plane_bounds() {
        contribution
            .set_popup_plane_bounds(key, Measurement::Measured(bounds))
            .expect("popup-plane bounds answer is unique");
    }
    for key in requirements.pane_minimums() {
        contribution
            .insert_pane_minimum(key, Measurement::Measured(profile.pane_minimum))
            .expect("pane minimum answer is unique");
    }
    for key in requirements.tab_intrinsics() {
        contribution
            .insert_tab_intrinsic(
                key,
                Measurement::Measured(
                    TabIntrinsic::new(profile.tab_content_width).expect("tab intrinsic is valid"),
                ),
            )
            .expect("tab intrinsic answer is unique");
    }
    for key in requirements.tab_strips() {
        let metrics = TabStripMetrics::new(profile.leading_reserved, profile.trailing_reserved)
            .expect("tab strip metrics are valid");
        let metrics = profile
            .tab_strip_controls
            .map_or(metrics, |controls| metrics.with_controls(controls));
        let metrics = profile
            .tab_list_menu
            .map_or(metrics, |menu| metrics.with_tab_list_menu(menu));
        let metrics = profile
            .scroll_offset
            .map_or(Ok(metrics), |offset| metrics.with_scroll_offset(offset));
        contribution
            .insert_tab_strip(
                key,
                Measurement::Measured(metrics.expect("tab strip scroll offset is valid")),
            )
            .expect("tab strip answer is unique");
    }
    contribution
}

pub fn install_surface_projection(
    engine: &mut DockEngine,
    host: &mut TestPresentationHost,
    surface: SurfaceId,
    bounds: LogicalRect,
) -> SurfaceSceneStamp {
    install_surface_projection_with(engine, host, surface, bounds, MeasurementProfile::default())
}

pub fn install_surface_projection_with(
    engine: &mut DockEngine,
    host: &mut TestPresentationHost,
    surface: SurfaceId,
    bounds: LogicalRect,
    profile: MeasurementProfile,
) -> SurfaceSceneStamp {
    reduce_surface_projection_with(engine, host, surface, bounds, profile, false)
}

pub fn publish_surface_with(
    engine: &mut DockEngine,
    host: &mut TestPresentationHost,
    surface: SurfaceId,
    bounds: LogicalRect,
    profile: MeasurementProfile,
) -> SurfaceSceneStamp {
    reduce_surface_projection_with(engine, host, surface, bounds, profile, true)
}

fn reduce_surface_projection_with(
    engine: &mut DockEngine,
    host: &mut TestPresentationHost,
    surface: SurfaceId,
    bounds: LogicalRect,
    profile: MeasurementProfile,
    painted: bool,
) -> SurfaceSceneStamp {
    let mut frame = host.begin(engine);
    let token = frame
        .view()
        .begin_surface_contribution(surface)
        .expect("surface contribution begins inside the current roster");
    let measurements = measurements_from_requirements(
        frame.view().presentation_requirements(),
        surface,
        bounds,
        profile,
    );
    let contribution = frame
        .view()
        .prepare_surface_contribution(token, measurements)
        .expect("complete test measurements prepare successfully");
    frame
        .push_surface_contribution(contribution)
        .expect("a test tick carries one surface contribution");
    complete_host_frame_with_retained_or_unavailable(engine, &mut frame);
    let transition = host.finish(frame, engine);
    let (stamp, output) = transition
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
                "expected ready surface contribution for {surface}, got {:?}",
                transition.surface_contributions()
            )
        });
    if painted {
        let emit_frame = host.begin(engine);
        let emit_frame = complete_host_frame_with_current_outputs(engine, emit_frame);
        let emitted = host.finish_presentation(emit_frame, engine);
        assert_eq!(
            emitted
                .presentation_emissions()
                .iter()
                .filter(|emission| emission.output().surface() == surface)
                .count(),
            1,
            "one host frame may emit exactly one output for its explicitly painted surface"
        );

        let mut observation_frame = host.begin(engine);
        complete_host_frame_with_retained_or_unavailable(engine, &mut observation_frame);
        let observed = host.finish(observation_frame, engine);
        assert!(
            observed
                .presentation_observations()
                .iter()
                .any(|outcome| matches!(
                outcome,
                dockspace::presentation_observation::HostPresentationObservationOutcome::Retired {
                    promotion_eligible: true,
                    ..
                }
            ))
        );
        assert_eq!(
            engine
                .scene()
                .surface(surface)
                .and_then(SurfaceScene::ready)
                .map(|ready| ready.output_ticket()),
            Some(output)
        );
    }
    stamp
}

pub fn publish_surface(
    engine: &mut DockEngine,
    host: &mut TestPresentationHost,
    surface: SurfaceId,
    bounds: LogicalRect,
) -> SurfaceSceneStamp {
    publish_surface_with(engine, host, surface, bounds, MeasurementProfile::default())
}

/// Paints and then observes one exact core-requested native staging pass.
pub fn present_native_staging(
    engine: &mut DockEngine,
    host: &mut TestPresentationHost,
    presentation: NativeStagingPresentation,
) -> EngineTransition {
    let paint_frame = host.begin(engine);
    present_native_staging_in_frame(engine, host, paint_frame, presentation)
}

/// Paints the current staging request for one exact native binding and phase.
pub fn present_requested_native_staging(
    engine: &mut DockEngine,
    host: &mut TestPresentationHost,
    binding: ViewportBinding,
    phase: NativeStagingPresentationPhase,
) -> EngineTransition {
    let paint_frame = host.begin(engine);
    let presentation = paint_frame
        .view()
        .native_staging_presentations()
        .find(|presentation| presentation.binding() == binding && presentation.phase() == phase)
        .expect("the exact native staging request must be current");
    present_native_staging_in_frame(engine, host, paint_frame, presentation)
}

fn present_native_staging_in_frame(
    engine: &mut DockEngine,
    host: &mut TestPresentationHost,
    mut paint_frame: CoreHostFrame,
    presentation: NativeStagingPresentation,
) -> EngineTransition {
    assert!(
        paint_frame
            .view()
            .native_staging_presentations()
            .any(|current| current == presentation)
    );
    complete_retained_or_unavailable_contributions(&mut paint_frame);
    let mut paint_frame = paint_frame
        .into_presentation()
        .expect("native staging frame enters its presentation phase");
    for obligation in paint_frame
        .take_presentation_obligations()
        .expect("native staging frame must issue its exact physical roster")
    {
        let disposition = match obligation.slot() {
            HostPresentationSlot::NativeStaging {
                presentation: actual,
            } if actual == presentation => {
                HostPresentationDisposition::Painted(HostInteractionPresentation::default())
            }
            _ => HostPresentationDisposition::Unavailable(
                HostPresentationUnavailableReason::OutputNotProduced,
            ),
        };
        paint_frame
            .resolve_presentation_obligation(obligation, disposition)
            .expect("native staging obligation must resolve exactly once");
    }
    let emitted = host.finish_presentation(paint_frame, engine);
    assert_eq!(
        emitted
            .presentation_emissions()
            .iter()
            .filter(|emission| {
                matches!(
                    emission.output().payload(),
                    dockspace::presentation_observation::HostPresentationOutputPayload::NativeStaging {
                        presentation: actual,
                    } if actual == presentation
                )
            })
            .count(),
        1
    );

    let mut observation_frame = host.begin(engine);
    complete_host_frame_with_retained_or_unavailable(engine, &mut observation_frame);
    host.finish(observation_frame, engine)
}

pub fn publish_surfaces(
    engine: &mut DockEngine,
    host: &mut TestPresentationHost,
    surfaces: impl IntoIterator<Item = (SurfaceId, LogicalRect)>,
) {
    let _ = publish_surfaces_batch(engine, host, surfaces);
}

/// Publishes one complete multi-surface host pass, records every actual paint,
/// and observes those emissions in the following host frame.
pub fn publish_surfaces_batch(
    engine: &mut DockEngine,
    host: &mut TestPresentationHost,
    surfaces: impl IntoIterator<Item = (SurfaceId, LogicalRect)>,
) -> BTreeMap<SurfaceId, SurfaceSceneStamp> {
    let supplied = surfaces.into_iter().collect::<BTreeMap<_, _>>();
    let expected = engine
        .presentation_requirements()
        .surfaces()
        .map(|(surface, _)| surface)
        .collect::<BTreeSet<_>>();
    assert_eq!(
        supplied.keys().copied().collect::<BTreeSet<_>>(),
        expected,
        "a batch publish must explicitly measure the complete frozen roster"
    );

    let mut frame = host.begin(engine);
    for (surface, bounds) in &supplied {
        let token = engine
            .begin_surface_contribution(*surface)
            .expect("batch surface contribution begins inside the current roster");
        let contribution = engine
            .prepare_surface_contribution(
                token,
                measurements(engine, *surface, *bounds, MeasurementProfile::default()),
            )
            .expect("batch measurements prepare successfully");
        frame
            .push_surface_contribution(contribution)
            .expect("each batch surface contributes once");
    }
    let transition = host.finish(frame, engine);
    let outputs = transition
        .surface_contributions()
        .iter()
        .map(|outcome| match outcome {
            SurfaceContributionOutcome::Ready {
                surface,
                stamp,
                ticket,
            } => (*surface, (*stamp, *ticket)),
            other => panic!("batch contribution must become Ready, got {other:?}"),
        })
        .collect::<BTreeMap<_, _>>();

    let emit_frame = host.begin(engine);
    let emit_frame = complete_host_frame_with_current_outputs(engine, emit_frame);
    let emitted = host.finish_presentation(emit_frame, engine);
    assert_eq!(emitted.presentation_emissions().len(), outputs.len());

    let mut observation_frame = host.begin(engine);
    complete_host_frame_with_retained_or_unavailable(engine, &mut observation_frame);
    let transition = host.finish(observation_frame, engine);
    assert!(transition
        .presentation_observations()
        .iter()
        .all(|outcome| matches!(
            outcome,
            dockspace::presentation_observation::HostPresentationObservationOutcome::Retired { .. }
        )));
    outputs
        .into_iter()
        .map(|(surface, (stamp, _))| (surface, stamp))
        .collect()
}

pub fn next_plan(engine: &DockEngine, surface: SurfaceId) -> &PresentationPlan {
    engine
        .scene()
        .surface(surface)
        .and_then(SurfaceScene::ready)
        .map(|ready| ready.plan())
        .expect("surface has a compiled next projection")
}

pub fn painted_plan(engine: &DockEngine, surface: SurfaceId) -> &PresentationPlan {
    engine
        .scene()
        .ready_surface(surface)
        .map(|ready| ready.plan())
        .expect("surface has painted interaction authority")
}
