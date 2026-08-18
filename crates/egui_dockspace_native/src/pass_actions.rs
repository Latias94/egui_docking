//! Minimal egui multipass retention for affine product actions.

use std::collections::BTreeMap;

use dockspace::model::SurfaceId;
use dockspace::runtime::{PreparedPaneFocusObservation, PreparedSurfaceAction};
use eframe::NativeOutputToken;
use egui_dockspace::native_support::NativeSurfacePaint;

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

#[derive(Debug, Default)]
pub(crate) struct NativePassActions {
    retained: BTreeMap<NativeOutputToken, RetainedPassActions>,
}

impl NativePassActions {
    pub(crate) fn has_pending_work(&self) -> bool {
        !self.retained.is_empty()
    }

    pub(crate) fn discard_pass(
        &mut self,
        token: NativeOutputToken,
        paint: &mut NativeSurfacePaint,
    ) -> Result<(), NativePassActionError> {
        let surface = paint.surface();
        let current = paint.take_local_actions();
        let retained = self
            .retained
            .entry(token)
            .or_insert_with(|| RetainedPassActions {
                surface,
                local: Vec::new(),
            });
        if retained.surface != surface {
            return Err(NativePassActionError::OutputChanged);
        }
        merge_local_actions(&mut retained.local, current)
    }

    pub(crate) fn finish_pass(
        &mut self,
        token: NativeOutputToken,
        paint: &mut NativeSurfacePaint,
    ) -> Result<NativeFinalPassActions, NativePassActionError> {
        let surface = paint.surface();
        let current = paint.take_local_actions();
        let mut local = if let Some(retained) = self.retained.remove(&token) {
            if retained.surface != surface {
                return Err(NativePassActionError::OutputChanged);
            }
            retained.local
        } else {
            Vec::new()
        };
        merge_local_actions(&mut local, current)?;
        Ok(NativeFinalPassActions {
            presentation: paint.take_presentation_actions(),
            pane_focus_observation: paint.take_pane_focus_observation(),
            local,
        })
    }

    pub(crate) fn abandon(&mut self, token: NativeOutputToken) {
        self.retained.remove(&token);
    }
}

pub(crate) struct NativeFinalPassActions {
    presentation: Vec<PreparedSurfaceAction>,
    pane_focus_observation: Option<PreparedPaneFocusObservation>,
    local: Vec<PreparedSurfaceAction>,
}

impl NativeFinalPassActions {
    pub(crate) fn has_actions(&self) -> bool {
        !self.presentation.is_empty()
            || self.pane_focus_observation.is_some()
            || !self.local.is_empty()
    }

    pub(crate) fn into_parts(
        self,
    ) -> (
        Vec<PreparedSurfaceAction>,
        Option<PreparedPaneFocusObservation>,
        Vec<PreparedSurfaceAction>,
    ) {
        (self.presentation, self.pane_focus_observation, self.local)
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
}
