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
enum DurableLocalActionState<T> {
    /// The affine action capability is still owned by the adapter.
    Pending(Vec<T>),
    /// The capability moved into an exact core host frame. The marker remains
    /// until that frame produces either a commit report or an explicit error.
    InFlight,
}

/// Bounded local-action ownership keyed by product surface, never by output.
///
/// A surface has at most one pending or in-flight batch. Multipass replay of
/// the same batch deduplicates; ambiguous replacement fails closed.
#[derive(Debug)]
struct DurableLocalActions<T> {
    by_surface: BTreeMap<SurfaceId, DurableLocalActionState<T>>,
}

impl<T> Default for DurableLocalActions<T> {
    fn default() -> Self {
        Self {
            by_surface: BTreeMap::new(),
        }
    }
}

impl<T> DurableLocalActions<T> {
    fn has_work(&self) -> bool {
        !self.by_surface.is_empty()
    }

    fn has_pending(&self, surface: SurfaceId) -> bool {
        matches!(
            self.by_surface.get(&surface),
            Some(DurableLocalActionState::Pending(actions)) if !actions.is_empty()
        )
    }
}

impl<T: PartialEq> DurableLocalActions<T> {
    fn retain(&mut self, surface: SurfaceId, current: Vec<T>) -> Result<(), NativePassActionError> {
        if current.is_empty() {
            return Ok(());
        }
        match self.by_surface.entry(surface) {
            std::collections::btree_map::Entry::Vacant(entry) => {
                entry.insert(DurableLocalActionState::Pending(current));
                Ok(())
            }
            std::collections::btree_map::Entry::Occupied(mut entry) => match entry.get_mut() {
                DurableLocalActionState::Pending(retained) => {
                    merge_local_actions(retained, current)
                }
                DurableLocalActionState::InFlight => {
                    Err(NativePassActionError::LocalActionConflict)
                }
            },
        }
    }

    fn begin_terminal(
        &mut self,
        surface: SurfaceId,
        current: Vec<T>,
    ) -> Result<Option<DurableLocalActionSubmission<T>>, NativePassActionError> {
        self.retain(surface, current)?;
        let Some(state) = self.by_surface.remove(&surface) else {
            return Ok(None);
        };
        match state {
            DurableLocalActionState::Pending(actions) => {
                self.by_surface
                    .insert(surface, DurableLocalActionState::InFlight);
                Ok(Some(DurableLocalActionSubmission { surface, actions }))
            }
            DurableLocalActionState::InFlight => {
                self.by_surface
                    .insert(surface, DurableLocalActionState::InFlight);
                Ok(None)
            }
        }
    }

    fn begin_attempt(
        &mut self,
        surface: SurfaceId,
        current: Vec<T>,
    ) -> Result<
        Option<(
            DurableLocalActionSubmission<T>,
            DurableLocalActionAttempt<'_, T>,
        )>,
        NativePassActionError,
    > {
        let Some(submission) = self.begin_terminal(surface, current)? else {
            return Ok(None);
        };
        Ok(Some((
            submission,
            DurableLocalActionAttempt {
                actions: self,
                surface,
                active: true,
            },
        )))
    }

    fn settle_core_commit(&mut self, surface: SurfaceId) -> Result<(), NativePassActionError> {
        self.settle_in_flight(surface)
    }

    fn settle_core_failure(&mut self, surface: SurfaceId) -> Result<(), NativePassActionError> {
        self.settle_in_flight(surface)
    }

    fn settle_core_attempt<R, E>(
        &mut self,
        surface: SurfaceId,
        result: Result<R, E>,
    ) -> Result<Result<R, E>, NativePassActionError> {
        // Both outcomes are terminals for the affine capability. Retrying an
        // opaque action after it entered a host frame could apply it twice;
        // the caller therefore propagates an explicit failure and stops.
        match result {
            Ok(value) => {
                self.settle_core_commit(surface)?;
                Ok(Ok(value))
            }
            Err(error) => {
                self.settle_core_failure(surface)?;
                Ok(Err(error))
            }
        }
    }

    fn settle_in_flight(&mut self, surface: SurfaceId) -> Result<(), NativePassActionError> {
        let Some(state) = self.by_surface.remove(&surface) else {
            return Err(NativePassActionError::OutputChanged);
        };
        match state {
            DurableLocalActionState::InFlight => Ok(()),
            pending @ DurableLocalActionState::Pending(_) => {
                self.by_surface.insert(surface, pending);
                Err(NativePassActionError::OutputChanged)
            }
        }
    }
}

#[derive(Debug)]
pub(crate) struct DurableLocalActionSubmission<T> {
    surface: SurfaceId,
    actions: Vec<T>,
}

impl<T> DurableLocalActionSubmission<T> {
    pub(crate) const fn surface(&self) -> SurfaceId {
        self.surface
    }

    #[cfg(test)]
    fn actions(&self) -> &[T] {
        &self.actions
    }

    pub(crate) fn into_parts(self) -> (SurfaceId, Vec<T>) {
        (self.surface, self.actions)
    }
}

pub(crate) type NativeLocalActionSubmission = DurableLocalActionSubmission<PreparedSurfaceAction>;

/// Exact ownership of one submitted affine local-action batch.
///
/// Dropping the attempt during unwind terminalizes the consumed capability so
/// a caught panic cannot leave the durable queue permanently in flight.
#[must_use = "a local-action attempt must be settled by its exact core result"]
pub(crate) struct DurableLocalActionAttempt<'a, T: PartialEq> {
    actions: &'a mut DurableLocalActions<T>,
    surface: SurfaceId,
    active: bool,
}

impl<T: PartialEq> DurableLocalActionAttempt<'_, T> {
    pub(crate) fn settle<R, E>(
        mut self,
        result: Result<R, E>,
    ) -> Result<Result<R, E>, NativePassActionError> {
        let settled = self.actions.settle_core_attempt(self.surface, result);
        // `settle_core_attempt` never leaves an in-flight state behind, even
        // when it reports an ownership mismatch and restores Pending.
        self.active = false;
        settled
    }
}

impl<T: PartialEq> Drop for DurableLocalActionAttempt<'_, T> {
    fn drop(&mut self) {
        if !self.active {
            return;
        }
        let settled = self.actions.settle_core_failure(self.surface);
        debug_assert!(
            std::thread::panicking() || settled.is_ok(),
            "an active local-action attempt owns one in-flight batch"
        );
        self.active = false;
    }
}

pub(crate) type NativeLocalActionAttempt<'a> = DurableLocalActionAttempt<'a, PreparedSurfaceAction>;

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

    pub(crate) fn paint_mut(&mut self) -> &mut NativeSurfacePaint {
        &mut self.paint
    }
}

enum StagedPass {
    RetainPrevious { surface: SurfaceId },
    RetainedOverlay,
    Semantic(StagedSemanticPass),
}

struct StagedSemanticPass {
    draft: NativePassDraft,
    output: Option<PaintedSurfaceOutput>,
}

#[derive(Default)]
pub(crate) struct NativePassActions {
    local_actions: DurableLocalActions<PreparedSurfaceAction>,
    output_surfaces: BTreeMap<NativeOutputToken, SurfaceId>,
    staged: BTreeMap<NativeOutputToken, StagedPass>,
}

impl NativePassActions {
    /// Returns work which survives the current egui output scope and therefore
    /// belongs to a later core action boundary.
    ///
    /// A staged output token is settled by the current output hook. Its mere
    /// presence must not make an otherwise stable runtime appear busy. Local
    /// actions retained from a discarded pass are different: they are durable
    /// obligations which must reach a later terminal pass.
    pub(crate) fn has_retained_action_work(&self) -> bool {
        self.local_actions.has_work()
    }

    pub(crate) fn prepare_pass(
        &self,
        token: NativeOutputToken,
        mut paint: NativeSurfacePaint,
        application_action_ready: bool,
    ) -> Result<NativePassDraft, NativePassActionError> {
        let surface = paint.surface();
        if self
            .output_surfaces
            .get(&token)
            .is_some_and(|expected| *expected != surface)
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

    pub(crate) fn pass_has_actions(&self, draft: &NativePassDraft) -> bool {
        !draft.presentation.is_empty()
            || draft.pane_focus_observation.is_some()
            || !draft.local.is_empty()
            || draft.application_action_ready
            || self.local_actions.has_pending(draft.surface)
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
        self.remember_output_surface(token, draft.surface)?;
        self.staged.insert(
            token,
            StagedPass::Semantic(StagedSemanticPass { draft, output }),
        );
        Ok(())
    }

    pub(crate) fn stage_retain_previous(
        &mut self,
        token: NativeOutputToken,
        surface: SurfaceId,
    ) -> Result<(), NativePassActionError> {
        if self.staged.contains_key(&token) {
            return Err(NativePassActionError::OutputChanged);
        }
        self.remember_output_surface(token, surface)?;
        self.staged
            .insert(token, StagedPass::RetainPrevious { surface });
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
        self.local_actions.retain(draft.surface, draft.local)
    }

    pub(crate) fn finish_pass(
        &mut self,
        token: NativeOutputToken,
    ) -> Result<Option<NativeFinalPass>, NativePassActionError> {
        let Some(staged) = self.staged.remove(&token) else {
            return Ok(None);
        };
        let expected_surface = self.output_surfaces.remove(&token);
        let StagedSemanticPass { draft, output } = match staged {
            StagedPass::RetainPrevious { surface } => {
                if expected_surface.is_some_and(|expected| expected != surface) {
                    return Err(NativePassActionError::OutputChanged);
                }
                return Ok(Some(NativeFinalPass::RetainPrevious {
                    surface,
                    local_actions_ready: self.local_actions.has_pending(surface),
                }));
            }
            StagedPass::RetainedOverlay => {
                return Ok(Some(NativeFinalPass::RetainedOverlay));
            }
            StagedPass::Semantic(semantic) => semantic,
        };
        if expected_surface.is_some_and(|expected| expected != draft.surface) {
            return Err(NativePassActionError::OutputChanged);
        }
        let local_actions_ready =
            !draft.local.is_empty() || self.local_actions.has_pending(draft.surface);
        Ok(Some(NativeFinalPass::Semantic(NativeSemanticFinalPass {
            presentation: draft.presentation,
            pane_focus_observation: draft.pane_focus_observation,
            local: draft.local,
            local_actions_ready,
            application_action_ready: draft.application_action_ready,
            paint: draft.paint,
            output,
        })))
    }

    pub(crate) fn abandon(
        &mut self,
        token: NativeOutputToken,
    ) -> Result<(), NativePassActionError> {
        self.output_surfaces.remove(&token);
        let Some(staged) = self.staged.remove(&token) else {
            return Ok(());
        };
        let StagedPass::Semantic(StagedSemanticPass { draft, output: _ }) = staged else {
            return Ok(());
        };
        // Output abandonment retires only renderer/presentation evidence. A
        // local action remains an adapter obligation until a core terminal.
        self.local_actions.retain(draft.surface, draft.local)
    }

    pub(crate) fn abandon_all_outputs_into(
        &mut self,
        mut on_error: impl FnMut(NativePassActionError),
    ) {
        loop {
            let token = self
                .staged
                .first_key_value()
                .map(|(token, _)| *token)
                .into_iter()
                .chain(
                    self.output_surfaces
                        .first_key_value()
                        .map(|(token, _)| *token),
                )
                .min();
            let Some(token) = token else {
                break;
            };
            if let Err(error) = self.abandon(token) {
                on_error(error);
            }
        }
    }

    pub(crate) fn retain_local_actions(
        &mut self,
        surface: SurfaceId,
        actions: Vec<PreparedSurfaceAction>,
    ) -> Result<bool, NativePassActionError> {
        self.local_actions.retain(surface, actions)?;
        Ok(self.local_actions.has_pending(surface))
    }

    pub(crate) fn begin_local_action_submission(
        &mut self,
        surface: SurfaceId,
    ) -> Result<(NativeLocalActionSubmission, NativeLocalActionAttempt<'_>), NativePassActionError>
    {
        self.local_actions
            .begin_attempt(surface, Vec::new())?
            .ok_or(NativePassActionError::OutputChanged)
    }

    fn remember_output_surface(
        &mut self,
        token: NativeOutputToken,
        surface: SurfaceId,
    ) -> Result<(), NativePassActionError> {
        match self.output_surfaces.entry(token) {
            std::collections::btree_map::Entry::Vacant(entry) => {
                entry.insert(surface);
                Ok(())
            }
            std::collections::btree_map::Entry::Occupied(entry) if *entry.get() == surface => {
                Ok(())
            }
            std::collections::btree_map::Entry::Occupied(_) => {
                Err(NativePassActionError::OutputChanged)
            }
        }
    }
}

pub(crate) enum NativeFinalPass {
    RetainPrevious {
        surface: SurfaceId,
        local_actions_ready: bool,
    },
    RetainedOverlay,
    Semantic(NativeSemanticFinalPass),
}

pub(crate) struct NativeSemanticFinalPass {
    presentation: Vec<PreparedSurfaceAction>,
    pane_focus_observation: Option<PreparedPaneFocusObservation>,
    local: Vec<PreparedSurfaceAction>,
    local_actions_ready: bool,
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
            || self.local_actions_ready
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn local_actions_are_retained_deduplicated_and_conflict_closed() {
        let mut retained = Vec::new();
        merge_local_actions(&mut retained, vec![1, 2]).expect("first pass is retained");
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

    #[test]
    fn durable_local_actions_cross_output_origins_and_wait_for_core_terminal() {
        let surface = SurfaceId::new(1);
        let mut actions = DurableLocalActions::<u8>::default();

        actions
            .retain(surface, vec![1, 2])
            .expect("a discarded pass retains its local actions");
        assert!(actions.has_work());

        let submission = actions
            .begin_terminal(surface, Vec::new())
            .expect("a later output may deliver durable local actions")
            .expect("the durable action batch is submitted");
        assert_eq!(submission.surface(), surface);
        assert_eq!(submission.actions(), &[1, 2]);
        assert!(actions.has_work(), "submission is not a core terminal");

        actions
            .settle_core_commit(surface)
            .expect("the exact core terminal retires the batch");
        assert!(!actions.has_work());
    }

    #[test]
    fn durable_local_actions_are_bounded_and_conflict_closed() {
        let surface = SurfaceId::new(1);
        let mut actions = DurableLocalActions::<u8>::default();

        actions
            .retain(surface, vec![1, 2])
            .expect("the first batch becomes durable");
        actions
            .retain(surface, vec![1, 2])
            .expect("the same pass may replay the same batch");
        assert_eq!(
            actions.retain(surface, vec![2, 1]),
            Err(NativePassActionError::LocalActionConflict)
        );

        let _submission = actions
            .begin_terminal(surface, Vec::new())
            .expect("an unambiguous later terminal may submit the batch")
            .expect("the durable batch is present");
        assert_eq!(
            actions.retain(surface, vec![3]),
            Err(NativePassActionError::LocalActionConflict)
        );
    }

    #[test]
    fn only_an_in_flight_core_submission_can_settle_durable_actions() {
        let surface = SurfaceId::new(1);
        let mut actions = DurableLocalActions::<u8>::default();

        actions
            .retain(surface, vec![1])
            .expect("the local action becomes durable");
        assert_eq!(
            actions.settle_core_commit(surface),
            Err(NativePassActionError::OutputChanged)
        );
        assert!(
            actions.has_work(),
            "an unrelated terminal cannot clear work"
        );

        let _submission = actions
            .begin_terminal(surface, Vec::new())
            .expect("the pending batch can enter a core frame")
            .expect("the batch remains available after the rejected terminal");
        actions
            .settle_core_commit(surface)
            .expect("the matching core terminal clears the in-flight marker");
        assert!(!actions.has_work());
    }

    #[test]
    fn submit_or_commit_failure_terminal_does_not_leave_an_in_flight_tombstone() {
        for error in [
            "submit_surface_action rejected the capability",
            "host_frame.commit rejected the frame",
        ] {
            let surface = SurfaceId::new(1);
            let mut actions = DurableLocalActions::<u8>::default();

            actions
                .retain(surface, vec![1])
                .expect("the local action becomes durable");
            let (_submission, attempt) = actions
                .begin_attempt(surface, Vec::new())
                .expect("the pending batch enters the core frame")
                .expect("the durable action is submitted");

            let result = attempt
                .settle(Err::<(), _>(error))
                .expect("the failed core attempt has a matching in-flight batch");
            assert_eq!(result, Err(error));
            assert!(!actions.has_work());
        }
    }

    #[test]
    fn unwound_core_attempt_does_not_leave_an_in_flight_tombstone() {
        let surface = SurfaceId::new(1);
        let mut actions = DurableLocalActions::<u8>::default();
        actions
            .retain(surface, vec![1])
            .expect("the local action becomes durable");

        let unwind = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let (_submission, _attempt) = actions
                .begin_attempt(surface, Vec::new())
                .expect("the pending batch can enter a core attempt")
                .expect("the durable batch is submitted");
            panic!("the core host frame unwound");
        }));

        assert!(unwind.is_err());
        assert!(
            !actions.has_work(),
            "unwind is a terminal failure for the consumed affine capability"
        );
    }
}
