//! Bounded event-owned UI ingress and private actual-pass payloads.

use std::collections::{BTreeMap, BTreeSet};

use dockspace::model::SurfaceId;
use dockspace::runtime::{
    DeferredSurfaceAction, DockspaceSession, NativeSurfaceBinding, PreparedPaneFocusObservation,
    PresentedDockspaceSurface, SurfacePresentationArtifact, SurfacePresentationCommit,
};
use eframe::{NativeOutputToken, egui};

use crate::host_frame::NativeSurfacePaint;

const LOCAL_ACTION_CAPACITY: usize = 64;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum NativePassActionError {
    OutputChanged,
    LocalActionConflict,
}

#[derive(Clone, Copy, PartialEq, Eq)]
struct UiActionEvent {
    run: NativeOutputToken,
    origin: egui::Id,
}

struct PendingLocalAction {
    event: UiActionEvent,
    owner: Option<NativeSurfaceBinding>,
    action: DeferredSurfaceAction,
}

/// Exact events acknowledged after publication; candidate clones leave ingress intact.
pub(crate) struct NativeUiActionAcknowledgement {
    surface: SurfaceId,
    events: Vec<UiActionEvent>,
    pane_focus: Option<(NativeOutputToken, SurfacePresentationArtifact)>,
}

/// One bounded ingress, with replay suppression confined to the original input run.
#[derive(Default)]
pub(crate) struct NativePassActions {
    local_actions: BTreeMap<SurfaceId, Vec<PendingLocalAction>>,
    pane_focus: BTreeMap<SurfaceId, PendingPaneFocus>,
    output_surfaces: BTreeMap<NativeOutputToken, SurfaceId>,
    staged: BTreeMap<NativeOutputToken, NativeFinalPass>,
}

struct PendingPaneFocus {
    origin: NativeOutputToken,
    owner: Option<NativeSurfaceBinding>,
    artifact: SurfacePresentationArtifact,
    observation: PreparedPaneFocusObservation,
}

impl NativePassActions {
    pub(crate) fn has_retained_action_work(&self) -> bool {
        !self.local_actions.is_empty() || !self.pane_focus.is_empty()
    }

    pub(crate) fn prepare_pass(
        &mut self,
        token: NativeOutputToken,
        owner: Option<NativeSurfaceBinding>,
        mut paint: NativeSurfacePaint,
    ) -> Result<NativeSurfacePaint, NativePassActionError> {
        let surface = paint.surface();
        if self
            .output_surfaces
            .get(&token)
            .is_some_and(|expected| *expected != surface)
        {
            return Err(NativePassActionError::OutputChanged);
        }
        let actions = paint
            .take_local_actions()
            .into_iter()
            .chain(paint.take_presentation_actions())
            .map(|record| PendingLocalAction {
                event: UiActionEvent {
                    run: token,
                    origin: record.origin(),
                },
                owner,
                action: record.into_deferred(),
            })
            .collect();
        self.retain_local_actions(surface, actions)?;
        Ok(paint)
    }

    fn retain_local_actions(
        &mut self,
        surface: SurfaceId,
        actions: Vec<PendingLocalAction>,
    ) -> Result<(), NativePassActionError> {
        let retained = self.local_actions.get(&surface);
        let mut additional: Vec<PendingLocalAction> = Vec::new();
        for action in actions {
            let previous = retained
                .into_iter()
                .flatten()
                .chain(&additional)
                .find(|old| old.event == action.event);
            match previous {
                Some(old)
                    if old.owner != action.owner || !old.action.same_intent(&action.action) =>
                {
                    return Err(NativePassActionError::LocalActionConflict);
                }
                Some(_) => {}
                None => additional.push(action),
            }
        }
        let pending = self.local_actions.values().map(Vec::len).sum::<usize>();
        if pending.saturating_add(additional.len()) > LOCAL_ACTION_CAPACITY {
            return Err(NativePassActionError::LocalActionConflict);
        }
        if additional.is_empty() {
            return Ok(());
        }
        self.local_actions
            .entry(surface)
            .or_default()
            .extend(additional);
        Ok(())
    }

    /// Clones earlier intents while retaining their originals until publication.
    pub(crate) fn peek_deferred_actions(
        &self,
        token: NativeOutputToken,
        surface: SurfaceId,
    ) -> (Vec<DeferredSurfaceAction>, NativeUiActionAcknowledgement) {
        let (events, actions) = self
            .local_actions
            .get(&surface)
            .into_iter()
            .flatten()
            .filter(|pending| pending.event.run != token)
            .map(|pending| (pending.event, pending.action.clone()))
            .unzip();
        (
            actions,
            NativeUiActionAcknowledgement {
                surface,
                events,
                pane_focus: None,
            },
        )
    }

    /// Acknowledges only the published candidate's inputs, preserving newly emitted work.
    pub(crate) fn acknowledge(&mut self, acknowledged: NativeUiActionAcknowledgement) {
        if let Some(actions) = self.local_actions.get_mut(&acknowledged.surface) {
            actions.retain(|pending| !acknowledged.events.contains(&pending.event));
            if actions.is_empty() {
                self.local_actions.remove(&acknowledged.surface);
            }
        }
        if self
            .pane_focus
            .get(&acknowledged.surface)
            .is_some_and(|pending| {
                acknowledged.pane_focus == Some((pending.origin, pending.artifact))
            })
        {
            self.pane_focus.remove(&acknowledged.surface);
        }
    }

    pub(crate) fn peek_pane_focus(
        &self,
        token: NativeOutputToken,
        presented: Option<PresentedDockspaceSurface>,
        acknowledged: &mut NativeUiActionAcknowledgement,
    ) -> Option<PreparedPaneFocusObservation> {
        let pending = self
            .pane_focus
            .get(&acknowledged.surface)
            .filter(|pending| {
                pending.origin != token
                    && presented.is_some_and(|presented| {
                        presented.matches_presentation_artifact(pending.artifact)
                    })
            })?;
        acknowledged.pane_focus = Some((pending.origin, pending.artifact));
        Some(pending.observation.clone())
    }

    /// Retires published identities, preserving live owners awaiting first-live output.
    pub(crate) fn retire_unavailable(&mut self, session: &DockspaceSession) {
        let current_version = session.version();
        let current_surfaces = session
            .view()
            .surfaces()
            .map(|surface| surface.id())
            .collect::<BTreeSet<_>>();
        // Pending first-live owners can precede their logical surface publication.
        let owner_is_current = |surface: &SurfaceId, owner: Option<NativeSurfaceBinding>| {
            owner.map_or_else(
                || current_surfaces.contains(surface),
                |owner| owner.surface() == *surface && session.is_current_native_binding(owner),
            )
        };
        self.local_actions.retain(|surface, actions| {
            actions.retain(|pending| {
                pending
                    .action
                    .expected_version()
                    .is_same_epoch(current_version)
                    && owner_is_current(surface, pending.owner)
            });
            !actions.is_empty()
        });
        self.pane_focus
            .retain(|surface, pending| owner_is_current(surface, pending.owner));
    }

    pub(crate) fn retain_pane_focus(
        &mut self,
        token: NativeOutputToken,
        surface: SurfaceId,
        owner: Option<NativeSurfaceBinding>,
        artifact: SurfacePresentationArtifact,
        observation: PreparedPaneFocusObservation,
    ) {
        self.pane_focus.insert(
            surface,
            PendingPaneFocus {
                origin: token,
                owner,
                artifact,
                observation,
            },
        );
    }

    pub(crate) fn commit_presentations(&mut self, commits: &[SurfacePresentationCommit]) {
        self.pane_focus.retain(|_, pending| {
            !commits
                .iter()
                .any(|commit| commit.retires(pending.artifact))
        });
    }

    pub(crate) fn stage_pass(
        &mut self,
        token: NativeOutputToken,
        paint: NativeSurfacePaint,
    ) -> Result<(), NativePassActionError> {
        self.remember_output_surface(token, paint.surface())?;
        self.stage(token, NativeFinalPass::Semantic(paint))
    }

    pub(crate) fn stage_retain_previous(
        &mut self,
        token: NativeOutputToken,
        surface: SurfaceId,
    ) -> Result<(), NativePassActionError> {
        self.remember_output_surface(token, surface)?;
        self.stage(token, NativeFinalPass::RetainPrevious)
    }

    pub(crate) fn stage_retained_overlay(
        &mut self,
        token: NativeOutputToken,
    ) -> Result<(), NativePassActionError> {
        self.stage(token, NativeFinalPass::RetainedOverlay)
    }

    fn stage(
        &mut self,
        token: NativeOutputToken,
        pass: NativeFinalPass,
    ) -> Result<(), NativePassActionError> {
        if self.staged.contains_key(&token) {
            return Err(NativePassActionError::OutputChanged);
        }
        self.staged.insert(token, pass);
        Ok(())
    }

    pub(crate) fn has_staged(&self, token: NativeOutputToken) -> bool {
        self.staged.contains_key(&token)
    }

    pub(crate) fn is_retained(&self, token: NativeOutputToken) -> bool {
        matches!(
            self.staged.get(&token),
            Some(NativeFinalPass::RetainPrevious)
        )
    }

    pub(crate) fn discard_pass(
        &mut self,
        token: NativeOutputToken,
    ) -> Result<(), NativePassActionError> {
        self.staged.remove(&token);
        Ok(())
    }

    pub(crate) fn finish_pass(
        &mut self,
        token: NativeOutputToken,
    ) -> Result<Option<NativeFinalPass>, NativePassActionError> {
        let pass = self.staged.remove(&token);
        self.output_surfaces.remove(&token);
        Ok(pass)
    }

    pub(crate) fn abandon(
        &mut self,
        token: NativeOutputToken,
    ) -> Result<(), NativePassActionError> {
        self.discard_pass(token)?;
        self.output_surfaces.remove(&token);
        Ok(())
    }

    pub(crate) fn abandon_all_outputs_into(
        &mut self,
        mut on_error: impl FnMut(NativePassActionError),
    ) {
        while let Some(token) = self
            .staged
            .keys()
            .chain(self.output_surfaces.keys())
            .copied()
            .min()
        {
            if let Err(error) = self.abandon(token) {
                on_error(error);
            }
        }
    }

    fn remember_output_surface(
        &mut self,
        token: NativeOutputToken,
        surface: SurfaceId,
    ) -> Result<(), NativePassActionError> {
        if let Some(expected) = self.output_surfaces.get(&token) {
            if *expected != surface {
                return Err(NativePassActionError::OutputChanged);
            }
        } else {
            self.output_surfaces.insert(token, surface);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests;

pub(crate) enum NativeFinalPass {
    RetainPrevious,
    RetainedOverlay,
    Semantic(NativeSurfacePaint),
}
