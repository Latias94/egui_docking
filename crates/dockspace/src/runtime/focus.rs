//! Opaque presentation-neutral pane-focus requests and observations.

use std::collections::BTreeMap;
use std::fmt;

use thiserror::Error;

use crate::engine::EngineInput;
use crate::ids::{EngineAuthorityDomainId, ItemId, SurfaceId, WorkspaceEpoch};
use crate::viewport_focus::{
    PaneFocusIntent, PaneFocusObservationGeneration, PaneFocusRequestObservation,
    PaneFocusRequestObservationState,
};

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

/// Opaque revision-bound request to focus one exact pane.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct DockspacePaneFocusRequest {
    authority_domain: EngineAuthorityDomainId,
    expected_epoch: WorkspaceEpoch,
    intent: PaneFocusIntent,
    item: ItemId,
}

impl DockspacePaneFocusRequest {
    pub(super) fn from_intent(
        authority_domain: EngineAuthorityDomainId,
        expected_epoch: WorkspaceEpoch,
        intent: PaneFocusIntent,
    ) -> Option<Self> {
        Some(Self {
            authority_domain,
            expected_epoch,
            intent,
            item: intent.item()?,
        })
    }

    /// Returns the logical surface whose pane must receive focus.
    #[must_use]
    pub const fn surface(self) -> SurfaceId {
        self.intent.surface()
    }

    /// Returns the exact stable item whose pane must receive focus.
    #[must_use]
    pub const fn item(self) -> ItemId {
        self.item
    }
}

impl fmt::Debug for DockspacePaneFocusRequest {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("DockspacePaneFocusRequest")
            .field("surface", &self.surface())
            .field("item", &self.item())
            .finish_non_exhaustive()
    }
}

/// Affine observation prepared from one exact [`DockspacePaneFocusRequest`].
#[must_use = "a prepared pane-focus observation must be submitted or deliberately discarded"]
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
    generations: BTreeMap<SurfaceId, PaneFocusObservationGeneration>,
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
        let current = self
            .generations
            .get(&request.surface())
            .copied()
            .unwrap_or_default()
            .max(baseline);
        let next = current.checked_next()?;
        self.generations.insert(request.surface(), next);
        Some(next)
    }
}
