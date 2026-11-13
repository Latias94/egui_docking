//! Pending pane-focus facts stay owned until a successful host-frame commit.

use dockspace::model::SurfaceId;
use dockspace::runtime::{
    DockspaceHostFrame, DockspaceRuntimeError, HostFrameReport, PreparedPaneFocusObservation,
    SurfacePresentationArtifact,
};

#[derive(Default)]
pub(crate) struct PendingFocusObservations {
    entries: Vec<(SurfacePresentationArtifact, PreparedPaneFocusObservation)>,
}

impl PendingFocusObservations {
    pub(crate) fn clear(&mut self) {
        self.entries.clear();
    }

    pub(crate) fn push(
        &mut self,
        artifact: SurfacePresentationArtifact,
        observation: PreparedPaneFocusObservation,
    ) {
        self.entries.push((artifact, observation));
    }

    pub(crate) fn submit_current(
        &self,
        frame: &mut DockspaceHostFrame<'_>,
        surface: SurfaceId,
    ) -> Result<Vec<SurfacePresentationArtifact>, DockspaceRuntimeError> {
        let mut accepted = Vec::new();
        for (artifact, observation) in &self.entries {
            if frame
                .presented_surface(surface)
                .is_some_and(|current| current.matches_presentation_artifact(*artifact))
            {
                frame.submit_pane_focus_observation(observation.clone())?;
                accepted.push(*artifact);
            }
        }
        Ok(accepted)
    }

    pub(crate) fn settle(
        &mut self,
        accepted: &[SurfacePresentationArtifact],
        report: &HostFrameReport,
    ) {
        self.entries.retain(|(artifact, _)| {
            !accepted.contains(artifact)
                && !report
                    .surface_presentation_commits()
                    .iter()
                    .any(|commit| commit.settles(*artifact))
        });
    }
}
