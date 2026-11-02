//! Private final-presentation sidecar for the public runtime facade.

use std::sync::{Arc, Mutex, PoisonError};

use thiserror::Error;

use crate::backend_ingress::{BackendIngressError, BackendIngressLease, BackendIngressRecorder};
use crate::engine::CoreHostFramePrelude;
use crate::engine::{DockEngine, EngineError};
use crate::ids::SurfaceId;
use crate::intent::Authority;
use crate::presentation_observation::{
    HostFrameKey, HostPresentationCaptureGeneration, HostPresentationEndpoint,
    HostPresentationObservation, HostPresentationObservationEntry,
    HostPresentationObservationOutcome, HostPresentationOutput, HostPresentationOutputPayload,
    HostPresentationProgress, HostPresentationStreamId, HostPresentationStreamObservation,
    NativeStagingPresentation, NativeStagingPresentationPhase as CoreNativeStagingPhase,
    PresentationStreamQuiescence,
};
use crate::transition::EngineTransition;

use super::native::{NativePlatformError, NativeSurfaceBinding};

/// Affine record for one complete surface emission accepted by the host sink.
///
/// The sink may draw immediately or own a display list or retained-node update.
/// Complete emission is distinct from renderer submission and final presentation.
/// The host reports the exact final result through
/// [`DockspaceSession::report_surface_presentation`](super::DockspaceSession::report_surface_presentation).
/// Dropping this record retires its emission without granting input authority.
#[derive(Debug)]
#[must_use = "report this exact output's final disposition or drop it to retire the emission"]
pub struct PaintedSurfaceOutput {
    output: HostPresentationOutput,
    abandoned: PresentationOutputs,
    armed: bool,
}

/// Opaque identity for one exact committed surface emission.
///
/// The identity binds the core stream, emission key, endpoint, semantic
/// output, coordinate capture, and transient interaction payload. It is not a
/// presentation proof: consumers may only compare it with a later
/// core-minted presented-surface query.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct SurfacePresentationArtifact {
    output: HostPresentationOutput,
}

impl std::fmt::Debug for SurfacePresentationArtifact {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("SurfacePresentationArtifact")
            .field("surface", &self.output.surface())
            .finish_non_exhaustive()
    }
}

impl SurfacePresentationArtifact {
    pub(super) const fn output(self) -> HostPresentationOutput {
        self.output
    }
}

/// Opaque result of one presentation-observation prefix accepted by core.
///
/// Hosts use this acknowledgement to promote or retire only artifacts from
/// the exact committed stream prefix. Its fields remain private so callers
/// cannot synthesize a core commit. Retaining an accepted artifact does not bypass
/// interaction gates, such as a popup awaiting its owner's presentation;
/// bind receiver descriptors through [`DockspaceSession::bind_presented_receiver`](super::DockspaceSession::bind_presented_receiver).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SurfacePresentationCommit {
    stream: HostPresentationStreamId,
    settled_through: HostFrameKey,
    presented: Option<SurfacePresentationArtifact>,
}

impl SurfacePresentationCommit {
    const fn new(
        stream: HostPresentationStreamId,
        settled_through: HostFrameKey,
        presented: Option<SurfacePresentationArtifact>,
    ) -> Self {
        Self {
            stream,
            settled_through,
            presented,
        }
    }

    /// Returns whether core accepted and retained this exact artifact as the
    /// presentation for the committed prefix. Receiver use still requires its
    /// current session binding.
    #[must_use]
    pub fn presents(self, artifact: SurfacePresentationArtifact) -> bool {
        self.presented == Some(artifact)
    }

    /// Returns whether this commit terminally settled the supplied artifact.
    #[must_use]
    pub fn settles(self, artifact: SurfacePresentationArtifact) -> bool {
        let output = artifact.output();
        output.stream() == self.stream && output.key() <= self.settled_through
    }

    /// Returns whether this commit retired the supplied artifact instead of
    /// selecting it as the final presentation.
    #[must_use]
    pub fn retires(self, artifact: SurfacePresentationArtifact) -> bool {
        self.settles(artifact) && !self.presents(artifact)
    }
}

/// Public lifecycle phase for a non-interactive native staging paint.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum NativeStagingPresentationPhase {
    /// Paint the hidden window before the core requests it to be shown.
    PreShow,
    /// Paint the visible placeholder before ownership admission.
    PostShow,
}

/// Opaque request to paint one exact native lifecycle staging output.
///
/// Staging is deliberately not a dock scene: it must not invoke pane UI or
/// publish hit-test, focus, or accessibility authority. The request is still
/// bound to the exact window incarnation and core lifecycle phase.
#[must_use = "a native staging request must be painted or explicitly deferred"]
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct NativeStagingPaintRequest {
    binding: NativeSurfaceBinding,
    phase: NativeStagingPresentationPhase,
    core: NativeStagingPresentation,
}

impl std::fmt::Debug for NativeStagingPaintRequest {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("NativeStagingPaintRequest")
            .field("surface", &self.binding.surface())
            .field("phase", &self.phase)
            .finish()
    }
}

impl NativeStagingPaintRequest {
    pub(super) const fn from_core(
        binding: NativeSurfaceBinding,
        core: NativeStagingPresentation,
    ) -> Self {
        let phase = match core.phase() {
            CoreNativeStagingPhase::PreShow => NativeStagingPresentationPhase::PreShow,
            CoreNativeStagingPhase::PostShow => NativeStagingPresentationPhase::PostShow,
        };
        Self {
            binding,
            phase,
            core,
        }
    }

    /// Returns the stable logical surface which owns this staging output.
    #[must_use]
    pub const fn surface(self) -> SurfaceId {
        self.binding.surface()
    }

    /// Returns the exact native binding to which the host must attach the paint.
    #[must_use]
    pub const fn binding(self) -> NativeSurfaceBinding {
        self.binding
    }

    /// Returns whether this is the pre-show or post-show lifecycle phase.
    #[must_use]
    pub const fn phase(self) -> NativeStagingPresentationPhase {
        self.phase
    }

    pub(super) const fn core(self) -> NativeStagingPresentation {
        self.core
    }
}

/// Affine terminal-presentation capability for one painted native staging output.
#[must_use = "a painted staging output must receive a Presented or Dropped result"]
pub struct PaintedNativeStagingOutput {
    output: HostPresentationOutput,
    request: NativeStagingPaintRequest,
    abandoned: PresentationOutputs,
    armed: bool,
}

impl std::fmt::Debug for PaintedNativeStagingOutput {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("PaintedNativeStagingOutput")
            .field("surface", &self.request.surface())
            .field("phase", &self.request.phase())
            .finish()
    }
}

impl PaintedNativeStagingOutput {
    /// Returns the logical surface represented by this staging output.
    #[must_use]
    pub const fn surface(&self) -> SurfaceId {
        self.request.surface()
    }

    /// Returns the exact native binding represented by this output.
    #[must_use]
    pub const fn binding(&self) -> NativeSurfaceBinding {
        self.request.binding()
    }

    /// Returns the lifecycle phase represented by this output.
    #[must_use]
    pub const fn phase(&self) -> NativeStagingPresentationPhase {
        self.request.phase()
    }

    pub(super) const fn raw_output(&self) -> HostPresentationOutput {
        self.output
    }

    fn disarm(&mut self) {
        self.armed = false;
    }
}

impl PartialEq for PaintedNativeStagingOutput {
    fn eq(&self, other: &Self) -> bool {
        self.output == other.output
    }
}

impl Eq for PaintedNativeStagingOutput {}

impl Drop for PaintedNativeStagingOutput {
    fn drop(&mut self) {
        if self.armed {
            self.abandoned.abandon(self.output);
        }
    }
}

impl PaintedSurfaceOutput {
    /// Returns the logical surface represented by this output.
    #[must_use]
    pub const fn surface(&self) -> SurfaceId {
        self.output.surface()
    }

    /// Returns the opaque exact-emission identity carried by this committed
    /// output. The identity can qualify adapter-owned artifacts only after the
    /// core later presents the matching emission.
    #[must_use]
    pub const fn presentation_artifact(&self) -> SurfacePresentationArtifact {
        SurfacePresentationArtifact {
            output: self.output,
        }
    }

    /// Returns whether this output was emitted for the exact native binding.
    ///
    /// The comparison includes the engine authority domain, workspace epoch,
    /// logical surface, platform window token, and window incarnation. A
    /// surface match alone is never sufficient after native window recreation.
    #[must_use]
    pub fn matches_native_binding(&self, binding: NativeSurfaceBinding) -> bool {
        match self.output.endpoint() {
            HostPresentationEndpoint::Headless => false,
            HostPresentationEndpoint::Native(endpoint) => {
                binding.matches_viewport_binding(endpoint)
            }
        }
    }

    /// Returns whether this emission accepted the exact semantic output.
    #[must_use]
    pub fn matches_semantic_output(&self, output: super::DockspaceSemanticOutput) -> bool {
        self.output.payload().scene() == Some(output.ticket)
    }

    fn disarm(&mut self) {
        self.armed = false;
    }
}

impl PartialEq for PaintedSurfaceOutput {
    fn eq(&self, other: &Self) -> bool {
        self.output == other.output
    }
}

impl Eq for PaintedSurfaceOutput {}

impl Drop for PaintedSurfaceOutput {
    fn drop(&mut self) {
        if self.armed {
            self.abandoned.abandon(self.output);
        }
    }
}

impl super::DockspaceSession {
    /// Reports the final host result for one previously emitted output.
    ///
    /// The terminal fact is submitted at the next host-frame prelude. A
    /// `Presented` result may grant interaction authority after a successful
    /// host-frame commit;
    /// a `Dropped` result retires the output without granting authority.
    /// Multiple reports for the same surface stream may arrive before that
    /// prelude; the facade coalesces them by exact output order.
    ///
    /// # Errors
    ///
    /// Returns an error carrying the original affine capability when it is
    /// foreign, stale, or already retired.
    pub fn report_surface_presentation(
        &mut self,
        output: PaintedSurfaceOutput,
        result: SurfacePresentationResult,
    ) -> Result<(), SurfacePresentationReportError> {
        self.presentation.report(output, result)
    }

    /// Reports the final renderer result for one native lifecycle staging output.
    ///
    /// A staging result advances lifecycle only when the exact output is
    /// reported as [`SurfacePresentationResult::Presented`].
    pub fn report_native_staging_presentation(
        &mut self,
        output: PaintedNativeStagingOutput,
        result: SurfacePresentationResult,
    ) -> Result<(), NativeStagingPresentationReportError> {
        self.presentation.report_staging(output, result)
    }
}

/// Terminal result reported by the renderer for one exact output.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SurfacePresentationResult {
    /// The exact output reached final presentation.
    Presented,
    /// The exact output was discarded without reaching final presentation.
    Dropped,
}

/// Failed presentation report with the original affine output preserved.
#[derive(Debug, Error)]
#[error("painted output is foreign, stale, or already retired")]
pub struct SurfacePresentationReportError {
    output: PaintedSurfaceOutput,
}

impl SurfacePresentationReportError {
    /// Recovers the unconsumed affine output for caller-controlled handling.
    #[must_use]
    pub fn into_output(self) -> PaintedSurfaceOutput {
        self.output
    }
}

/// Failed staging presentation report with the original affine output preserved.
#[derive(Debug, Error)]
#[error("painted native staging output is foreign, stale, or already retired")]
pub struct NativeStagingPresentationReportError {
    output: PaintedNativeStagingOutput,
}

impl NativeStagingPresentationReportError {
    /// Recovers the unconsumed affine staging output.
    #[must_use]
    pub fn into_output(self) -> PaintedNativeStagingOutput {
        self.output
    }
}

/// Failure while synchronizing the facade presentation sidecar with the core.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub(super) enum PresentationObservationError {
    /// The facade and core disagree about the exact pending stream roster.
    #[error("presentation sidecar does not match the core-owned pending stream roster")]
    PendingRosterMismatch,
    /// A provider capture generation cannot advance without wrapping.
    #[error("presentation capture generation is exhausted")]
    CaptureGenerationExhausted,
    /// A previously recorded backend report changed before core accepted
    /// it, which would make replay ambiguous.
    #[error("backend presentation report changed before core acceptance")]
    BackendReportConflict,
}

#[derive(Debug, Default)]
pub(super) struct RuntimePresentationState {
    pending: PresentationOutputs,
    streams: Vec<RuntimePresentationStream>,
    slot_limit: Option<usize>,
}

#[derive(Debug, Clone, Copy)]
struct PendingPresentationOutput {
    output: HostPresentationOutput,
    result: Option<SurfacePresentationResult>,
}

/// The original pending output is also its terminal-result slot. Affine handles
/// share this store so Drop never allocates or creates another result owner.
#[derive(Debug, Clone, Default)]
struct PresentationOutputs(Arc<Mutex<Vec<PendingPresentationOutput>>>);

impl PresentationOutputs {
    fn lock(&self) -> std::sync::MutexGuard<'_, Vec<PendingPresentationOutput>> {
        self.0.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn is_empty(&self) -> bool {
        self.lock().is_empty()
    }

    fn streams(&self) -> Vec<HostPresentationStreamId> {
        let pending = self.lock();
        let mut streams = Vec::with_capacity(pending.len());
        streams.extend(pending.iter().map(|entry| entry.output.stream()));
        streams.sort_unstable();
        streams.dedup();
        streams
    }

    fn record(&self, output: HostPresentationOutput, result: SurfacePresentationResult) -> bool {
        let mut pending = self.lock();
        let Some(entry) = pending.iter_mut().find(|entry| entry.output == output) else {
            return false;
        };
        if entry.result.is_some() {
            return false;
        }
        entry.result = Some(result);
        true
    }

    fn abandon(&self, output: HostPresentationOutput) {
        let _ = self.record(output, SurfacePresentationResult::Dropped);
    }
}

pub(super) struct PreparedPresentationEmissions {
    pending: PresentationOutputs,
    entries: Vec<PendingPresentationOutput>,
    streams: Vec<RuntimePresentationStream>,
    surfaces: Vec<PaintedSurfaceOutput>,
    staging: Vec<PaintedNativeStagingOutput>,
}

impl PreparedPresentationEmissions {
    /// Installation follows core publication; all backing was admitted earlier.
    pub(super) fn commit(
        mut self,
        state: &mut RuntimePresentationState,
    ) -> (Vec<PaintedSurfaceOutput>, Vec<PaintedNativeStagingOutput>) {
        state.streams.append(&mut self.streams);
        self.pending.lock().append(&mut self.entries);
        for output in &mut self.surfaces {
            output.armed = true;
        }
        for output in &mut self.staging {
            output.armed = true;
        }
        (self.surfaces, self.staging)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct PendingPresentationReport {
    settled_through: HostFrameKey,
    presented: Option<HostFrameKey>,
}

impl PendingPresentationReport {
    const fn authority(self) -> Authority<Option<HostFrameKey>> {
        Authority::Known(self.presented)
    }

    fn record(&mut self, output: HostFrameKey, result: SurfacePresentationResult) {
        if output > self.settled_through {
            self.settled_through = output;
        }
        if result == SurfacePresentationResult::Presented {
            self.presented = Some(output);
        }
    }
}

fn submitted_retirement(
    outcome: &HostPresentationObservationOutcome,
    submitted: &SubmittedPresentationObservation,
) -> Option<(HostPresentationStreamId, HostFrameKey)> {
    let HostPresentationObservationOutcome::Retired {
        stream,
        generation,
        settled_through,
        presented,
        ..
    } = outcome
    else {
        return None;
    };
    let (submitted_generation, report) = submitted.get(*stream)?;
    (generation == &submitted_generation
        && settled_through == &report.settled_through
        && presented == &report.authority())
        .then_some((*stream, *settled_through))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct RecordedPresentationReport {
    provider: BackendIngressLease,
    generation: HostPresentationCaptureGeneration,
    report: PendingPresentationReport,
}

#[derive(Debug, Clone, Copy)]
struct RuntimePresentationStream {
    stream: HostPresentationStreamId,
    generation: HostPresentationCaptureGeneration,
    recorded: Option<RecordedPresentationReport>,
    presented: Option<SurfacePresentationArtifact>,
}

#[derive(Debug, Default)]
pub(super) struct SubmittedPresentationObservation {
    captured: Vec<(
        HostPresentationStreamId,
        HostPresentationCaptureGeneration,
        PendingPresentationReport,
    )>,
}

impl SubmittedPresentationObservation {
    fn with_capacity(streams: usize) -> Self {
        Self {
            captured: Vec::with_capacity(streams),
        }
    }

    fn get(
        &self,
        stream: HostPresentationStreamId,
    ) -> Option<(HostPresentationCaptureGeneration, PendingPresentationReport)> {
        self.captured
            .iter()
            .find_map(|(candidate, generation, report)| {
                (*candidate == stream).then_some((*generation, *report))
            })
    }
}

impl RuntimePresentationState {
    /// Counts the original pending store and its in-place result slots once.
    pub(super) fn owned_data_capacity_bytes(&self) -> usize {
        size_of::<Self>()
            .saturating_add(size_of::<Mutex<Vec<PendingPresentationOutput>>>())
            .saturating_add(
                self.pending
                    .lock()
                    .capacity()
                    .saturating_mul(size_of::<PendingPresentationOutput>()),
            )
            .saturating_add(
                self.streams
                    .capacity()
                    .saturating_mul(size_of::<RuntimePresentationStream>()),
            )
    }

    pub(super) fn with_capacity_limit(slots: usize) -> Result<Self, NativePlatformError> {
        let mut pending = Vec::new();
        pending
            .try_reserve_exact(slots)
            .map_err(|_| NativePlatformError::InputCapacity)?;
        if pending.capacity() > slots {
            return Err(NativePlatformError::InputCapacity);
        }
        let mut streams = Vec::new();
        streams
            .try_reserve_exact(slots)
            .map_err(|_| NativePlatformError::InputCapacity)?;
        if streams.capacity() > slots {
            return Err(NativePlatformError::InputCapacity);
        }
        Ok(Self {
            pending: PresentationOutputs(Arc::new(Mutex::new(pending))),
            streams,
            slot_limit: Some(slots),
        })
    }

    pub(super) fn is_quiescent(&self) -> bool {
        self.pending.is_empty() && self.streams.is_empty()
    }

    /// Covers retained slots, the complete prepared emission, returned handles
    /// and per-stream report construction/settlement alongside retained history.
    pub(super) fn reservation_bytes(slots: usize) -> Option<usize> {
        let per_slot = 2 * size_of::<PendingPresentationOutput>()
            + size_of::<PaintedSurfaceOutput>().max(size_of::<PaintedNativeStagingOutput>())
            + 4 * size_of::<HostPresentationStreamId>()
            + 2 * size_of::<RuntimePresentationStream>()
            + size_of::<(
                HostPresentationStreamId,
                HostPresentationCaptureGeneration,
                PendingPresentationReport,
            )>()
            + size_of::<HostPresentationObservationEntry>()
            + size_of::<SurfacePresentationCommit>()
            + size_of::<PresentationStreamQuiescence>();
        size_of::<Self>()
            .checked_add(size_of::<Mutex<Vec<PendingPresentationOutput>>>())?
            .checked_add(size_of::<PreparedPresentationEmissions>())?
            .checked_add(size_of::<SubmittedPresentationObservation>())?
            .checked_add(4 * size_of::<Vec<HostPresentationObservationEntry>>())?
            .checked_add(slots.checked_mul(per_slot)?)
    }

    pub(super) fn reclaim_quiescent_streams(
        &mut self,
        engine: &mut DockEngine,
        host: crate::presentation_observation::PresentationHostLease,
    ) -> Result<(), super::DockspaceRuntimeError> {
        let pending_streams = self.pending.streams();
        let mut candidates = Vec::with_capacity(self.streams.len());
        candidates.extend(self.streams.iter().filter_map(|state| {
            (!pending_streams.contains(&state.stream) && state.recorded.is_none())
                .then_some(state.stream)
        }));
        let mut quiescences = Vec::with_capacity(candidates.len());
        for stream in candidates {
            if let Some(quiescence) =
                engine.try_prepare_presentation_stream_quiescence(host, stream)?
            {
                quiescences.push(quiescence);
            }
        }
        if !quiescences.is_empty() {
            engine.confirm_presentation_stream_quiescence_batch(quiescences)?;
            self.reclaim_compacted_streams(engine);
        }
        Ok(())
    }

    pub(super) fn reclaim_compacted_streams(&mut self, engine: &DockEngine) {
        let retained = engine.presentation_retention_manifest();
        self.streams
            .retain(|state| retained.retains_stream(state.stream));
    }

    #[cfg(test)]
    pub(super) fn retained_capture_generation_count(&self) -> usize {
        self.streams
            .iter()
            .filter(|state| state.generation != HostPresentationCaptureGeneration::default())
            .count()
    }

    fn compile_report(
        &self,
        stream: HostPresentationStreamId,
    ) -> Option<PendingPresentationReport> {
        let pending = self.pending.lock();
        let mut report: Option<PendingPresentationReport> = None;
        for entry in pending
            .iter()
            .filter(|entry| entry.output.stream() == stream)
        {
            let Some(result) = entry.result else {
                break;
            };
            let output = entry.output;
            match &mut report {
                Some(report) => report.record(output.key(), result),
                None => {
                    report = Some(PendingPresentationReport {
                        settled_through: output.key(),
                        presented: (result == SurfacePresentationResult::Presented)
                            .then_some(output.key()),
                    });
                }
            }
        }
        report
    }

    pub(super) fn has_unrecorded_backend_report(&mut self) -> bool {
        self.pending.streams().into_iter().any(|stream| {
            self.compile_report(stream).is_some_and(|report| {
                self.streams
                    .iter()
                    .find(|state| state.stream == stream)
                    .and_then(|state| state.recorded)
                    .is_none_or(|recorded| recorded.report != report)
            })
        })
    }

    fn observation_scope(
        &self,
        prelude: &CoreHostFramePrelude,
    ) -> Result<Vec<HostPresentationStreamId>, PresentationObservationError> {
        let mut scope = Vec::with_capacity(self.streams.len());
        for stream in prelude.pending_presentation_streams() {
            if scope.len() == self.streams.len() {
                return Err(PresentationObservationError::PendingRosterMismatch);
            }
            scope.push(stream);
        }
        scope.sort_unstable();
        if scope != self.pending.streams() {
            return Err(PresentationObservationError::PendingRosterMismatch);
        }
        Ok(scope)
    }

    /// Submits the rendering host's bootstrap observation and records terminal
    /// output reports in the joined backend ingress order.
    ///
    /// The prelude deliberately receives `NoUpdate`: terminal presentation is
    /// a backend fact and must be reduced in the same causal stream as native
    /// inventory, close, and effect observations. The sidecar remembers every
    /// accepted record until the corresponding core transition commits, so a
    /// dropped host frame replays the exact same record instead of allocating a
    /// second observation.
    pub(super) fn submit_backend_observation(
        &mut self,
        prelude: &mut CoreHostFramePrelude,
        engine: &DockEngine,
        recorder: &mut BackendIngressRecorder,
    ) -> Result<(SubmittedPresentationObservation, bool), super::DockspaceRuntimeError> {
        let scope = self.observation_scope(prelude)?;
        prelude.submit_presentation_observation(HostPresentationObservation::NoUpdate)?;

        let provider = recorder.lease();
        let mut submitted = SubmittedPresentationObservation::with_capacity(scope.len());
        let mut capacity_blocked = false;
        for stream in scope {
            let state = self
                .streams
                .iter()
                .find(|state| state.stream == stream)
                .ok_or(PresentationObservationError::PendingRosterMismatch)?;
            let generation = state
                .generation
                .checked_next()
                .ok_or(PresentationObservationError::CaptureGenerationExhausted)?;

            let recorded = state.recorded;
            let report = match recorded {
                Some(recorded)
                    if recorded.provider == provider && recorded.generation == generation =>
                {
                    recorded.report
                }
                Some(_) => {
                    return Err(PresentationObservationError::BackendReportConflict.into());
                }
                None => {
                    let Some(report) = self.compile_report(stream) else {
                        continue;
                    };
                    if capacity_blocked {
                        continue;
                    }
                    let entry = HostPresentationObservationEntry::new(
                        stream,
                        HostPresentationStreamObservation::Captured {
                            generation,
                            progress: HostPresentationProgress::Retired {
                                settled_through: report.settled_through,
                                presented: report.authority(),
                            },
                        },
                    );
                    match engine.record_backend_presentation_observation(recorder, entry) {
                        Ok(_) => {}
                        Err(EngineError::BackendIngress {
                            source:
                                BackendIngressError::NativeTransportCapacityExceeded { .. }
                                | BackendIngressError::CapacityAllocationFailed,
                        }) => {
                            capacity_blocked = true;
                            continue;
                        }
                        Err(error) => return Err(error.into()),
                    }
                    self.streams
                        .iter_mut()
                        .find(|state| state.stream == stream)
                        .expect("the pending stream owns its report metadata")
                        .recorded = Some(RecordedPresentationReport {
                        provider,
                        generation,
                        report,
                    });
                    report
                }
            };
            submitted.captured.push((stream, generation, report));
        }
        Ok((submitted, capacity_blocked))
    }

    pub(super) fn submit_observation(
        &mut self,
        prelude: &mut CoreHostFramePrelude,
    ) -> Result<SubmittedPresentationObservation, PresentationObservationError> {
        let scope = self.observation_scope(prelude)?;
        if scope.is_empty() {
            prelude
                .submit_presentation_observation(HostPresentationObservation::NoUpdate)
                .map_err(|_| PresentationObservationError::PendingRosterMismatch)?;
            return Ok(SubmittedPresentationObservation::default());
        }

        let mut submitted = SubmittedPresentationObservation::with_capacity(scope.len());
        let mut entries = Vec::with_capacity(scope.len());
        for stream in scope {
            let observation = if let Some(report) = self.compile_report(stream) {
                let generation = self
                    .streams
                    .iter()
                    .find(|state| state.stream == stream)
                    .ok_or(PresentationObservationError::PendingRosterMismatch)?
                    .generation
                    .checked_next()
                    .ok_or(PresentationObservationError::CaptureGenerationExhausted)?;
                submitted.captured.push((stream, generation, report));
                HostPresentationStreamObservation::Captured {
                    generation,
                    progress: HostPresentationProgress::Retired {
                        settled_through: report.settled_through,
                        presented: report.authority(),
                    },
                }
            } else {
                HostPresentationStreamObservation::NoUpdate
            };
            entries.push(HostPresentationObservationEntry::new(stream, observation));
        }
        prelude
            .submit_presentation_observation(HostPresentationObservation::Batch(entries))
            .map_err(|_| PresentationObservationError::PendingRosterMismatch)?;
        Ok(submitted)
    }

    #[allow(
        clippy::needless_pass_by_value,
        reason = "the painted-output capability is affine and must be consumed"
    )]
    pub(super) fn report(
        &mut self,
        output: PaintedSurfaceOutput,
        result: SurfacePresentationResult,
    ) -> Result<(), SurfacePresentationReportError> {
        let mut output = output;
        let raw_output = output.output;
        if !self.pending.record(raw_output, result) {
            return Err(SurfacePresentationReportError { output });
        }
        output.disarm();
        Ok(())
    }

    #[allow(
        clippy::needless_pass_by_value,
        reason = "the painted-output capability is affine and must be consumed"
    )]
    pub(super) fn report_staging(
        &mut self,
        output: PaintedNativeStagingOutput,
        result: SurfacePresentationResult,
    ) -> Result<(), NativeStagingPresentationReportError> {
        let mut output = output;
        let raw_output = output.raw_output();
        if !matches!(
            raw_output.payload(),
            HostPresentationOutputPayload::NativeStaging { .. }
        ) || !self.pending.record(raw_output, result)
        {
            return Err(NativeStagingPresentationReportError { output });
        }
        output.disarm();
        Ok(())
    }

    pub(super) fn commit_observation(
        &mut self,
        engine: &DockEngine,
        transition: &EngineTransition,
        submitted: &SubmittedPresentationObservation,
    ) -> Vec<SurfacePresentationCommit> {
        let mut commits = Vec::with_capacity(submitted.captured.len());
        for outcome in transition.presentation_observations() {
            let HostPresentationObservationOutcome::Retired {
                stream,
                generation,
                settled_through,
                presented,
                promotion_eligible,
                ..
            } = outcome
            else {
                continue;
            };
            let Some((submitted_generation, report)) = submitted.get(*stream) else {
                continue;
            };
            if generation != &submitted_generation
                || settled_through != &report.settled_through
                || presented != &report.authority()
            {
                continue;
            }
            let state = self
                .streams
                .iter_mut()
                .find(|state| state.stream == *stream)
                .expect("the submitted observation retains its stream metadata");
            let presented_artifact = report.presented.map_or_else(
                || {
                    state.presented.filter(|artifact| {
                        engine.scene().retains_observed_emission(artifact.output())
                    })
                },
                |presented| {
                    self.pending
                        .lock()
                        .iter()
                        .map(|entry| entry.output)
                        .find(|output| output.stream() == *stream && output.key() == presented)
                        .filter(|output| {
                            *promotion_eligible && engine.scene().retains_observed_emission(*output)
                        })
                        .map(|output| SurfacePresentationArtifact { output })
                },
            );
            commits.push(SurfacePresentationCommit::new(
                *stream,
                *settled_through,
                presented_artifact,
            ));
            state.generation = *generation;
            state.recorded = None;
            state.presented = presented_artifact;
            self.pending.lock().retain(|entry| {
                entry.output.stream() != *stream || entry.output.key() > *settled_through
            });
        }
        commits
    }

    pub(super) fn prepare_emissions(
        &mut self,
        transition: &EngineTransition,
        submitted: &SubmittedPresentationObservation,
    ) -> Result<PreparedPresentationEmissions, NativePlatformError> {
        let emissions = transition.presentation_emissions();
        let mut pending = self.pending.lock();
        let surviving = pending
            .iter()
            .filter(|entry| {
                !transition
                    .presentation_observations()
                    .iter()
                    .filter_map(|outcome| submitted_retirement(outcome, submitted))
                    .any(|(stream, through)| {
                        entry.output.stream() == stream && entry.output.key() <= through
                    })
            })
            .count();
        let total = surviving
            .checked_add(emissions.len())
            .ok_or(NativePlatformError::InputCapacity)?;
        if let Some(limit) = self.slot_limit {
            if total > limit {
                return Err(NativePlatformError::InputCapacity);
            }
        }
        // Stream metadata is retained until core compaction, independently of
        // whether any output currently awaits a terminal report.
        let is_new_stream = |index: usize| {
            let stream = emissions[index].output().stream();
            !self.streams.iter().any(|state| state.stream == stream)
                && !emissions[..index]
                    .iter()
                    .any(|emission| emission.output().stream() == stream)
        };
        let new_stream_count = (0..emissions.len())
            .filter(|index| is_new_stream(*index))
            .count();
        let stream_count = self
            .streams
            .len()
            .checked_add(new_stream_count)
            .ok_or(NativePlatformError::InputCapacity)?;
        if self.slot_limit.is_some_and(|limit| stream_count > limit) {
            return Err(NativePlatformError::InputCapacity);
        }
        let mut streams = Vec::new();
        streams
            .try_reserve_exact(new_stream_count)
            .map_err(|_| NativePlatformError::InputCapacity)?;
        for index in (0..emissions.len()).filter(|index| is_new_stream(*index)) {
            streams.push(RuntimePresentationStream {
                stream: emissions[index].output().stream(),
                generation: HostPresentationCaptureGeneration::default(),
                recorded: None,
                presented: None,
            });
        }
        self.streams
            .try_reserve_exact(streams.len())
            .map_err(|_| NativePlatformError::InputCapacity)?;
        let additional = total.saturating_sub(pending.len());
        pending
            .try_reserve_exact(additional)
            .map_err(|_| NativePlatformError::InputCapacity)?;
        drop(pending);
        let mut entries = Vec::new();
        let mut surfaces = Vec::new();
        let mut staging = Vec::new();
        entries
            .try_reserve_exact(emissions.len())
            .map_err(|_| NativePlatformError::InputCapacity)?;
        let surface_count = emissions
            .iter()
            .filter(|emission| {
                matches!(
                    emission.output().payload(),
                    HostPresentationOutputPayload::Paint { .. }
                )
            })
            .count();
        surfaces
            .try_reserve_exact(surface_count)
            .map_err(|_| NativePlatformError::InputCapacity)?;
        staging
            .try_reserve_exact(emissions.len() - surface_count)
            .map_err(|_| NativePlatformError::InputCapacity)?;
        for emission in emissions {
            let output = emission.output();
            entries.push(PendingPresentationOutput {
                output,
                result: None,
            });
            match output.payload() {
                HostPresentationOutputPayload::Paint { .. } => {
                    surfaces.push(PaintedSurfaceOutput {
                        output,
                        abandoned: self.pending.clone(),
                        armed: false,
                    })
                }
                HostPresentationOutputPayload::NativeStaging { presentation } => {
                    let HostPresentationEndpoint::Native(binding) = output.endpoint() else {
                        return Err(NativePlatformError::ProtocolInvariant);
                    };
                    staging.push(PaintedNativeStagingOutput {
                        output,
                        request: NativeStagingPaintRequest::from_core(
                            NativeSurfaceBinding::from_binding(
                                presentation.basis().platform_provider(),
                                binding,
                            ),
                            presentation,
                        ),
                        abandoned: self.pending.clone(),
                        armed: false,
                    });
                }
                HostPresentationOutputPayload::Bootstrap
                | HostPresentationOutputPayload::Unavailable => {
                    return Err(NativePlatformError::ProtocolInvariant);
                }
            }
        }
        Ok(PreparedPresentationEmissions {
            pending: self.pending.clone(),
            entries,
            streams,
            surfaces,
            staging,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geometry::{LogicalRect, LogicalSize};
    use crate::graph::{Node, RootRecord, SurfacePresentation, Workspace};
    use crate::ids::{ItemId, RootId, SurfaceId};
    use crate::model::{DockAnchor, DockPlacement};
    use crate::policy::DockPolicy;
    use crate::runtime::{
        DockspaceSession, NativePointerRoster, NativeReceiverAnswer, SurfaceUnavailableReason,
        UniformSurfaceMetrics,
    };

    fn single_surface_session() -> DockspaceSession {
        let mut builder = Workspace::builder();
        let tabs = builder.insert_node(Node::tabs([ItemId::new(1)]));
        builder.set_root(RootId::new(1), RootRecord::new(tabs).with_central(tabs));
        builder.set_surface(
            SurfaceId::new(1),
            SurfacePresentation::with_main(RootId::new(1)),
        );
        DockspaceSession::from_workspace_for_test(
            builder
                .build()
                .expect("presentation test workspace validates"),
            DockPolicy::default(),
        )
        .expect("presentation test session initializes")
    }

    fn measure_single_surface(session: &mut DockspaceSession) {
        let metrics = UniformSurfaceMetrics::new(
            LogicalRect::new(0.0, 0.0, 640.0, 480.0).expect("presentation test bounds validate"),
            LogicalSize::new(32.0, 24.0).expect("presentation test minimum validates"),
            80.0,
        )
        .expect("presentation test metrics validate");
        let mut frame = session
            .begin_host_frame()
            .expect("presentation measurement frame begins");
        frame
            .measure_surface(SurfaceId::new(1), metrics)
            .expect("presentation surface measures");
        frame.commit().expect("presentation measurement commits");
    }

    fn paint_single_surface(session: &mut DockspaceSession) -> PaintedSurfaceOutput {
        let mut frame = session
            .begin_host_frame()
            .expect("presentation paint frame begins");
        let receipt = frame
            .paint_plan(SurfaceId::new(1))
            .expect("presentation paint plan resolves")
            .expect("presentation surface is paintable")
            .execute_surface_program(|_| Ok::<(), ()>(()))
            .expect("presentation renderer executes the complete program");
        frame
            .confirm_surface_painted(receipt)
            .expect("presentation paint confirms");
        let mut report = frame.commit().expect("presentation paint commits");
        report
            .take_painted_outputs()
            .pop()
            .expect("presentation paint emits one output")
    }

    fn settle_presentations(session: &mut DockspaceSession) -> super::super::HostFrameReport {
        let mut frame = session
            .begin_host_frame()
            .expect("presentation settlement frame begins");
        frame
            .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
            .expect("presentation settlement retains the surface");
        frame.commit().expect("presentation settlement commits")
    }

    #[test]
    fn bounded_output_rejection_preserves_publication_and_one_slot_can_retire_and_reemit() {
        let mut session = single_surface_session();
        session.presentation = RuntimePresentationState::with_capacity_limit(1).unwrap();
        measure_single_surface(&mut session);
        let first = paint_single_surface(&mut session);
        let artifact = first.presentation_artifact();
        let frontier = session.engine.presentation_identity_frontier();
        let version = session.version();
        let capacity = session.presentation.pending.lock().capacity();
        let mut frame = session.begin_host_frame().unwrap();
        let receipt = frame
            .paint_plan(SurfaceId::new(1))
            .unwrap()
            .unwrap()
            .execute_surface_program(|_| Ok::<(), ()>(()))
            .unwrap();
        frame.confirm_surface_painted(receipt).unwrap();
        let error = frame.commit().unwrap_err();
        assert_eq!(
            error.native_kind(),
            Some(super::super::NativeHostErrorKind::InputCapacity)
        );
        assert_eq!(session.version(), version);
        assert_eq!(session.engine.presentation_identity_frontier(), frontier);
        assert_eq!(session.presentation.pending.lock().capacity(), capacity);
        assert_eq!(session.presentation.pending.lock().len(), 1);
        assert!(session.presentation.pending.lock()[0].result.is_none());

        session
            .report_surface_presentation(first, SurfacePresentationResult::Presented)
            .unwrap();
        let mut frame = session.begin_host_frame().unwrap();
        let receipt = frame
            .paint_plan(SurfaceId::new(1))
            .unwrap()
            .unwrap()
            .execute_surface_program(|_| Ok::<(), ()>(()))
            .unwrap();
        frame.confirm_surface_painted(receipt).unwrap();
        let mut report = frame.commit().unwrap();
        assert!(report.surface_presentation_commits()[0].presents(artifact));
        assert_eq!(session.presentation.pending.lock().capacity(), capacity);
        assert_eq!(session.presentation.pending.lock().len(), 1);
        let replacement = report.take_painted_outputs().pop().unwrap();
        let replacement_artifact = replacement.presentation_artifact();
        let before_drop = session.presentation.owned_data_capacity_bytes();
        std::thread::spawn(move || drop(replacement))
            .join()
            .unwrap();
        assert_eq!(
            session.presentation.owned_data_capacity_bytes(),
            before_drop
        );
        assert!(
            settle_presentations(&mut session).surface_presentation_commits()[0]
                .retires(replacement_artifact)
        );
        assert_eq!(session.presentation.pending.lock().capacity(), capacity);
        assert!(session.presentation.pending.is_empty());
    }

    #[test]
    fn late_cross_thread_drop_keeps_its_slot_after_an_earlier_prefix_settles() {
        let mut session = single_surface_session();
        session.presentation = RuntimePresentationState::with_capacity_limit(3).unwrap();
        measure_single_surface(&mut session);
        let first = paint_single_surface(&mut session);
        let second = paint_single_surface(&mut session);
        let third = paint_single_surface(&mut session);
        let third_artifact = third.presentation_artifact();
        let capacity = session.presentation.pending.lock().capacity();
        drop(first);
        session
            .report_surface_presentation(second, SurfacePresentationResult::Presented)
            .unwrap();
        settle_presentations(&mut session);
        assert_eq!(session.presentation.pending.lock().len(), 1);
        assert_eq!(session.presentation.pending.lock().capacity(), capacity);
        let fourth = paint_single_surface(&mut session);
        let fourth_artifact = fourth.presentation_artifact();
        let mut frame = session.begin_host_frame().unwrap();
        frame
            .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
            .unwrap();
        std::thread::spawn(move || drop(third)).join().unwrap();
        frame.commit().unwrap();
        assert_eq!(session.presentation.pending.lock().capacity(), capacity);
        session
            .report_surface_presentation(fourth, SurfacePresentationResult::Presented)
            .unwrap();
        let report = settle_presentations(&mut session);
        assert!(report.surface_presentation_commits()[0].retires(third_artifact));
        assert!(report.surface_presentation_commits()[0].presents(fourth_artifact));
        assert_eq!(session.presentation.pending.lock().capacity(), capacity);
        assert!(session.presentation.pending.is_empty());
    }

    #[test]
    fn full_native_prelude_keeps_later_recorded_stream_in_exact_commit_settlement() {
        let mut builder = Workspace::builder();
        for index in 1..=2 {
            let root = RootId::new(index);
            let tabs = builder.insert_node(Node::tabs([ItemId::new(index)]));
            builder.set_root(root, RootRecord::new(tabs).with_central(tabs));
            builder.set_surface(SurfaceId::new(index), SurfacePresentation::with_main(root));
        }
        let mut session = DockspaceSession::from_workspace_for_test(
            builder.build().unwrap(),
            DockPolicy::default(),
        )
        .unwrap();
        session
            .enable_managed_native_host(
                crate::runtime::NativeCreateStrategy::Staged,
                NativePointerRoster::Exact(Vec::new()),
            )
            .unwrap();
        let metrics = UniformSurfaceMetrics::new(
            LogicalRect::new(0.0, 0.0, 640.0, 480.0).unwrap(),
            LogicalSize::new(32.0, 24.0).unwrap(),
            80.0,
        )
        .unwrap();
        let mut measured = session.begin_host_frame().unwrap();
        for surface in measured.surfaces() {
            measured.measure_surface(surface, metrics).unwrap();
        }
        measured.commit().unwrap();
        let mut painted = session.begin_host_frame().unwrap();
        for surface in painted.surfaces() {
            let receipt = painted
                .paint_plan(surface)
                .unwrap()
                .unwrap()
                .execute_surface_program(|_| Ok::<(), ()>(()))
                .unwrap();
            painted.confirm_surface_painted(receipt).unwrap();
        }
        let mut outputs = painted.commit().unwrap().take_painted_outputs();
        outputs.sort_by_key(|output| output.output.stream());
        let [earlier, later] = outputs.try_into().unwrap();
        let earlier_artifact = earlier.presentation_artifact();
        let later_artifact = later.presentation_artifact();
        let native = session.native.as_mut().unwrap();
        native
            .reclaim_committed_prefix(&mut session.engine)
            .unwrap();
        native
            .recorder_mut()
            .configure_native_transport_capacity(std::num::NonZeroUsize::new(1).unwrap(), 1 << 20)
            .unwrap();

        session
            .report_surface_presentation(later, SurfacePresentationResult::Presented)
            .unwrap();
        let before = session.engine.backend_ingress_committed_through();
        drop(session.begin_host_frame().unwrap());
        let recorded = session.native_input_recorded_count();
        assert_eq!(recorded, before.get() + 1);
        assert_eq!(
            session
                .presentation
                .streams
                .iter()
                .filter(|state| state.recorded.is_some())
                .count(),
            1
        );
        assert_eq!(session.engine.backend_ingress_committed_through(), before);

        session
            .report_surface_presentation(earlier, SurfacePresentationResult::Presented)
            .unwrap();
        let mut frame = session.begin_host_frame().unwrap();
        assert!(frame.native_input_pending());
        assert!(frame.cancel_native_input_for_overload().is_err());
        frame
            .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
            .unwrap();
        let report = frame.commit().unwrap();
        assert!(report.native_input_pending());
        let [later_commit] = report.surface_presentation_commits() else {
            panic!("the already recorded later stream must settle despite earlier backpressure");
        };
        assert!(later_commit.presents(later_artifact));
        assert_eq!(session.native_input_recorded_count(), recorded);
        assert!(
            session
                .presentation
                .streams
                .iter()
                .all(|state| state.recorded.is_none())
        );

        let report = settle_presentations(&mut session);
        assert!(!report.native_input_pending());
        let [earlier_commit] = report.surface_presentation_commits() else {
            panic!("the deferred earlier stream must settle exactly once after reclamation");
        };
        assert!(earlier_commit.presents(earlier_artifact));
        assert_eq!(session.native_input_recorded_count(), recorded + 1);
        assert!(session.presentation.pending.is_empty());
        assert!(
            settle_presentations(&mut session)
                .surface_presentation_commits()
                .is_empty()
        );
    }

    #[test]
    fn core_commit_ack_selects_one_exact_artifact_and_retires_the_settled_prefix() {
        let mut session = single_surface_session();
        measure_single_surface(&mut session);

        let first = paint_single_surface(&mut session);
        let first_artifact = first.presentation_artifact();
        session
            .report_surface_presentation(first, SurfacePresentationResult::Presented)
            .expect("the first renderer result records");
        let first_commit = settle_presentations(&mut session);
        let [first_ack] = first_commit.surface_presentation_commits() else {
            panic!("the first renderer result must produce one core commit ack")
        };
        assert!(first_ack.presents(first_artifact));
        assert!(!first_ack.retires(first_artifact));

        let second = paint_single_surface(&mut session);
        let second_artifact = second.presentation_artifact();
        assert_ne!(first_artifact, second_artifact);
        session
            .report_surface_presentation(second, SurfacePresentationResult::Dropped)
            .expect("the second renderer result records");
        let second_commit = settle_presentations(&mut session);
        let [second_ack] = second_commit.surface_presentation_commits() else {
            panic!("the dropped successor must produce one core commit ack")
        };
        assert!(!second_ack.retires(first_artifact));
        assert!(second_ack.retires(second_artifact));
        assert!(second_ack.presents(first_artifact));
        assert!(!second_ack.presents(second_artifact));
    }

    #[test]
    fn headless_surface_retirement_reclaims_after_native_enrollment() {
        let first_surface = SurfaceId::new(1);
        let second_surface = SurfaceId::new(2);
        let first_root = RootId::new(1);
        let second_root = RootId::new(2);
        let first_item = ItemId::new(1);
        let second_item = ItemId::new(2);
        let mut builder = Workspace::builder();
        let first_tabs = builder.insert_node(Node::tabs([first_item]));
        let second_tabs = builder.insert_node(Node::tabs([second_item]));
        builder.set_root(first_root, RootRecord::new(first_tabs));
        builder.set_root(second_root, RootRecord::new(second_tabs));
        builder.set_surface(first_surface, SurfacePresentation::with_main(first_root));
        builder.set_surface(second_surface, SurfacePresentation::with_main(second_root));
        let mut session = DockspaceSession::from_workspace_for_test(
            builder.build().expect("the headless workspace validates"),
            DockPolicy::default(),
        )
        .expect("the headless runtime initializes");
        let metrics = UniformSurfaceMetrics::new(
            LogicalRect::new(0.0, 0.0, 640.0, 480.0).expect("test bounds validate"),
            LogicalSize::new(32.0, 24.0).expect("test minimum validates"),
            80.0,
        )
        .expect("test metrics validate");

        let mut measured = session
            .begin_host_frame()
            .expect("the measurement frame begins");
        for surface in measured.surfaces() {
            measured
                .measure_surface(surface, metrics)
                .expect("each surface measurement stages");
        }
        measured.commit().expect("the measurements commit");

        let mut painted = session.begin_host_frame().expect("the paint frame begins");
        let receipt = painted
            .paint_plan(first_surface)
            .expect("the first paint plan resolves")
            .expect("the first surface is paintable")
            .execute_surface_program(|_| Ok::<(), ()>(()))
            .expect("the complete first-surface program executes");
        painted
            .confirm_surface_painted(receipt)
            .expect("the first surface paints");
        painted
            .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
            .expect("the second surface remains deferred");
        let mut report = painted.commit().expect("the painted output emits");
        let output = report
            .take_painted_outputs()
            .pop()
            .expect("the first surface emits one output");
        session
            .report_surface_presentation(output, SurfacePresentationResult::Presented)
            .expect("the exact output presentation records");
        let mut observed = session
            .begin_host_frame()
            .expect("the presentation observation frame begins");
        observed
            .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
            .expect("the observation frame settles every surface");
        observed
            .commit()
            .expect("the presentation observation commits");
        assert_eq!(session.presentation.retained_capture_generation_count(), 1);

        let mut redock = session.begin_host_frame().expect("the redock frame begins");
        redock
            .dock_root_current(
                first_root,
                DockPlacement::Center(DockAnchor::Item(second_item)),
            )
            .expect("the first root redocks into the second surface");
        redock
            .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
            .expect("the redock frame settles every surface");
        redock.commit().expect("the redock commits");
        assert_eq!(session.presentation.retained_capture_generation_count(), 1);

        session
            .enable_managed_native_host(
                crate::runtime::NativeCreateStrategy::Staged,
                NativePointerRoster::Exact(Vec::new()),
            )
            .expect("the managed native provider enrolls after headless painting");
        let mut reclaimed = session
            .begin_native_host_frame(|_| NativeReceiverAnswer::Unknown)
            .expect("the reclamation boundary begins");
        reclaimed
            .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
            .expect("the reclamation frame settles every surface");
        reclaimed.commit().expect("the reclamation frame commits");
        assert_eq!(session.presentation.retained_capture_generation_count(), 0);
        assert_eq!(
            session
                .engine
                .runtime_retention_manifest()
                .presentation_hosts()
                .retained_stream_states(),
            0,
        );
    }
}
