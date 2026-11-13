use super::*;

pub(crate) fn authorize_workspace_command(
    workspace: &Workspace,
    policy: &DockPolicySnapshot,
    command: &WorkspaceCommand,
) -> Result<(), CommandError> {
    if !matches!(
        command,
        WorkspaceCommand::Select { .. } | WorkspaceCommand::RaiseContained { .. }
    ) {
        policy.check_arrangement_unlocked()?;
    }
    match command {
        // Selection is a programmatic focus semantic, not a docking mutation.
        WorkspaceCommand::Select { .. } => Ok(()),
        WorkspaceCommand::Reorder {
            source,
            insertion_index,
        } => authorize_reorder(workspace, policy, source, *insertion_index),
        WorkspaceCommand::Open { item, target } => authorize_open(workspace, policy, *item, target),
        WorkspaceCommand::Move { payload, target } => {
            authorize_move(workspace, policy, payload, target)
        }
        WorkspaceCommand::ResizeSplits { splits } => {
            validate_split_resizes(workspace, policy, splits).map(|_| ())
        }
        WorkspaceCommand::CreateSurfaceRoot {
            surface, content, ..
        } => authorize_presentation(
            workspace,
            policy,
            presentation_payload_facts(workspace, content)?,
            DockPresentationTarget::Native(*surface),
        ),
        WorkspaceCommand::CreateContainedRoot {
            surface, content, ..
        } => authorize_presentation(
            workspace,
            policy,
            presentation_payload_facts(workspace, content)?,
            DockPresentationTarget::Contained(*surface),
        ),
        WorkspaceCommand::InstallMainRoot {
            surface, content, ..
        } => authorize_presentation(
            workspace,
            policy,
            presentation_payload_facts(workspace, content)?,
            DockPresentationTarget::Tiled(*surface),
        ),
        WorkspaceCommand::RehomeRoot { source, target } => {
            let (root, _) = validate_root_source(workspace, source)?;
            let current = workspace
                .presentation_for_root(root)
                .ok_or(CommandError::Invariant {
                    stage: "locate root presentation before rehome authorization",
                })?;
            if let RootPresentationTarget::Contained { surface, .. } = target
                && let RootPresentationOwner::Contained {
                    surface: source_surface,
                    ..
                } = current
                && source_surface == *surface
            {
                return authorize_contained_transform(
                    policy,
                    root_payload_policy_facts(workspace, root, false)?,
                    *surface,
                );
            }
            let changes_owner = !matches!(
                (current, *target),
                (
                    RootPresentationOwner::Main {
                        surface: current_surface,
                    },
                    RootPresentationTarget::Main {
                        surface: target_surface,
                    },
                ) if current_surface == target_surface
            );
            let target = match target {
                RootPresentationTarget::NewSurface { surface } => {
                    DockPresentationTarget::Native(*surface)
                }
                RootPresentationTarget::Main { surface } => DockPresentationTarget::Tiled(*surface),
                RootPresentationTarget::Contained { surface, .. } => {
                    DockPresentationTarget::Contained(*surface)
                }
            };
            authorize_presentation(
                workspace,
                policy,
                root_payload_policy_facts(workspace, root, changes_owner)?,
                target,
            )
        }
        WorkspaceCommand::PromoteContained {
            source, surface, ..
        } => {
            let (root, _) = validate_root_source(workspace, source)?;
            authorize_presentation(
                workspace,
                policy,
                root_payload_policy_facts(workspace, root, true)?,
                DockPresentationTarget::Tiled(*surface),
            )
        }
        WorkspaceCommand::UpdateContainedRect {
            surface,
            root,
            floating,
            ..
        } => {
            validate_contained(workspace, *surface, *root, *floating)?;
            authorize_contained_transform(
                policy,
                root_payload_policy_facts(workspace, *root, false)?,
                *surface,
            )
        }
        WorkspaceCommand::UpdateContainedPresentation {
            source,
            expected_roster,
            ..
        } => {
            let (root, _) = validate_root_source(workspace, source)?;
            authorize_contained_transform(
                policy,
                root_payload_policy_facts(workspace, root, false)?,
                expected_roster.surface(),
            )
        }
        WorkspaceCommand::RaiseContained {
            source,
            floating,
            expected_roster,
        } => {
            let (root, _) = validate_root_source(workspace, source)?;
            let owner = workspace
                .presentation_for_root(root)
                .ok_or(CommandError::Invariant {
                    stage: "locate contained presentation before authorize raise",
                })?;
            let RootPresentationOwner::Contained {
                surface,
                floating: actual_floating,
            } = owner
            else {
                return floating_presentation_mismatch(
                    workspace,
                    *floating,
                    root,
                    expected_roster.surface(),
                );
            };
            if actual_floating != *floating {
                return floating_presentation_mismatch(workspace, *floating, root, surface);
            }
            if expected_roster.surface() != surface {
                return Err(CommandError::ContainedRosterSurfaceMismatch {
                    captured_surface: expected_roster.surface(),
                    actual_surface: surface,
                });
            }
            validate_contained(workspace, surface, root, *floating)?;
            evaluate_policy(
                policy,
                &DockPolicyRequest::RaiseContained(DockContainedTransformPolicyRequest::new(
                    root_payload_policy_facts(workspace, root, false)?,
                    surface,
                )),
            )
        }
        // Empty container lifetime and application cleanup are lock-gated above.
        // Irreversible recovery uses its own authority.
        WorkspaceCommand::AttachEmptySurface { .. }
        | WorkspaceCommand::RetireEmptySurface { .. }
        | WorkspaceCommand::RemoveEmptyRoot { .. } => Ok(()),
    }
}

/// Read-only eligibility facts reused while one frozen drag evaluates many targets.
pub(crate) struct DropCommandEligibility<'a> {
    workspace: &'a Workspace,
    policy: &'a DockPolicySnapshot,
    payload: &'a MovePayload,
    retained_facts: DockPayloadPolicyFacts,
    undocked_facts: DockPayloadPolicyFacts,
    has_reorder_barriers: bool,
}

impl<'a> DropCommandEligibility<'a> {
    pub(crate) fn new_indexed(
        workspace: &'a Workspace,
        version: WorkspaceVersion,
        index: &WorkspaceIndex,
        policy: &'a DockPolicySnapshot,
        payload: &'a MovePayload,
    ) -> Result<Self, CommandError> {
        validate_move_payload_with(
            workspace,
            ReferenceAuthority::Indexed { version, index },
            payload,
        )?;
        Self::new_validated(workspace, policy, payload)
    }

    fn new_validated(
        workspace: &'a Workspace,
        policy: &'a DockPolicySnapshot,
        payload: &'a MovePayload,
    ) -> Result<Self, CommandError> {
        let retained_facts = move_payload_policy_facts_validated(workspace, payload, false)?;
        let source = retained_facts.source().ok_or(CommandError::Invariant {
            stage: "derive existing source facts for drop eligibility",
        })?;
        let undocked_facts = DockPayloadPolicyFacts::new(
            retained_facts.kind(),
            retained_facts.items().iter().copied(),
            source.root(),
            source.surface(),
            source.presentation(),
            true,
        );
        // All targets share one immutable source. Unrelated item rules must
        // not be rescanned for every insertion gap in its tab strip.
        let has_reorder_barriers = if policy.has_item_rules()
            && let MovePayload::Item(source) = payload
            && let Some(Node::Tabs { items, .. }) = workspace.storage.nodes.get(source.tabs())
        {
            items.iter().any(|item| {
                policy
                    .policy()
                    .item_rule(*item)
                    .is_some_and(|rule| !rule.allows_tab_reorder())
            })
        } else {
            false
        };
        Ok(Self {
            workspace,
            policy,
            payload,
            retained_facts,
            undocked_facts,
            has_reorder_barriers,
        })
    }

    pub(crate) fn check_target_indexed(
        &self,
        version: WorkspaceVersion,
        index: &WorkspaceIndex,
        target: &DockTarget,
    ) -> Result<(), CommandError> {
        self.check_target_with(ReferenceAuthority::Indexed { version, index }, target)
    }

    fn check_target_with(
        &self,
        references: ReferenceAuthority<'_>,
        target: &DockTarget,
    ) -> Result<(), CommandError> {
        validate_target_with(self.workspace, references, target)?;
        validate_move_target_semantics(self.workspace, self.payload, target)?;
        self.check_validated_target(target)
    }

    fn check_validated_target(&self, target: &DockTarget) -> Result<(), CommandError> {
        if self.has_reorder_barriers
            && let MovePayload::Item(source) = self.payload
            && let DockTarget::TabGap { target, index } = target
            && source.tabs() == target.tabs()
        {
            authorize_reorder_barriers(self.workspace, self.policy, source, *index)?;
        }
        let payload = if move_undocks_source(self.workspace, self.payload, target) {
            &self.undocked_facts
        } else {
            &self.retained_facts
        };
        authorize_move_target(self.policy, payload, target)
    }
}

fn authorize_reorder(
    workspace: &Workspace,
    policy: &DockPolicySnapshot,
    source: &ItemSource,
    insertion_index: usize,
) -> Result<(), CommandError> {
    validate_item_source(workspace, source)?;
    let len = tabs_len(workspace, source.tabs())?;
    if insertion_index > len {
        return Err(CommandError::TabIndexOutOfBounds {
            tabs: source.tabs(),
            index: insertion_index,
            len,
        });
    }
    let target = DockTarget::TabGap {
        target: workspace.capture_tab_target(source.root(), source.tabs())?,
        index: insertion_index,
    };
    authorize_tab_bar(policy, &target)?;
    if policy.has_item_rules() {
        authorize_reorder_barriers(workspace, policy, source, insertion_index)?;
    }
    let payload = move_payload_policy_facts(workspace, &MovePayload::Item(source.clone()), false)?;
    authorize_drop(policy, DockDropOperation::TabMerge, &payload, &target)
}

fn authorize_open(
    workspace: &Workspace,
    policy: &DockPolicySnapshot,
    item: ItemId,
    target: &DockTarget,
) -> Result<(), CommandError> {
    validate_target(workspace, target)?;
    if matches!(target, DockTarget::TabGap { .. }) {
        authorize_tab_bar(policy, target)?;
    }
    let payload = DockPayloadPolicyFacts::opened(DockPayloadKind::Item, [item]);
    authorize_drop(
        policy,
        if target_is_tabs(target) {
            DockDropOperation::TabMerge
        } else {
            DockDropOperation::EdgeSplit
        },
        &payload,
        target,
    )
}

fn authorize_move(
    workspace: &Workspace,
    policy: &DockPolicySnapshot,
    payload: &MovePayload,
    target: &DockTarget,
) -> Result<(), CommandError> {
    check_move(workspace, Some(policy), payload, target)
}

/// Derives current facts for one command only. The caller's immutable borrow
/// prevents edits until every source, target, semantic and policy check ends.
pub(super) fn check_move(
    workspace: &Workspace,
    policy: Option<&DockPolicySnapshot>,
    payload: &MovePayload,
    target: &DockTarget,
) -> Result<(), CommandError> {
    validate_move_payload(workspace, payload)?;
    validate_target(workspace, target)?;
    validate_move_target_semantics(workspace, payload, target)?;
    if let Some(policy) = policy {
        authorize_move_reorder(workspace, policy, payload, target)?;
        let facts = move_payload_policy_facts_validated(
            workspace,
            payload,
            move_undocks_source(workspace, payload, target),
        )?;
        authorize_move_target(policy, &facts, target)?;
    }
    Ok(())
}

fn authorize_move_reorder(
    workspace: &Workspace,
    policy: &DockPolicySnapshot,
    payload: &MovePayload,
    target: &DockTarget,
) -> Result<(), CommandError> {
    if let MovePayload::Item(source) = payload
        && let DockTarget::TabGap { target, index } = target
        && source.tabs() == target.tabs()
        && policy.has_item_rules()
    {
        authorize_reorder_barriers(workspace, policy, source, *index)?;
    }
    Ok(())
}

/// Restricts relative order inside the current group, independently of undocking.
fn authorize_reorder_barriers(
    workspace: &Workspace,
    policy: &DockPolicySnapshot,
    source: &ItemSource,
    insertion_index: usize,
) -> Result<(), CommandError> {
    let tabs = source.tabs();
    let Some(Node::Tabs { items, .. }) = workspace.storage.nodes.get(tabs) else {
        return Err(CommandError::NodeIsNotTabs { node: tabs });
    };
    let from = items.iter().position(|item| *item == source.item()).ok_or(
        CommandError::ItemNotInTabs {
            tabs,
            item: source.item(),
        },
    )?;
    let to = if insertion_index > from {
        insertion_index - 1
    } else {
        insertion_index
    };
    if from == to {
        return Ok(());
    }
    let crossed =
        items
            .get(from.min(to)..=from.max(to))
            .ok_or(CommandError::TabIndexOutOfBounds {
                tabs,
                index: insertion_index,
                len: items.len(),
            })?;
    if let Some(&item) = crossed.iter().find(|item| {
        policy
            .policy()
            .item_rule(**item)
            .is_some_and(|rule| !rule.allows_tab_reorder())
    }) {
        return Err(crate::policy::PolicyRejection::TabReorderDisabled { item }.into());
    }
    Ok(())
}

#[cfg(test)]
#[path = "authorization/tab_reorder_tests.rs"]
mod tab_reorder_tests;

pub(crate) fn authorize_captured_drag_source(
    workspace: &Workspace,
    policy: &DockPolicySnapshot,
    payload: &MovePayload,
) -> Result<(), CommandError> {
    let payload = move_payload_policy_facts_validated(workspace, payload, false)?;
    match policy.evaluate_drag_source_facts(&payload) {
        PolicyDecision::Allow => Ok(()),
        PolicyDecision::Reject(reason) => Err(reason.into()),
    }
}

fn authorize_tab_bar(policy: &DockPolicySnapshot, target: &DockTarget) -> Result<(), CommandError> {
    // Commands address semantic insertion points. Auto-hide does not disable
    // them: inserting a second item may itself make the bar visible, and the
    // transient reveal state belongs to presentation rather than the workspace.
    let declaration = policy.tab_bar_declaration(target.surface(), Some(target.rule()));
    if declaration.visibility() == crate::policy::TabBarVisibility::Hidden {
        return Err(crate::policy::PolicyRejection::TabBarHidden.into());
    }
    if declaration.interaction() == crate::policy::TabBarInteraction::Disabled {
        return Err(crate::policy::PolicyRejection::TabBarInteractionDisabled.into());
    }
    Ok(())
}

fn authorize_move_target(
    policy: &DockPolicySnapshot,
    payload: &DockPayloadPolicyFacts,
    target: &DockTarget,
) -> Result<(), CommandError> {
    let operation = if target_is_tabs(target) {
        DockDropOperation::TabMerge
    } else {
        DockDropOperation::EdgeSplit
    };
    if matches!(target, DockTarget::TabGap { .. }) {
        authorize_tab_bar(policy, target)?;
    }
    authorize_drop(policy, operation, payload, target)
}

fn authorize_drop(
    policy: &DockPolicySnapshot,
    operation: DockDropOperation,
    payload: &DockPayloadPolicyFacts,
    target: &DockTarget,
) -> Result<(), CommandError> {
    match policy.evaluate_drop_facts(
        operation,
        payload,
        DockDropTargetFacts::new(target.surface(), Some(target.rule()), target.is_central()),
    ) {
        PolicyDecision::Allow => Ok(()),
        PolicyDecision::Reject(reason) => Err(reason.into()),
    }
}

fn authorize_presentation(
    workspace: &Workspace,
    policy: &DockPolicySnapshot,
    payload: DockPayloadPolicyFacts,
    target: DockPresentationTarget,
) -> Result<(), CommandError> {
    if matches!(
        target,
        DockPresentationTarget::Tiled(_) | DockPresentationTarget::Contained(_)
    ) {
        let surface = target.surface();
        if workspace.surface(surface).is_none() {
            return Err(CommandError::MissingSurface { surface });
        }
    }
    evaluate_policy(
        policy,
        &DockPolicyRequest::Present(DockPresentationPolicyRequest::new(payload, target)),
    )
}

fn authorize_contained_transform(
    policy: &DockPolicySnapshot,
    payload: DockPayloadPolicyFacts,
    surface: SurfaceId,
) -> Result<(), CommandError> {
    match contained_transform_policy_decision(policy, payload, surface) {
        PolicyDecision::Allow => Ok(()),
        PolicyDecision::Reject(reason) => Err(reason.into()),
    }
}

pub(crate) fn contained_transform_operable(
    workspace: &Workspace,
    policy: &DockPolicySnapshot,
    surface: SurfaceId,
    root: RootId,
) -> Result<bool, CommandError> {
    let payload = root_payload_policy_facts(workspace, root, false)?;
    Ok(matches!(
        contained_transform_policy_decision(policy, payload, surface),
        PolicyDecision::Allow
    ))
}

fn contained_transform_policy_decision(
    policy: &DockPolicySnapshot,
    payload: DockPayloadPolicyFacts,
    surface: SurfaceId,
) -> PolicyDecision {
    policy.evaluate(&DockPolicyRequest::TransformContained(
        DockContainedTransformPolicyRequest::new(payload, surface),
    ))
}

pub(super) fn evaluate_policy(
    policy: &DockPolicySnapshot,
    request: &DockPolicyRequest,
) -> Result<(), CommandError> {
    match policy.evaluate(&request) {
        PolicyDecision::Allow => Ok(()),
        PolicyDecision::Reject(reason) => Err(reason.into()),
    }
}

fn presentation_payload_facts(
    workspace: &Workspace,
    content: &RootContent,
) -> Result<DockPayloadPolicyFacts, CommandError> {
    validate_root_content(workspace, content)?;
    match content {
        RootContent::OpenItem(item) => Ok(DockPayloadPolicyFacts::opened(
            DockPayloadKind::Item,
            [*item],
        )),
        RootContent::Move(payload) => move_payload_policy_facts_validated(workspace, payload, true),
    }
}

fn move_payload_policy_facts(
    workspace: &Workspace,
    payload: &MovePayload,
    undocks_source: bool,
) -> Result<DockPayloadPolicyFacts, CommandError> {
    validate_move_payload(workspace, payload)?;
    move_payload_policy_facts_validated(workspace, payload, undocks_source)
}

fn move_payload_policy_facts_validated(
    workspace: &Workspace,
    payload: &MovePayload,
    undocks_source: bool,
) -> Result<DockPayloadPolicyFacts, CommandError> {
    let source_root = payload_root(payload);
    let (source_surface, source_presentation) = root_policy_owner(workspace, source_root)?;
    let (kind, items) = match payload {
        MovePayload::Item(source) => (DockPayloadKind::Item, vec![source.item()]),
        MovePayload::Tabs(source) => (
            DockPayloadKind::TabGroup,
            workspace.collect_items_in_subtree(source.node()),
        ),
        MovePayload::Subtree(source) => (
            DockPayloadKind::Subtree,
            workspace.collect_items_in_subtree(source.node()),
        ),
    };
    Ok(DockPayloadPolicyFacts::new(
        kind,
        items,
        source_root,
        source_surface,
        source_presentation,
        undocks_source,
    ))
}

fn root_payload_policy_facts(
    workspace: &Workspace,
    root: RootId,
    undocks_source: bool,
) -> Result<DockPayloadPolicyFacts, CommandError> {
    let record = workspace
        .storage
        .roots
        .get(&root)
        .ok_or(CommandError::MissingRoot { root })?;
    let items = workspace.collect_items_in_subtree(record.node);
    let (surface, presentation) = root_policy_owner(workspace, root)?;
    Ok(DockPayloadPolicyFacts::new(
        DockPayloadKind::Root,
        items,
        root,
        surface,
        presentation,
        undocks_source,
    ))
}

fn root_policy_owner(
    workspace: &Workspace,
    root: RootId,
) -> Result<(SurfaceId, crate::policy::DockPresentationMode), CommandError> {
    match workspace.presentation_for_root(root) {
        Some(RootPresentationOwner::Main { surface }) => {
            Ok((surface, crate::policy::DockPresentationMode::Tiled))
        }
        Some(RootPresentationOwner::Contained { surface, .. }) => {
            Ok((surface, crate::policy::DockPresentationMode::Contained))
        }
        None => Err(CommandError::Invariant {
            stage: "derive policy presentation owner",
        }),
    }
}

fn move_undocks_source(workspace: &Workspace, payload: &MovePayload, target: &DockTarget) -> bool {
    match payload {
        MovePayload::Item(source) => match target {
            DockTarget::Center(target) | DockTarget::TabGap { target, .. } => {
                target.root() != source.root() || target.tabs() != source.tabs()
            }
            DockTarget::InnerEdge(target) | DockTarget::OuterEdge(target)
                if target.root() == source.root() =>
            {
                if target.node() != source.tabs() {
                    return true;
                }
                !matches!(
                    workspace.storage.nodes.get(source.tabs()),
                    Some(Node::Tabs { items, .. }) if items.len() == 1
                        && workspace.storage.roots
                            .get(&source.root())
                            .is_some_and(|root| root.central != Some(source.tabs()))
                )
            }
            DockTarget::InnerEdge(_) | DockTarget::OuterEdge(_) => true,
        },
        MovePayload::Tabs(source) | MovePayload::Subtree(source) => {
            !(target_root(target) == source.root()
                && target_node(target) == source.node()
                && target_is_tabs(target))
        }
    }
}

fn validate_move_target_semantics(
    workspace: &Workspace,
    payload: &MovePayload,
    target: &DockTarget,
) -> Result<(), CommandError> {
    if let Some(source_node) = payload_node(payload) {
        let target_node = target_node(target);
        if workspace.subtree_contains(source_node, target_node) {
            if source_node == target_node
                && target_is_tabs(target)
                && matches!(
                    workspace.storage.nodes.get(source_node),
                    Some(Node::Tabs { .. })
                )
            {
                return Ok(());
            }
            return Err(CommandError::TargetInsidePayload {
                source_node,
                target: target_node,
            });
        }
        if target_is_tabs(target)
            && !matches!(
                workspace.storage.nodes.get(source_node),
                Some(Node::Tabs { .. })
            )
        {
            return Err(CommandError::SplitPayloadIntoTabs { node: source_node });
        }

        let source_root = payload_root(payload);
        let source_record = workspace
            .storage
            .roots
            .get(&source_root)
            .ok_or(CommandError::MissingRoot { root: source_root })?;
        if source_record.node != source_node
            && let Some(central) = source_record.central
            && workspace.subtree_contains(source_node, central)
            && !moves_central_tab_contents(workspace, payload)
        {
            return Err(CommandError::CentralNodeWouldDetach {
                root: source_root,
                source_node,
                central,
            });
        }
    }

    validate_edge_insertion_weight(workspace, target)
}

fn validate_edge_insertion_weight(
    workspace: &Workspace,
    target: &DockTarget,
) -> Result<(), CommandError> {
    let target = match target {
        DockTarget::InnerEdge(target) | DockTarget::OuterEdge(target) => target,
        DockTarget::Center(_) | DockTarget::TabGap { .. } => return Ok(()),
    };
    let (axis, _) = edge_axis_and_order(target.edge());
    let fraction = target.fraction().get();
    let branch = workspace
        .parent_link(target.root(), target.node())
        .and_then(|parent| match workspace.storage.nodes.get(parent.parent) {
            Some(Node::Split {
                axis: parent_axis,
                weights,
                ..
            }) if *parent_axis == axis => weights.get(parent.index).copied().map(SplitWeight::get),
            Some(Node::Tabs { .. } | Node::Split { .. }) | None => None,
        });
    let (payload_weight, target_weight) = branch.map_or((fraction, 1.0 - fraction), |branch| {
        let payload = branch * fraction;
        (payload, branch - payload)
    });
    SplitWeight::new(payload_weight).map_err(|_| CommandError::InvalidEdgeWeight { axis })?;
    SplitWeight::new(target_weight).map_err(|_| CommandError::InvalidEdgeWeight { axis })?;
    Ok(())
}
