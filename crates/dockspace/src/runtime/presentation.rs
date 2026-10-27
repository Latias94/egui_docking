//! Private final-presentation sidecar for the public runtime facade.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Arc, Mutex, PoisonError};

use thiserror::Error;

use crate::backend_ingress::{BackendIngressLease, BackendIngressRecorder};
use crate::engine::CoreHostFramePrelude;
use crate::engine::DockEngine;
use crate::ids::SurfaceId;
use crate::intent::Authority;
use crate::presentation_observation::{
    HostFrameKey, HostPresentationCaptureGeneration, HostPresentationEmission,
    HostPresentationEndpoint, HostPresentationObservation, HostPresentationObservationEntry,
    HostPresentationObservationOutcome, HostPresentationOutput, HostPresentationOutputPayload,
    HostPresentationProgress, HostPresentationStreamId, HostPresentationStreamObservation,
    NativeStagingPresentation, NativeStagingPresentationPhase as CoreNativeStagingPhase,
};
use crate::transition::EngineTransition;

use super::native::NativeSurfaceBinding;

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
    abandoned: PresentationDropQueue,
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
/// interaction gates, such as a popup awaiting presentation on another surface;
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
    abandoned: PresentationDropQueue,
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
            self.abandoned.push(self.output);
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
            self.abandoned.push(self.output);
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
    pending: BTreeMap<HostPresentationStreamId, Vec<HostPresentationOutput>>,
    results: BTreeMap<HostPresentationStreamId, BTreeMap<HostFrameKey, SurfacePresentationResult>>,
    capture_generations: BTreeMap<HostPresentationStreamId, HostPresentationCaptureGeneration>,
    backend_recorded: BTreeMap<HostPresentationStreamId, RecordedPresentationReport>,
    abandoned: PresentationDropQueue,
}

/// Session-owned sink for painted outputs abandoned after core emission.
///
/// This is a small private terminal-result queue. It prevents a forgotten
/// affine output from blocking every newer result in the same ordered stream.
#[derive(Debug, Clone, Default)]
struct PresentationDropQueue(Arc<Mutex<Vec<HostPresentationOutput>>>);

impl PresentationDropQueue {
    fn lock(&self) -> std::sync::MutexGuard<'_, Vec<HostPresentationOutput>> {
        self.0.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn push(&self, output: HostPresentationOutput) {
        self.lock().push(output);
    }

    fn take(&self) -> Vec<HostPresentationOutput> {
        std::mem::take(&mut *self.lock())
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
        self.presented = (result == SurfacePresentationResult::Presented).then_some(output);
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct RecordedPresentationReport {
    provider: BackendIngressLease,
    generation: HostPresentationCaptureGeneration,
    report: PendingPresentationReport,
}

#[derive(Debug, Default)]
pub(super) struct SubmittedPresentationObservation {
    captured: BTreeMap<
        HostPresentationStreamId,
        (HostPresentationCaptureGeneration, PendingPresentationReport),
    >,
}

impl RuntimePresentationState {
    pub(super) fn reclaim_quiescent_streams(
        &mut self,
        engine: &mut DockEngine,
        host: crate::presentation_observation::PresentationHostLease,
    ) -> Result<(), super::DockspaceRuntimeError> {
        let candidates = self
            .capture_generations
            .keys()
            .copied()
            .filter(|stream| {
                !self.pending.contains_key(stream)
                    && !self.results.contains_key(stream)
                    && !self.backend_recorded.contains_key(stream)
            })
            .collect::<Vec<_>>();
        let mut quiescences = Vec::new();
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
        self.capture_generations
            .retain(|stream, _| retained.retains_stream(*stream));
        self.backend_recorded
            .retain(|stream, _| retained.retains_stream(*stream));
    }

    #[cfg(test)]
    pub(super) fn retained_capture_generation_count(&self) -> usize {
        self.capture_generations.len()
    }

    fn drain_abandoned(&mut self) {
        for output in self.abandoned.take() {
            let is_pending = self
                .pending
                .get(&output.stream())
                .is_some_and(|pending| pending.contains(&output));
            if is_pending {
                self.results
                    .entry(output.stream())
                    .or_default()
                    .entry(output.key())
                    .or_insert(SurfacePresentationResult::Dropped);
            }
        }
    }

    fn compile_report(
        &self,
        stream: HostPresentationStreamId,
    ) -> Option<PendingPresentationReport> {
        let pending = self.pending.get(&stream)?;
        let results = self.results.get(&stream)?;
        let mut report: Option<PendingPresentationReport> = None;
        for output in pending {
            let Some(result) = results.get(&output.key()).copied() else {
                break;
            };
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
    ) -> Result<SubmittedPresentationObservation, super::DockspaceRuntimeError> {
        self.drain_abandoned();
        let scope = prelude
            .pending_presentation_streams()
            .collect::<BTreeSet<_>>();
        if scope != self.pending.keys().copied().collect() {
            return Err(PresentationObservationError::PendingRosterMismatch.into());
        }
        prelude.submit_presentation_observation(HostPresentationObservation::NoUpdate)?;

        let provider = recorder.lease();
        let mut submitted = SubmittedPresentationObservation::default();
        for stream in scope {
            let generation = self
                .capture_generations
                .get(&stream)
                .copied()
                .unwrap_or_default()
                .checked_next()
                .ok_or(PresentationObservationError::CaptureGenerationExhausted)?;

            let recorded = self.backend_recorded.get(&stream).copied();
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
                    engine.record_backend_presentation_observation(recorder, entry)?;
                    self.backend_recorded.insert(
                        stream,
                        RecordedPresentationReport {
                            provider,
                            generation,
                            report,
                        },
                    );
                    report
                }
            };
            submitted.captured.insert(stream, (generation, report));
        }
        Ok(submitted)
    }

    pub(super) fn submit_observation(
        &mut self,
        prelude: &mut CoreHostFramePrelude,
    ) -> Result<SubmittedPresentationObservation, PresentationObservationError> {
        self.drain_abandoned();
        let scope = prelude
            .pending_presentation_streams()
            .collect::<BTreeSet<_>>();
        if scope != self.pending.keys().copied().collect() {
            return Err(PresentationObservationError::PendingRosterMismatch);
        }
        if scope.is_empty() {
            prelude
                .submit_presentation_observation(HostPresentationObservation::NoUpdate)
                .map_err(|_| PresentationObservationError::PendingRosterMismatch)?;
            return Ok(SubmittedPresentationObservation::default());
        }

        let mut submitted = SubmittedPresentationObservation::default();
        let mut entries = Vec::with_capacity(scope.len());
        for stream in scope {
            let observation = if let Some(report) = self.compile_report(stream) {
                let generation = self
                    .capture_generations
                    .get(&stream)
                    .copied()
                    .unwrap_or_default()
                    .checked_next()
                    .ok_or(PresentationObservationError::CaptureGenerationExhausted)?;
                submitted.captured.insert(stream, (generation, report));
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
        let Some(pending) = self.pending.get(&raw_output.stream()) else {
            return Err(SurfacePresentationReportError { output });
        };
        if !pending.contains(&raw_output) {
            return Err(SurfacePresentationReportError { output });
        }
        let stream = raw_output.stream();
        self.results
            .entry(stream)
            .or_default()
            .insert(raw_output.key(), result);
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
        let Some(pending) = self.pending.get(&raw_output.stream()) else {
            return Err(NativeStagingPresentationReportError { output });
        };
        if !pending.contains(&raw_output)
            || !matches!(
                raw_output.payload(),
                HostPresentationOutputPayload::NativeStaging { .. }
            )
        {
            return Err(NativeStagingPresentationReportError { output });
        }
        let stream = raw_output.stream();
        self.results
            .entry(stream)
            .or_default()
            .insert(raw_output.key(), result);
        output.disarm();
        Ok(())
    }

    pub(super) fn commit_observation(
        &mut self,
        engine: &DockEngine,
        transition: &EngineTransition,
        submitted: &SubmittedPresentationObservation,
    ) -> Vec<SurfacePresentationCommit> {
        let mut commits = Vec::new();
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
            let Some((submitted_generation, report)) = submitted.captured.get(stream) else {
                continue;
            };
            if generation != submitted_generation
                || settled_through != &report.settled_through
                || presented != &report.authority()
            {
                continue;
            }
            let presented_artifact = report.presented.and_then(|presented| {
                self.pending.get(stream).and_then(|outputs| {
                    outputs
                        .iter()
                        .find(|output| output.key() == presented)
                        .copied()
                        .filter(|output| {
                            *promotion_eligible && engine.scene().retains_observed_emission(*output)
                        })
                        .map(|output| SurfacePresentationArtifact { output })
                })
            });
            commits.push(SurfacePresentationCommit::new(
                *stream,
                *settled_through,
                presented_artifact,
            ));
            self.capture_generations.insert(*stream, *generation);
            let remove_stream = if let Some(outputs) = self.pending.get_mut(stream)
                && let Some(index) = outputs
                    .iter()
                    .position(|output| output.key() == *settled_through)
            {
                outputs.drain(..=index);
                outputs.is_empty()
            } else {
                false
            };
            if remove_stream {
                self.pending.remove(stream);
            }
            let remove_results = self.results.get_mut(stream).is_some_and(|results| {
                results.retain(|output, _| *output > *settled_through);
                results.is_empty()
            });
            if remove_results {
                self.results.remove(stream);
            }
            self.backend_recorded.remove(stream);
        }
        commits
    }

    pub(super) fn retain_emissions(
        &mut self,
        emissions: &[HostPresentationEmission],
    ) -> (Vec<PaintedSurfaceOutput>, Vec<PaintedNativeStagingOutput>) {
        let mut surfaces = Vec::new();
        let mut staging = Vec::new();
        for emission in emissions {
            let output = emission.output();
            self.pending
                .entry(output.stream())
                .or_default()
                .push(output);
            match output.payload() {
                HostPresentationOutputPayload::Paint { .. } => {
                    surfaces.push(PaintedSurfaceOutput {
                        output,
                        abandoned: self.abandoned.clone(),
                        armed: true,
                    })
                }
                HostPresentationOutputPayload::NativeStaging { presentation } => {
                    // The public request is reconstructed from the exact core
                    // payload; no caller-supplied surface or phase is trusted.
                    let binding = match output.endpoint() {
                        HostPresentationEndpoint::Native(binding) => binding,
                        HostPresentationEndpoint::Headless => {
                            debug_assert!(false, "native staging output has a native endpoint");
                            self.abandoned.push(output);
                            continue;
                        }
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
                        abandoned: self.abandoned.clone(),
                        armed: true,
                    });
                }
                HostPresentationOutputPayload::Bootstrap
                | HostPresentationOutputPayload::Unavailable => {
                    debug_assert!(
                        false,
                        "bootstrap/unavailable output cannot be painted by runtime"
                    );
                    self.abandoned.push(output);
                }
            }
        }
        (surfaces, staging)
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
        assert!(second_ack.retires(first_artifact));
        assert!(second_ack.retires(second_artifact));
        assert!(!second_ack.presents(first_artifact));
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
            .enable_managed_native_host(NativePointerRoster::Exact(Vec::new()))
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
