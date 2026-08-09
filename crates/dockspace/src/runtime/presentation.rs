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
    HostPresentationObservation, HostPresentationObservationEntry,
    HostPresentationObservationOutcome, HostPresentationOutput, HostPresentationProgress,
    HostPresentationStreamId, HostPresentationStreamObservation,
};
use crate::transition::EngineTransition;

/// Affine record for one output which the host actually painted.
///
/// This capability is not presentation authority. The renderer may consume it
/// only after receiving an exact final-presentation result for that output.
#[derive(Debug)]
#[must_use = "a painted output must be explicitly confirmed after final presentation"]
pub struct PaintedSurfaceOutput {
    output: HostPresentationOutput,
    abandoned: PresentationDropQueue,
    armed: bool,
}

impl PaintedSurfaceOutput {
    /// Returns the logical surface represented by this output.
    #[must_use]
    pub const fn surface(&self) -> SurfaceId {
        self.output.surface()
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
    /// Reports one actual renderer result for a previously painted output.
    ///
    /// The terminal fact is submitted at the next host-frame prelude. A
    /// `Presented` result may grant interaction authority after publication;
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

    pub(super) fn commit_observation(
        &mut self,
        transition: &EngineTransition,
        submitted: &SubmittedPresentationObservation,
    ) {
        for outcome in transition.presentation_observations() {
            let HostPresentationObservationOutcome::Retired {
                stream,
                generation,
                settled_through,
                presented,
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
    }

    pub(super) fn retain_emissions(
        &mut self,
        emissions: &[HostPresentationEmission],
    ) -> Vec<PaintedSurfaceOutput> {
        emissions
            .iter()
            .map(|emission| {
                let output = emission.output();
                self.pending
                    .entry(output.stream())
                    .or_default()
                    .push(output);
                PaintedSurfaceOutput {
                    output,
                    abandoned: self.abandoned.clone(),
                    armed: true,
                }
            })
            .collect()
    }
}
