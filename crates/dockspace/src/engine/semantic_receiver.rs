//! Presentation-bound keyboard and accessibility interaction reduction.

use crate::command::WorkspaceCommand;
use crate::event::WorkspaceEvent;
use crate::intent::{CloseActivation, CloseSceneTarget};
use crate::interaction::{
    InteractionEvent, InteractionEventKind, InteractionOutcome, InteractionRejection,
};
use crate::policy::DockPolicySnapshot;
use crate::presentation_hit::PresentationHitRegionKind;
use crate::semantic_input::{
    SemanticAccessibilityAction, SemanticKey, SemanticReceiverAction, SemanticReceiverEvent,
};
use crate::transition::{InputOutcome, WorkspaceVersion};

use super::{DockEngine, EngineError};

impl DockEngine {
    #[allow(clippy::too_many_arguments)]
    pub(super) fn reduce_semantic_receiver_input(
        &mut self,
        input: crate::ids::InputSequence,
        expected: WorkspaceVersion,
        application_base: WorkspaceVersion,
        event: SemanticReceiverEvent,
        semantic_presentations: Option<
            &std::collections::BTreeMap<
                crate::ids::SurfaceId,
                crate::journal_presentation::JournalSurfacePresentation,
            >,
        >,
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

        let projection = match Self::validate_semantic_receiver(event, semantic_presentations) {
            Ok(projection) => projection,
            Err(rejection) => return Ok(self.semantic_rejection(rejection)),
        };
        let scene = projection.plan_stamp();
        let target = event.target();
        let action = event.action();

        match (target, action) {
            (
                PresentationHitRegionKind::TabBody(tab),
                SemanticReceiverAction::Key(
                    SemanticKey::ArrowLeft
                    | SemanticKey::ArrowRight
                    | SemanticKey::Home
                    | SemanticKey::End,
                ),
            ) => {
                let destination =
                    match self.semantic_tab_destination(projection.plan(), tab, action) {
                        Some(destination) => destination,
                        None => {
                            return Ok(self.semantic_rejection(
                                InteractionRejection::SemanticActionUnsupported { target, action },
                            ));
                        }
                    };
                self.reduce_semantic_tab_selection(
                    input,
                    expected,
                    application_base,
                    destination,
                    policy,
                    events,
                    interaction_events,
                )
            }
            (
                PresentationHitRegionKind::TabBody(tab),
                SemanticReceiverAction::Key(SemanticKey::Enter | SemanticKey::Space)
                | SemanticReceiverAction::Accessibility(
                    SemanticAccessibilityAction::Click | SemanticAccessibilityAction::Focus,
                ),
            ) => self.reduce_semantic_tab_selection(
                input,
                expected,
                application_base,
                tab,
                policy,
                events,
                interaction_events,
            ),
            (
                PresentationHitRegionKind::TabClose(tab),
                SemanticReceiverAction::Key(SemanticKey::Enter | SemanticKey::Space)
                | SemanticReceiverAction::Accessibility(SemanticAccessibilityAction::Click),
            ) => self.reduce_versioned_interaction(expected, |engine| {
                engine.request_close_plan(
                    input,
                    scene,
                    CloseSceneTarget::Tab(tab),
                    CloseActivation::Semantic,
                    policy,
                )
            }),
            (
                PresentationHitRegionKind::ContainedClose(floating),
                SemanticReceiverAction::Key(SemanticKey::Enter | SemanticKey::Space)
                | SemanticReceiverAction::Accessibility(SemanticAccessibilityAction::Click),
            ) => self.reduce_versioned_interaction(expected, |engine| {
                engine.request_close_plan(
                    input,
                    scene,
                    CloseSceneTarget::Contained(floating),
                    CloseActivation::Semantic,
                    policy,
                )
            }),
            (
                PresentationHitRegionKind::SplitterHandle(splitter),
                SemanticReceiverAction::Key(key),
            ) => {
                let Some(direction) = semantic_splitter_direction(projection.plan(), splitter, key)
                else {
                    return Ok(self.semantic_rejection(
                        InteractionRejection::SemanticActionUnsupported { target, action },
                    ));
                };
                self.reduce_splitter_adjustment_input(
                    input,
                    expected,
                    scene,
                    splitter,
                    self.presentation_authority
                        .presentation_config
                        .splitter_keyboard_step()
                        * direction,
                    policy,
                    events,
                    interaction_events,
                )
            }
            (
                PresentationHitRegionKind::SplitterHandle(splitter),
                SemanticReceiverAction::Accessibility(
                    SemanticAccessibilityAction::Increment | SemanticAccessibilityAction::Decrement,
                ),
            ) => {
                let direction = match action {
                    SemanticReceiverAction::Accessibility(
                        SemanticAccessibilityAction::Increment,
                    ) => 1.0,
                    SemanticReceiverAction::Accessibility(
                        SemanticAccessibilityAction::Decrement,
                    ) => -1.0,
                    _ => unreachable!("the match arm admits only increment and decrement"),
                };
                self.reduce_splitter_adjustment_input(
                    input,
                    expected,
                    scene,
                    splitter,
                    self.presentation_authority
                        .presentation_config
                        .splitter_keyboard_step()
                        * direction,
                    policy,
                    events,
                    interaction_events,
                )
            }
            _ => Ok(
                self.semantic_rejection(InteractionRejection::SemanticActionUnsupported {
                    target,
                    action,
                }),
            ),
        }
    }

    fn validate_semantic_receiver(
        event: SemanticReceiverEvent,
        semantic_presentations: Option<
            &std::collections::BTreeMap<
                crate::ids::SurfaceId,
                crate::journal_presentation::JournalSurfacePresentation,
            >,
        >,
    ) -> Result<crate::scene::SurfaceInteractionProjection<'_>, InteractionRejection> {
        let surface = event.output().surface();
        let projection = semantic_presentations
            .and_then(|presentations| presentations.get(&surface))
            .map(crate::journal_presentation::JournalSurfacePresentation::projection)
            .ok_or(InteractionRejection::SemanticPresentationUnavailable { surface })?;
        if projection.output_ticket() != event.output() {
            return Err(InteractionRejection::SemanticOutputUnavailable { surface });
        }
        if projection.authority().emission() != event.emission() {
            return Err(InteractionRejection::SemanticEmissionUnavailable { surface });
        }
        let actual = projection.authority().endpoint();
        let delivery_matches = match event.delivery() {
            crate::semantic_input::SemanticDelivery::Headless => {
                actual == crate::presentation_observation::HostPresentationEndpoint::Headless
            }
            crate::semantic_input::SemanticDelivery::Native(binding) => {
                actual == crate::presentation_observation::HostPresentationEndpoint::Native(binding)
            }
        };
        if !delivery_matches {
            return Err(InteractionRejection::SemanticDeliveryMismatch {
                expected: event.delivery(),
                actual,
            });
        }
        let retained = projection
            .hit_manifest()
            .regions()
            .iter()
            .any(|region| region.id().kind() == event.target());
        if !retained {
            return Err(InteractionRejection::SemanticReceiverUnavailable {
                target: event.target(),
            });
        }
        Ok(projection)
    }

    fn semantic_tab_destination(
        &self,
        plan: &crate::scene::PresentationPlan,
        current: crate::scene::TabSceneId,
        action: SemanticReceiverAction,
    ) -> Option<crate::scene::TabSceneId> {
        let bar = plan
            .tab_bar_records()
            .iter()
            .find(|bar| bar.id().root == current.root && bar.id().tabs == current.tabs)?;
        let members = bar.members();
        let selected = match self.workspace.node(current.tabs) {
            Some(crate::graph::Node::Tabs { selected, .. }) => *selected,
            _ => None,
        };
        let current_index = selected
            .and_then(|selected| {
                members
                    .iter()
                    .position(|member| member.tab().item == selected)
            })
            .or_else(|| members.iter().position(|member| member.tab() == current))?;
        let destination = match action {
            SemanticReceiverAction::Key(SemanticKey::ArrowLeft) => {
                current_index.checked_sub(1).unwrap_or(members.len() - 1)
            }
            SemanticReceiverAction::Key(SemanticKey::ArrowRight) => {
                (current_index + 1) % members.len()
            }
            SemanticReceiverAction::Key(SemanticKey::Home) => 0,
            SemanticReceiverAction::Key(SemanticKey::End) => members.len() - 1,
            _ => return None,
        };
        Some(members[destination].tab())
    }

    #[allow(clippy::too_many_arguments)]
    fn reduce_semantic_tab_selection(
        &mut self,
        input: crate::ids::InputSequence,
        expected: WorkspaceVersion,
        application_base: WorkspaceVersion,
        tab: crate::scene::TabSceneId,
        policy: &DockPolicySnapshot,
        events: &mut Vec<WorkspaceEvent>,
        interaction_events: &mut Vec<InteractionEvent>,
    ) -> Result<InputOutcome, EngineError> {
        let source = match self
            .workspace
            .capture_item_source(tab.root, tab.tabs, tab.item)
        {
            Ok(source) => source,
            Err(error) => {
                return Ok(self.semantic_rejection(InteractionRejection::CommandRejected(error)));
            }
        };
        let outcome = self.reduce_workspace_command(
            input,
            expected,
            application_base,
            &WorkspaceCommand::Select { source },
            policy,
            events,
            interaction_events,
        )?;
        if matches!(outcome, InputOutcome::CommandProcessed { .. }) {
            interaction_events.push(InteractionEvent::new(
                input,
                self.version,
                InteractionEventKind::SemanticFocusRequested { tab },
            ));
        }
        Ok(outcome)
    }

    fn semantic_rejection(&self, rejection: InteractionRejection) -> InputOutcome {
        InputOutcome::InteractionProcessed {
            outcome: InteractionOutcome::Rejected(rejection),
            version: self.version,
        }
    }
}

fn semantic_splitter_direction(
    plan: &crate::scene::PresentationPlan,
    splitter: crate::scene::SplitterSceneId,
    key: SemanticKey,
) -> Option<f64> {
    let axis = plan.splitter_record(splitter)?.axis();
    match (axis, key) {
        (crate::graph::Axis::Horizontal, SemanticKey::ArrowLeft)
        | (crate::graph::Axis::Vertical, SemanticKey::ArrowUp) => Some(-1.0),
        (crate::graph::Axis::Horizontal, SemanticKey::ArrowRight)
        | (crate::graph::Axis::Vertical, SemanticKey::ArrowDown) => Some(1.0),
        _ => None,
    }
}
