//! Adapter-owned presentation output retention and settlement.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use dockspace::backend::engine::CoreHostFramePrelude;
use dockspace::backend::ids::SurfaceId;
use dockspace::backend::ingress::{BackendIngressLease, BackendIngressOrdinal};
use dockspace::backend::intent::{Authority, AuthorityUnavailableReason};
use dockspace::backend::presentation_observation::{
    HostFrameKey as CorePresentationEmissionKey, HostPresentationCaptureGeneration,
    HostPresentationObservation, HostPresentationObservationEntry,
    HostPresentationObservationOutcome, HostPresentationOutput, HostPresentationProgress,
    HostPresentationStreamId, HostPresentationStreamObservation,
};
use dockspace::backend::retention::PresentationRetentionManifest;
use dockspace::backend::transition::EngineTransition;
use egui::{Context, ViewportId};

use crate::error::DockspaceErrorSource;
use crate::presentation_settlement::{
    EguiRendererCompletion, OuterPresentationCompletion, PendingEguiPresentation,
};
#[cfg(test)]
use crate::test_support::{
    CompletedTestPresentationBoundary, TestPresentationBoundary, TestPresentationProviderSnapshot,
    current_presentation_provider,
};

use super::EguiFrameScheduleKey;

/// Scheduling facts observed at one successful automatic egui callback.
///
/// These values identify callback ownership only. They are never interpreted
/// as proof that the callback reached a renderer presentation boundary.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct AutomaticPresentationProgress {
    pub(super) host_frame: EguiFrameScheduleKey,
    pub(super) cumulative_frame: u64,
    #[cfg(test)]
    pub(super) test_presentation_boundary: Option<TestPresentationBoundary>,
}

#[derive(Clone, Copy, Debug)]
struct AutomaticPresentationEmission {
    progress: AutomaticPresentationProgress,
}

/// Retained output facts for one exact core-minted stream incarnation.
struct AutomaticPresentationStream {
    context: Context,
    viewport: ViewportId,
    last_unknown_output: Option<CorePresentationEmissionKey>,
    emissions: BTreeMap<CorePresentationEmissionKey, AutomaticPresentationEmission>,
}

struct OuterPresentationStream {
    surface: SurfaceId,
    outputs: BTreeMap<CorePresentationEmissionKey, Arc<OuterPresentationCompletion>>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct OuterPresentationCapture {
    stream: HostPresentationStreamId,
    generation: HostPresentationCaptureGeneration,
    settled_through: CorePresentationEmissionKey,
}

#[derive(Default)]
pub(super) struct OuterPresentationFrame {
    captures: Vec<OuterPresentationCapture>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum AutomaticPresentationCaptureKind {
    Unknown,
    #[cfg(test)]
    Retired,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct AutomaticPresentationCapture {
    stream: HostPresentationStreamId,
    generation: HostPresentationCaptureGeneration,
    reported_through: CorePresentationEmissionKey,
    kind: AutomaticPresentationCaptureKind,
}

/// Automatic egui facts staged with one host frame.
pub(super) struct AutomaticPresentationFrame {
    pub(super) progress: AutomaticPresentationProgress,
    pub(super) context: Context,
    pub(super) viewport: ViewportId,
    captures: Vec<AutomaticPresentationCapture>,
    #[cfg(test)]
    pub(super) test_presentation_provider: Option<TestPresentationProviderSnapshot>,
}

#[derive(Clone)]
struct AutomaticEguiCallback {
    progress: AutomaticPresentationProgress,
    context: Context,
    viewport: ViewportId,
}

/// Exact observation outcomes accepted by the core for one committed frame.
pub(super) struct AcceptedPresentationDelta {
    unknown: BTreeSet<(HostPresentationStreamId, HostPresentationCaptureGeneration)>,
    retired: BTreeSet<(
        HostPresentationStreamId,
        HostPresentationCaptureGeneration,
        CorePresentationEmissionKey,
    )>,
    processed_ordered: BTreeMap<HostPresentationStreamId, usize>,
}

impl AcceptedPresentationDelta {
    pub(super) fn from_transition(transition: &EngineTransition) -> Self {
        let mut unknown = BTreeSet::new();
        let mut retired = BTreeSet::new();
        let mut processed_ordered = BTreeMap::new();
        for outcome in transition.presentation_observations() {
            match outcome {
                HostPresentationObservationOutcome::CapturedUnknown {
                    stream, generation, ..
                } => {
                    unknown.insert((*stream, *generation));
                    *processed_ordered.entry(*stream).or_default() += 1;
                }
                HostPresentationObservationOutcome::Retired {
                    stream,
                    generation,
                    settled_through,
                    ..
                } => {
                    retired.insert((*stream, *generation, *settled_through));
                    *processed_ordered.entry(*stream).or_default() += 1;
                }
                HostPresentationObservationOutcome::Rejected { stream, .. } => {
                    *processed_ordered.entry(*stream).or_default() += 1;
                }
                HostPresentationObservationOutcome::NoUpdate { .. } => {}
            }
        }
        Self {
            unknown,
            retired,
            processed_ordered,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct OrderedOuterCapture {
    stream: HostPresentationStreamId,
    generation: HostPresentationCaptureGeneration,
    settled_through: CorePresentationEmissionKey,
    presented: Option<CorePresentationEmissionKey>,
    recorded: Option<(BackendIngressLease, BackendIngressOrdinal)>,
}

impl OrderedOuterCapture {
    pub(super) const fn entry(self) -> HostPresentationObservationEntry {
        HostPresentationObservationEntry::new(
            self.stream,
            HostPresentationStreamObservation::Captured {
                generation: self.generation,
                progress: HostPresentationProgress::Retired {
                    settled_through: self.settled_through,
                    presented: Authority::Known(self.presented),
                },
            },
        )
    }
}

/// Retention diagnostics for the adapter-owned half of presentation settlement.
#[cfg(test)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) struct PresentationLedgerDiagnostics {
    automatic_streams: usize,
    automatic_outputs: usize,
    outer_streams: usize,
    outer_outputs: usize,
    pending_outer_outputs: usize,
    capture_generation_streams: usize,
}

#[cfg(test)]
impl PresentationLedgerDiagnostics {
    pub(super) const fn automatic_streams(self) -> usize {
        self.automatic_streams
    }

    pub(super) const fn automatic_outputs(self) -> usize {
        self.automatic_outputs
    }

    pub(super) const fn outer_streams(self) -> usize {
        self.outer_streams
    }

    pub(super) const fn outer_outputs(self) -> usize {
        self.outer_outputs
    }

    pub(super) const fn pending_outer_outputs(self) -> usize {
        self.pending_outer_outputs
    }

    pub(super) const fn capture_generation_streams(self) -> usize {
        self.capture_generation_streams
    }
}

/// Adapter-side ledger for concrete automatic and outer-renderer outputs.
///
/// Output records are reclaimed only after an exact core-accepted terminal
/// watermark. Empty output stream records are then removed immediately. The
/// capture-generation map is intentionally different: absence from a pending
/// roster is not a stream-retirement fact, and the same stream may emit again.
/// It is reclaimed only when the core retention manifest omits the exact stream
/// incarnation. Time, frame age, and temporary output-map emptiness are never
/// used as substitutes for that authority.
#[derive(Default)]
pub(super) struct PresentationOutputLedger {
    automatic_streams: BTreeMap<HostPresentationStreamId, AutomaticPresentationStream>,
    outer_streams: BTreeMap<HostPresentationStreamId, OuterPresentationStream>,
    capture_generations: BTreeMap<HostPresentationStreamId, HostPresentationCaptureGeneration>,
    ordered_outer_captures: BTreeMap<HostPresentationStreamId, Vec<OrderedOuterCapture>>,
    automatic_callback: Option<AutomaticEguiCallback>,
    next_automatic_host_sequence: u64,
}

impl PresentationOutputLedger {
    pub(super) fn retain(&mut self, manifest: &PresentationRetentionManifest) {
        self.automatic_streams
            .retain(|stream, _| manifest.retains_stream(*stream));
        self.outer_streams
            .retain(|stream, _| manifest.retains_stream(*stream));
        self.capture_generations
            .retain(|stream, _| manifest.retains_stream(*stream));
        self.ordered_outer_captures
            .retain(|stream, _| manifest.retains_stream(*stream));
        self.debug_assert_invariants();
    }

    pub(super) fn prepare_automatic_frame(
        &self,
        last_host_frame: Option<EguiFrameScheduleKey>,
        context: &Context,
        viewport: ViewportId,
        cumulative_frame: u64,
        pass: u32,
    ) -> Result<AutomaticPresentationFrame, DockspaceErrorSource> {
        #[cfg(test)]
        let test_presentation_provider = current_presentation_provider(context);
        let same_egui_frame = self.automatic_callback.as_ref().is_some_and(|known| {
            known.context.eq(context)
                && known.viewport == viewport
                && known.progress.cumulative_frame == cumulative_frame
        });
        let sequence = if same_egui_frame {
            self.automatic_callback
                .as_ref()
                .expect("same automatic frame has a callback record")
                .progress
                .host_frame
                .sequence()
        } else {
            let baseline = last_host_frame.map_or(self.next_automatic_host_sequence, |frame| {
                self.next_automatic_host_sequence.max(frame.sequence())
            });
            baseline
                .checked_add(1)
                .ok_or(crate::error::DockspaceErrorSource::AutomaticHostFrameSequenceExhausted)?
        };
        Ok(AutomaticPresentationFrame {
            progress: AutomaticPresentationProgress {
                host_frame: EguiFrameScheduleKey::new(sequence, pass),
                cumulative_frame,
                #[cfg(test)]
                test_presentation_boundary: test_presentation_provider
                    .map(|provider| provider.active),
            },
            context: context.clone(),
            viewport,
            captures: Vec::new(),
            #[cfg(test)]
            test_presentation_provider,
        })
    }

    pub(super) fn automatic_observation(
        &self,
        prelude: &CoreHostFramePrelude,
        automatic: &mut AutomaticPresentationFrame,
        outer: &mut OuterPresentationFrame,
    ) -> Result<HostPresentationObservation, DockspaceErrorSource> {
        let streams = prelude.pending_presentation_streams().collect::<Vec<_>>();
        if streams.is_empty() {
            return Ok(HostPresentationObservation::NoUpdate);
        }

        let mut entries = Vec::with_capacity(streams.len());
        for stream in streams {
            let observation = if self
                .outer_streams
                .get(&stream)
                .is_some_and(|state| !state.outputs.is_empty())
            {
                self.outer_stream_observation(stream, outer)?
            } else {
                self.automatic_stream_observation(stream, automatic)?
            };
            entries.push(HostPresentationObservationEntry::new(stream, observation));
        }
        Ok(HostPresentationObservation::Batch(entries))
    }

    pub(super) fn outer_observation(
        &self,
        prelude: &CoreHostFramePrelude,
        outer: &mut OuterPresentationFrame,
    ) -> Result<HostPresentationObservation, DockspaceErrorSource> {
        let streams = prelude.pending_presentation_streams().collect::<Vec<_>>();
        if streams.is_empty() {
            return Ok(HostPresentationObservation::NoUpdate);
        }

        let mut entries = Vec::with_capacity(streams.len());
        for stream in streams {
            let observation = self.outer_stream_observation(stream, outer)?;
            entries.push(HostPresentationObservationEntry::new(stream, observation));
        }
        Ok(HostPresentationObservation::Batch(entries))
    }

    fn outer_stream_observation(
        &self,
        stream: HostPresentationStreamId,
        frame: &mut OuterPresentationFrame,
    ) -> Result<HostPresentationStreamObservation, DockspaceErrorSource> {
        let Some(state) = self.outer_streams.get(&stream) else {
            return Ok(HostPresentationStreamObservation::NoUpdate);
        };

        let mut settled_through = None;
        let mut presented = None;
        for (key, completion) in &state.outputs {
            let Some(result) = completion.result() else {
                break;
            };
            settled_through = Some(*key);
            if result == EguiRendererCompletion::Accepted {
                presented = Some(*key);
            }
        }
        let Some(settled_through) = settled_through else {
            return Ok(HostPresentationStreamObservation::NoUpdate);
        };
        let generation = self.next_capture_generation(stream).ok_or(
            crate::error::DockspaceErrorSource::OuterPresentationCaptureGenerationExhausted {
                stream,
            },
        )?;
        frame.captures.push(OuterPresentationCapture {
            stream,
            generation,
            settled_through,
        });
        Ok(HostPresentationStreamObservation::Captured {
            generation,
            progress: HostPresentationProgress::Retired {
                settled_through,
                presented: Authority::Known(presented),
            },
        })
    }

    fn automatic_stream_observation(
        &self,
        stream: HostPresentationStreamId,
        frame: &mut AutomaticPresentationFrame,
    ) -> Result<HostPresentationStreamObservation, DockspaceErrorSource> {
        let Some(state) = self.automatic_streams.get(&stream) else {
            return Ok(HostPresentationStreamObservation::NoUpdate);
        };
        #[cfg(test)]
        if let Some(provider) = frame.test_presentation_provider {
            return match provider.completed {
                Some(completed) => {
                    self.deterministic_test_stream_observation(stream, state, completed, frame)
                }
                None => Ok(HostPresentationStreamObservation::NoUpdate),
            };
        }

        let Some((latest_output, emission)) = state.emissions.last_key_value() else {
            return Ok(HostPresentationStreamObservation::NoUpdate);
        };
        let same_egui_stream = state.context.eq(&frame.context) && state.viewport == frame.viewport;
        let same_egui_frame = same_egui_stream
            && frame.progress.cumulative_frame == emission.progress.cumulative_frame;
        if same_egui_frame || state.last_unknown_output == Some(*latest_output) {
            return Ok(HostPresentationStreamObservation::NoUpdate);
        }

        let generation = self.next_capture_generation(stream).ok_or(
            crate::error::DockspaceErrorSource::AutomaticPresentationCaptureGenerationExhausted {
                stream,
            },
        )?;
        frame.captures.push(AutomaticPresentationCapture {
            stream,
            generation,
            reported_through: *latest_output,
            kind: AutomaticPresentationCaptureKind::Unknown,
        });
        Ok(HostPresentationStreamObservation::Captured {
            generation,
            progress: HostPresentationProgress::Unknown(
                AuthorityUnavailableReason::ProviderUnavailable,
            ),
        })
    }

    #[cfg(test)]
    fn deterministic_test_stream_observation(
        &self,
        stream: HostPresentationStreamId,
        state: &AutomaticPresentationStream,
        completed: CompletedTestPresentationBoundary,
        frame: &mut AutomaticPresentationFrame,
    ) -> Result<HostPresentationStreamObservation, DockspaceErrorSource> {
        let Some((reported_through, _)) = state.emissions.iter().rev().find(|(_, emission)| {
            emission
                .progress
                .test_presentation_boundary
                .is_some_and(|emitted| emitted <= completed.boundary)
        }) else {
            return Ok(HostPresentationStreamObservation::NoUpdate);
        };
        let generation = self.next_capture_generation(stream).ok_or(
            crate::error::DockspaceErrorSource::AutomaticPresentationCaptureGenerationExhausted {
                stream,
            },
        )?;
        let reported_through = *reported_through;
        frame.captures.push(AutomaticPresentationCapture {
            stream,
            generation,
            reported_through,
            kind: AutomaticPresentationCaptureKind::Retired,
        });
        let presented = state
            .emissions
            .iter()
            .rev()
            .find(|(_, candidate)| {
                candidate.progress.test_presentation_boundary == Some(completed.boundary)
                    && candidate.progress.host_frame.pass() == completed.final_pass
            })
            .map(|(key, _)| *key);
        Ok(HostPresentationStreamObservation::Captured {
            generation,
            progress: HostPresentationProgress::Retired {
                settled_through: reported_through,
                presented: Authority::Known(presented),
            },
        })
    }

    pub(super) fn commit_outer(
        &mut self,
        frame: OuterPresentationFrame,
        accepted: &AcceptedPresentationDelta,
    ) {
        for capture in frame.captures {
            if !accepted.retired.contains(&(
                capture.stream,
                capture.generation,
                capture.settled_through,
            )) {
                continue;
            }
            self.capture_generations
                .insert(capture.stream, capture.generation);
            if let Some(state) = self.outer_streams.get_mut(&capture.stream) {
                state
                    .outputs
                    .retain(|key, _| *key > capture.settled_through);
            }
        }
        self.outer_streams
            .retain(|_, state| !state.outputs.is_empty());
        self.debug_assert_invariants();
    }

    pub(super) fn commit_ordered_outer(&mut self, accepted: &AcceptedPresentationDelta) {
        for (stream, generation) in &accepted.unknown {
            self.capture_generations.insert(*stream, *generation);
        }
        for (stream, generation, settled_through) in &accepted.retired {
            self.capture_generations.insert(*stream, *generation);
            if let Some(state) = self.outer_streams.get_mut(stream) {
                state.outputs.retain(|key, _| *key > *settled_through);
            }
        }
        self.outer_streams
            .retain(|_, state| !state.outputs.is_empty());
        for (stream, count) in &accepted.processed_ordered {
            let Some(captures) = self.ordered_outer_captures.get_mut(stream) else {
                continue;
            };
            let drain = (*count).min(captures.len());
            captures.drain(..drain);
        }
        self.ordered_outer_captures
            .retain(|_, captures| !captures.is_empty());
        self.debug_assert_invariants();
    }

    pub(super) fn next_ordered_outer_capture(
        &self,
    ) -> Result<Option<OrderedOuterCapture>, DockspaceErrorSource> {
        for (stream, state) in &self.outer_streams {
            let pending = self
                .ordered_outer_captures
                .get(stream)
                .and_then(|captures| captures.last());
            let already_settled = pending.map(|capture| capture.settled_through);
            let mut settled_through = None;
            let mut presented = None;
            for (key, completion) in &state.outputs {
                let Some(result) = completion.result() else {
                    break;
                };
                settled_through = Some(*key);
                if already_settled.is_none_or(|settled| *key > settled)
                    && result == EguiRendererCompletion::Accepted
                {
                    presented = Some(*key);
                }
            }
            let Some(settled_through) = settled_through else {
                continue;
            };
            if already_settled.is_some_and(|settled| settled_through <= settled) {
                continue;
            }
            let generation = pending
                .map(|capture| capture.generation)
                .or_else(|| self.capture_generations.get(stream).copied())
                .unwrap_or_default()
                .checked_next()
                .ok_or(
                    crate::error::DockspaceErrorSource::OuterPresentationCaptureGenerationExhausted { stream: *stream },
                )?;
            return Ok(Some(OrderedOuterCapture {
                stream: *stream,
                generation,
                settled_through,
                presented,
                recorded: None,
            }));
        }
        Ok(None)
    }

    pub(super) fn stage_ordered_outer_capture(
        &mut self,
        mut capture: OrderedOuterCapture,
        lease: BackendIngressLease,
        ordinal: BackendIngressOrdinal,
    ) {
        capture.recorded = Some((lease, ordinal));
        self.ordered_outer_captures
            .entry(capture.stream)
            .or_default()
            .push(capture);
        self.debug_assert_invariants();
    }

    pub(super) fn rollback_ordered_outer_after(
        &mut self,
        lease: BackendIngressLease,
        recorded_through: BackendIngressOrdinal,
    ) {
        for captures in self.ordered_outer_captures.values_mut() {
            captures.retain(|capture| {
                !capture.recorded.is_some_and(|(recorded_lease, ordinal)| {
                    recorded_lease == lease && ordinal > recorded_through
                })
            });
        }
        self.ordered_outer_captures
            .retain(|_, captures| !captures.is_empty());
        self.debug_assert_invariants();
    }

    pub(super) fn register_outer(
        &mut self,
        outputs: Vec<HostPresentationOutput>,
    ) -> Vec<PendingEguiPresentation> {
        let mut pending = Vec::with_capacity(outputs.len());
        for output in outputs {
            let completion = Arc::new(OuterPresentationCompletion::pending());
            let state = self
                .outer_streams
                .entry(output.stream())
                .or_insert_with(|| OuterPresentationStream {
                    surface: output.surface(),
                    outputs: BTreeMap::new(),
                });
            debug_assert_eq!(
                state.surface,
                output.surface(),
                "one core stream incarnation cannot change logical surface",
            );
            let previous = state.outputs.insert(output.key(), Arc::clone(&completion));
            debug_assert!(
                previous.is_none(),
                "core presentation emission keys are unique within a stream",
            );
            pending.push(PendingEguiPresentation::new(output, completion));
        }
        self.debug_assert_invariants();
        pending
    }

    pub(super) fn outer_surface_has_pending(&self, surface: SurfaceId) -> bool {
        self.outer_streams.values().any(|stream| {
            stream.surface == surface
                && stream
                    .outputs
                    .values()
                    .any(|completion| completion.result().is_none())
        })
    }

    pub(super) fn commit_automatic(
        &mut self,
        frame: AutomaticPresentationFrame,
        accepted: &AcceptedPresentationDelta,
        emissions: impl IntoIterator<Item = HostPresentationOutput>,
    ) {
        for capture in &frame.captures {
            let capture_accepted = match capture.kind {
                AutomaticPresentationCaptureKind::Unknown => accepted
                    .unknown
                    .contains(&(capture.stream, capture.generation)),
                #[cfg(test)]
                AutomaticPresentationCaptureKind::Retired => accepted.retired.contains(&(
                    capture.stream,
                    capture.generation,
                    capture.reported_through,
                )),
            };
            if !capture_accepted {
                continue;
            }
            self.capture_generations
                .insert(capture.stream, capture.generation);
            if let Some(state) = self.automatic_streams.get_mut(&capture.stream) {
                match capture.kind {
                    AutomaticPresentationCaptureKind::Unknown => {
                        state.last_unknown_output = Some(capture.reported_through);
                    }
                    #[cfg(test)]
                    AutomaticPresentationCaptureKind::Retired => {
                        state
                            .emissions
                            .retain(|key, _| *key > capture.reported_through);
                        if state
                            .last_unknown_output
                            .is_some_and(|key| key <= capture.reported_through)
                        {
                            state.last_unknown_output = None;
                        }
                    }
                }
            }
        }

        for output in emissions {
            let stream = output.stream();
            let state = self.automatic_streams.entry(stream).or_insert_with(|| {
                AutomaticPresentationStream {
                    context: frame.context.clone(),
                    viewport: frame.viewport,
                    last_unknown_output: None,
                    emissions: BTreeMap::new(),
                }
            });
            state.context = frame.context.clone();
            state.viewport = frame.viewport;
            let previous = state.emissions.insert(
                output.key(),
                AutomaticPresentationEmission {
                    progress: frame.progress,
                },
            );
            debug_assert!(
                previous.is_none(),
                "core presentation emission keys are unique within a stream",
            );
        }

        self.automatic_streams
            .retain(|_, state| !state.emissions.is_empty());
        self.next_automatic_host_sequence = self
            .next_automatic_host_sequence
            .max(frame.progress.host_frame.sequence());
        self.automatic_callback = Some(AutomaticEguiCallback {
            progress: frame.progress,
            context: frame.context,
            viewport: frame.viewport,
        });
        self.debug_assert_invariants();
    }

    #[cfg(test)]
    pub(super) fn diagnostics(&self) -> PresentationLedgerDiagnostics {
        PresentationLedgerDiagnostics {
            automatic_streams: self.automatic_streams.len(),
            automatic_outputs: self
                .automatic_streams
                .values()
                .map(|stream| stream.emissions.len())
                .sum(),
            outer_streams: self.outer_streams.len(),
            outer_outputs: self
                .outer_streams
                .values()
                .map(|stream| stream.outputs.len())
                .sum(),
            pending_outer_outputs: self
                .outer_streams
                .values()
                .flat_map(|stream| stream.outputs.values())
                .filter(|completion| completion.result().is_none())
                .count(),
            capture_generation_streams: self.capture_generations.len(),
        }
    }

    #[cfg(test)]
    pub(super) fn first_automatic_output(
        &self,
    ) -> Option<(HostPresentationStreamId, CorePresentationEmissionKey)> {
        self.automatic_streams.iter().find_map(|(stream, state)| {
            state
                .emissions
                .first_key_value()
                .map(|(output, _)| (*stream, *output))
        })
    }

    fn next_capture_generation(
        &self,
        stream: HostPresentationStreamId,
    ) -> Option<HostPresentationCaptureGeneration> {
        self.capture_generations
            .get(&stream)
            .copied()
            .unwrap_or_default()
            .checked_next()
    }

    fn debug_assert_invariants(&self) {
        debug_assert!(self.automatic_streams.iter().all(|(stream, state)| {
            !state.emissions.is_empty()
                && state
                    .emissions
                    .keys()
                    .all(|output| output.stream() == *stream)
                && state
                    .last_unknown_output
                    .is_none_or(|output| state.emissions.contains_key(&output))
        }));
        debug_assert!(self.outer_streams.iter().all(|(stream, state)| {
            !state.outputs.is_empty()
                && state
                    .outputs
                    .keys()
                    .all(|output| output.stream() == *stream)
        }));
        debug_assert!(
            self.ordered_outer_captures
                .iter()
                .all(|(stream, captures)| {
                    !captures.is_empty()
                        && captures.iter().all(|capture| capture.stream == *stream)
                        && captures.windows(2).all(|pair| {
                            pair[0].generation < pair[1].generation
                                && pair[0].settled_through < pair[1].settled_through
                        })
                })
        );
    }
}

pub(super) fn should_request_presentation_follow_up_pass(
    outcomes: &[HostPresentationObservationOutcome],
) -> bool {
    outcomes.iter().any(|outcome| {
        matches!(
            outcome,
            HostPresentationObservationOutcome::Retired {
                promotion_eligible: true,
                ..
            }
        )
    })
}

#[cfg(test)]
mod tests {
    use super::PresentationOutputLedger;

    #[test]
    fn empty_ledger_has_no_retained_output_or_generation_state() {
        let diagnostics = PresentationOutputLedger::default().diagnostics();

        assert_eq!(diagnostics.automatic_streams(), 0);
        assert_eq!(diagnostics.automatic_outputs(), 0);
        assert_eq!(diagnostics.outer_streams(), 0);
        assert_eq!(diagnostics.outer_outputs(), 0);
        assert_eq!(diagnostics.pending_outer_outputs(), 0);
        assert_eq!(diagnostics.capture_generation_streams(), 0);
    }
}
