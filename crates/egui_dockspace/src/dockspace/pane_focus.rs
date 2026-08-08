//! Adapter-owned pane-focus observations and request fences.

use std::collections::{BTreeMap, VecDeque};

use dockspace::backend::frame::PanelFocus;
use dockspace::backend::ingress::{
    BackendIngressCommitWatermark, BackendIngressLease, BackendIngressOrdinal,
};
use dockspace::backend::transition::EngineTransition;
use dockspace::backend::viewport_focus::{
    PaneFocusIntentId, PaneFocusObservation, PaneFocusObservationGeneration,
};
use dockspace::ids::SurfaceId;
use dockspace::viewport::ViewportBinding;
use egui::Context;

use super::frame_contract::EguiFrameScheduleKey;
use crate::pane::{PaneFocusState, PaneView};
use crate::projection::EguiSurfacePaintResources;
use crate::response::DockspaceUnavailableReason;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct EnqueuedPaneFocus {
    pub(super) binding: ViewportBinding,
    pub(super) focus: PanelFocus,
    pub(super) acknowledges: Option<PaneFocusIntentId>,
}

pub(super) fn observe_surface_pane_focus(
    resources: &EguiSurfacePaintResources,
    panes: &dyn PaneView,
    context: &Context,
) -> Result<PanelFocus, DockspaceUnavailableReason> {
    let mut focused = None;
    for item in resources.items() {
        if panes.focus_target(item).is_none() {
            return Err(DockspaceUnavailableReason::PaneFocusTargetMissing { item });
        }
        match panes.focus_state(item, context) {
            PaneFocusState::Unknown => {
                return Err(DockspaceUnavailableReason::PaneFocusStateUnknown { item });
            }
            PaneFocusState::Focused => {
                if focused.replace(item).is_some() {
                    return Err(DockspaceUnavailableReason::ConflictingPaneFocus);
                }
            }
            PaneFocusState::Unfocused => {}
        }
    }
    Ok(focused.map_or(PanelFocus::None, PanelFocus::Item))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct PaneFocusRequestFence {
    pub(super) intent: PaneFocusIntentId,
    pub(super) frame: EguiFrameScheduleKey,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct RecordedPaneFocusObservation {
    lease: BackendIngressLease,
    ordinal: BackendIngressOrdinal,
    observation: PaneFocusObservation,
}

#[derive(Clone, Default)]
pub(crate) struct PaneFocusAdapterState {
    observation_generations: BTreeMap<SurfaceId, PaneFocusObservationGeneration>,
    pub(super) last_enqueued: BTreeMap<SurfaceId, EnqueuedPaneFocus>,
    pub(super) request_fence: Option<PaneFocusRequestFence>,
    pending_backend_observations: VecDeque<PaneFocusObservation>,
    recorded_backend_observations: VecDeque<RecordedPaneFocusObservation>,
}

impl PaneFocusAdapterState {
    pub(super) fn accept_transition(&mut self, transition: &EngineTransition) {
        for change in transition.focus_delta().surface_focus() {
            let surface = change.surface();
            let observation = change
                .state()
                .after()
                .as_ref()
                .and_then(|state| state.observation());
            if let Some(observation) = observation {
                self.observation_generations
                    .entry(surface)
                    .and_modify(|current| *current = (*current).max(observation.generation()))
                    .or_insert(observation.generation());
                self.last_enqueued.insert(
                    surface,
                    EnqueuedPaneFocus {
                        binding: observation.binding(),
                        focus: observation.focus(),
                        acknowledges: observation.acknowledges(),
                    },
                );
            } else if change.state().after().is_none() {
                self.last_enqueued.remove(&surface);
            }
        }

        let Some(change) = transition.focus_delta().pane_intent() else {
            return;
        };
        let current = change.after().as_ref().map(|intent| intent.id());
        if self
            .request_fence
            .is_some_and(|fence| Some(fence.intent) != current)
        {
            self.request_fence = None;
        }
    }

    pub(super) fn next_observation_generation(
        &self,
        surface: SurfaceId,
        baseline: Option<PaneFocusObservationGeneration>,
    ) -> Option<PaneFocusObservationGeneration> {
        self.observation_generations
            .get(&surface)
            .copied()
            .into_iter()
            .chain(baseline)
            .max()
            .unwrap_or_default()
            .checked_next()
    }

    pub(super) fn record_enqueued(&mut self, observation: PaneFocusObservation) {
        self.observation_generations
            .insert(observation.binding().surface(), observation.generation());
        self.last_enqueued.insert(
            observation.binding().surface(),
            EnqueuedPaneFocus {
                binding: observation.binding(),
                focus: observation.focus(),
                acknowledges: observation.acknowledges(),
            },
        );
    }

    pub(super) fn observation_was_enqueued(
        &self,
        binding: ViewportBinding,
        focus: PanelFocus,
        acknowledges: Option<PaneFocusIntentId>,
    ) -> bool {
        self.last_enqueued
            .get(&binding.surface())
            .is_some_and(|last| {
                last.binding == binding
                    && last.focus == focus
                    && acknowledges.is_none_or(|intent| last.acknowledges == Some(intent))
            })
    }

    pub(super) fn queue_backend_observation(&mut self, observation: PaneFocusObservation) {
        if self
            .pending_backend_observations
            .back()
            .is_some_and(|pending| *pending == observation)
            || self.observation_was_enqueued(
                observation.binding(),
                observation.focus(),
                observation.acknowledges(),
            )
        {
            return;
        }
        self.pending_backend_observations.push_back(observation);
    }

    pub(super) fn next_backend_observation(&self) -> Option<PaneFocusObservation> {
        self.pending_backend_observations.front().copied()
    }

    pub(super) fn stage_backend_observation(
        &mut self,
        lease: BackendIngressLease,
        ordinal: BackendIngressOrdinal,
        observation: PaneFocusObservation,
    ) {
        let staged = self
            .pending_backend_observations
            .pop_front()
            .expect("a recorded backend observation remains queued");
        debug_assert_eq!(staged, observation);
        self.recorded_backend_observations
            .push_back(RecordedPaneFocusObservation {
                lease,
                ordinal,
                observation,
            });
    }

    pub(super) fn accept_backend_commit(&mut self, watermark: BackendIngressCommitWatermark) {
        while self
            .recorded_backend_observations
            .front()
            .is_some_and(|recorded| {
                recorded.lease == watermark.lease() && recorded.ordinal <= watermark.through()
            })
        {
            let recorded = self
                .recorded_backend_observations
                .pop_front()
                .expect("the accepted observation was inspected above");
            self.record_enqueued(recorded.observation);
        }
    }

    pub(super) fn rollback_backend_observations_after(
        &mut self,
        lease: BackendIngressLease,
        recorded_through: BackendIngressOrdinal,
    ) {
        let mut replay = VecDeque::new();
        self.recorded_backend_observations.retain(|recorded| {
            let rolled_back = recorded.lease == lease && recorded.ordinal > recorded_through;
            if rolled_back {
                replay.push_back(recorded.observation);
            }
            !rolled_back
        });
        replay.append(&mut self.pending_backend_observations);
        self.pending_backend_observations = replay;
    }

    pub(super) fn reconcile_backend_provider(&mut self, active: BackendIngressLease) {
        if self
            .recorded_backend_observations
            .front()
            .is_none_or(|recorded| recorded.lease == active)
        {
            return;
        }
        let mut replay = self
            .recorded_backend_observations
            .drain(..)
            .map(|recorded| recorded.observation)
            .collect::<VecDeque<_>>();
        replay.append(&mut self.pending_backend_observations);
        self.pending_backend_observations = replay;
    }
}
