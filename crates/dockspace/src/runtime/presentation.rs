//! Private final-presentation sidecar for the public runtime facade.

use std::collections::{BTreeMap, BTreeSet};

use thiserror::Error;

use crate::engine::CoreHostFramePrelude;
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
#[derive(Debug, PartialEq, Eq)]
#[must_use = "a painted output must be explicitly confirmed after final presentation"]
pub struct PaintedSurfaceOutput {
    output: HostPresentationOutput,
}

impl PaintedSurfaceOutput {
    /// Returns the logical surface represented by this output.
    #[must_use]
    pub const fn surface(&self) -> SurfaceId {
        self.output.surface()
    }
}

/// Failure while consuming a final-presentation capability.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum PresentationConfirmationError {
    /// The output did not originate from this session or is no longer pending.
    #[error("painted output is foreign, stale, or already retired")]
    OutputNotPending,
    /// One stream already selected a concrete output for its next capture.
    #[error("presentation stream already has a confirmed output awaiting observation")]
    ConfirmationAlreadyPending,
    /// The facade and core disagree about the exact pending stream roster.
    #[error("presentation sidecar does not match the core-owned pending stream roster")]
    PendingRosterMismatch,
    /// A provider capture generation cannot advance without wrapping.
    #[error("presentation capture generation is exhausted")]
    CaptureGenerationExhausted,
}

#[derive(Debug, Default)]
pub(super) struct RuntimePresentationState {
    pending: BTreeMap<HostPresentationStreamId, Vec<HostPresentationOutput>>,
    confirmed: BTreeMap<HostPresentationStreamId, HostFrameKey>,
    capture_generations: BTreeMap<HostPresentationStreamId, HostPresentationCaptureGeneration>,
}

#[derive(Debug, Default)]
pub(super) struct SubmittedPresentationObservation {
    captured: BTreeMap<HostPresentationStreamId, HostFrameKey>,
}

impl RuntimePresentationState {
    pub(super) fn submit_observation(
        &mut self,
        prelude: &mut CoreHostFramePrelude,
    ) -> Result<SubmittedPresentationObservation, PresentationConfirmationError> {
        let scope = prelude
            .pending_presentation_streams()
            .collect::<BTreeSet<_>>();
        if scope != self.pending.keys().copied().collect() {
            return Err(PresentationConfirmationError::PendingRosterMismatch);
        }
        if scope.is_empty() {
            prelude
                .submit_presentation_observation(HostPresentationObservation::NoUpdate)
                .map_err(|_| PresentationConfirmationError::PendingRosterMismatch)?;
            return Ok(SubmittedPresentationObservation::default());
        }

        let mut submitted = SubmittedPresentationObservation::default();
        let mut entries = Vec::with_capacity(scope.len());
        for stream in scope {
            let observation = if let Some(presented) = self.confirmed.get(&stream).copied() {
                let generation = self
                    .capture_generations
                    .get(&stream)
                    .copied()
                    .unwrap_or_default()
                    .checked_next()
                    .ok_or(PresentationConfirmationError::CaptureGenerationExhausted)?;
                self.capture_generations.insert(stream, generation);
                submitted.captured.insert(stream, presented);
                HostPresentationStreamObservation::Captured {
                    generation,
                    progress: HostPresentationProgress::Retired {
                        settled_through: presented,
                        presented: Authority::Known(Some(presented)),
                    },
                }
            } else {
                HostPresentationStreamObservation::NoUpdate
            };
            entries.push(HostPresentationObservationEntry::new(stream, observation));
        }
        prelude
            .submit_presentation_observation(HostPresentationObservation::Batch(entries))
            .map_err(|_| PresentationConfirmationError::PendingRosterMismatch)?;
        Ok(submitted)
    }

    #[allow(
        clippy::needless_pass_by_value,
        reason = "the painted-output capability is affine and must be consumed"
    )]
    pub(super) fn confirm_presented(
        &mut self,
        output: PaintedSurfaceOutput,
    ) -> Result<(), PresentationConfirmationError> {
        let output = output.output;
        let Some(pending) = self.pending.get(&output.stream()) else {
            return Err(PresentationConfirmationError::OutputNotPending);
        };
        if !pending.contains(&output) {
            return Err(PresentationConfirmationError::OutputNotPending);
        }
        if self.confirmed.contains_key(&output.stream()) {
            return Err(PresentationConfirmationError::ConfirmationAlreadyPending);
        }
        self.confirmed.insert(output.stream(), output.key());
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
                settled_through,
                ..
            } = outcome
            else {
                continue;
            };
            if submitted.captured.get(stream) != Some(settled_through) {
                continue;
            }
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
            self.confirmed.remove(stream);
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
                PaintedSurfaceOutput { output }
            })
            .collect()
    }
}
