//! Local semantic actions retaining the exact scene that exposed their targets.

use super::*;

impl DockEngine {
    #[allow(clippy::too_many_arguments)]
    pub(super) fn reduce_deferred_surface_action(
        &mut self,
        input: InputSequence,
        cause: ReductionCause,
        focus_causal: FocusCausalStamp,
        expected: WorkspaceVersion,
        witness: &crate::scene::SurfacePlanScene,
        action: &LocalSurfaceAction,
        policy: &DockPolicySnapshot,
        events: &mut Vec<WorkspaceEvent>,
        interaction_events: &mut Vec<InteractionEvent>,
    ) -> Result<InputOutcome, EngineError> {
        if expected.epoch() != self.version.epoch() {
            return Ok(InputOutcome::StaleRejected {
                expected,
                accepted_base: self.version,
            });
        }
        // Return destinations and native recovery surfaces can lie outside
        // this surface's frozen plan. Keep their durable destination lookup
        // version-bound until a complete destination witness is available.
        let external_destination = match action {
            LocalSurfaceAction::PresentationCommand {
                action: ProductAction::DockBackRoot { .. },
                ..
            } => true,
            LocalSurfaceAction::PresentationCommand {
                action: ProductAction::FloatRoot { surface, .. },
                ..
            } => *surface != witness.stamp().surface(),
            _ => false,
        };
        if external_destination && expected != self.version {
            return Ok(InputOutcome::StaleRejected {
                expected,
                accepted_base: self.version,
            });
        }
        if self
            .validate_deferred_surface_action(witness, action, policy)
            .is_err()
        {
            return Ok(InputOutcome::StaleRejected {
                expected,
                accepted_base: self.version,
            });
        }

        let outcome = match action {
            LocalSurfaceAction::SelectTab { tab, focus, .. } => self.select_tab_from_candidate(
                input,
                focus_causal,
                witness,
                *tab,
                *focus,
                policy,
                events,
                interaction_events,
            ),
            LocalSurfaceAction::Close { target, .. } => {
                self.close_from_candidate(input, witness, *target, policy)
            }
            LocalSurfaceAction::CloseRoot { root, .. } => {
                self.close_root_from_candidate(input, witness, *root, policy)
            }
            LocalSurfaceAction::PresentationCommand { action, .. } => self
                .reduce_presentation_command_from_candidate(
                    input,
                    cause,
                    focus_causal,
                    witness,
                    *action,
                    policy,
                    events,
                    interaction_events,
                ),
            LocalSurfaceAction::TabChrome { action, .. } => self.apply_tab_chrome_from_candidate(
                cause,
                focus_causal,
                witness,
                action,
                policy,
                events,
            ),
            LocalSurfaceAction::AdjustSplitter {
                splitter, delta, ..
            } => {
                let outcome = self.adjust_splitter_from_candidate(
                    input,
                    witness,
                    *splitter,
                    *delta,
                    policy,
                    events,
                    interaction_events,
                )?;
                Ok(InputOutcome::InteractionProcessed {
                    outcome,
                    version: self.version,
                })
            }
            LocalSurfaceAction::AdjustSplitterJunction {
                junction,
                axis,
                delta,
                ..
            } => {
                let outcome = self.adjust_splitter_junction_from_candidate(
                    input,
                    witness,
                    *junction,
                    *axis,
                    *delta,
                    policy,
                    events,
                    interaction_events,
                )?;
                Ok(InputOutcome::InteractionProcessed {
                    outcome,
                    version: self.version,
                })
            }
            LocalSurfaceAction::AdjustContainedResize {
                floating,
                direction,
                delta,
                ..
            } => self.resize_contained_from_candidate(
                input,
                witness,
                *floating,
                *direction,
                *delta,
                policy,
                events,
                interaction_events,
            ),
            LocalSurfaceAction::CancelWithEscape { .. } => self.reduce_escape_input(
                input,
                self.version,
                EscapeDelivery::Surface(witness.stamp().surface()),
                interaction_events,
            ),
        }?;
        Ok(match outcome {
            InputOutcome::CommandProcessed {
                changed, version, ..
            } if matches!(action, LocalSurfaceAction::SelectTab { .. }) => {
                let LocalSurfaceAction::SelectTab { tab, .. } = action else {
                    unreachable!("the guard selects the same action variant");
                };
                InputOutcome::ProductActionProcessed {
                    outcome: crate::model::DockspaceActionOutcome::Selected {
                        item: tab.item,
                        changed,
                    },
                    version,
                }
            }
            InputOutcome::InteractionProcessed {
                outcome: InteractionOutcome::Rejected(_),
                ..
            }
            | InputOutcome::CommandRejected { .. } => InputOutcome::StaleRejected {
                expected,
                accepted_base: self.version,
            },
            other => other,
        })
    }

    fn validate_deferred_surface_action(
        &self,
        witness: &crate::scene::SurfacePlanScene,
        action: &LocalSurfaceAction,
        policy: &DockPolicySnapshot,
    ) -> Result<(), InteractionRejection> {
        let stamp = witness.stamp();
        let ticket = stamp.requirement();
        let surface = stamp.surface();
        let plan = witness.plan();
        if ticket.authority_domain() != self.authority_domain
            || ticket.workspace_epoch() != self.version.epoch()
            || ticket.policy() != policy.revision()
            || ticket.config() != self.presentation_authority.presentation_config_revision
            || plan.measurement_ticket() != Some(ticket)
            || plan.surface() != surface
            || action.scene().is_some_and(|scene| scene != stamp)
        {
            return Err(InteractionRejection::StaleScene);
        }
        if self.workspace.surface(surface).is_none()
            || self
                .presentation_authority
                .presentation_requirements
                .surface(surface)
                .is_none()
            || plan.popup()
                != self
                    .presentation_authority
                    .tab_strip_states
                    .popup_requirement()
            || !self.presented_layering_is_current(plan)
            || !self.presented_transient_geometry_is_current(plan)
        {
            return Err(InteractionRejection::TargetAuthorityInvalid);
        }
        if !Self::coordinate_capture_matches_current(
            witness.coordinate_capture(),
            self.viewport.viewport(surface),
            self.viewport.surface_coordinate_authority(surface),
        ) {
            return Err(InteractionRejection::TargetAuthorityInvalid);
        }
        // A sibling floating can move over a frozen receiver without changing
        // z-order. Its current rectangle is part of the receiver's authority.
        if !plan.contained_records().iter().all(|record| {
            self.workspace.presentation_for_root(record.root())
                == Some(crate::RootPresentationOwner::Contained {
                    surface,
                    floating: record.floating(),
                })
                && plan.unclipped_contained_bounds(record.floating())
                    == self
                        .contained_layout_projection(surface)
                        .filter(|projection| projection.floating == record.floating())
                        .map(|projection| projection.rect)
                        .or_else(|| {
                            self.workspace
                                .contained_floating(record.floating())
                                .map(|current| current.rect)
                        })
        }) {
            return Err(InteractionRejection::TargetAuthorityInvalid);
        }

        let root_is_current = |root| self.presented_root_structure_is_current(plan, root);
        let item_receiver_is_current = |kind| {
            witness
                .hit_manifest()
                .region_for_kind(kind)
                .is_some_and(|region| self.presented_receiver_is_current(plan, region.id()))
        };
        let current = match action {
            LocalSurfaceAction::SelectTab { tab, .. } => {
                item_receiver_is_current(PresentationHitRegionKind::TabBody(*tab))
            }
            LocalSurfaceAction::Close {
                target: CloseSceneTarget::Tab(tab),
                ..
            } => item_receiver_is_current(PresentationHitRegionKind::TabClose(*tab)),
            LocalSurfaceAction::Close {
                target: CloseSceneTarget::Contained(floating),
                ..
            }
            | LocalSurfaceAction::AdjustContainedResize { floating, .. } => plan
                .contained_record(*floating)
                .is_some_and(|record| root_is_current(record.root())),
            LocalSurfaceAction::CloseRoot { root, .. } => root_is_current(*root),
            LocalSurfaceAction::PresentationCommand { action, .. } => {
                product_action::product_presentation_transition_root(*action)
                    .is_some_and(root_is_current)
                    && self.deferred_presentation_destination_is_current(plan, *action)
            }
            LocalSurfaceAction::TabChrome { action, .. } => {
                self.deferred_tab_chrome_is_current(plan, action)
            }
            LocalSurfaceAction::AdjustSplitter { splitter, .. } => {
                root_is_current(splitter.root)
                    && self.deferred_splitter_geometry_is_current(witness)
            }
            LocalSurfaceAction::AdjustSplitterJunction { junction, .. } => {
                junction
                    .splitters()
                    .iter()
                    .all(|splitter| root_is_current(splitter.root))
                    && self.deferred_splitter_geometry_is_current(witness)
            }
            LocalSurfaceAction::CancelWithEscape { status } => {
                *status == self.interaction.status() && *status != InteractionStatus::Idle
            }
        };
        current
            .then_some(())
            .ok_or(InteractionRejection::TargetAuthorityInvalid)
    }

    fn deferred_splitter_geometry_is_current(
        &self,
        witness: &crate::scene::SurfacePlanScene,
    ) -> bool {
        let Some(current) = self.scene().surface(witness.stamp().surface()) else {
            return false;
        };
        if current.stamp().requirement() != witness.stamp().requirement() {
            return false;
        }
        match current {
            SurfaceScene::Ready(ready) => ready
                .plan()
                .layout_facts()
                .zip(witness.plan().layout_facts())
                .is_some_and(|(current, frozen)| current.measurements() == frozen.measurements()),
            // A discarded UI pass can supply the first exact geometry for the
            // current requirements. An explicit failed observation cannot.
            SurfaceScene::Bootstrap(scene) => matches!(
                scene.reason(),
                crate::scene::BootstrapSurfaceSceneReason::AwaitingContribution
                    | crate::scene::BootstrapSurfaceSceneReason::RequirementsChanged
                    | crate::scene::BootstrapSurfaceSceneReason::WorkspaceReplaced
            ),
            SurfaceScene::Stale(scene) => matches!(
                scene.reason(),
                crate::scene::StaleSurfaceSceneReason::RequirementsChanged
            ),
        }
    }

    fn deferred_presentation_destination_is_current(
        &self,
        plan: &PresentationPlan,
        action: ProductAction,
    ) -> bool {
        use crate::model::{DockAnchor, DockPlacement};
        let ProductAction::DockRoot { placement, .. } = action else {
            return true;
        };
        let anchor = match placement {
            DockPlacement::Center(anchor) | DockPlacement::InnerEdge { anchor, .. } => anchor,
            DockPlacement::Before(item) | DockPlacement::After(item) => DockAnchor::Item(item),
            DockPlacement::OuterEdge { root, .. } => DockAnchor::Central(root),
            DockPlacement::Main(surface) => {
                return surface == plan.surface()
                    && self
                        .workspace
                        .surface(surface)
                        .is_some_and(|current| current.main_root.is_none())
                    && plan.layout_facts().is_some_and(|facts| {
                        facts.roots().all(|root| {
                            plan.contained_records()
                                .iter()
                                .any(|record| record.root() == root.root())
                        })
                    });
            }
        };
        let root = match anchor {
            DockAnchor::Central(root) => root,
            DockAnchor::Item(item) => {
                let Some(tab) = plan
                    .tab_records()
                    .iter()
                    .find(|record| record.id().item == item)
                else {
                    return false;
                };
                let tab = tab.id();
                if !self
                    .workspace
                    .locate_item_by_id(item)
                    .is_some_and(|(root, tabs, _)| root == tab.root && tabs == tab.tabs)
                {
                    return false;
                }
                tab.root
            }
        };
        self.presented_root_structure_is_current(plan, root)
    }

    fn deferred_tab_chrome_is_current(
        &self,
        plan: &PresentationPlan,
        action: &LocalTabChromeAction,
    ) -> bool {
        if let LocalTabChromeAction::DismissMenuFromBackdrop(backdrop) = action {
            return self
                .validate_tab_list_menu_popup(plan, backdrop.session, backdrop.revision)
                .is_ok()
                && backdrop.record.session() == backdrop.session
                && backdrop.record.revision() == backdrop.revision
                && plan
                    .tab_list_menu_backdrop_records()
                    .contains(&backdrop.record);
        }
        let key = match action {
            LocalTabChromeAction::ActivateControl(control) => control.key,
            LocalTabChromeAction::ScrollStrip { key, .. } => *key,
            LocalTabChromeAction::ActivateMenuRow(row) => row.session.key(),
            LocalTabChromeAction::DismissMenu { session, .. }
            | LocalTabChromeAction::ScrollMenu { session, .. }
            | LocalTabChromeAction::NavigateMenu { session, .. } => session.key(),
            LocalTabChromeAction::DismissMenuFromBackdrop(backdrop) => backdrop.session.key(),
        };
        let owner_is_current = match self.workspace.presentation_for_root(key.bar().root) {
            Some(
                crate::RootPresentationOwner::Main { surface }
                | crate::RootPresentationOwner::Contained { surface, .. },
            ) => surface == key.surface(),
            None => false,
        };
        if key.surface() != plan.surface() || !owner_is_current {
            return false;
        }
        let Some(bar) = plan
            .tab_bar_records()
            .iter()
            .find(|record| *record.id() == key.bar())
        else {
            return false;
        };
        if !matches!(self.workspace.node(key.bar().tabs), Some(Node::Tabs { items, .. })
            if items.iter().copied().eq(bar.members().iter().map(|member| member.tab().item)))
        {
            return false;
        }
        match action {
            LocalTabChromeAction::ActivateControl(_) | LocalTabChromeAction::ScrollStrip { .. } => {
                self.presentation_authority
                    .tab_strip_states
                    .state(key)
                    .is_some_and(|state| {
                        state
                            .scroll_offset()
                            .is_none_or(|offset| offset == bar.scroll_offset())
                    })
            }
            _ => self
                .presentation_authority
                .tab_strip_states
                .active_menu_for(key)
                .is_some_and(|active| {
                    plan.tab_list_menu_records()
                        .iter()
                        .find(|record| record.session() == active.session())
                        .is_some_and(|record| {
                            record.scroll_offset() == active.scroll_offset()
                                && record
                                    .rows()
                                    .iter()
                                    .find(|row| row.focused())
                                    .map(|row| row.tab().item)
                                    == Some(active.focus())
                                && record
                                    .rows()
                                    .iter()
                                    .map(|row| row.tab().item)
                                    .eq(active.items().iter().copied())
                        })
                }),
        }
    }

    fn close_from_candidate(
        &mut self,
        input: InputSequence,
        candidate: &crate::scene::SurfacePlanScene,
        target: CloseSceneTarget,
        policy: &DockPolicySnapshot,
    ) -> Result<InputOutcome, EngineError> {
        let plan = candidate.plan();
        let captured = match target {
            CloseSceneTarget::Tab(tab) => plan
                .tab_records()
                .iter()
                .find(|record| *record.id() == tab)
                .and_then(|record| {
                    record
                        .close_bounds()
                        .map(|bounds| (bounds, record.layer(), ContentCloseTarget::Item(tab.item)))
                }),
            CloseSceneTarget::Contained(floating) => {
                plan.contained_record(floating).and_then(|record| {
                    record.close_bounds().map(|bounds| {
                        (
                            bounds,
                            record.layer(),
                            ContentCloseTarget::Root(record.root()),
                        )
                    })
                })
            }
        };
        let Some((bounds, layer, content)) = captured else {
            return Ok(self.candidate_action_rejection(
                InteractionRejection::CloseControlUnavailable { target },
            ));
        };
        if !plan.region_is_operable(bounds, layer) {
            return Ok(self.candidate_action_rejection(
                InteractionRejection::CloseActivationOccluded { target },
            ));
        }
        self.request_deferred_content_close(input, content, policy)
    }

    fn close_root_from_candidate(
        &mut self,
        input: InputSequence,
        candidate: &crate::scene::SurfacePlanScene,
        root: RootId,
        policy: &DockPolicySnapshot,
    ) -> Result<InputOutcome, EngineError> {
        let plan = candidate.plan();
        if !plan
            .presentation_menu_anchor_records()
            .iter()
            .any(|anchor| {
                anchor.root() == root
                    && anchor
                        .ready_bounds()
                        .is_some_and(|bounds| plan.region_is_operable(bounds, anchor.layer()))
            })
        {
            return Ok(InputOutcome::ContentCloseRejected {
                target: ContentCloseTarget::Root(root),
                reason: ContentCloseRequestRejection::RootUnavailable { root },
                version: self.version,
            });
        }
        self.request_deferred_content_close(input, ContentCloseTarget::Root(root), policy)
    }

    fn request_deferred_content_close(
        &mut self,
        input: InputSequence,
        target: ContentCloseTarget,
        policy: &DockPolicySnapshot,
    ) -> Result<InputOutcome, EngineError> {
        let outcome = self.reduce_content_close_request(input, self.version, target, policy)?;
        Ok(match outcome {
            InputOutcome::ContentCloseRequested {
                plan,
                reused,
                version,
                ..
            } => InputOutcome::InteractionProcessed {
                outcome: InteractionOutcome::CloseRequested { plan, reused },
                version,
            },
            other => other,
        })
    }
}
