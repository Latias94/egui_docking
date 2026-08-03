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

/// Terminal result reported by the renderer for one exact output.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SurfacePresentationResult {
    /// The exact output reached final presentation.
    Presented,
    /// The exact output was discarded without reaching final presentation.
    Dropped,
}

impl SurfacePresentationResult {
    const fn authority(self, output: HostFrameKey) -> Authority<Option<HostFrameKey>> {
        match self {
            Self::Presented => Authority::Known(Some(output)),
            Self::Dropped => Authority::Known(None),
        }
    }
}

/// Typed rejection while settling a final-presentation capability.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum PresentationSettlementRejection {
    /// The output did not originate from this session or is no longer pending.
    #[error("painted output is foreign, stale, or already retired")]
    OutputNotPending,
    /// One stream already selected a terminal output for its next capture.
    #[error("presentation stream already has a settled output awaiting observation")]
    SettlementAlreadyPending,
}

/// Failed affine settlement with the original capability preserved for retry.
#[derive(Debug, Error)]
#[error("{rejection}")]
pub struct PresentationSettlementError {
    rejection: PresentationSettlementRejection,
    output: PaintedSurfaceOutput,
}

impl PresentationSettlementError {
    /// Returns the typed fail-closed rejection.
    #[must_use]
    pub const fn rejection(&self) -> PresentationSettlementRejection {
        self.rejection
    }

    /// Recovers the unconsumed affine output capability.
    #[must_use]
    pub fn into_output(self) -> PaintedSurfaceOutput {
        self.output
    }

    /// Splits the error into its rejection and unconsumed capability.
    #[must_use]
    pub fn into_parts(self) -> (PresentationSettlementRejection, PaintedSurfaceOutput) {
        (self.rejection, self.output)
    }
}

/// Failure while synchronizing the facade presentation sidecar with the core.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum PresentationObservationError {
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
    settlements: BTreeMap<HostPresentationStreamId, PendingPresentationSettlement>,
    capture_generations: BTreeMap<HostPresentationStreamId, HostPresentationCaptureGeneration>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct PendingPresentationSettlement {
    settled_through: HostFrameKey,
    result: SurfacePresentationResult,
}

#[derive(Debug, Default)]
pub(super) struct SubmittedPresentationObservation {
    captured: BTreeMap<
        HostPresentationStreamId,
        (
            HostPresentationCaptureGeneration,
            PendingPresentationSettlement,
        ),
    >,
}

impl RuntimePresentationState {
    pub(super) fn submit_observation(
        &self,
        prelude: &mut CoreHostFramePrelude,
    ) -> Result<SubmittedPresentationObservation, PresentationObservationError> {
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
            let observation = if let Some(settlement) = self.settlements.get(&stream).copied() {
                let generation = self
                    .capture_generations
                    .get(&stream)
                    .copied()
                    .unwrap_or_default()
                    .checked_next()
                    .ok_or(PresentationObservationError::CaptureGenerationExhausted)?;
                submitted.captured.insert(stream, (generation, settlement));
                HostPresentationStreamObservation::Captured {
                    generation,
                    progress: HostPresentationProgress::Retired {
                        settled_through: settlement.settled_through,
                        presented: settlement.result.authority(settlement.settled_through),
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
    pub(super) fn settle(
        &mut self,
        output: PaintedSurfaceOutput,
        result: SurfacePresentationResult,
    ) -> Result<(), PresentationSettlementError> {
        let raw_output = output.output;
        let Some(pending) = self.pending.get(&raw_output.stream()) else {
            return Err(PresentationSettlementError {
                rejection: PresentationSettlementRejection::OutputNotPending,
                output,
            });
        };
        if !pending.contains(&raw_output) {
            return Err(PresentationSettlementError {
                rejection: PresentationSettlementRejection::OutputNotPending,
                output,
            });
        }
        if self.settlements.contains_key(&raw_output.stream()) {
            return Err(PresentationSettlementError {
                rejection: PresentationSettlementRejection::SettlementAlreadyPending,
                output,
            });
        }
        self.settlements.insert(
            raw_output.stream(),
            PendingPresentationSettlement {
                settled_through: raw_output.key(),
                result,
            },
        );
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
            let Some((submitted_generation, settlement)) = submitted.captured.get(stream) else {
                continue;
            };
            if generation != submitted_generation
                || settled_through != &settlement.settled_through
                || presented != &settlement.result.authority(settlement.settled_through)
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
            self.settlements.remove(stream);
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
