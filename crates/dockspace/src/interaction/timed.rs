//! Finite stationary work retained by the existing gesture owner.

use std::time::Duration;

use super::*;

/// Opaque permission to recheck one pending timed gesture action.
///
/// Hosts cannot mint tokens. Copying one permits frame retry, not repeated
/// execution: only its current gesture owner can consume the matching generation.
/// A token carries no receiver or presentation authority.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PointerContinuationToken {
    // The live gesture retains the lane. Session and token generation bind
    // this stream reference without enlarging every high-frequency edge.
    pub(crate) owner: PointerStreamId,
    pub(crate) session: PointerContinuationSession,
    pub(crate) generation: u64,
    pub(crate) deadline: Duration,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PointerContinuationSession {
    Click(ClickSessionId),
    Drag(DragSessionId),
}

/// Touch waiting shares the existing pressed-click owner and frozen output.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct TouchPressState {
    pub(crate) initial_pointer: LogicalPoint,
    pub(crate) current_pointer: LogicalPoint,
    pub(crate) threshold_origin: JournalDragThresholdOrigin,
    pub(crate) request: Option<PointerContinuationRequest>,
    pub(crate) armed: bool,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct PointerContinuationRequest {
    pub(crate) token: PointerContinuationToken,
    pub(crate) surface: SurfaceId,
    pub(crate) point: LogicalPoint,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct HoverSelectionState {
    pub(crate) request: PointerContinuationRequest,
    pub(crate) target: TabSceneId,
    pub(crate) authority: PresentedSurfaceAuthority,
    pub(crate) workspace: WorkspaceVersion,
    pub(crate) started_at: Duration,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TabStripEdgeDirection {
    Backward,
    Forward,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct TabStripEdgeScrollState {
    pub(crate) request: PointerContinuationRequest,
    pub(crate) authority: PresentedSurfaceAuthority,
    pub(crate) output: crate::presentation_observation::SurfacePresentationOutputTicket,
    pub(crate) workspace: WorkspaceVersion,
    pub(crate) key: crate::tab_strip::TabStripStateKey,
    pub(crate) direction: TabStripEdgeDirection,
    pub(crate) viewport: LogicalRect,
    pub(crate) offset: f64,
    pub(crate) maximum: f64,
    pub(crate) started_at: Duration,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum DragTimedContinuation {
    HoverSelection(HoverSelectionState),
    TabStripEdgeScroll(TabStripEdgeScrollState),
}

impl DragTimedContinuation {
    pub(crate) const fn request(&self) -> PointerContinuationRequest {
        match self {
            Self::HoverSelection(state) => state.request,
            Self::TabStripEdgeScroll(state) => state.request,
        }
    }
}

impl InteractionState {
    pub(crate) fn mint_continuation(
        &mut self,
        owner: GestureOwner,
        session: PointerContinuationSession,
        deadline: Duration,
    ) -> Result<PointerContinuationToken, InteractionCounterError> {
        self.last_continuation_generation = self
            .last_continuation_generation
            .checked_add(1)
            .ok_or(InteractionCounterError::ContinuationGenerationExhausted)?;
        Ok(PointerContinuationToken {
            owner: owner.stream(),
            session,
            generation: self.last_continuation_generation,
            deadline,
        })
    }
}
