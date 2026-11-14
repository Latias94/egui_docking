//! Opaque presentation-neutral pane-focus requests and observations.

use std::fmt;

use thiserror::Error;

use crate::engine::EngineInput;
use crate::ids::{EngineAuthorityDomainId, ItemId, SurfaceId, WorkspaceEpoch};
use crate::platform::focus::{
    PaneFocusIntent, PaneFocusObservationGeneration, PaneFocusRequestObservation,
    PaneFocusRequestObservationState,
};

pub use crate::platform::focus::HostFocusState as DockspaceHostFocus;

use super::{DockspaceHostFrame, DockspaceRuntimeError, DockspaceSession};

impl DockspaceSession {
    /// Returns an admitted pane-focus request without reserving renderer output.
    ///
    /// Only an intent admitted at a successful commit is observable. The request
    /// remains subject to exact authority checks when its observation is submitted.
    #[must_use]
    pub fn pane_focus_request(&self, surface: SurfaceId) -> Option<DockspacePaneFocusRequest> {
        self.engine
            .viewport_focus()
            .published_pane_intent()
            .filter(|intent| intent.surface() == surface)
            .map(|intent| {
                DockspacePaneFocusRequest::from_intent(
                    self.engine.authority_domain(),
                    self.version().epoch(),
                    intent,
                )
            })
    }
}

impl DockspaceHostFrame<'_> {
    /// Returns the admitted focus request still current after this frame's input.
    ///
    /// This query neither seals input nor reserves encoding capacity. A new
    /// candidate intent is unavailable until its publication boundary commits.
    #[must_use]
    pub fn pane_focus_request(&self, surface: SurfaceId) -> Option<DockspacePaneFocusRequest> {
        let view = self.frame.view();
        let committed = self.session.pane_focus_request(surface)?;
        (view.version().epoch() == committed.expected_epoch
            && view.published_pane_focus_intent() == Some(committed.intent))
        .then_some(committed)
    }

    /// Reports actual framework focus within this host's current surface scope.
    ///
    /// Unlike an exact request acknowledgement, this also represents unrelated
    /// application UI, a positively observed absence of focus, and unavailable
    /// focus information. These facts preserve per-surface restoration history.
    /// They never grant native-window activation or replace platform focus proof.
    /// Returns `false` when this host has no current presentation stream for the
    /// surface. The observation and its semantic validation publish atomically
    /// with the frame.
    ///
    /// # Errors
    ///
    /// Returns an error if semantic input is closed or observation identities
    /// can no longer advance without wrapping.
    pub fn report_host_focus(
        &mut self,
        surface: SurfaceId,
        state: DockspaceHostFocus,
    ) -> Result<bool, DockspaceRuntimeError> {
        self.ensure_semantic_input_allowed()?;
        let baseline = self
            .frame
            .view()
            .viewport_focus()
            .pane_observation_generation(surface)
            .unwrap_or_default();
        let Some(probe) = self
            .frame
            .capture_host_focus_observation(surface, baseline, state)
        else {
            return Ok(false);
        };
        if self
            .frame
            .view()
            .viewport_focus()
            .matches_current_host_focus(probe)
        {
            return Ok(true);
        }
        let generation = self
            .session
            .focus
            .next_after(baseline)
            .ok_or_else(DockspaceRuntimeError::pane_focus_observation_generation_exhausted)?;
        let observation = crate::platform::focus::HostFocusObservation::new(
            generation,
            probe.host(),
            probe.stream(),
            surface,
            state,
        );
        self.append(EngineInput::PublishHostFocusObservation {
            expected_epoch: self.frame.view().version().epoch(),
            observation,
        })?;
        Ok(true)
    }
}

/// Exact adapter observation of one published pane-focus request.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DockspacePaneFocusObservation {
    /// The requested pane target owns framework focus.
    Focused,
    /// The target is available but does not yet own framework focus.
    NotFocused,
    /// The adapter cannot bind this request to a framework focus target.
    Unavailable,
}

impl DockspacePaneFocusObservation {
    const fn into_core(self) -> PaneFocusRequestObservationState {
        match self {
            Self::Focused => PaneFocusRequestObservationState::Focused,
            Self::NotFocused => PaneFocusRequestObservationState::NotFocused,
            Self::Unavailable => PaneFocusRequestObservationState::Unavailable,
        }
    }
}

/// Semantic focus destination within one logical surface.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DockspaceFocusTarget {
    /// The application-owned content target of one open pane.
    Pane(ItemId),
    /// The surface workspace after its prior content target disappeared.
    Workspace,
}

/// Opaque revision-bound request to focus one pane or its surface workspace.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct DockspacePaneFocusRequest {
    authority_domain: EngineAuthorityDomainId,
    expected_epoch: WorkspaceEpoch,
    intent: PaneFocusIntent,
    target: DockspaceFocusTarget,
}

impl DockspacePaneFocusRequest {
    pub(super) fn from_intent(
        authority_domain: EngineAuthorityDomainId,
        expected_epoch: WorkspaceEpoch,
        intent: PaneFocusIntent,
    ) -> Self {
        Self {
            authority_domain,
            expected_epoch,
            intent,
            target: intent
                .item()
                .map_or(DockspaceFocusTarget::Workspace, DockspaceFocusTarget::Pane),
        }
    }

    /// Returns the logical surface whose content or workspace must receive focus.
    #[must_use]
    pub const fn surface(self) -> SurfaceId {
        self.intent.surface()
    }

    /// Returns the exact pane item, or `None` for the surface workspace.
    #[must_use]
    pub const fn item(self) -> Option<ItemId> {
        match self.target {
            DockspaceFocusTarget::Pane(item) => Some(item),
            DockspaceFocusTarget::Workspace => None,
        }
    }

    /// Returns the exact semantic focus destination, without inferring a pane.
    #[must_use]
    pub const fn target(self) -> DockspaceFocusTarget {
        self.target
    }

    /// Prepares an actual host observation without requiring a paint plan.
    ///
    /// Preparing does not acknowledge or refresh this request. Submission checks
    /// its session, epoch, intent, target and any native activation guard again.
    #[must_use]
    pub const fn prepare_observation(
        self,
        observation: DockspacePaneFocusObservation,
    ) -> PreparedPaneFocusObservation {
        PreparedPaneFocusObservation::new(self, observation)
    }
}

impl fmt::Debug for DockspacePaneFocusRequest {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("DockspacePaneFocusRequest")
            .field("surface", &self.surface())
            .field("target", &self.target())
            .finish_non_exhaustive()
    }
}

/// Owned observation prepared from one exact [`DockspacePaneFocusRequest`].
///
/// Retain the original while a cloned attempt enters an unpublished update.
/// Acknowledge it only after publication; a clone does not establish a new
/// focus request or permit a stale intent to override its successor.
#[must_use = "a prepared pane-focus observation must be submitted or deliberately discarded"]
#[derive(Clone)]
pub struct PreparedPaneFocusObservation {
    request: DockspacePaneFocusRequest,
    observation: DockspacePaneFocusObservation,
}

impl PreparedPaneFocusObservation {
    pub(super) const fn new(
        request: DockspacePaneFocusRequest,
        observation: DockspacePaneFocusObservation,
    ) -> Self {
        Self {
            request,
            observation,
        }
    }

    pub(super) fn into_engine_input(
        self,
        expected_authority: EngineAuthorityDomainId,
        generation: PaneFocusObservationGeneration,
    ) -> Result<EngineInput, PreparedPaneFocusObservationAuthorityMismatch> {
        if self.request.authority_domain != expected_authority {
            return Err(PreparedPaneFocusObservationAuthorityMismatch);
        }
        Ok(EngineInput::PublishPaneFocusRequestObservation {
            expected_epoch: self.request.expected_epoch,
            observation: PaneFocusRequestObservation::new(
                generation,
                self.request.intent,
                self.observation.into_core(),
            ),
        })
    }

    pub(super) fn validate_authority(
        &self,
        expected_authority: EngineAuthorityDomainId,
    ) -> Result<(), PreparedPaneFocusObservationAuthorityMismatch> {
        if self.request.authority_domain != expected_authority {
            return Err(PreparedPaneFocusObservationAuthorityMismatch);
        }
        Ok(())
    }

    pub(super) const fn request(&self) -> DockspacePaneFocusRequest {
        self.request
    }
}

impl fmt::Debug for PreparedPaneFocusObservation {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("PreparedPaneFocusObservation")
            .field("request", &self.request)
            .field("observation", &self.observation)
            .finish_non_exhaustive()
    }
}

#[derive(Debug, Error)]
#[error("prepared pane-focus observation belongs to another dockspace session")]
pub(super) struct PreparedPaneFocusObservationAuthorityMismatch;

#[derive(Debug, Default)]
pub(super) struct RuntimePaneFocusState {
    generation: PaneFocusObservationGeneration,
}

impl RuntimePaneFocusState {
    pub(super) fn next_generation(
        &mut self,
        request: DockspacePaneFocusRequest,
    ) -> Option<PaneFocusObservationGeneration> {
        let baseline = request
            .intent
            .pane_observation_baseline()
            .unwrap_or_default();
        self.next_after(baseline)
    }

    fn next_after(
        &mut self,
        baseline: PaneFocusObservationGeneration,
    ) -> Option<PaneFocusObservationGeneration> {
        // Surface observations require increasing, not consecutive generations.
        // One provider watermark also prevents reuse after an abandoned frame
        // without retaining identities of surfaces that have been removed.
        let next = self.generation.max(baseline).checked_next()?;
        self.generation = next;
        Some(next)
    }
}
