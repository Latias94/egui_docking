//! Presentation-bound keyboard and accessibility interaction reduction.

use crate::command::WorkspaceCommand;
use crate::event::{ReductionCause, WorkspaceEvent};
use crate::intent::{CloseActivation, CloseSceneTarget};
use crate::interaction::{
    InteractionEvent, InteractionEventKind, InteractionOutcome, InteractionRejection,
};
use crate::policy::DockPolicySnapshot;
use crate::presentation_hit::PresentationHitRegionKind;
use crate::semantic_input::{
    SemanticAccessibilityAction, SemanticKey, SemanticReceiverAction, SemanticReceiverEvent,
};
use crate::tab_strip::TabStripStateKey;
use crate::transition::{InputOutcome, WorkspaceVersion};
use crate::viewport_focus::FocusCausalStamp;

use super::{
    DockEngine, EngineError, PreparedTabListMenuNavigation, PreparedTabListMenuRowActivation,
    PreparedTabListMenuScroll, PreparedTabStripControlActivation, TabListMenuNavigation,
    TabScrollAdjustment,
};

impl DockEngine {
    #[allow(clippy::too_many_arguments)]
    pub(super) fn reduce_semantic_receiver_input(
        &mut self,
        input: crate::ids::InputSequence,
        cause: ReductionCause,
        focus_causal: FocusCausalStamp,
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
            (PresentationHitRegionKind::TabBody(tab), SemanticReceiverAction::Key(key)) => {
                let navigation = match key {
                    SemanticKey::ArrowLeft => Some(crate::tab_strip::TabNavigation::Previous),
                    SemanticKey::ArrowRight => Some(crate::tab_strip::TabNavigation::Next),
                    SemanticKey::Home => Some(crate::tab_strip::TabNavigation::First),
                    SemanticKey::End => Some(crate::tab_strip::TabNavigation::Last),
                    SemanticKey::ArrowUp
                    | SemanticKey::ArrowDown
                    | SemanticKey::Enter
                    | SemanticKey::Space => None,
                };
                if let Some(navigation) = navigation {
                    let destination = match crate::tab_strip::tab_navigation_destination(
                        projection.plan(),
                        tab,
                        match self.workspace.node(tab.tabs) {
                            Some(crate::graph::Node::Tabs { selected, .. }) => *selected,
                            _ => None,
                        },
                        navigation,
                    ) {
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
                } else if matches!(key, SemanticKey::Enter | SemanticKey::Space) {
                    self.reduce_semantic_tab_selection(
                        input,
                        expected,
                        application_base,
                        tab,
                        policy,
                        events,
                        interaction_events,
                    )
                } else {
                    Ok(
                        self.semantic_rejection(InteractionRejection::SemanticActionUnsupported {
                            target,
                            action,
                        }),
                    )
                }
            }
            (
                PresentationHitRegionKind::TabBody(tab),
                SemanticReceiverAction::Accessibility(
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
            (
                PresentationHitRegionKind::TabStripControl(control),
                SemanticReceiverAction::Key(SemanticKey::Enter | SemanticKey::Space)
                | SemanticReceiverAction::Accessibility(SemanticAccessibilityAction::Click),
            ) => match self.prepare_semantic_tab_strip_control(projection, control) {
                Ok(prepared) => self.reduce_prepared_tab_strip_control(cause, &prepared),
                Err(rejection) => Ok(self.semantic_rejection(rejection)),
            },
            (
                PresentationHitRegionKind::TabListMenuRow { menu, tab },
                SemanticReceiverAction::Key(SemanticKey::Enter | SemanticKey::Space)
                | SemanticReceiverAction::Accessibility(SemanticAccessibilityAction::Click),
            ) => match self.prepare_semantic_tab_list_menu_row(projection, menu, tab) {
                Ok(prepared) => self.reduce_prepared_tab_list_menu_row(
                    cause,
                    focus_causal,
                    &prepared,
                    policy,
                    events,
                ),
                Err(rejection) => Ok(self.semantic_rejection(rejection)),
            },
            (
                PresentationHitRegionKind::TabListMenuRow { menu, tab },
                SemanticReceiverAction::Accessibility(SemanticAccessibilityAction::Focus),
            ) => match self.prepare_semantic_tab_list_menu_navigation(
                projection,
                menu,
                TabListMenuNavigation::Focus(tab.item),
            ) {
                Ok(prepared) => self.reduce_prepared_tab_list_menu_navigation(cause, &prepared),
                Err(rejection) => Ok(self.semantic_rejection(rejection)),
            },
            (
                PresentationHitRegionKind::TabListMenuRow { menu, tab },
                SemanticReceiverAction::Accessibility(SemanticAccessibilityAction::ScrollIntoView),
            ) => match self.prepare_semantic_tab_list_menu_scroll(
                projection,
                menu,
                TabScrollAdjustment::reveal_item(tab.item),
            ) {
                Ok(prepared) => self.reduce_prepared_tab_list_menu_scroll(cause, &prepared),
                Err(rejection) => Ok(self.semantic_rejection(rejection)),
            },
            (
                PresentationHitRegionKind::TabListMenuRow { menu, .. },
                SemanticReceiverAction::Key(
                    key @ (SemanticKey::ArrowUp
                    | SemanticKey::ArrowDown
                    | SemanticKey::Home
                    | SemanticKey::End),
                ),
            ) => {
                let navigation = match key {
                    SemanticKey::ArrowUp => TabListMenuNavigation::Previous,
                    SemanticKey::ArrowDown => TabListMenuNavigation::Next,
                    SemanticKey::Home => TabListMenuNavigation::First,
                    SemanticKey::End => TabListMenuNavigation::Last,
                    _ => unreachable!("the match arm admits only menu navigation keys"),
                };
                match self.prepare_semantic_tab_list_menu_navigation(projection, menu, navigation) {
                    Ok(prepared) => self.reduce_prepared_tab_list_menu_navigation(cause, &prepared),
                    Err(rejection) => Ok(self.semantic_rejection(rejection)),
                }
            }
            (
                PresentationHitRegionKind::TabListMenuScroll(menu),
                SemanticReceiverAction::Accessibility(
                    action @ (SemanticAccessibilityAction::Increment
                    | SemanticAccessibilityAction::Decrement),
                ),
            ) => {
                let Some(step) = projection
                    .plan()
                    .tab_list_menu_records()
                    .iter()
                    .find(|record| record.session() == menu)
                    .and_then(|record| record.rows().first())
                    .map(|row| row.bounds().height())
                    .filter(|step| step.is_finite() && *step > 0.0)
                else {
                    return Ok(self.semantic_rejection(
                        InteractionRejection::SemanticActionUnsupported {
                            target,
                            action: SemanticReceiverAction::Accessibility(action),
                        },
                    ));
                };
                let direction = if action == SemanticAccessibilityAction::Increment {
                    1.0
                } else {
                    -1.0
                };
                let adjustment = TabScrollAdjustment::scroll_by(step * direction)
                    .expect("finite positive row height produces a valid scroll adjustment");
                match self.prepare_semantic_tab_list_menu_scroll(projection, menu, adjustment) {
                    Ok(prepared) => self.reduce_prepared_tab_list_menu_scroll(cause, &prepared),
                    Err(rejection) => Ok(self.semantic_rejection(rejection)),
                }
            }
            (
                PresentationHitRegionKind::ContainedResize {
                    floating,
                    direction,
                },
                SemanticReceiverAction::Key(
                    key @ (SemanticKey::ArrowLeft
                    | SemanticKey::ArrowRight
                    | SemanticKey::ArrowUp
                    | SemanticKey::ArrowDown),
                ),
            ) => self.reduce_semantic_contained_resize(
                input,
                expected,
                projection,
                floating,
                direction,
                semantic_direction(key),
                policy,
                events,
                interaction_events,
            ),
            (
                PresentationHitRegionKind::ContainedResize {
                    floating,
                    direction,
                },
                SemanticReceiverAction::Accessibility(
                    action @ (SemanticAccessibilityAction::Increment
                    | SemanticAccessibilityAction::Decrement),
                ),
            ) => self.reduce_semantic_contained_resize(
                input,
                expected,
                projection,
                floating,
                direction,
                if action == SemanticAccessibilityAction::Increment {
                    1.0
                } else {
                    -1.0
                },
                policy,
                events,
                interaction_events,
            ),
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
        let manifest = projection.semantic_manifest();
        if !manifest.contains(event.target()) {
            return Err(InteractionRejection::SemanticReceiverUnavailable {
                target: event.target(),
            });
        }
        if !manifest.supports(event.target(), event.action()) {
            return Err(InteractionRejection::SemanticActionUnsupported {
                target: event.target(),
                action: event.action(),
            });
        }
        Ok(projection)
    }

    pub(super) fn prepare_semantic_tab_strip_control(
        &self,
        projection: crate::scene::SurfaceInteractionProjection<'_>,
        control: crate::tab_strip::TabStripControlId,
    ) -> Result<PreparedTabStripControlActivation, InteractionRejection> {
        let key = TabStripStateKey::new(projection.output_ticket().surface(), control.bar());
        let plan = projection.plan();
        let record = plan
            .tab_strip_control_records()
            .iter()
            .copied()
            .find(|record| record.id() == control)
            .ok_or(InteractionRejection::TabStripControlUnavailable { control })?;
        if !record.enabled() {
            return Err(InteractionRejection::TabStripControlDisabled { control });
        }
        if self
            .presentation_authority
            .tab_strip_states
            .state(key)
            .is_none()
            || !plan
                .tab_bar_records()
                .iter()
                .any(|bar| *bar.id() == control.bar())
        {
            return Err(InteractionRejection::TabStripSourceUnavailable { key });
        }
        Ok(PreparedTabStripControlActivation {
            presentation: Self::freeze_interaction_projection(projection),
            control: crate::interaction::FrozenTabStripControlClick { key, record },
        })
    }

    pub(super) fn prepare_semantic_tab_list_menu_row(
        &self,
        projection: crate::scene::SurfaceInteractionProjection<'_>,
        session: crate::tab_strip::TabListMenuSessionId,
        tab: crate::scene::TabSceneId,
    ) -> Result<PreparedTabListMenuRowActivation, InteractionRejection> {
        let plan = projection.plan();
        let revision = plan.popup().revision();
        self.validate_tab_list_menu_popup(plan, session, revision)?;
        let record = plan
            .tab_list_menu_records()
            .iter()
            .find(|menu| menu.session() == session)
            .and_then(|menu| menu.rows().iter().copied().find(|row| row.tab() == tab))
            .ok_or(InteractionRejection::TabListMenuRowUnavailable { session, tab })?;
        if !self
            .presentation_authority
            .tab_strip_states
            .active_menu_for(session.key())
            .is_some_and(|active| active.session() == session && active.items().contains(&tab.item))
        {
            return Err(InteractionRejection::TabListMenuSessionUnavailable { session });
        }
        Ok(PreparedTabListMenuRowActivation {
            presentation: Self::freeze_interaction_projection(projection),
            row: crate::interaction::FrozenTabListMenuRowClick {
                session,
                record,
                revision,
            },
        })
    }

    pub(super) fn prepare_semantic_tab_list_menu_scroll(
        &self,
        projection: crate::scene::SurfaceInteractionProjection<'_>,
        session: crate::tab_strip::TabListMenuSessionId,
        adjustment: TabScrollAdjustment,
    ) -> Result<PreparedTabListMenuScroll, InteractionRejection> {
        let plan = projection.plan();
        let revision = plan.popup().revision();
        self.validate_tab_list_menu_popup(plan, session, revision)?;
        let record = plan
            .tab_list_menu_records()
            .iter()
            .find(|record| record.session() == session)
            .cloned()
            .ok_or(InteractionRejection::TabListMenuSessionUnavailable { session })?;
        let active = self
            .presentation_authority
            .tab_strip_states
            .active_menu_for(session.key())
            .filter(|active| active.session() == session)
            .ok_or(InteractionRejection::TabListMenuSessionUnavailable { session })?;
        let requested = match &adjustment.0 {
            super::TabScrollAdjustmentKind::ScrollByPreserving { keep_visible, .. } => {
                keep_visible.as_slice()
            }
            super::TabScrollAdjustmentKind::RevealItem(item) => std::slice::from_ref(item),
        };
        if let Some(item) = requested.iter().copied().find(|item| {
            !active.items().contains(item)
                || !record.rows().iter().any(|row| row.tab().item == *item)
        }) {
            return Err(InteractionRejection::TabListMenuScrollItemUnavailable { session, item });
        }
        Ok(PreparedTabListMenuScroll {
            presentation: Self::freeze_interaction_projection(projection),
            session,
            revision,
            record,
            adjustment,
        })
    }

    pub(super) fn prepare_semantic_tab_list_menu_navigation(
        &self,
        projection: crate::scene::SurfaceInteractionProjection<'_>,
        session: crate::tab_strip::TabListMenuSessionId,
        navigation: TabListMenuNavigation,
    ) -> Result<PreparedTabListMenuNavigation, InteractionRejection> {
        let plan = projection.plan();
        let revision = plan.popup().revision();
        self.validate_tab_list_menu_popup(plan, session, revision)?;
        let record = plan
            .tab_list_menu_records()
            .iter()
            .find(|record| record.session() == session)
            .cloned()
            .ok_or(InteractionRejection::TabListMenuSessionUnavailable { session })?;
        let active = self
            .presentation_authority
            .tab_strip_states
            .active_menu_for(session.key())
            .filter(|active| active.session() == session)
            .ok_or(InteractionRejection::TabListMenuSessionUnavailable { session })?;
        let current = active
            .items()
            .iter()
            .position(|item| *item == active.focus())
            .ok_or(InteractionRejection::TabListMenuFocusItemUnavailable {
                session,
                item: active.focus(),
            })?;
        let last = active.items().len().checked_sub(1).ok_or(
            InteractionRejection::TabListMenuFocusItemUnavailable {
                session,
                item: active.focus(),
            },
        )?;
        let target = match navigation {
            TabListMenuNavigation::Previous => active.items()[current.saturating_sub(1)],
            TabListMenuNavigation::Next => active.items()[current.saturating_add(1).min(last)],
            TabListMenuNavigation::First => active.items()[0],
            TabListMenuNavigation::Last => active.items()[last],
            TabListMenuNavigation::Focus(item) => item,
        };
        if !active.items().contains(&target)
            || !record.rows().iter().any(|row| row.tab().item == target)
        {
            return Err(InteractionRejection::TabListMenuFocusItemUnavailable {
                session,
                item: target,
            });
        }
        Ok(PreparedTabListMenuNavigation {
            presentation: Self::freeze_interaction_projection(projection),
            session,
            revision,
            record,
            target,
        })
    }

    #[allow(clippy::too_many_arguments)]
    fn reduce_semantic_contained_resize(
        &mut self,
        input: crate::ids::InputSequence,
        expected: WorkspaceVersion,
        projection: crate::scene::SurfaceInteractionProjection<'_>,
        floating: crate::ids::FloatingPresentationId,
        direction: crate::scene::ContainedResizeDirection,
        direction_sign: f64,
        policy: &DockPolicySnapshot,
        events: &mut Vec<WorkspaceEvent>,
        interaction_events: &mut Vec<InteractionEvent>,
    ) -> Result<InputOutcome, EngineError> {
        let delta = self
            .presentation_authority
            .presentation_config
            .splitter_keyboard_step()
            * direction_sign;
        let placement = match self.prepare_cardinal_contained_resize_placement(
            projection.plan_stamp(),
            projection.plan(),
            floating,
            direction,
            delta,
        ) {
            Ok(placement) => placement,
            Err(rejection) => return Ok(self.semantic_rejection(rejection)),
        };
        self.reduce_contained_placement_input(
            input,
            expected,
            placement,
            policy,
            events,
            interaction_events,
        )
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

const fn semantic_direction(key: SemanticKey) -> f64 {
    match key {
        SemanticKey::ArrowLeft | SemanticKey::ArrowUp => -1.0,
        SemanticKey::ArrowRight | SemanticKey::ArrowDown => 1.0,
        SemanticKey::Home | SemanticKey::End | SemanticKey::Enter | SemanticKey::Space => 0.0,
    }
}
