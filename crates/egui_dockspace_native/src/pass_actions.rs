//! Minimal egui multipass retention for affine product actions and outputs.

use std::collections::BTreeMap;

use dockspace::model::SurfaceId;
use dockspace::runtime::{
    PaintedSurfaceOutput, PreparedPaneFocusObservation, PreparedSurfaceAction,
};
use eframe::NativeOutputToken;

use crate::host_frame::NativeSurfacePaint;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum NativePassActionError {
    OutputChanged,
    LocalActionConflict,
}

#[derive(Debug)]
struct RetainedPassActions {
    surface: SurfaceId,
    local: Vec<PreparedSurfaceAction>,
}

pub(crate) struct NativePassDraft {
    surface: SurfaceId,
    paint: NativeSurfacePaint,
    presentation: Vec<PreparedSurfaceAction>,
    pane_focus_observation: Option<PreparedPaneFocusObservation>,
    local: Vec<PreparedSurfaceAction>,
    application_action_ready: bool,
}

impl NativePassDraft {
    pub(crate) fn had_ready_plan(&self) -> bool {
        self.paint.had_ready_plan()
    }

    pub(crate) fn transient_visuals_complete(&self) -> bool {
        self.paint.transient_visuals_complete()
    }

    pub(crate) fn deferred_measurement(&self) -> bool {
        self.paint.deferred_measurement()
    }
}

enum StagedPass {
    RetainPrevious,
    RetainedOverlay,
    Semantic(StagedSemanticPass),
}

struct StagedSemanticPass {
    draft: NativePassDraft,
    output: Option<PaintedSurfaceOutput>,
}

#[derive(Default)]
pub(crate) struct NativePassActions {
    retained: BTreeMap<NativeOutputToken, RetainedPassActions>,
    staged: BTreeMap<NativeOutputToken, StagedPass>,
}

impl NativePassActions {
    /// Returns work which survives the current egui pass and therefore belongs
    /// to the last committed runtime boundary.
    ///
    /// A staged output token is settled by the current output hook. Its mere
    /// presence must not make an otherwise stable runtime appear busy. Local
    /// actions retained from a discarded pass are different: they are durable
    /// obligations which must reach a later terminal pass.
    pub(crate) fn has_retained_action_work(&self) -> bool {
        self.retained
            .values()
            .any(|retained| retained_actions_block_quiescence(&retained.local))
    }

    pub(crate) fn prepare_pass(
        &self,
        token: NativeOutputToken,
        mut paint: NativeSurfacePaint,
        application_action_ready: bool,
    ) -> Result<NativePassDraft, NativePassActionError> {
        let surface = paint.surface();
        if self
            .retained
            .get(&token)
            .is_some_and(|retained| retained.surface != surface)
        {
            return Err(NativePassActionError::OutputChanged);
        }
        Ok(NativePassDraft {
            surface,
            presentation: paint.take_presentation_actions(),
            pane_focus_observation: paint.take_pane_focus_observation(),
            local: paint.take_local_actions(),
            application_action_ready,
            paint,
        })
    }

    pub(crate) fn pass_has_actions(
        &self,
        token: NativeOutputToken,
        draft: &NativePassDraft,
    ) -> bool {
        !draft.presentation.is_empty()
            || draft.pane_focus_observation.is_some()
            || !draft.local.is_empty()
            || draft.application_action_ready
            || self
                .retained
                .get(&token)
                .is_some_and(|retained| !retained.local.is_empty())
    }

    pub(crate) fn stage_pass(
        &mut self,
        token: NativeOutputToken,
        draft: NativePassDraft,
        output: Option<PaintedSurfaceOutput>,
    ) -> Result<(), NativePassActionError> {
        if self.staged.contains_key(&token) {
            return Err(NativePassActionError::OutputChanged);
        }
        self.staged.insert(
            token,
            StagedPass::Semantic(StagedSemanticPass { draft, output }),
        );
        Ok(())
    }

    pub(crate) fn stage_retain_previous(
        &mut self,
        token: NativeOutputToken,
    ) -> Result<(), NativePassActionError> {
        if self.staged.contains_key(&token) {
            return Err(NativePassActionError::OutputChanged);
        }
        self.staged.insert(token, StagedPass::RetainPrevious);
        Ok(())
    }

    pub(crate) fn stage_retained_overlay(
        &mut self,
        token: NativeOutputToken,
    ) -> Result<(), NativePassActionError> {
        if self.staged.contains_key(&token) {
            return Err(NativePassActionError::OutputChanged);
        }
        self.staged.insert(token, StagedPass::RetainedOverlay);
        Ok(())
    }

    pub(crate) fn has_staged(&self, token: NativeOutputToken) -> bool {
        self.staged.contains_key(&token)
    }

    pub(crate) fn discard_pass(
        &mut self,
        token: NativeOutputToken,
    ) -> Result<(), NativePassActionError> {
        let Some(staged) = self.staged.remove(&token) else {
            return Ok(());
        };
        let StagedPass::Semantic(StagedSemanticPass { draft, output: _ }) = staged else {
            return Ok(());
        };
        // Presentation, focus, output, and the pass-local application-action
        // readiness fact die with `staged`. The bounded application queue is
        // sampled again by the next pass; only local UI actions survive here.
        let retained = self
            .retained
            .entry(token)
            .or_insert_with(|| RetainedPassActions {
                surface: draft.surface,
                local: Vec::new(),
            });
        if retained.surface != draft.surface {
            return Err(NativePassActionError::OutputChanged);
        }
        merge_local_actions(&mut retained.local, draft.local)
    }

    pub(crate) fn finish_pass(
        &mut self,
        token: NativeOutputToken,
    ) -> Result<Option<NativeFinalPass>, NativePassActionError> {
        let Some(staged) = self.staged.remove(&token) else {
            return Ok(None);
        };
        let StagedSemanticPass { draft, output } = match staged {
            StagedPass::RetainPrevious => {
                self.retained.remove(&token);
                return Ok(Some(NativeFinalPass::RetainPrevious));
            }
            StagedPass::RetainedOverlay => {
                self.retained.remove(&token);
                return Ok(Some(NativeFinalPass::RetainedOverlay));
            }
            StagedPass::Semantic(semantic) => semantic,
        };
        let mut local = if let Some(retained) = self.retained.remove(&token) {
            if retained.surface != draft.surface {
                return Err(NativePassActionError::OutputChanged);
            }
            retained.local
        } else {
            Vec::new()
        };
        merge_local_actions(&mut local, draft.local)?;
        Ok(Some(NativeFinalPass::Semantic(NativeSemanticFinalPass {
            presentation: draft.presentation,
            pane_focus_observation: draft.pane_focus_observation,
            local,
            application_action_ready: draft.application_action_ready,
            paint: draft.paint,
            output,
        })))
    }

    pub(crate) fn abandon(&mut self, token: NativeOutputToken) {
        self.retained.remove(&token);
        self.staged.remove(&token);
    }

    pub(crate) fn clear(&mut self) {
        self.retained.clear();
        self.staged.clear();
    }
}

pub(crate) enum NativeFinalPass {
    RetainPrevious,
    RetainedOverlay,
    Semantic(NativeSemanticFinalPass),
}

pub(crate) struct NativeSemanticFinalPass {
    presentation: Vec<PreparedSurfaceAction>,
    pane_focus_observation: Option<PreparedPaneFocusObservation>,
    local: Vec<PreparedSurfaceAction>,
    application_action_ready: bool,
    paint: NativeSurfacePaint,
    output: Option<PaintedSurfaceOutput>,
}

impl NativeSemanticFinalPass {
    pub(crate) fn paint_surface(&self) -> SurfaceId {
        self.paint.surface()
    }

    pub(crate) fn has_actions(&self) -> bool {
        !self.presentation.is_empty()
            || self.pane_focus_observation.is_some()
            || !self.local.is_empty()
    }

    pub(crate) const fn application_action_ready(&self) -> bool {
        self.application_action_ready
    }

    pub(crate) fn into_parts(
        self,
    ) -> (
        Vec<PreparedSurfaceAction>,
        Option<PreparedPaneFocusObservation>,
        Vec<PreparedSurfaceAction>,
        bool,
        NativeSurfacePaint,
        Option<PaintedSurfaceOutput>,
    ) {
        (
            self.presentation,
            self.pane_focus_observation,
            self.local,
            self.application_action_ready,
            self.paint,
            self.output,
        )
    }
}

fn merge_local_actions<T: PartialEq>(
    retained: &mut Vec<T>,
    current: Vec<T>,
) -> Result<(), NativePassActionError> {
    if current.is_empty() {
        return Ok(());
    }
    if retained.is_empty() {
        *retained = current;
        return Ok(());
    }
    if *retained == current {
        return Ok(());
    }
    Err(NativePassActionError::LocalActionConflict)
}

fn retained_actions_block_quiescence<T>(actions: &[T]) -> bool {
    !actions.is_empty()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn local_actions_are_retained_deduplicated_and_conflict_closed() {
        let mut retained = Vec::new();
        assert!(!retained_actions_block_quiescence(&retained));
        merge_local_actions(&mut retained, vec![1, 2]).expect("first pass is retained");
        assert!(retained_actions_block_quiescence(&retained));
        merge_local_actions(&mut retained, Vec::new()).expect("missing final action preserves it");
        merge_local_actions(&mut retained, vec![1, 2]).expect("same final action deduplicates");
        assert_eq!(retained, vec![1, 2]);
        assert_eq!(
            merge_local_actions(&mut retained, vec![2, 1]),
            Err(NativePassActionError::LocalActionConflict)
        );
    }

    #[test]
    fn an_empty_current_output_scope_is_not_committed_boundary_work() {
        assert!(!NativePassActions::default().has_retained_action_work());
    }
}
