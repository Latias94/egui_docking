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
    pub(crate) owner: GestureOwner,
    pub(crate) session: DragSessionId,
    pub(crate) generation: u64,
    pub(crate) deadline: Duration,
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

impl InteractionState {
    pub(crate) fn mint_continuation(
        &mut self,
        owner: GestureOwner,
        session: DragSessionId,
        deadline: Duration,
    ) -> Result<PointerContinuationToken, InteractionCounterError> {
        self.last_continuation_generation = self
            .last_continuation_generation
            .checked_add(1)
            .ok_or(InteractionCounterError::ContinuationGenerationExhausted)?;
        Ok(PointerContinuationToken {
            owner,
            session,
            generation: self.last_continuation_generation,
            deadline,
        })
    }
}
