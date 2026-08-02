//! Internal checked command execution against isolated workspace candidates.

use std::collections::{BTreeMap, BTreeSet};

use crate::RootPresentationOwner;
use crate::canonical::canonicalize_workspace;
use crate::close_plan::CloseItemRequirement;
use crate::command::{
    CloseCommitOutcome, CommandOutcome, ContainedPosition, ContainedRosterSource, DockTarget, Edge,
    EdgeTarget, EdgeTargetScope, ItemSource, MovePayload, NodeSource, RootContent,
    RootPresentationTarget, SplitResize, SurfaceRosterSource, TabTarget, WorkspaceCommand,
};
use crate::error::{CommandError, ReferenceRole, TransactionError, TransactionPreconditionError};
use crate::graph::{
    Axis, ContainedFloating, NORMALIZED_WEIGHT_TOLERANCE, Node, RootRecord, SplitWeight,
    SurfacePresentation, Workspace,
};
use crate::ids::{FloatingPresentationId, ItemId, NodeId, RootId, SurfaceId};
use crate::policy::{
    DockContainedTransformPolicyRequest, DockDropOperation, DockDropTargetFacts, DockPayloadKind,
    DockPayloadPolicyFacts, DockPolicyRequest, DockPolicySnapshot, DockPresentationPolicyRequest,
    DockPresentationTarget, DockResizePolicyRequest, DockTabBarPolicyRequest, PolicyDecision,
};
use crate::surface_recovery::SurfaceRecoveryTransaction;
use crate::transaction::PreparedTransaction;
use crate::transition::WorkspaceVersion;
use crate::workspace::WorkspaceIndex;

/// Core-private frozen payload for an approved content close.
///
/// Unlike [`WorkspaceCommand`], this type cannot be constructed by callers and
/// always carries the complete item multiset and policy capabilities captured
/// when the close plan was opened.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum PreparedContentClose {
    Item {
        source: ItemSource,
        expected_items: BTreeMap<ItemId, usize>,
        requirement: CloseItemRequirement,
    },
    Root {
        source: NodeSource,
        items: Vec<ItemId>,
        expected_items: BTreeMap<ItemId, usize>,
        requirements: Vec<CloseItemRequirement>,
    },
}

/// Core-private frozen payload for closing one complete surface's content.
///
/// Root entries are ordered as the optional main root followed by contained
/// roots in normative back-to-front roster order. Each entry carries the exact
/// root source and its deterministic subtree item order.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct PreparedSurfaceContentClose {
    roster: SurfaceRosterSource,
    roots: Vec<(NodeSource, Vec<ItemId>)>,
    items: Vec<ItemId>,
    requirements: Vec<CloseItemRequirement>,
}

impl PreparedSurfaceContentClose {
    #[must_use]
    pub(crate) fn new(
        roster: SurfaceRosterSource,
        roots: Vec<(NodeSource, Vec<ItemId>)>,
        requirements: Vec<CloseItemRequirement>,
    ) -> Self {
        let items = roots
            .iter()
            .flat_map(|(_, items)| items.iter().copied())
            .collect();
        Self {
            roster,
            roots,
            items,
            requirements,
        }
    }

    pub(crate) const fn roster(&self) -> &SurfaceRosterSource {
        &self.roster
    }

    pub(crate) const fn surface(&self) -> SurfaceId {
        self.roster.surface()
    }

    pub(crate) fn roots(&self) -> &[(NodeSource, Vec<ItemId>)] {
        &self.roots
    }

    pub(crate) fn items(&self) -> &[ItemId] {
        &self.items
    }

    pub(crate) fn requirements(&self) -> &[CloseItemRequirement] {
        &self.requirements
    }
}

#[derive(Debug)]
pub(crate) struct PreparedContentCloseCommit {
    pub(crate) candidate: Workspace,
    pub(crate) outcome: CloseCommitOutcome,
}

/// Prepares one approved complete-surface content close against an isolated workspace candidate.
///
/// No topology is removed until every frozen roster, root, item, and policy fact
/// has been revalidated against the same candidate.
pub(crate) fn prepare_surface_content_close(
    workspace: &Workspace,
    policy: &DockPolicySnapshot,
    prepared: &PreparedSurfaceContentClose,
) -> Result<PreparedContentCloseCommit, TransactionError> {
    let mut candidate = workspace.clone();
    if !candidate.matches_surface_roster_source(prepared.roster()) {
        return Err(TransactionError::Precondition {
            index: 0,
            source: TransactionPreconditionError::StaleSurfaceRoster {
                surface: prepared.surface(),
            },
        });
    }
    evaluate_policy(
        policy,
        &DockPolicyRequest::CloseSurface {
            surface: prepared.surface(),
        },
    )
    .map_err(close_command_error)?;

    let roster_sources = prepared
        .roster()
        .main_source()
        .into_iter()
        .chain(
            prepared
                .roster()
                .contained()
                .iter()
                .map(crate::command::ContainedRootSource::source),
        )
        .collect::<Vec<_>>();
    if roster_sources.len() != prepared.roots().len()
        || roster_sources
            .iter()
            .zip(prepared.roots())
            .any(|(roster, (frozen, _))| *roster != frozen)
    {
        return Err(close_invariant("validate frozen close-surface root order"));
    }

    let roster_items = prepared
        .roots()
        .iter()
        .flat_map(|(_, items)| items.iter().copied())
        .collect::<Vec<_>>();
    if roster_items.as_slice() != prepared.items() || prepared.items().is_empty() {
        return Err(close_invariant("validate frozen close-surface item order"));
    }

    let requirement_items = prepared
        .requirements()
        .iter()
        .map(|requirement| requirement.item())
        .collect::<Vec<_>>();
    if requirement_items.as_slice() != prepared.items() {
        return Err(close_invariant(
            "validate frozen close-surface item requirements",
        ));
    }

    let mut frozen_roots = BTreeSet::new();
    let mut frozen_items = BTreeSet::new();
    for (source, items) in prepared.roots() {
        if !frozen_roots.insert(source.root()) {
            return Err(close_invariant(
                "validate duplicate frozen close-surface root",
            ));
        }
        for item in items {
            if !frozen_items.insert(*item) {
                return Err(close_invariant(
                    "validate duplicate frozen close-surface item",
                ));
            }
        }
    }
    for requirement in prepared.requirements() {
        validate_close_requirement(policy, *requirement)?;
    }
    for (source, frozen_items) in prepared.roots() {
        let (_, root_node) =
            validate_root_source(&candidate, source).map_err(close_command_error)?;
        if candidate.collect_items_in_subtree(root_node) != *frozen_items {
            return Err(close_invariant(
                "validate frozen close-surface root item order",
            ));
        }
    }

    // The baseline is deliberately captured at apply time. A close plan owns
    // only its frozen source roster; unrelated surfaces may change while a
    // native close confirmation is pending.
    let before_items = candidate.item_multiset();
    for item in prepared.items() {
        if before_items.get(item) != Some(&1) {
            return Err(close_invariant(
                "validate unique frozen close-surface item ownership",
            ));
        }
    }
    let mut expected_after = before_items;
    for item in prepared.items() {
        subtract_closed_item(&mut expected_after, *item)?;
    }
    for (source, _) in prepared.roots() {
        let record = remove_root_and_presentation(&mut candidate, source.root())
            .map_err(close_command_error)?;
        remove_subtree_nodes(&mut candidate, record.node).map_err(close_command_error)?;
    }

    prune_vacant_surfaces(&mut candidate);
    canonicalize_workspace(&mut candidate).map_err(TransactionError::from)?;
    candidate.validate().map_err(TransactionError::from)?;
    let actual_after = candidate.item_multiset();
    if actual_after != expected_after {
        return Err(TransactionError::ItemReconciliation {
            command_index: None,
            expected: expected_after,
            actual: actual_after,
        });
    }

    Ok(PreparedContentCloseCommit {
        candidate,
        outcome: CloseCommitOutcome::SurfaceClosed {
            surface: prepared.surface(),
            items: prepared.items().to_vec(),
        },
    })
}

/// Prepare one approved close against a fresh candidate workspace.
///
/// This is intentionally separate from the public command transaction. A
/// close changes the item multiset and is therefore only reachable through a
/// ClosePlan-approved payload.
pub(crate) fn prepare_content_close(
    workspace: &Workspace,
    policy: &DockPolicySnapshot,
    prepared: &PreparedContentClose,
) -> Result<PreparedContentCloseCommit, TransactionError> {
    let expected_items = match prepared {
        PreparedContentClose::Item { expected_items, .. }
        | PreparedContentClose::Root { expected_items, .. } => expected_items,
    };
    let actual_items = workspace.item_multiset();
    if &actual_items != expected_items {
        return Err(TransactionError::ItemReconciliation {
            command_index: None,
            expected: expected_items.clone(),
            actual: actual_items,
        });
    }

    let mut candidate = workspace.clone();
    let outcome = match prepared {
        PreparedContentClose::Item {
            source,
            requirement,
            ..
        } => {
            validate_close_requirement(policy, *requirement)?;
            validate_item_source(&candidate, source).map_err(close_command_error)?;
            let root = source.root();
            let item = source.item();
            remove_item(&mut candidate, source.tabs(), item).map_err(close_command_error)?;
            cleanup_item_source_root(&mut candidate, root).map_err(close_command_error)?;
            CloseCommitOutcome::ItemClosed { item, root }
        }
        PreparedContentClose::Root {
            source,
            items,
            requirements,
            ..
        } => {
            for requirement in requirements {
                validate_close_requirement(policy, *requirement)?;
            }
            let (root, root_node) =
                validate_root_source(&candidate, source).map_err(close_command_error)?;
            let current_items = candidate.collect_items_in_subtree(root_node);
            if current_items != *items {
                return Err(TransactionError::Command {
                    index: 0,
                    source: CommandError::Invariant {
                        stage: "validate frozen close-root item order",
                    },
                });
            }
            if items.is_empty() {
                return Err(TransactionError::Command {
                    index: 0,
                    source: CommandError::RootEmpty { root },
                });
            }
            let record =
                remove_root_and_presentation(&mut candidate, root).map_err(close_command_error)?;
            remove_subtree_nodes(&mut candidate, record.node).map_err(close_command_error)?;
            CloseCommitOutcome::RootClosed {
                root,
                items: items.clone(),
            }
        }
    };

    let mut expected_after = expected_items.clone();
    match &outcome {
        CloseCommitOutcome::ItemClosed { item, .. } => {
            expected_after.remove(item);
        }
        CloseCommitOutcome::RootClosed { items, .. } => {
            for item in items {
                if expected_after.remove(item).is_none() {
                    return Err(TransactionError::Command {
                        index: 0,
                        source: CommandError::Invariant {
                            stage: "reconcile frozen close-root item multiset",
                        },
                    });
                }
            }
        }
        CloseCommitOutcome::SurfaceClosed { .. } => {
            return Err(close_invariant(
                "reject surface outcome from single-content close",
            ));
        }
    }
    prune_vacant_surfaces(&mut candidate);
    canonicalize_workspace(&mut candidate).map_err(TransactionError::from)?;
    candidate.validate().map_err(TransactionError::from)?;
    let actual_after = candidate.item_multiset();
    if actual_after != expected_after {
        return Err(TransactionError::ItemReconciliation {
            command_index: None,
            expected: expected_after,
            actual: actual_after,
        });
    }
    Ok(PreparedContentCloseCommit { candidate, outcome })
}

fn close_command_error(error: CommandError) -> TransactionError {
    TransactionError::Command {
        index: 0,
        source: error,
    }
}

fn close_invariant(stage: &'static str) -> TransactionError {
    close_command_error(CommandError::Invariant { stage })
}

fn subtract_closed_item(
    items: &mut BTreeMap<ItemId, usize>,
    item: ItemId,
) -> Result<(), TransactionError> {
    let Some(count) = items.get_mut(&item) else {
        return Err(close_invariant(
            "reconcile frozen close-surface item multiset",
        ));
    };
    if *count == 0 {
        return Err(close_invariant(
            "reconcile zero-count close-surface item multiset",
        ));
    }
    if *count == 1 {
        items.remove(&item);
    } else {
        *count -= 1;
    }
    Ok(())
}

fn validate_close_requirement(
    policy: &DockPolicySnapshot,
    requirement: CloseItemRequirement,
) -> Result<(), TransactionError> {
    evaluate_policy(
        policy,
        &DockPolicyRequest::ClosePane {
            item: requirement.item(),
            deferred: false,
        },
    )
    .map_err(close_command_error)?;
    let actual = policy.pane_close_capability(requirement.item());
    if actual != requirement.capability() {
        return Err(TransactionError::Command {
            index: 0,
            source: CommandError::Invariant {
                stage: "validate frozen close policy capability",
            },
        });
    }
    if !actual.allows_close() {
        return Err(TransactionError::Command {
            index: 0,
            source: CommandError::Invariant {
                stage: "close policy disabled after plan capture",
            },
        });
    }
    Ok(())
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum ItemDelta {
    None,
    Open(ItemId),
}

struct AppliedCommand {
    outcome: CommandOutcome,
    delta: ItemDelta,
}

#[derive(Debug)]
enum DetachedPayload {
    Item {
        source_root: RootId,
        item: ItemId,
    },
    Node {
        source_root: RootId,
        node: NodeId,
        items: Vec<ItemId>,
        selected: Option<ItemId>,
        mru: Option<Vec<ItemId>>,
        is_tabs: bool,
        central: Option<NodeId>,
    },
}

impl DetachedPayload {
    fn source_root(&self) -> RootId {
        match self {
            Self::Item { source_root, .. } | Self::Node { source_root, .. } => *source_root,
        }
    }

    fn items(&self) -> Vec<ItemId> {
        match self {
            Self::Item { item, .. } => vec![*item],
            Self::Node { items, .. } => items.clone(),
        }
    }
}

pub(crate) fn prepare_transaction(
    workspace: &Workspace,
    policy: &DockPolicySnapshot,
    commands: &[WorkspaceCommand],
) -> Result<PreparedTransaction, TransactionError> {
    prepare_transaction_inner(workspace, TransactionAuthority::Policy(policy), commands)
}

pub(crate) fn prepare_surface_recovery_transaction(
    workspace: &Workspace,
    transaction: &SurfaceRecoveryTransaction,
) -> Result<PreparedTransaction, TransactionError> {
    if !transaction.is_exact_program() {
        return Err(TransactionError::Command {
            index: 0,
            source: CommandError::Invariant {
                stage: "authorize exact surface recovery program",
            },
        });
    }
    let roster = transaction.roster();
    if !workspace.matches_surface_roster_source(roster) {
        return Err(TransactionError::Precondition {
            index: 0,
            source: TransactionPreconditionError::StaleSurfaceRoster {
                surface: roster.surface(),
            },
        });
    }
    prepare_transaction_inner(
        workspace,
        TransactionAuthority::SurfaceRecovery,
        transaction.commands(),
    )
}

#[derive(Debug, Clone, Copy)]
enum TransactionAuthority<'a> {
    Policy(&'a DockPolicySnapshot),
    SurfaceRecovery,
}

fn prepare_transaction_inner(
    workspace: &Workspace,
    authority: TransactionAuthority<'_>,
    commands: &[WorkspaceCommand],
) -> Result<PreparedTransaction, TransactionError> {
    #[cfg(test)]
    {
        crate::drop_resolver::structural_work::record_transaction_prepare(commands.len());
        crate::drop_resolver::structural_work::record_transaction_candidate_clone(workspace);
    }
    let mut candidate = workspace.clone();
    let mut expected_items = workspace.item_multiset();
    let mut outcomes = Vec::with_capacity(commands.len());

    for (index, command) in commands.iter().enumerate() {
        let move_baseline =
            (index > 0 && matches!(command, WorkspaceCommand::Move { .. })).then(|| {
                #[cfg(test)]
                crate::drop_resolver::structural_work::record_multi_command_move_baseline_clone(
                    &candidate,
                );
                candidate.clone()
            });
        if let TransactionAuthority::Policy(policy) = authority {
            authorize_workspace_command(&candidate, policy, command)
                .map_err(|source| TransactionError::Command { index, source })?;
        }
        let mut applied = apply_command(&mut candidate, command)
            .map_err(|source| TransactionError::Command { index, source })?;
        if let CommandOutcome::Moved { changed, .. } = &mut applied.outcome {
            *changed = move_baseline
                .as_ref()
                .map_or_else(|| candidate != *workspace, |before| candidate != *before);
        }
        apply_item_delta(&mut expected_items, applied.delta)
            .map_err(|source| TransactionError::Command { index, source })?;
        let actual_items = candidate.item_multiset();
        if actual_items != expected_items {
            return Err(TransactionError::ItemReconciliation {
                command_index: Some(index),
                expected: expected_items,
                actual: actual_items,
            });
        }
        outcomes.push(applied.outcome);
    }

    prune_vacant_surfaces(&mut candidate);
    canonicalize_workspace(&mut candidate)?;
    candidate.validate()?;
    if commands.len() == 1
        && let Some(CommandOutcome::Moved { changed, .. }) = outcomes.first_mut()
    {
        *changed = candidate != *workspace;
    }
    let actual_items = candidate.item_multiset();
    if actual_items != expected_items {
        return Err(TransactionError::ItemReconciliation {
            command_index: None,
            expected: expected_items,
            actual: actual_items,
        });
    }

    Ok(PreparedTransaction {
        candidate,
        outcomes,
    })
}

pub(crate) fn authorize_workspace_command(
    workspace: &Workspace,
    policy: &DockPolicySnapshot,
    command: &WorkspaceCommand,
) -> Result<(), CommandError> {
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
            if let RootPresentationTarget::Contained { surface, .. } = target
                && let Some(RootPresentationOwner::Contained {
                    surface: source_surface,
                    ..
                }) = workspace.presentation_for_root(root)
                && source_surface == *surface
            {
                return authorize_contained_transform(
                    policy,
                    root_payload_policy_facts(workspace, root, false)?,
                    *surface,
                );
            }
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
                root_payload_policy_facts(workspace, root, true)?,
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
            authorize_contained_transform(
                policy,
                root_payload_policy_facts(workspace, root, false)?,
                surface,
            )
        }
        // Removing an already empty root is structural lifecycle cleanup.
        WorkspaceCommand::RemoveEmptyRoot { .. } => Ok(()),
    }
}

/// Read-only eligibility facts reused while one frozen drag evaluates many targets.
pub(crate) struct DropCommandEligibility<'a> {
    workspace: &'a Workspace,
    policy: &'a DockPolicySnapshot,
    payload: &'a MovePayload,
    retained_facts: DockPayloadPolicyFacts,
    undocked_facts: DockPayloadPolicyFacts,
}

impl<'a> DropCommandEligibility<'a> {
    pub(crate) fn new(
        workspace: &'a Workspace,
        policy: &'a DockPolicySnapshot,
        payload: &'a MovePayload,
    ) -> Result<Self, CommandError> {
        validate_move_payload(workspace, payload)?;
        Self::new_validated(workspace, policy, payload)
    }

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
        Ok(Self {
            workspace,
            policy,
            payload,
            retained_facts,
            undocked_facts,
        })
    }

    pub(crate) fn check_target(&self, target: &DockTarget) -> Result<(), CommandError> {
        self.check_target_with(ReferenceAuthority::Live, target)
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

        let operation = if target_is_tabs(target) {
            DockDropOperation::TabMerge
        } else {
            DockDropOperation::EdgeSplit
        };
        if matches!(target, DockTarget::TabGap { .. }) {
            authorize_tab_bar(self.policy, target)?;
        }
        let payload = if move_undocks_source(self.workspace, self.payload, target) {
            &self.undocked_facts
        } else {
            &self.retained_facts
        };
        authorize_drop(self.policy, operation, payload, target)
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
    let payload = move_payload_policy_facts(workspace, &MovePayload::Item(source.clone()), false)?;
    authorize_drop(policy, DockDropOperation::TabMerge, &payload, &target)
}

fn authorize_open(
    workspace: &Workspace,
    policy: &DockPolicySnapshot,
    item: ItemId,
    target: &DockTarget,
) -> Result<(), CommandError> {
    if workspace.item_multiset().contains_key(&item) {
        return Err(CommandError::ItemAlreadyOpen { item });
    }
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
    DropCommandEligibility::new(workspace, policy, payload)?.check_target(target)
}

fn authorize_tab_bar(policy: &DockPolicySnapshot, target: &DockTarget) -> Result<(), CommandError> {
    evaluate_policy(
        policy,
        &DockPolicyRequest::InteractWithTabBar(DockTabBarPolicyRequest::new(
            target.surface(),
            Some(target.rule()),
        )),
    )
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
    evaluate_policy(
        policy,
        &DockPolicyRequest::TransformContained(DockContainedTransformPolicyRequest::new(
            payload, surface,
        )),
    )
}

fn evaluate_policy(
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
                    workspace.nodes.get(source.tabs()),
                    Some(Node::Tabs { items, .. }) if items.len() == 1
                        && workspace
                            .roots
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
                && matches!(workspace.nodes.get(source_node), Some(Node::Tabs { .. }))
            {
                return Ok(());
            }
            return Err(CommandError::TargetInsidePayload {
                source_node,
                target: target_node,
            });
        }
        if target_is_tabs(target)
            && !matches!(workspace.nodes.get(source_node), Some(Node::Tabs { .. }))
        {
            return Err(CommandError::SplitPayloadIntoTabs { node: source_node });
        }

        let source_root = payload_root(payload);
        let source_record = workspace
            .roots
            .get(&source_root)
            .ok_or(CommandError::MissingRoot { root: source_root })?;
        if source_record.node != source_node
            && let Some(central) = source_record.central
            && workspace.subtree_contains(source_node, central)
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
        .and_then(|parent| match workspace.nodes.get(parent.parent) {
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

fn apply_item_delta(
    items: &mut BTreeMap<ItemId, usize>,
    delta: ItemDelta,
) -> Result<(), CommandError> {
    match delta {
        ItemDelta::None => Ok(()),
        ItemDelta::Open(item) => {
            if items.contains_key(&item) {
                return Err(CommandError::Invariant {
                    stage: "reconcile unique opened item",
                });
            }
            items.insert(item, 1);
            Ok(())
        }
    }
}

fn apply_command(
    workspace: &mut Workspace,
    command: &WorkspaceCommand,
) -> Result<AppliedCommand, CommandError> {
    match command {
        WorkspaceCommand::Select { source } => select(workspace, source),
        WorkspaceCommand::Reorder {
            source,
            insertion_index,
        } => reorder(workspace, source, *insertion_index),
        WorkspaceCommand::Open { item, target } => open(workspace, *item, target),
        WorkspaceCommand::Move { payload, target } => move_payload(workspace, payload, target),
        WorkspaceCommand::ResizeSplits { splits } => resize_splits(workspace, splits),
        WorkspaceCommand::CreateSurfaceRoot {
            surface,
            root,
            content,
        } => create_surface_root(workspace, *surface, *root, content),
        WorkspaceCommand::CreateContainedRoot {
            surface,
            root,
            floating,
            rect,
            position,
            content,
        } => create_contained_root(
            workspace, *surface, *root, *floating, *rect, *position, content,
        ),
        WorkspaceCommand::InstallMainRoot {
            surface,
            root,
            content,
        } => install_main_root(workspace, *surface, *root, content),
        WorkspaceCommand::RehomeRoot { source, target } => rehome_root(workspace, source, *target),
        WorkspaceCommand::PromoteContained {
            source,
            surface,
            floating,
        } => promote_contained(workspace, source, *surface, *floating),
        WorkspaceCommand::UpdateContainedRect {
            surface,
            root,
            floating,
            expected_rect,
            rect,
        } => update_contained_rect(workspace, *surface, *root, *floating, *expected_rect, *rect),
        WorkspaceCommand::UpdateContainedPresentation {
            source,
            floating,
            expected_rect,
            expected_roster,
            rect,
            position,
        } => update_contained_presentation(
            workspace,
            source,
            *floating,
            *expected_rect,
            expected_roster,
            *rect,
            *position,
        ),
        WorkspaceCommand::RaiseContained {
            source,
            floating,
            expected_roster,
        } => raise_contained(workspace, source, *floating, expected_roster),
        WorkspaceCommand::RemoveEmptyRoot { source } => remove_empty_root(workspace, source),
    }
}

fn select(workspace: &mut Workspace, source: &ItemSource) -> Result<AppliedCommand, CommandError> {
    validate_item_source(workspace, source)?;
    let tabs = source.tabs();
    let item = source.item();
    let node = workspace
        .nodes
        .get_mut(tabs)
        .ok_or(CommandError::MissingNode {
            role: ReferenceRole::Source,
            node: tabs,
        })?;
    let Node::Tabs { selected, .. } = node else {
        return Err(CommandError::NodeIsNotTabs { node: tabs });
    };
    let changed = *selected != Some(item);
    *selected = Some(item);
    promote_tab_mru(workspace, tabs, item)?;
    Ok(AppliedCommand {
        outcome: CommandOutcome::Selected {
            item,
            tabs,
            changed,
        },
        delta: ItemDelta::None,
    })
}

fn reorder(
    workspace: &mut Workspace,
    source: &ItemSource,
    insertion_index: usize,
) -> Result<AppliedCommand, CommandError> {
    validate_item_source(workspace, source)?;
    let tabs = source.tabs();
    let item = source.item();
    let (from, to, changed) = reorder_item(workspace, tabs, item, insertion_index)?;
    Ok(AppliedCommand {
        outcome: CommandOutcome::Reordered {
            item,
            tabs,
            from,
            to,
            changed,
        },
        delta: ItemDelta::None,
    })
}

fn open(
    workspace: &mut Workspace,
    item: ItemId,
    target: &DockTarget,
) -> Result<AppliedCommand, CommandError> {
    if workspace.item_multiset().contains_key(&item) {
        return Err(CommandError::ItemAlreadyOpen { item });
    }
    validate_target(workspace, target)?;
    let root = target_root(target);
    let payload = DetachedPayload::Item {
        source_root: root,
        item,
    };
    insert_payload(workspace, payload, target)?;
    Ok(AppliedCommand {
        outcome: CommandOutcome::Opened { item, root },
        delta: ItemDelta::Open(item),
    })
}

fn move_payload(
    workspace: &mut Workspace,
    payload: &MovePayload,
    target: &DockTarget,
) -> Result<AppliedCommand, CommandError> {
    validate_move_payload(workspace, payload)?;
    validate_target(workspace, target)?;

    if let MovePayload::Item(source) = payload
        && let Some(changed) = move_item_within_same_tabs(workspace, source, target)?
    {
        return Ok(AppliedCommand {
            outcome: CommandOutcome::Moved {
                items: vec![source.item()],
                source_root: source.root(),
                target_root: target_root(target),
                changed,
            },
            delta: ItemDelta::None,
        });
    }

    let source_node = payload_node(payload);
    if let Some(source_node) = source_node {
        let target_node = target_node(target);
        if workspace.subtree_contains(source_node, target_node) {
            if source_node == target_node
                && target_is_tabs(target)
                && matches!(workspace.nodes.get(source_node), Some(Node::Tabs { .. }))
            {
                return Ok(AppliedCommand {
                    outcome: CommandOutcome::Moved {
                        items: workspace.collect_items_in_subtree(source_node),
                        source_root: payload_root(payload),
                        target_root: target_root(target),
                        changed: false,
                    },
                    delta: ItemDelta::None,
                });
            }
            return Err(CommandError::TargetInsidePayload {
                source_node,
                target: target_node,
            });
        }
        if target_is_tabs(target)
            && !matches!(workspace.nodes.get(source_node), Some(Node::Tabs { .. }))
        {
            return Err(CommandError::SplitPayloadIntoTabs { node: source_node });
        }
    }

    let detached = detach_payload(workspace, payload)?;
    let source_root = detached.source_root();
    let items = detached.items();
    let target_root = target_root(target);
    insert_payload(workspace, detached, target)?;
    Ok(AppliedCommand {
        outcome: CommandOutcome::Moved {
            items,
            source_root,
            target_root,
            changed: true,
        },
        delta: ItemDelta::None,
    })
}

pub(crate) fn validate_split_resizes(
    workspace: &Workspace,
    policy: &DockPolicySnapshot,
    splits: &[SplitResize],
) -> Result<Vec<(NodeId, bool)>, CommandError> {
    let validated = validate_split_resizes_structure(workspace, splits)?;
    for resize in splits {
        let split = resize.split();
        let axis = match workspace.nodes.get(split.node()) {
            Some(Node::Split { axis, .. }) => *axis,
            _ => {
                return Err(CommandError::Invariant {
                    stage: "authorize structurally validated split resize",
                });
            }
        };
        let surface = workspace
            .presentation_for_root(split.root())
            .map(|owner| match owner {
                RootPresentationOwner::Main { surface }
                | RootPresentationOwner::Contained { surface, .. } => surface,
            });
        evaluate_policy(
            policy,
            &DockPolicyRequest::Resize(DockResizePolicyRequest::new(axis, surface)),
        )?;
    }
    Ok(validated)
}

fn validate_split_resizes_structure(
    workspace: &Workspace,
    splits: &[SplitResize],
) -> Result<Vec<(NodeId, bool)>, CommandError> {
    if splits.is_empty() {
        return Err(CommandError::EmptySplitResizeBatch);
    }
    let mut seen = std::collections::BTreeSet::new();
    let mut validated = Vec::with_capacity(splits.len());
    for resize in splits {
        let split = resize.split();
        let weights = resize.weights();
        validate_node_source(workspace, split)?;
        let node_id = split.node();
        if !seen.insert(node_id) {
            return Err(CommandError::DuplicateSplitResize { split: node_id });
        }
        let node = workspace
            .nodes
            .get(node_id)
            .ok_or(CommandError::MissingNode {
                role: ReferenceRole::Source,
                node: node_id,
            })?;
        let Node::Split {
            children,
            weights: current,
            ..
        } = node
        else {
            return Err(CommandError::NodeIsNotSplit { node: node_id });
        };
        if children.len() != weights.len() {
            return Err(CommandError::SplitWeightCountMismatch {
                split: node_id,
                children: children.len(),
                weights: weights.len(),
            });
        }
        let sum: f64 = weights.iter().map(|weight| f64::from(weight.get())).sum();
        if (sum - 1.0).abs() > NORMALIZED_WEIGHT_TOLERANCE {
            return Err(CommandError::SplitWeightsNotNormalized {
                split: node_id,
                sum,
            });
        }
        validated.push((node_id, current != weights));
    }
    Ok(validated)
}

fn resize_splits(
    workspace: &mut Workspace,
    splits: &[SplitResize],
) -> Result<AppliedCommand, CommandError> {
    let validated = validate_split_resizes_structure(workspace, splits)?;
    for resize in splits {
        let current = match workspace.nodes.get_mut(resize.split().node()) {
            Some(Node::Split { weights, .. }) => weights,
            _ => {
                return Err(CommandError::Invariant {
                    stage: "apply validated split resize batch",
                });
            }
        };
        current.clone_from_slice(resize.weights());
    }
    Ok(AppliedCommand {
        outcome: CommandOutcome::SplitsResized {
            splits: validated.iter().map(|(split, _)| *split).collect(),
            changed: validated.iter().any(|(_, changed)| *changed),
        },
        delta: ItemDelta::None,
    })
}

fn create_surface_root(
    workspace: &mut Workspace,
    surface: SurfaceId,
    root: RootId,
    content: &RootContent,
) -> Result<AppliedCommand, CommandError> {
    ensure_root_ids_available(workspace, root)?;
    if workspace.surfaces.contains_key(&surface) {
        return Err(CommandError::SurfaceIdCollision { surface });
    }
    validate_root_content(workspace, content)?;
    let (node, central, items, delta) = detach_root_content(workspace, content)?;
    workspace.roots.insert(root, RootRecord { node, central });
    workspace
        .surfaces
        .insert(surface, SurfacePresentation::with_main(root));
    Ok(AppliedCommand {
        outcome: CommandOutcome::SurfaceRootCreated {
            surface,
            root,
            items,
        },
        delta,
    })
}

#[allow(clippy::too_many_arguments)]
fn create_contained_root(
    workspace: &mut Workspace,
    surface: SurfaceId,
    root: RootId,
    floating: FloatingPresentationId,
    rect: crate::geometry::LogicalRect,
    position: ContainedPosition,
    content: &RootContent,
) -> Result<AppliedCommand, CommandError> {
    ensure_root_ids_available(workspace, root)?;
    if workspace.contained_floatings.contains_key(&floating) {
        return Err(CommandError::FloatingIdCollision { floating });
    }
    if !workspace.surfaces.contains_key(&surface) {
        return Err(CommandError::MissingSurface { surface });
    }
    contained_insertion_index(workspace, surface, floating, position)?;
    validate_root_content(workspace, content)?;
    let (node, central, items, delta) = detach_root_content(workspace, content)?;
    if !workspace.surfaces.contains_key(&surface) {
        return Err(CommandError::MissingSurface { surface });
    }
    let insertion_index = contained_insertion_index(workspace, surface, floating, position)?;
    workspace.roots.insert(root, RootRecord { node, central });
    workspace
        .contained_floatings
        .insert(floating, ContainedFloating::new(root, rect));
    let presentation = workspace
        .surfaces
        .get_mut(&surface)
        .ok_or(CommandError::MissingSurface { surface })?;
    if insertion_index > presentation.contained.len() {
        return Err(CommandError::Invariant {
            stage: "insert contained at validated roster position",
        });
    }
    presentation.contained.insert(insertion_index, floating);
    Ok(AppliedCommand {
        outcome: CommandOutcome::ContainedRootCreated {
            surface,
            root,
            floating,
            items,
        },
        delta,
    })
}

fn install_main_root(
    workspace: &mut Workspace,
    surface: SurfaceId,
    root: RootId,
    content: &RootContent,
) -> Result<AppliedCommand, CommandError> {
    ensure_root_ids_available(workspace, root)?;
    require_rootless_surface(workspace, surface)?;
    validate_root_content(workspace, content)?;
    let (node, central, items, delta) = detach_root_content(workspace, content)?;
    let presentation = workspace
        .surfaces
        .get_mut(&surface)
        .ok_or(CommandError::MissingSurface { surface })?;
    if let Some(existing) = presentation.main_root {
        return Err(CommandError::SurfaceMainOccupied {
            surface,
            root: existing,
        });
    }
    workspace.roots.insert(root, RootRecord { node, central });
    presentation.main_root = Some(root);
    Ok(AppliedCommand {
        outcome: CommandOutcome::MainRootInstalled {
            surface,
            root,
            items,
        },
        delta,
    })
}

fn rehome_root(
    workspace: &mut Workspace,
    source: &NodeSource,
    target: RootPresentationTarget,
) -> Result<AppliedCommand, CommandError> {
    validate_node_source(workspace, source)?;
    let root = source.root();
    let root_node = workspace
        .roots
        .get(&root)
        .ok_or(CommandError::MissingRoot { root })?
        .node;
    if source.node() != root_node {
        return Err(CommandError::NodeIsNotRoot {
            root,
            node: source.node(),
        });
    }

    let current = workspace
        .presentation_for_root(root)
        .ok_or(CommandError::Invariant {
            stage: "locate root presentation before rehome",
        })?;
    match target {
        RootPresentationTarget::NewSurface { surface } => {
            rehome_root_to_new_surface(workspace, root, current, surface)
        }
        RootPresentationTarget::Main { surface } => {
            rehome_root_to_main(workspace, root, current, surface)
        }
        RootPresentationTarget::Contained {
            surface,
            floating,
            rect,
            position,
        } => rehome_root_to_contained(
            workspace,
            root,
            current,
            ContainedRehomeTarget {
                surface,
                floating,
                rect,
                position,
            },
        ),
    }
}

fn rehome_root_to_new_surface(
    workspace: &mut Workspace,
    root: RootId,
    current: RootPresentationOwner,
    surface: SurfaceId,
) -> Result<AppliedCommand, CommandError> {
    if workspace.surfaces.contains_key(&surface) {
        return Err(CommandError::SurfaceIdCollision { surface });
    }
    detach_root_presentation(workspace, root, current)?;
    workspace
        .surfaces
        .insert(surface, SurfacePresentation::with_main(root));
    Ok(rehome_outcome(root, surface, None, true))
}

fn rehome_root_to_main(
    workspace: &mut Workspace,
    root: RootId,
    current: RootPresentationOwner,
    surface: SurfaceId,
) -> Result<AppliedCommand, CommandError> {
    require_rootless_surface(workspace, surface)?;
    if let RootPresentationOwner::Contained {
        surface: current_surface,
        floating,
    } = current
        && current_surface == surface
    {
        return Err(CommandError::ContainedPromotionRequiresDedicatedCommand { surface, floating });
    }
    detach_root_presentation(workspace, root, current)?;
    let presentation = workspace
        .surfaces
        .get_mut(&surface)
        .ok_or(CommandError::MissingSurface { surface })?;
    if let Some(existing) = presentation.main_root {
        return Err(CommandError::SurfaceMainOccupied {
            surface,
            root: existing,
        });
    }
    presentation.main_root = Some(root);
    Ok(rehome_outcome(root, surface, None, true))
}

#[derive(Debug, Clone, Copy)]
struct ContainedRehomeTarget {
    surface: SurfaceId,
    floating: FloatingPresentationId,
    rect: crate::geometry::LogicalRect,
    position: ContainedPosition,
}

fn rehome_root_to_contained(
    workspace: &mut Workspace,
    root: RootId,
    current: RootPresentationOwner,
    target: ContainedRehomeTarget,
) -> Result<AppliedCommand, CommandError> {
    let ContainedRehomeTarget {
        surface,
        floating,
        rect,
        position,
    } = target;
    if !workspace.surfaces.contains_key(&surface) {
        return Err(CommandError::MissingSurface { surface });
    }
    if let RootPresentationOwner::Contained {
        surface: current_surface,
        floating: current_floating,
    } = current
    {
        if floating != current_floating {
            return Err(CommandError::FloatingIdentityWouldChange {
                root,
                existing: current_floating,
                requested: floating,
            });
        }
        if current_surface == surface {
            return Err(CommandError::RehomeMetadataRequiresDedicatedCommand { floating });
        }
    } else if workspace.contained_floatings.contains_key(&floating) {
        return Err(CommandError::FloatingIdCollision { floating });
    }

    contained_insertion_index(workspace, surface, floating, position)?;
    detach_root_presentation(workspace, root, current)?;
    if !workspace.surfaces.contains_key(&surface) {
        return Err(CommandError::MissingSurface { surface });
    }
    let insertion_index = contained_insertion_index(workspace, surface, floating, position)?;
    workspace
        .contained_floatings
        .insert(floating, ContainedFloating::new(root, rect));
    let presentation = workspace
        .surfaces
        .get_mut(&surface)
        .ok_or(CommandError::MissingSurface { surface })?;
    if insertion_index > presentation.contained.len() {
        return Err(CommandError::Invariant {
            stage: "rehome contained at validated roster position",
        });
    }
    presentation.contained.insert(insertion_index, floating);
    Ok(rehome_outcome(root, surface, Some(floating), true))
}

fn promote_contained(
    workspace: &mut Workspace,
    source: &NodeSource,
    surface: SurfaceId,
    floating: FloatingPresentationId,
) -> Result<AppliedCommand, CommandError> {
    let (root, _) = validate_root_source(workspace, source)?;
    require_rootless_surface(workspace, surface)?;
    let current = workspace
        .presentation_for_root(root)
        .ok_or(CommandError::Invariant {
            stage: "locate contained presentation before promotion",
        })?;
    if current != (RootPresentationOwner::Contained { surface, floating }) {
        return floating_presentation_mismatch(workspace, floating, root, surface);
    }

    detach_root_presentation(workspace, root, current)?;
    let presentation = workspace
        .surfaces
        .get_mut(&surface)
        .ok_or(CommandError::MissingSurface { surface })?;
    if let Some(existing) = presentation.main_root {
        return Err(CommandError::SurfaceMainOccupied {
            surface,
            root: existing,
        });
    }
    presentation.main_root = Some(root);
    Ok(AppliedCommand {
        outcome: CommandOutcome::ContainedPromoted {
            surface,
            root,
            floating,
        },
        delta: ItemDelta::None,
    })
}

fn rehome_outcome(
    root: RootId,
    surface: SurfaceId,
    floating: Option<FloatingPresentationId>,
    changed: bool,
) -> AppliedCommand {
    AppliedCommand {
        outcome: CommandOutcome::RootRehomed {
            root,
            surface,
            floating,
            changed,
        },
        delta: ItemDelta::None,
    }
}

fn update_contained_rect(
    workspace: &mut Workspace,
    surface: SurfaceId,
    root: RootId,
    floating: FloatingPresentationId,
    expected_rect: crate::geometry::LogicalRect,
    rect: crate::geometry::LogicalRect,
) -> Result<AppliedCommand, CommandError> {
    validate_contained(workspace, surface, root, floating)?;
    let record = workspace
        .contained_floatings
        .get_mut(&floating)
        .ok_or(CommandError::MissingFloating { floating })?;
    if record.rect != expected_rect {
        return Err(CommandError::StaleContainedRect {
            floating,
            expected: expected_rect,
            actual: record.rect,
        });
    }
    let changed = record.rect != rect;
    record.rect = rect;
    Ok(AppliedCommand {
        outcome: CommandOutcome::ContainedRectUpdated { floating, changed },
        delta: ItemDelta::None,
    })
}

#[allow(clippy::too_many_arguments)]
fn update_contained_presentation(
    workspace: &mut Workspace,
    source: &NodeSource,
    floating: FloatingPresentationId,
    expected_rect: crate::geometry::LogicalRect,
    expected_roster: &ContainedRosterSource,
    rect: crate::geometry::LogicalRect,
    position: ContainedPosition,
) -> Result<AppliedCommand, CommandError> {
    let (root, _) = validate_root_source(workspace, source)?;
    let surface = expected_roster.surface();
    let owner = workspace
        .presentation_for_root(root)
        .ok_or(CommandError::Invariant {
            stage: "locate contained presentation before update",
        })?;
    if owner != (RootPresentationOwner::Contained { surface, floating }) {
        return floating_presentation_mismatch(workspace, floating, root, surface);
    }
    validate_contained(workspace, surface, root, floating)?;

    let record = workspace
        .contained_floatings
        .get(&floating)
        .ok_or(CommandError::MissingFloating { floating })?;
    if record.rect != expected_rect {
        return Err(CommandError::StaleContainedRect {
            floating,
            expected: expected_rect,
            actual: record.rect,
        });
    }

    let presentation = workspace
        .surfaces
        .get(&surface)
        .ok_or(CommandError::MissingSurface { surface })?;
    if presentation.contained.as_slice() != expected_roster.contained() {
        return Err(CommandError::StaleContainedRoster {
            surface,
            expected: expected_roster.contained().to_vec(),
            actual: presentation.contained.clone(),
        });
    }
    let from = presentation
        .contained
        .iter()
        .position(|entry| *entry == floating)
        .ok_or(CommandError::MissingFloating { floating })?;
    let to = contained_reinsertion_index(workspace, surface, floating, from, position)?;
    let rect_changed = expected_rect != rect;
    let order_changed = from != to;

    if rect_changed {
        workspace
            .contained_floatings
            .get_mut(&floating)
            .ok_or(CommandError::MissingFloating { floating })?
            .rect = rect;
    }
    if order_changed {
        let presentation = workspace
            .surfaces
            .get_mut(&surface)
            .ok_or(CommandError::MissingSurface { surface })?;
        presentation.contained.remove(from);
        presentation.contained.insert(to, floating);
    }

    Ok(AppliedCommand {
        outcome: CommandOutcome::ContainedPresentationUpdated {
            floating,
            from,
            to,
            rect_changed,
            order_changed,
        },
        delta: ItemDelta::None,
    })
}

fn raise_contained(
    workspace: &mut Workspace,
    source: &NodeSource,
    floating: FloatingPresentationId,
    expected_roster: &ContainedRosterSource,
) -> Result<AppliedCommand, CommandError> {
    let (root, _) = validate_root_source(workspace, source)?;
    let owner = workspace
        .presentation_for_root(root)
        .ok_or(CommandError::Invariant {
            stage: "locate contained presentation before raise",
        })?;
    let RootPresentationOwner::Contained {
        surface,
        floating: id,
    } = owner
    else {
        return floating_presentation_mismatch(
            workspace,
            floating,
            root,
            expected_roster.surface(),
        );
    };
    if id != floating {
        return floating_presentation_mismatch(workspace, floating, root, surface);
    }
    if expected_roster.surface() != surface {
        return Err(CommandError::ContainedRosterSurfaceMismatch {
            captured_surface: expected_roster.surface(),
            actual_surface: surface,
        });
    }
    validate_contained(workspace, surface, root, floating)?;
    let presentation = workspace
        .surfaces
        .get_mut(&surface)
        .ok_or(CommandError::MissingSurface { surface })?;
    if presentation.contained.as_slice() != expected_roster.contained() {
        return Err(CommandError::StaleContainedRoster {
            surface,
            expected: expected_roster.contained().to_vec(),
            actual: presentation.contained.clone(),
        });
    }
    let from = presentation
        .contained
        .iter()
        .position(|entry| *entry == floating)
        .ok_or(CommandError::MissingFloating { floating })?;
    let to = presentation
        .contained
        .len()
        .checked_sub(1)
        .ok_or(CommandError::Invariant {
            stage: "raise contained in non-empty roster",
        })?;
    if from == to {
        return Ok(AppliedCommand {
            outcome: CommandOutcome::ContainedRaised {
                floating,
                from,
                to,
                changed: false,
            },
            delta: ItemDelta::None,
        });
    }
    presentation.contained.remove(from);
    presentation.contained.push(floating);
    Ok(AppliedCommand {
        outcome: CommandOutcome::ContainedRaised {
            floating,
            from,
            to,
            changed: true,
        },
        delta: ItemDelta::None,
    })
}

fn remove_empty_root(
    workspace: &mut Workspace,
    source: &NodeSource,
) -> Result<AppliedCommand, CommandError> {
    let (root, root_node) = validate_root_source(workspace, source)?;
    let item_count = workspace.collect_items_in_subtree(root_node).len();
    if item_count != 0 {
        return Err(CommandError::RootNotEmpty {
            root,
            items: item_count,
        });
    }
    remove_root_and_presentation(workspace, root)?;
    Ok(AppliedCommand {
        outcome: CommandOutcome::EmptyRootRemoved { root },
        delta: ItemDelta::None,
    })
}

#[derive(Clone, Copy)]
enum ReferenceAuthority<'a> {
    Live,
    Indexed {
        version: WorkspaceVersion,
        index: &'a WorkspaceIndex,
    },
}

impl ReferenceAuthority<'_> {
    fn verify_reference(
        self,
        workspace: &Workspace,
        root: RootId,
        node: NodeId,
        expected: &crate::command::NodeFingerprint,
        role: ReferenceRole,
    ) -> Result<(), CommandError> {
        match self {
            Self::Live => workspace.verify_reference(root, node, expected, role),
            Self::Indexed { version, index } => {
                index.verify_reference(workspace, version, root, node, expected, role)
            }
        }
    }

    fn capture_tab_target(
        self,
        workspace: &Workspace,
        root: RootId,
        tabs: NodeId,
    ) -> Result<TabTarget, CommandError> {
        match self {
            Self::Live => workspace.capture_tab_target(root, tabs),
            Self::Indexed { version, index } => {
                index.capture_tab_target(workspace, version, root, tabs)
            }
        }
    }

    fn capture_inner_edge_target(
        self,
        workspace: &Workspace,
        target: &EdgeTarget,
    ) -> Result<EdgeTarget, CommandError> {
        match self {
            Self::Live => workspace.capture_inner_edge_target(
                target.root(),
                target.node(),
                target.edge(),
                target.fraction(),
            ),
            Self::Indexed { version, index } => index.capture_inner_edge_target(
                workspace,
                version,
                target.root(),
                target.node(),
                target.edge(),
                target.fraction(),
            ),
        }
    }

    fn capture_outer_edge_target(
        self,
        workspace: &Workspace,
        target: &EdgeTarget,
    ) -> Result<EdgeTarget, CommandError> {
        match self {
            Self::Live => {
                workspace.capture_outer_edge_target(target.root(), target.edge(), target.fraction())
            }
            Self::Indexed { version, index } => index.capture_outer_edge_target(
                workspace,
                version,
                target.root(),
                target.edge(),
                target.fraction(),
            ),
        }
    }
}

fn validate_item_source(workspace: &Workspace, source: &ItemSource) -> Result<(), CommandError> {
    validate_item_source_with(workspace, ReferenceAuthority::Live, source)
}

fn validate_item_source_with(
    workspace: &Workspace,
    references: ReferenceAuthority<'_>,
    source: &ItemSource,
) -> Result<(), CommandError> {
    references.verify_reference(
        workspace,
        source.root(),
        source.tabs(),
        source.fingerprint(),
        ReferenceRole::Source,
    )?;
    match workspace.nodes.get(source.tabs()) {
        Some(Node::Tabs { items, .. }) if items.contains(&source.item()) => Ok(()),
        Some(Node::Tabs { .. }) => Err(CommandError::ItemNotInTabs {
            tabs: source.tabs(),
            item: source.item(),
        }),
        Some(Node::Split { .. }) => Err(CommandError::NodeIsNotTabs {
            node: source.tabs(),
        }),
        None => Err(CommandError::MissingNode {
            role: ReferenceRole::Source,
            node: source.tabs(),
        }),
    }
}

fn validate_node_source(workspace: &Workspace, source: &NodeSource) -> Result<(), CommandError> {
    validate_node_source_with(workspace, ReferenceAuthority::Live, source)
}

fn validate_node_source_with(
    workspace: &Workspace,
    references: ReferenceAuthority<'_>,
    source: &NodeSource,
) -> Result<(), CommandError> {
    references.verify_reference(
        workspace,
        source.root(),
        source.node(),
        source.fingerprint(),
        ReferenceRole::Source,
    )
}

fn validate_root_source(
    workspace: &Workspace,
    source: &NodeSource,
) -> Result<(RootId, NodeId), CommandError> {
    validate_node_source(workspace, source)?;
    let root = source.root();
    let root_node = workspace
        .roots
        .get(&root)
        .ok_or(CommandError::MissingRoot { root })?
        .node;
    if root_node != source.node() {
        return Err(CommandError::NodeIsNotRoot {
            root,
            node: source.node(),
        });
    }
    Ok((root, root_node))
}

fn validate_move_payload(workspace: &Workspace, payload: &MovePayload) -> Result<(), CommandError> {
    validate_move_payload_with(workspace, ReferenceAuthority::Live, payload)
}

fn validate_move_payload_with(
    workspace: &Workspace,
    references: ReferenceAuthority<'_>,
    payload: &MovePayload,
) -> Result<(), CommandError> {
    match payload {
        MovePayload::Item(source) => validate_item_source_with(workspace, references, source),
        MovePayload::Tabs(source) => {
            validate_node_source_with(workspace, references, source)?;
            if !matches!(workspace.nodes.get(source.node()), Some(Node::Tabs { .. })) {
                return Err(CommandError::NodeIsNotTabs {
                    node: source.node(),
                });
            }
            ensure_nonempty_payload(workspace, source.node())
        }
        MovePayload::Subtree(source) => {
            validate_node_source_with(workspace, references, source)?;
            ensure_nonempty_payload(workspace, source.node())
        }
    }
}

fn ensure_nonempty_payload(workspace: &Workspace, node: NodeId) -> Result<(), CommandError> {
    if workspace.collect_items_in_subtree(node).is_empty() {
        Err(CommandError::EmptyPayload { node })
    } else {
        Ok(())
    }
}

fn validate_target(workspace: &Workspace, target: &DockTarget) -> Result<(), CommandError> {
    validate_target_with(workspace, ReferenceAuthority::Live, target)
}

fn validate_target_with(
    workspace: &Workspace,
    references: ReferenceAuthority<'_>,
    target: &DockTarget,
) -> Result<(), CommandError> {
    match target {
        DockTarget::Center(target) => validate_tab_target(workspace, references, target, None),
        DockTarget::TabGap { target, index } => {
            validate_tab_target(workspace, references, target, Some(*index))
        }
        DockTarget::InnerEdge(target) => {
            validate_edge_target(workspace, references, target, EdgeTargetScope::Inner)
        }
        DockTarget::OuterEdge(target) => {
            validate_edge_target(workspace, references, target, EdgeTargetScope::Outer)
        }
    }
}

fn validate_edge_target(
    workspace: &Workspace,
    references: ReferenceAuthority<'_>,
    target: &EdgeTarget,
    requested: EdgeTargetScope,
) -> Result<(), CommandError> {
    if target.scope() != requested {
        return Err(CommandError::EdgeTargetScopeMismatch {
            node: target.node(),
            captured: target.scope(),
            requested,
        });
    }
    references.verify_reference(
        workspace,
        target.root(),
        target.node(),
        target.fingerprint(),
        ReferenceRole::Target,
    )?;
    let actual = match requested {
        EdgeTargetScope::Inner => references.capture_inner_edge_target(workspace, target)?,
        EdgeTargetScope::Outer => references.capture_outer_edge_target(workspace, target)?,
    };
    if actual == *target {
        Ok(())
    } else {
        Err(CommandError::Invariant {
            stage: "rederive frozen edge target facts",
        })
    }
}

fn validate_tab_target(
    workspace: &Workspace,
    references: ReferenceAuthority<'_>,
    target: &TabTarget,
    insertion_index: Option<usize>,
) -> Result<(), CommandError> {
    references.verify_reference(
        workspace,
        target.root(),
        target.tabs(),
        target.fingerprint(),
        ReferenceRole::Target,
    )?;
    let actual = references.capture_tab_target(workspace, target.root(), target.tabs())?;
    if actual != *target {
        return Err(CommandError::Invariant {
            stage: "rederive frozen tabs target facts",
        });
    }
    match workspace.nodes.get(target.tabs()) {
        Some(Node::Tabs { items, .. }) => {
            if let Some(index) = insertion_index
                && index > items.len()
            {
                return Err(CommandError::TabIndexOutOfBounds {
                    tabs: target.tabs(),
                    index,
                    len: items.len(),
                });
            }
            Ok(())
        }
        Some(Node::Split { .. }) => Err(CommandError::NodeIsNotTabs {
            node: target.tabs(),
        }),
        None => Err(CommandError::MissingNode {
            role: ReferenceRole::Target,
            node: target.tabs(),
        }),
    }
}

fn validate_root_content(workspace: &Workspace, content: &RootContent) -> Result<(), CommandError> {
    match content {
        RootContent::OpenItem(item) => {
            if workspace.item_multiset().contains_key(item) {
                Err(CommandError::ItemAlreadyOpen { item: *item })
            } else {
                Ok(())
            }
        }
        RootContent::Move(payload) => {
            validate_move_payload(workspace, payload)?;
            if move_removes_complete_root(workspace, payload)? {
                return Err(CommandError::WholeRootRequiresRehome {
                    root: payload_root(payload),
                });
            }
            Ok(())
        }
    }
}

fn move_removes_complete_root(
    workspace: &Workspace,
    payload: &MovePayload,
) -> Result<bool, CommandError> {
    let root = payload_root(payload);
    let record = workspace
        .roots
        .get(&root)
        .ok_or(CommandError::MissingRoot { root })?;
    Ok(match payload {
        MovePayload::Item(_) => {
            record.central.is_none() && workspace.collect_items_in_subtree(record.node).len() == 1
        }
        MovePayload::Tabs(source) | MovePayload::Subtree(source) => source.node() == record.node,
    })
}

fn ensure_root_ids_available(workspace: &Workspace, root: RootId) -> Result<(), CommandError> {
    if workspace.roots.contains_key(&root) {
        Err(CommandError::RootIdCollision { root })
    } else {
        Ok(())
    }
}

fn detach_root_content(
    workspace: &mut Workspace,
    content: &RootContent,
) -> Result<(NodeId, Option<NodeId>, Vec<ItemId>, ItemDelta), CommandError> {
    match content {
        RootContent::OpenItem(item) => {
            let node = workspace.insert_runtime_node(Node::tabs([*item]));
            Ok((node, None, vec![*item], ItemDelta::Open(*item)))
        }
        RootContent::Move(payload) => {
            let detached = detach_payload(workspace, payload)?;
            let items = detached.items();
            match detached {
                DetachedPayload::Item { item, .. } => {
                    let node = workspace.insert_runtime_node(Node::tabs([item]));
                    Ok((node, None, items, ItemDelta::None))
                }
                DetachedPayload::Node { node, central, .. } => {
                    Ok((node, central, items, ItemDelta::None))
                }
            }
        }
    }
}

fn detach_payload(
    workspace: &mut Workspace,
    payload: &MovePayload,
) -> Result<DetachedPayload, CommandError> {
    match payload {
        MovePayload::Item(source) => {
            remove_item(workspace, source.tabs(), source.item())?;
            cleanup_item_source_root(workspace, source.root())?;
            Ok(DetachedPayload::Item {
                source_root: source.root(),
                item: source.item(),
            })
        }
        MovePayload::Tabs(source) | MovePayload::Subtree(source) => {
            let node = source.node();
            let source_root = source.root();
            let items = workspace.collect_items_in_subtree(node);
            let (selected, mru, is_tabs) = match workspace.nodes.get(node) {
                Some(Node::Tabs { selected, .. }) => (
                    *selected,
                    Some(
                        workspace
                            .tab_mru
                            .get(&node)
                            .ok_or(CommandError::Invariant {
                                stage: "detach tabs MRU",
                            })?
                            .clone(),
                    ),
                    true,
                ),
                Some(Node::Split { .. }) => (None, None, false),
                None => {
                    return Err(CommandError::MissingNode {
                        role: ReferenceRole::Source,
                        node,
                    });
                }
            };
            let central = detach_node(workspace, source_root, node)?;
            Ok(DetachedPayload::Node {
                source_root,
                node,
                items,
                selected,
                mru,
                is_tabs,
                central,
            })
        }
    }
}

fn detach_node(
    workspace: &mut Workspace,
    root: RootId,
    node: NodeId,
) -> Result<Option<NodeId>, CommandError> {
    let record = *workspace
        .roots
        .get(&root)
        .ok_or(CommandError::MissingRoot { root })?;
    if record.node == node {
        remove_root_and_presentation(workspace, root)?;
        return Ok(record.central);
    }
    if let Some(central) = record.central
        && workspace.subtree_contains(node, central)
    {
        return Err(CommandError::CentralNodeWouldDetach {
            root,
            source_node: node,
            central,
        });
    }
    let parent = workspace
        .parent_link(root, node)
        .ok_or(CommandError::NodeOutsideRoot {
            role: ReferenceRole::Source,
            root,
            node,
        })?;
    remove_split_child(workspace, parent.parent, parent.index)?;
    Ok(None)
}

fn remove_split_child(
    workspace: &mut Workspace,
    parent: NodeId,
    index: usize,
) -> Result<(), CommandError> {
    let node = workspace
        .nodes
        .get_mut(parent)
        .ok_or(CommandError::MissingNode {
            role: ReferenceRole::Source,
            node: parent,
        })?;
    let Node::Split {
        children, weights, ..
    } = node
    else {
        return Err(CommandError::NodeIsNotSplit { node: parent });
    };
    if index >= children.len() || index >= weights.len() {
        return Err(CommandError::Invariant {
            stage: "detach indexed split child",
        });
    }
    children.remove(index);
    weights.remove(index);
    if weights.len() == 1 {
        weights[0] = SplitWeight::new(1.0).map_err(|_| CommandError::Invariant {
            stage: "assign singleton split weight",
        })?;
    } else if !weights.is_empty() {
        *weights =
            SplitWeight::normalize(weights.iter().map(|weight| weight.get())).map_err(|_| {
                CommandError::Invariant {
                    stage: "normalize weights after detaching child",
                }
            })?;
    }
    Ok(())
}

fn insert_payload(
    workspace: &mut Workspace,
    payload: DetachedPayload,
    target: &DockTarget,
) -> Result<(), CommandError> {
    match target {
        DockTarget::Center(target) => {
            let index = tabs_len(workspace, target.tabs())?;
            merge_payload_into_tabs(workspace, payload, target.tabs(), index)
        }
        DockTarget::TabGap { target, index } => {
            merge_payload_into_tabs(workspace, payload, target.tabs(), *index)
        }
        DockTarget::InnerEdge(target) | DockTarget::OuterEdge(target) => {
            insert_payload_at_edge(workspace, &payload, target)
        }
    }
}

fn merge_payload_into_tabs(
    workspace: &mut Workspace,
    payload: DetachedPayload,
    target: NodeId,
    index: usize,
) -> Result<(), CommandError> {
    let (items, selected, mru) = match payload {
        DetachedPayload::Item { item, .. } => (vec![item], Some(item), None),
        DetachedPayload::Node {
            node,
            items,
            selected,
            mru,
            is_tabs,
            ..
        } => {
            if !is_tabs {
                return Err(CommandError::SplitPayloadIntoTabs { node });
            }
            let source = workspace
                .nodes
                .get_mut(node)
                .ok_or(CommandError::MissingNode {
                    role: ReferenceRole::Source,
                    node,
                })?;
            let Node::Tabs {
                items: source_items,
                selected: source_selected,
            } = source
            else {
                return Err(CommandError::NodeIsNotTabs { node });
            };
            source_items.clear();
            *source_selected = None;
            workspace
                .tab_mru
                .get_mut(&node)
                .ok_or(CommandError::Invariant {
                    stage: "clear merged source tabs MRU",
                })?
                .clear();
            (items, selected, mru)
        }
    };
    insert_items(workspace, target, index, &items, selected, mru.as_deref())
}

fn insert_items(
    workspace: &mut Workspace,
    tabs: NodeId,
    index: usize,
    inserted: &[ItemId],
    selected_item: Option<ItemId>,
    source_mru: Option<&[ItemId]>,
) -> Result<(), CommandError> {
    let node = workspace
        .nodes
        .get_mut(tabs)
        .ok_or(CommandError::MissingNode {
            role: ReferenceRole::Target,
            node: tabs,
        })?;
    let Node::Tabs { items, selected } = node else {
        return Err(CommandError::NodeIsNotTabs { node: tabs });
    };
    if index > items.len() {
        return Err(CommandError::TabIndexOutOfBounds {
            tabs,
            index,
            len: items.len(),
        });
    }
    let target_was_empty = items.is_empty();
    items.splice(index..index, inserted.iter().copied());
    if let Some(item) = selected_item {
        *selected = Some(item);
    }
    let target_mru = workspace
        .tab_mru
        .get_mut(&tabs)
        .ok_or(CommandError::Invariant {
            stage: "merge into target tabs MRU",
        })?;
    if target_was_empty && let Some(source_mru) = source_mru {
        *target_mru = source_mru.to_vec();
    } else {
        if let Some(item) = selected_item {
            target_mru.retain(|candidate| *candidate != item);
            target_mru.insert(0, item);
        }
        for &item in inserted {
            if Some(item) != selected_item {
                target_mru.push(item);
            }
        }
    }
    Ok(())
}

fn insert_payload_at_edge(
    workspace: &mut Workspace,
    payload: &DetachedPayload,
    target: &EdgeTarget,
) -> Result<(), CommandError> {
    let payload_node = match payload {
        DetachedPayload::Item { item, .. } => workspace.insert_runtime_node(Node::tabs([*item])),
        DetachedPayload::Node { node, .. } => *node,
    };
    let (axis, before) = edge_axis_and_order(target.edge());
    if let Some(parent) = workspace.parent_link(target.root(), target.node()) {
        let parent_axis = match workspace.nodes.get(parent.parent) {
            Some(Node::Split { axis, .. }) => *axis,
            _ => {
                return Err(CommandError::Invariant {
                    stage: "inspect edge target parent",
                });
            }
        };
        if parent_axis == axis {
            return split_existing_branch(
                workspace,
                parent.parent,
                target.node(),
                payload_node,
                before,
                target.fraction().get(),
                axis,
            );
        }
    }
    wrap_target_branch(
        workspace,
        target.root(),
        target.node(),
        payload_node,
        axis,
        before,
        target.fraction().get(),
    )
}

#[allow(clippy::too_many_arguments)]
fn split_existing_branch(
    workspace: &mut Workspace,
    parent: NodeId,
    target: NodeId,
    payload: NodeId,
    before: bool,
    fraction: f32,
    axis: Axis,
) -> Result<(), CommandError> {
    let node = workspace
        .nodes
        .get_mut(parent)
        .ok_or(CommandError::MissingNode {
            role: ReferenceRole::Target,
            node: parent,
        })?;
    let Node::Split {
        children, weights, ..
    } = node
    else {
        return Err(CommandError::NodeIsNotSplit { node: parent });
    };
    let index =
        children
            .iter()
            .position(|child| *child == target)
            .ok_or(CommandError::Invariant {
                stage: "locate edge target in same-axis parent",
            })?;
    let branch = weights
        .get(index)
        .copied()
        .ok_or(CommandError::Invariant {
            stage: "locate edge target branch weight",
        })?
        .get();
    let payload_weight = SplitWeight::new(branch * fraction)
        .map_err(|_| CommandError::InvalidEdgeWeight { axis })?;
    let target_weight = SplitWeight::new(branch - payload_weight.get())
        .map_err(|_| CommandError::InvalidEdgeWeight { axis })?;
    weights[index] = target_weight;
    let insertion = if before { index } else { index + 1 };
    children.insert(insertion, payload);
    weights.insert(insertion, payload_weight);
    Ok(())
}

fn wrap_target_branch(
    workspace: &mut Workspace,
    root: RootId,
    target: NodeId,
    payload: NodeId,
    axis: Axis,
    before: bool,
    fraction: f32,
) -> Result<(), CommandError> {
    let payload_weight =
        SplitWeight::new(fraction).map_err(|_| CommandError::InvalidEdgeWeight { axis })?;
    let target_weight =
        SplitWeight::new(1.0 - fraction).map_err(|_| CommandError::InvalidEdgeWeight { axis })?;
    let (children, weights) = if before {
        (vec![payload, target], vec![payload_weight, target_weight])
    } else {
        (vec![target, payload], vec![target_weight, payload_weight])
    };
    let wrapper = workspace.insert_runtime_node(Node::Split {
        axis,
        children,
        weights,
    });
    if let Some(parent) = workspace.parent_link(root, target) {
        let node = workspace
            .nodes
            .get_mut(parent.parent)
            .ok_or(CommandError::MissingNode {
                role: ReferenceRole::Target,
                node: parent.parent,
            })?;
        let Node::Split { children, .. } = node else {
            return Err(CommandError::NodeIsNotSplit {
                node: parent.parent,
            });
        };
        let slot = children
            .get_mut(parent.index)
            .ok_or(CommandError::Invariant {
                stage: "replace wrapped target branch",
            })?;
        *slot = wrapper;
    } else {
        let record = workspace
            .roots
            .get_mut(&root)
            .ok_or(CommandError::MissingRoot { root })?;
        if record.node != target {
            return Err(CommandError::NodeOutsideRoot {
                role: ReferenceRole::Target,
                root,
                node: target,
            });
        }
        record.node = wrapper;
    }
    Ok(())
}

fn move_item_within_same_tabs(
    workspace: &mut Workspace,
    source: &ItemSource,
    target: &DockTarget,
) -> Result<Option<bool>, CommandError> {
    match target {
        DockTarget::Center(target) if target.tabs() == source.tabs() => Ok(Some(false)),
        DockTarget::TabGap { target, index } if target.tabs() == source.tabs() => {
            let (_, _, changed) = reorder_item(workspace, source.tabs(), source.item(), *index)?;
            Ok(Some(changed))
        }
        DockTarget::InnerEdge(target) | DockTarget::OuterEdge(target)
            if target.node() == source.tabs() =>
        {
            let is_noncentral_singleton = matches!(
                workspace.nodes.get(source.tabs()),
                Some(Node::Tabs { items, .. }) if items.len() == 1
            ) && workspace
                .roots
                .get(&source.root())
                .is_some_and(|record| record.central != Some(source.tabs()));
            Ok(is_noncentral_singleton.then_some(false))
        }
        DockTarget::Center(_)
        | DockTarget::TabGap { .. }
        | DockTarget::InnerEdge(_)
        | DockTarget::OuterEdge(_) => Ok(None),
    }
}

fn reorder_item(
    workspace: &mut Workspace,
    tabs: NodeId,
    item: ItemId,
    insertion_index: usize,
) -> Result<(usize, usize, bool), CommandError> {
    let node = workspace
        .nodes
        .get_mut(tabs)
        .ok_or(CommandError::MissingNode {
            role: ReferenceRole::Source,
            node: tabs,
        })?;
    let Node::Tabs { items, .. } = node else {
        return Err(CommandError::NodeIsNotTabs { node: tabs });
    };
    if insertion_index > items.len() {
        return Err(CommandError::TabIndexOutOfBounds {
            tabs,
            index: insertion_index,
            len: items.len(),
        });
    }
    let from = items
        .iter()
        .position(|entry| *entry == item)
        .ok_or(CommandError::ItemNotInTabs { tabs, item })?;
    let to = if insertion_index > from {
        insertion_index - 1
    } else {
        insertion_index
    };
    if from == to {
        return Ok((from, to, false));
    }
    items.remove(from);
    items.insert(to, item);
    Ok((from, to, true))
}

fn remove_item(workspace: &mut Workspace, tabs: NodeId, item: ItemId) -> Result<(), CommandError> {
    let node = workspace
        .nodes
        .get_mut(tabs)
        .ok_or(CommandError::MissingNode {
            role: ReferenceRole::Source,
            node: tabs,
        })?;
    let Node::Tabs { items, selected } = node else {
        return Err(CommandError::NodeIsNotTabs { node: tabs });
    };
    let index = items
        .iter()
        .position(|entry| *entry == item)
        .ok_or(CommandError::ItemNotInTabs { tabs, item })?;
    items.remove(index);
    let was_selected = *selected == Some(item);
    let mru = workspace
        .tab_mru
        .get_mut(&tabs)
        .ok_or(CommandError::Invariant {
            stage: "remove item from tabs MRU",
        })?;
    let mru_index =
        mru.iter()
            .position(|candidate| *candidate == item)
            .ok_or(CommandError::Invariant {
                stage: "locate removed item in tabs MRU",
            })?;
    mru.remove(mru_index);
    if was_selected {
        *selected = mru.first().copied();
    }
    Ok(())
}

fn promote_tab_mru(
    workspace: &mut Workspace,
    tabs: NodeId,
    item: ItemId,
) -> Result<(), CommandError> {
    let mru = workspace
        .tab_mru
        .get_mut(&tabs)
        .ok_or(CommandError::Invariant {
            stage: "select tabs MRU",
        })?;
    let index =
        mru.iter()
            .position(|candidate| *candidate == item)
            .ok_or(CommandError::Invariant {
                stage: "locate selected item in tabs MRU",
            })?;
    if index != 0 {
        mru.remove(index);
        mru.insert(0, item);
    }
    Ok(())
}

fn cleanup_item_source_root(workspace: &mut Workspace, root: RootId) -> Result<(), CommandError> {
    let record = *workspace
        .roots
        .get(&root)
        .ok_or(CommandError::MissingRoot { root })?;
    if workspace.collect_items_in_subtree(record.node).is_empty() && record.central.is_none() {
        remove_root_and_presentation(workspace, root)?;
    }
    Ok(())
}

fn remove_root_and_presentation(
    workspace: &mut Workspace,
    root: RootId,
) -> Result<RootRecord, CommandError> {
    let presentation = workspace
        .presentation_for_root(root)
        .ok_or(CommandError::Invariant {
            stage: "locate root presentation owner",
        })?;
    detach_root_presentation(workspace, root, presentation)?;
    workspace
        .roots
        .remove(&root)
        .ok_or(CommandError::MissingRoot { root })
}

fn remove_subtree_nodes(workspace: &mut Workspace, root: NodeId) -> Result<(), CommandError> {
    let mut stack = vec![root];
    while let Some(node_id) = stack.pop() {
        let node = workspace
            .remove_runtime_node(node_id)
            .ok_or(CommandError::MissingNode {
                role: ReferenceRole::Source,
                node: node_id,
            })?;
        if let Node::Split { children, .. } = node {
            stack.extend(children.into_iter().rev());
        }
    }
    Ok(())
}

fn detach_root_presentation(
    workspace: &mut Workspace,
    root: RootId,
    presentation: RootPresentationOwner,
) -> Result<(), CommandError> {
    match presentation {
        RootPresentationOwner::Main { surface } => {
            if workspace
                .surfaces
                .get(&surface)
                .map(|entry| entry.main_root)
                != Some(Some(root))
            {
                return Err(CommandError::Invariant {
                    stage: "detach matching main-root presentation",
                });
            }
            workspace
                .surfaces
                .get_mut(&surface)
                .ok_or(CommandError::MissingSurface { surface })?
                .main_root = None;
        }
        RootPresentationOwner::Contained { surface, floating } => {
            let record = workspace
                .contained_floatings
                .remove(&floating)
                .ok_or(CommandError::MissingFloating { floating })?;
            if record.root != root {
                return Err(CommandError::Invariant {
                    stage: "detach matching contained-root presentation",
                });
            }
            let presentation = workspace
                .surfaces
                .get_mut(&surface)
                .ok_or(CommandError::MissingSurface { surface })?;
            let index = presentation
                .contained
                .iter()
                .position(|entry| *entry == floating)
                .ok_or(CommandError::Invariant {
                    stage: "remove contained presentation backlink",
                })?;
            presentation.contained.remove(index);
        }
    }
    Ok(())
}

fn validate_contained(
    workspace: &Workspace,
    surface: SurfaceId,
    root: RootId,
    floating: FloatingPresentationId,
) -> Result<(), CommandError> {
    let record = workspace
        .contained_floatings
        .get(&floating)
        .ok_or(CommandError::MissingFloating { floating })?;
    if record.root != root || contained_surface(workspace, floating) != Some(surface) {
        return floating_presentation_mismatch(workspace, floating, root, surface);
    }
    let presentation = workspace
        .surfaces
        .get(&surface)
        .ok_or(CommandError::MissingSurface { surface })?;
    if !presentation.contained.contains(&floating) {
        return Err(CommandError::Invariant {
            stage: "validate contained presentation backlink",
        });
    }
    Ok(())
}

fn require_rootless_surface(workspace: &Workspace, surface: SurfaceId) -> Result<(), CommandError> {
    let presentation = workspace
        .surfaces
        .get(&surface)
        .ok_or(CommandError::MissingSurface { surface })?;
    if let Some(root) = presentation.main_root {
        Err(CommandError::SurfaceMainOccupied { surface, root })
    } else {
        Ok(())
    }
}

fn contained_insertion_index(
    workspace: &Workspace,
    surface: SurfaceId,
    floating: FloatingPresentationId,
    position: ContainedPosition,
) -> Result<usize, CommandError> {
    let presentation = workspace
        .surfaces
        .get(&surface)
        .ok_or(CommandError::MissingSurface { surface })?;
    let anchor = match position {
        ContainedPosition::Front => return Ok(presentation.contained.len()),
        ContainedPosition::Before(anchor) | ContainedPosition::After(anchor) => anchor,
    };
    if anchor == floating {
        return Err(CommandError::ContainedAnchorIsSelf { floating });
    }
    if let Some(index) = presentation
        .contained
        .iter()
        .position(|entry| *entry == anchor)
    {
        let after = usize::from(matches!(position, ContainedPosition::After(_)));
        return index.checked_add(after).ok_or(CommandError::Invariant {
            stage: "calculate contained roster insertion index",
        });
    }
    if let Some(actual_surface) = contained_surface(workspace, anchor) {
        return Err(CommandError::ContainedAnchorOnDifferentSurface {
            anchor,
            expected_surface: surface,
            actual_surface,
        });
    }
    Err(CommandError::MissingContainedAnchor { surface, anchor })
}

fn contained_reinsertion_index(
    workspace: &Workspace,
    surface: SurfaceId,
    floating: FloatingPresentationId,
    from: usize,
    position: ContainedPosition,
) -> Result<usize, CommandError> {
    let presentation = workspace
        .surfaces
        .get(&surface)
        .ok_or(CommandError::MissingSurface { surface })?;
    let anchor =
        match position {
            ContainedPosition::Front => {
                return presentation.contained.len().checked_sub(1).ok_or(
                    CommandError::Invariant {
                        stage: "reinsert contained in non-empty roster",
                    },
                );
            }
            ContainedPosition::Before(anchor) | ContainedPosition::After(anchor) => anchor,
        };
    if anchor == floating {
        return Err(CommandError::ContainedAnchorIsSelf { floating });
    }
    if let Some(anchor_index) = presentation
        .contained
        .iter()
        .position(|entry| *entry == anchor)
    {
        let anchor_after_removal = anchor_index - usize::from(anchor_index > from);
        return anchor_after_removal
            .checked_add(usize::from(matches!(position, ContainedPosition::After(_))))
            .ok_or(CommandError::Invariant {
                stage: "calculate contained reinsertion index",
            });
    }
    if let Some(actual_surface) = contained_surface(workspace, anchor) {
        return Err(CommandError::ContainedAnchorOnDifferentSurface {
            anchor,
            expected_surface: surface,
            actual_surface,
        });
    }
    Err(CommandError::MissingContainedAnchor { surface, anchor })
}

fn contained_surface(workspace: &Workspace, floating: FloatingPresentationId) -> Option<SurfaceId> {
    workspace
        .surfaces
        .iter()
        .find_map(|(surface, presentation)| {
            presentation
                .contained
                .contains(&floating)
                .then_some(*surface)
        })
}

fn floating_presentation_mismatch<T>(
    workspace: &Workspace,
    floating: FloatingPresentationId,
    expected_root: RootId,
    expected_surface: SurfaceId,
) -> Result<T, CommandError> {
    let record = workspace
        .contained_floatings
        .get(&floating)
        .ok_or(CommandError::MissingFloating { floating })?;
    let actual_surface = contained_surface(workspace, floating).ok_or(CommandError::Invariant {
        stage: "locate contained roster owner",
    })?;
    Err(CommandError::FloatingPresentationMismatch {
        floating,
        expected_root,
        expected_surface,
        actual_root: record.root,
        actual_surface,
    })
}

fn prune_vacant_surfaces(workspace: &mut Workspace) {
    workspace.surfaces.retain(|_, presentation| {
        presentation.main_root.is_some() || !presentation.contained.is_empty()
    });
}

fn tabs_len(workspace: &Workspace, tabs: NodeId) -> Result<usize, CommandError> {
    match workspace.nodes.get(tabs) {
        Some(Node::Tabs { items, .. }) => Ok(items.len()),
        Some(Node::Split { .. }) => Err(CommandError::NodeIsNotTabs { node: tabs }),
        None => Err(CommandError::MissingNode {
            role: ReferenceRole::Target,
            node: tabs,
        }),
    }
}

fn target_root(target: &DockTarget) -> RootId {
    match target {
        DockTarget::Center(target) | DockTarget::TabGap { target, .. } => target.root(),
        DockTarget::InnerEdge(target) | DockTarget::OuterEdge(target) => target.root(),
    }
}

fn target_node(target: &DockTarget) -> NodeId {
    match target {
        DockTarget::Center(target) | DockTarget::TabGap { target, .. } => target.tabs(),
        DockTarget::InnerEdge(target) | DockTarget::OuterEdge(target) => target.node(),
    }
}

fn target_is_tabs(target: &DockTarget) -> bool {
    matches!(target, DockTarget::Center(_) | DockTarget::TabGap { .. })
}

fn payload_root(payload: &MovePayload) -> RootId {
    match payload {
        MovePayload::Item(source) => source.root(),
        MovePayload::Tabs(source) | MovePayload::Subtree(source) => source.root(),
    }
}

fn payload_node(payload: &MovePayload) -> Option<NodeId> {
    match payload {
        MovePayload::Item(_) => None,
        MovePayload::Tabs(source) | MovePayload::Subtree(source) => Some(source.node()),
    }
}

fn edge_axis_and_order(edge: Edge) -> (Axis, bool) {
    match edge {
        Edge::Left => (Axis::Horizontal, true),
        Edge::Right => (Axis::Horizontal, false),
        Edge::Top => (Axis::Vertical, true),
        Edge::Bottom => (Axis::Vertical, false),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::command::{ContainedRootSource, SurfaceRosterSource};
    use crate::geometry::LogicalRect;
    use crate::graph::{ContainedFloating, Node, RootRecord, WorkspaceBuilder};
    use crate::policy::{
        CloseCapability, DockPolicy, DockSurfaceRule, PolicyRejection, PolicyRevision,
    };

    const SOURCE: SurfaceId = SurfaceId::new(1);
    const SURVIVOR: SurfaceId = SurfaceId::new(2);
    const MAIN: RootId = RootId::new(10);
    const CONTAINED_A: RootId = RootId::new(11);
    const CONTAINED_B: RootId = RootId::new(12);
    const SURVIVOR_ROOT: RootId = RootId::new(20);
    const FLOATING_A: FloatingPresentationId = FloatingPresentationId::new(101);
    const FLOATING_B: FloatingPresentationId = FloatingPresentationId::new(102);

    fn rect(offset: f64) -> LogicalRect {
        LogicalRect::new(offset, offset, 320.0, 240.0).expect("test rectangle must be valid")
    }

    fn insert_root(builder: &mut WorkspaceBuilder, root: RootId, items: &[u64]) {
        let node = builder.insert_node(Node::tabs(items.iter().copied().map(ItemId::new)));
        builder.set_root(root, RootRecord::new(node));
    }

    fn workspace_with_surface(main: Option<RootId>) -> Workspace {
        let mut builder = Workspace::builder();
        if let Some(root) = main {
            insert_root(&mut builder, root, &[1, 2]);
        }
        insert_root(&mut builder, CONTAINED_A, &[3]);
        insert_root(&mut builder, CONTAINED_B, &[4, 5]);
        insert_root(&mut builder, SURVIVOR_ROOT, &[9]);
        builder.set_surface(
            SOURCE,
            main.map_or_else(
                SurfacePresentation::rootless,
                SurfacePresentation::with_main,
            ),
        );
        builder.set_surface(SURVIVOR, SurfacePresentation::with_main(SURVIVOR_ROOT));
        for (floating, root, bounds) in [
            (FLOATING_A, CONTAINED_A, rect(10.0)),
            (FLOATING_B, CONTAINED_B, rect(20.0)),
        ] {
            builder.set_contained_floating(floating, ContainedFloating::new(root, bounds));
            builder
                .attach_contained(SOURCE, floating)
                .expect("source surface must exist");
        }
        builder.build().expect("test workspace must be valid")
    }

    fn capture_roster(workspace: &Workspace, surface: SurfaceId) -> SurfaceRosterSource {
        let presentation = workspace.surface(surface).expect("test surface must exist");
        let main = presentation.main_root.map(|root| {
            let node = workspace.root(root).expect("main root must exist").node;
            workspace
                .capture_node_source(root, node)
                .expect("main source must be capturable")
        });
        let contained = presentation
            .contained
            .iter()
            .copied()
            .map(|floating| {
                let record = workspace
                    .contained_floating(floating)
                    .expect("contained record must exist");
                let node = workspace
                    .root(record.root)
                    .expect("contained root must exist")
                    .node;
                ContainedRootSource::new(
                    floating,
                    workspace
                        .capture_node_source(record.root, node)
                        .expect("contained source must be capturable"),
                    record.rect,
                )
            })
            .collect();
        SurfaceRosterSource::new(surface, main, contained)
    }

    fn default_policy() -> DockPolicySnapshot {
        DockPolicy::default().snapshot(PolicyRevision::new(1))
    }

    fn capture_surface_close(
        workspace: &Workspace,
        policy: &DockPolicySnapshot,
    ) -> PreparedSurfaceContentClose {
        let roster = capture_roster(workspace, SOURCE);
        let sources = roster
            .main_source()
            .into_iter()
            .chain(roster.contained().iter().map(ContainedRootSource::source))
            .cloned()
            .collect::<Vec<_>>();
        let roots = sources
            .into_iter()
            .map(|source| {
                let items = workspace.collect_items_in_subtree(source.node());
                (source, items)
            })
            .collect::<Vec<_>>();
        let requirements = roots
            .iter()
            .flat_map(|(_, items)| items.iter().copied())
            .map(|item| CloseItemRequirement::new(item, policy.pane_close_capability(item)))
            .collect();
        PreparedSurfaceContentClose::new(roster, roots, requirements)
    }

    fn append_item_to_root(workspace: &mut Workspace, root: RootId, item: ItemId) {
        let node = workspace.root(root).expect("test root must exist").node;
        let Node::Tabs { items, .. } = workspace
            .nodes
            .get_mut(node)
            .expect("test root node must exist")
        else {
            panic!("test root must be a tabs node");
        };
        items.push(item);
        workspace
            .tab_mru
            .get_mut(&node)
            .expect("test tabs MRU must exist")
            .push(item);
    }

    #[test]
    fn rooted_surface_close_removes_main_and_ordered_contained_roots_atomically() {
        let workspace = workspace_with_surface(Some(MAIN));
        let policy = default_policy();
        let prepared = capture_surface_close(&workspace, &policy);

        let commit = prepare_surface_content_close(&workspace, &policy, &prepared)
            .expect("complete rooted surface close must prepare");

        assert_eq!(
            commit.outcome,
            CloseCommitOutcome::SurfaceClosed {
                surface: SOURCE,
                items: vec![
                    ItemId::new(1),
                    ItemId::new(2),
                    ItemId::new(3),
                    ItemId::new(4),
                    ItemId::new(5),
                ],
            }
        );
        assert!(commit.candidate.surface(SOURCE).is_none());
        for root in [MAIN, CONTAINED_A, CONTAINED_B] {
            assert!(commit.candidate.root(root).is_none());
        }
        assert!(commit.candidate.contained_floating(FLOATING_A).is_none());
        assert!(commit.candidate.contained_floating(FLOATING_B).is_none());
        assert!(commit.candidate.surface(SURVIVOR).is_some());
        assert_eq!(
            commit.candidate.item_multiset(),
            BTreeMap::from([(ItemId::new(9), 1)])
        );
        assert!(workspace.surface(SOURCE).is_some());
    }

    #[test]
    fn rootless_surface_close_removes_contained_only_roster() {
        let workspace = workspace_with_surface(None);
        let policy = default_policy();
        let prepared = capture_surface_close(&workspace, &policy);

        let commit = prepare_surface_content_close(&workspace, &policy, &prepared)
            .expect("rootless contained-only surface close must prepare");

        assert_eq!(
            commit.outcome,
            CloseCommitOutcome::SurfaceClosed {
                surface: SOURCE,
                items: vec![ItemId::new(3), ItemId::new(4), ItemId::new(5)],
            }
        );
        assert!(commit.candidate.surface(SOURCE).is_none());
        assert!(commit.candidate.root(CONTAINED_A).is_none());
        assert!(commit.candidate.root(CONTAINED_B).is_none());
        assert!(commit.candidate.surface(SURVIVOR).is_some());
    }

    #[test]
    fn changed_surface_roster_rejects_frozen_close_without_mutation() {
        let mut workspace = workspace_with_surface(Some(MAIN));
        let policy = default_policy();
        let prepared = capture_surface_close(&workspace, &policy);
        workspace
            .surfaces
            .get_mut(&SOURCE)
            .expect("source surface must exist")
            .contained
            .swap(0, 1);
        let unchanged = workspace.clone();

        let error = prepare_surface_content_close(&workspace, &policy, &prepared)
            .expect_err("changed roster must invalidate the prepared close");

        assert!(matches!(
            error,
            TransactionError::Precondition {
                source: TransactionPreconditionError::StaleSurfaceRoster { surface: SOURCE },
                ..
            }
        ));
        assert_eq!(workspace, unchanged);
    }

    #[test]
    fn unrelated_surface_item_change_does_not_invalidate_frozen_surface_close() {
        let mut workspace = workspace_with_surface(Some(MAIN));
        let policy = default_policy();
        let prepared = capture_surface_close(&workspace, &policy);
        append_item_to_root(&mut workspace, SURVIVOR_ROOT, ItemId::new(10));

        let commit = prepare_surface_content_close(&workspace, &policy, &prepared)
            .expect("unrelated source changes must not invalidate a surface close");

        assert_eq!(
            commit.candidate.item_multiset(),
            BTreeMap::from([(ItemId::new(9), 1), (ItemId::new(10), 1)])
        );
        assert!(commit.candidate.surface(SOURCE).is_none());
        assert!(commit.candidate.surface(SURVIVOR).is_some());
    }

    #[test]
    fn changed_source_item_rejects_frozen_close_without_mutation() {
        let mut workspace = workspace_with_surface(Some(MAIN));
        let policy = default_policy();
        let prepared = capture_surface_close(&workspace, &policy);
        append_item_to_root(&mut workspace, MAIN, ItemId::new(6));
        let unchanged = workspace.clone();

        let error = prepare_surface_content_close(&workspace, &policy, &prepared)
            .expect_err("changed source content must invalidate the frozen close");

        assert!(matches!(
            error,
            TransactionError::Precondition {
                source: TransactionPreconditionError::StaleSurfaceRoster { surface: SOURCE },
                ..
            }
        ));
        assert_eq!(workspace, unchanged);
    }

    #[test]
    fn changed_close_policy_rejects_frozen_surface_close() {
        let workspace = workspace_with_surface(Some(MAIN));
        let prepared = capture_surface_close(&workspace, &default_policy());
        let mut changed_policy = DockPolicy::default();
        changed_policy.set_close_capability(CloseCapability::Disabled);
        let changed_policy = changed_policy.snapshot(PolicyRevision::new(2));
        let unchanged = workspace.clone();

        let error = prepare_surface_content_close(&workspace, &changed_policy, &prepared)
            .expect_err("changed close policy must invalidate the prepared close");

        match error {
            TransactionError::Command {
                index: 0,
                source: CommandError::Policy(PolicyRejection::PaneCloseDisabled { item }),
            } => assert_eq!(item, ItemId::new(1)),
            actual => panic!("expected typed pane-close policy rejection, got {actual:?}"),
        }
        assert_eq!(workspace, unchanged);
    }

    #[test]
    fn changed_surface_close_policy_rejects_frozen_surface_close_with_typed_policy_error() {
        let workspace = workspace_with_surface(Some(MAIN));
        let prepared = capture_surface_close(&workspace, &default_policy());
        let mut surface_rule = DockSurfaceRule::new();
        surface_rule.set_close_enabled(false);
        let mut changed_policy = DockPolicy::default();
        changed_policy.set_surface_rule(SOURCE, surface_rule);
        let changed_policy = changed_policy.snapshot(PolicyRevision::new(2));
        let unchanged = workspace.clone();

        let error = prepare_surface_content_close(&workspace, &changed_policy, &prepared)
            .expect_err("changed surface close policy must invalidate the prepared close");

        assert!(matches!(
            error,
            TransactionError::Command {
                index: 0,
                source: CommandError::Policy(PolicyRejection::SurfaceCloseDisabled {
                    surface: SOURCE,
                }),
            }
        ));
        assert_eq!(workspace, unchanged);
    }

    #[test]
    fn invalid_frozen_root_item_order_does_not_mutate_workspace() {
        let workspace = workspace_with_surface(Some(MAIN));
        let policy = default_policy();
        let captured = capture_surface_close(&workspace, &policy);
        let mut roots = captured.roots().to_vec();
        roots
            .last_mut()
            .expect("fixture must contain a final root")
            .1
            .reverse();
        let requirements = roots
            .iter()
            .flat_map(|(_, items)| items.iter().copied())
            .map(|item| CloseItemRequirement::new(item, policy.pane_close_capability(item)))
            .collect();
        let injected =
            PreparedSurfaceContentClose::new(captured.roster().clone(), roots, requirements);
        let unchanged = workspace.clone();

        let error = prepare_surface_content_close(&workspace, &policy, &injected)
            .expect_err("injected item-order corruption must reject the close");

        assert!(matches!(error, TransactionError::Command { .. }));
        assert_eq!(workspace, unchanged);
    }
}
