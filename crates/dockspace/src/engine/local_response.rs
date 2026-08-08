//! Current-frame framework response actions over an exact Ready candidate.

use super::*;

impl DockEngine {
    pub(super) fn local_response_candidate(
        &self,
        scene: SurfaceSceneStamp,
    ) -> Result<&crate::scene::SurfacePlanScene, InteractionRejection> {
        let candidate = self
            .presentation_authority
            .scene
            .surface(scene.surface())
            .and_then(SurfaceScene::ready)
            .map(crate::scene::ReadySurfaceScene::candidate)
            .filter(|candidate| candidate.stamp() == scene)
            .filter(|candidate| {
                candidate.stamp().requirement().workspace_epoch() == self.version.epoch()
            })
            .ok_or(InteractionRejection::StaleScene)?;
        Ok(candidate)
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn reduce_local_scene_tab_select(
        &mut self,
        input: InputSequence,
        expected: WorkspaceVersion,
        application_base: WorkspaceVersion,
        scene: SurfaceSceneStamp,
        tab: crate::scene::TabSceneId,
        policy: &DockPolicySnapshot,
        events: &mut Vec<WorkspaceEvent>,
        interaction_events: &mut Vec<InteractionEvent>,
    ) -> Result<InputOutcome, EngineError> {
        if expected != application_base {
            return Ok(InputOutcome::StaleRejected {
                expected,
                accepted_base: application_base,
            });
        }

        let source = {
            let candidate = match self.local_response_candidate(scene) {
                Ok(candidate) => candidate,
                Err(error) => return Ok(self.local_response_rejection(error)),
            };
            if !candidate
                .plan()
                .tab_records()
                .iter()
                .any(|record| *record.id() == tab)
            {
                return Ok(self.local_response_rejection(
                    InteractionRejection::TabGestureSourceUnavailable {
                        source: TabGestureSource::Item(tab),
                    },
                ));
            }
            match self
                .workspace
                .capture_item_source(tab.root, tab.tabs, tab.item)
            {
                Ok(source) => source,
                Err(_) => {
                    return Ok(self.local_response_rejection(
                        InteractionRejection::TabGestureSourceUnavailable {
                            source: TabGestureSource::Item(tab),
                        },
                    ));
                }
            }
        };

        self.reduce_workspace_command(
            input,
            expected,
            application_base,
            &WorkspaceCommand::Select { source },
            policy,
            events,
            interaction_events,
        )
    }

    pub(super) fn reduce_local_scene_close_input(
        &mut self,
        input: InputSequence,
        expected: WorkspaceVersion,
        scene: SurfaceSceneStamp,
        target: CloseSceneTarget,
        policy: &DockPolicySnapshot,
    ) -> Result<InputOutcome, EngineError> {
        self.reduce_versioned_interaction(expected, |engine| {
            engine.request_close_plan(input, scene, target, CloseActivation::LocalResponse, policy)
        })
    }

    fn local_response_rejection(&self, outcome: InteractionRejection) -> InputOutcome {
        InputOutcome::InteractionProcessed {
            outcome: InteractionOutcome::Rejected(outcome),
            version: self.version,
        }
    }
}
