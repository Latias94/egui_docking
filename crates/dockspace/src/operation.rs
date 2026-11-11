//! Internal checked command execution against isolated workspace candidates.

use std::collections::{BTreeMap, BTreeSet};
use std::ops::Deref;
use std::sync::Arc;

use crate::RootPresentationOwner;
use crate::canonical::canonicalize_draft;
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
use crate::transaction::{TransactionReport, clone_workspace_draft};
use crate::transition::WorkspaceVersion;
use crate::workspace::WorkspaceIndex;

#[path = "operation/authorization.rs"]
mod authorization;
#[path = "operation/close.rs"]
mod close;
#[path = "operation/references.rs"]
mod references;
#[path = "operation/topology.rs"]
mod topology;
pub(crate) use authorization::{
    DropCommandEligibility, authorize_captured_drag_source, authorize_workspace_command,
    contained_transform_operable,
};
use authorization::{check_move, evaluate_policy};
pub(crate) use close::{
    PreparedContentClose, PreparedContentCloseCommit, PreparedSurfaceContentClose,
    prepare_content_close, prepare_surface_content_close, prepare_surface_content_closes,
};
use references::{
    ReferenceAuthority, validate_item_source, validate_move_payload, validate_move_payload_with,
    validate_node_source, validate_root_content, validate_root_source, validate_target,
    validate_target_with,
};
pub(crate) use topology::validate_split_resizes;
#[cfg(test)]
use topology::validate_split_resizes_structure;
use topology::{
    apply_command, cleanup_item_source_root, floating_presentation_mismatch,
    moves_central_tab_contents, remove_item, remove_root_and_presentation, remove_subtree_nodes,
    select, select_tabs_item, validate_contained,
};

/// A completed candidate paired with the report frozen by its prepare owner.
#[derive(Debug)]
pub(crate) struct PreparedTransaction {
    candidate: PreparedWorkspace,
    report: TransactionReport,
}

#[derive(Debug)]
enum PreparedWorkspace {
    Owned(Workspace),
    Unchanged(WorkspaceSnapshot),
}

impl PreparedTransaction {
    fn new(candidate: Workspace, outcomes: Vec<CommandOutcome>, changed: bool) -> Self {
        Self {
            candidate: PreparedWorkspace::Owned(candidate),
            report: TransactionReport::new(outcomes, changed),
        }
    }

    fn unchanged_selection(candidate: WorkspaceSnapshot, source: &ItemSource) -> Self {
        Self {
            candidate: PreparedWorkspace::Unchanged(candidate),
            report: TransactionReport::new(
                vec![CommandOutcome::Selected {
                    item: source.item(),
                    tabs: source.tabs(),
                    changed: false,
                }],
                false,
            ),
        }
    }

    /// Borrows the completed candidate without allowing its report to drift.
    pub(crate) fn candidate(&self) -> &Workspace {
        match &self.candidate {
            PreparedWorkspace::Owned(workspace) => workspace,
            PreparedWorkspace::Unchanged(snapshot) => snapshot,
        }
    }

    /// Consumes this candidate and its preparation-relative report.
    /// Publication barriers still run before the engine publishes the candidate.
    pub(crate) fn into_parts(self) -> (Workspace, TransactionReport) {
        let workspace = match self.candidate {
            PreparedWorkspace::Owned(workspace) => workspace,
            PreparedWorkspace::Unchanged(snapshot) => snapshot.into_workspace(),
        };
        (workspace, self.report)
    }

    pub(crate) fn into_snapshot_parts(self) -> (WorkspaceSnapshot, TransactionReport) {
        let snapshot = match self.candidate {
            PreparedWorkspace::Owned(workspace) => workspace.into(),
            PreparedWorkspace::Unchanged(snapshot) => snapshot,
        };
        (snapshot, self.report)
    }

    #[cfg(test)]
    pub(crate) fn publish(self, workspace: &mut Workspace) -> TransactionReport {
        let (candidate, report) = self.into_parts();
        *workspace = candidate;
        report
    }
}

/// Immutable carrier for a workspace issued by the graph owner.
#[derive(Debug, Clone)]
pub(crate) struct WorkspaceSnapshot {
    workspace: Arc<Workspace>,
}

impl WorkspaceSnapshot {
    pub(crate) fn validated(
        workspace: Workspace,
    ) -> Result<Self, crate::validation::WorkspaceValidationErrors> {
        // Strict validation rejects every topology rewrite and vacant surface
        // that the general transaction would otherwise have to clean up.
        workspace.validate()?;
        Ok(Self::from(workspace))
    }

    pub(crate) fn as_arc(&self) -> &Arc<Workspace> {
        &self.workspace
    }

    pub(crate) fn into_workspace(self) -> Workspace {
        Arc::unwrap_or_clone(self.workspace)
    }

    pub(crate) fn apply_draft_command(
        &mut self,
        policy: &DockPolicySnapshot,
        command: &WorkspaceCommand,
    ) -> Result<CommandOutcome, TransactionError> {
        apply_owned_draft_command(Arc::make_mut(&mut self.workspace), policy, command)
    }

    pub(crate) fn prepare_transaction(
        &self,
        policy: &DockPolicySnapshot,
        commands: &[WorkspaceCommand],
    ) -> Result<PreparedTransaction, TransactionError> {
        if let [command @ WorkspaceCommand::Select { source }] = commands
            && matches!(self.node(source.tabs()), Some(Node::Tabs { selected, .. })
                if *selected == Some(source.item()))
        {
            #[cfg(test)]
            crate::drop_resolver::structural_work::record_transaction_prepare(commands.len());
            authorize_workspace_command(self, policy, command)
                .and_then(|()| validate_item_source(self, source))
                .map_err(|source| TransactionError::Command { index: 0, source })?;
            // A validated snapshot has selected-first MRU. This exact command
            // cannot change values or issuance; batches retain their own draft
            // even when their final durable values equal the original (ABA).
            return Ok(PreparedTransaction::unchanged_selection(
                self.clone(),
                source,
            ));
        }
        prepare_transaction(self, policy, commands)
    }
}

impl Deref for WorkspaceSnapshot {
    type Target = Workspace;

    fn deref(&self) -> &Self::Target {
        &self.workspace
    }
}

impl AsRef<Workspace> for WorkspaceSnapshot {
    fn as_ref(&self) -> &Workspace {
        self
    }
}

impl PartialEq for WorkspaceSnapshot {
    fn eq(&self, other: &Self) -> bool {
        self.workspace == other.workspace
    }
}

impl From<Workspace> for WorkspaceSnapshot {
    fn from(workspace: Workspace) -> Self {
        Self {
            workspace: Arc::new(workspace),
        }
    }
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
    prepare_surface_recovery_transactions(workspace, &[transaction])
}

/// Validates complete recovery programs before executing their combined draft.
pub(crate) fn prepare_surface_recovery_transactions(
    workspace: &Workspace,
    transactions: &[&SurfaceRecoveryTransaction],
) -> Result<PreparedTransaction, TransactionError> {
    let mut surfaces = BTreeSet::new();
    for transaction in transactions {
        if !transaction.is_exact_program() || !surfaces.insert(transaction.roster().surface()) {
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
    }
    let commands = transactions
        .iter()
        .flat_map(|transaction| transaction.commands().iter().cloned())
        .collect::<Vec<_>>();
    prepare_transaction_inner(workspace, TransactionAuthority::SurfaceRecovery, &commands)
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
    crate::drop_resolver::structural_work::record_transaction_prepare(commands.len());
    let mut candidate = clone_workspace_draft(workspace);
    if let [command @ WorkspaceCommand::Select { source }] = commands
        && let TransactionAuthority::Policy(policy) = authority
    {
        authorize_workspace_command(&candidate, policy, command)
            .map_err(|source| TransactionError::Command { index: 0, source })?;
        let applied = select(&mut candidate, source)
            .map_err(|source| TransactionError::Command { index: 0, source })?;
        // Selected and its exact MRU permutation preserve all other invariants.
        let changed = applied.outcome.changes_workspace();
        return Ok(PreparedTransaction::new(
            candidate,
            vec![applied.outcome],
            changed,
        ));
    }
    if let [
        command @ WorkspaceCommand::Move {
            payload: MovePayload::Item(source),
            target,
        },
    ] = commands
        && matches!(authority, TransactionAuthority::Policy(_))
    {
        // Only removing a singleton, unprotected root can vacate its surface.
        let retires_root = workspace.root(source.root()).is_some_and(|record| {
            record.node == source.tabs() && record.central.is_none()
                && matches!(workspace.node(source.tabs()), Some(Node::Tabs { items, .. }) if items.len() == 1)
        });
        let source_surface = if retires_root {
            workspace
                .presentation_for_root(source.root())
                .map(|owner| match owner {
                    RootPresentationOwner::Main { surface }
                    | RootPresentationOwner::Contained { surface, .. } => surface,
                })
        } else {
            None
        };
        let (applied, inserted_split) = apply_command(&mut candidate, authority, command)
            .map_err(|source| TransactionError::Command { index: 0, source })?;
        if let Some(surface) = source_surface {
            prune_vacant_surface(&mut candidate, surface);
        }
        // A checked Item move transfers one existing item. Repair its source
        // path and any outer wrapper without revalidating unrelated roots.
        super::canonical::finish_item_move(
            &mut candidate,
            source.root(),
            source.tabs(),
            target_root(target),
            match target {
                DockTarget::OuterEdge(edge) => Some(edge.root()),
                _ => None,
            },
            inserted_split,
        )?;
        // Different-pane center/gap transfers change membership. A real edge
        // transfer retains a newly allocated nonempty tabs identity; normalization
        // cannot restore the original Workspace. All no-ops return before detach.
        let changed = applied.outcome.changes_workspace();
        return Ok(PreparedTransaction::new(
            candidate,
            vec![applied.outcome],
            changed,
        ));
    }
    prepare_general_candidate(workspace, authority, commands, candidate)
}

fn prepare_general_candidate(
    workspace: &Workspace,
    authority: TransactionAuthority<'_>,
    commands: &[WorkspaceCommand],
    candidate: Workspace,
) -> Result<PreparedTransaction, TransactionError> {
    let mut stage = TransactionDraft::new(workspace, authority, candidate);
    for command in commands {
        stage.push_command(command)?;
    }
    stage.finish(workspace)
}

#[cfg(test)]
pub(crate) fn prepare_transaction_with(
    workspace: &Workspace,
    policy: &DockPolicySnapshot,
    edit: impl FnOnce(
        &mut crate::transaction::WorkspaceTransactionStage<'_>,
    ) -> Result<(), TransactionError>,
) -> Result<PreparedTransaction, TransactionError> {
    let mut stage = crate::transaction::WorkspaceTransactionStage::new(TransactionDraft::new(
        workspace,
        TransactionAuthority::Policy(policy),
        clone_workspace_draft(workspace),
    ));
    edit(&mut stage)?;
    stage.finish(workspace)
}

/// One concrete candidate, executor, item ledger and terminal failure.
/// Frozen vectors and incremental capture both pass through push_command.
pub(crate) struct TransactionDraft<'policy> {
    candidate: Workspace,
    authority: TransactionAuthority<'policy>,
    item_ledger: BTreeMap<ItemId, usize>,
    outcomes: Vec<CommandOutcome>,
    failure: Option<TransactionError>,
}

impl<'policy> TransactionDraft<'policy> {
    fn new(
        workspace: &Workspace,
        authority: TransactionAuthority<'policy>,
        candidate: Workspace,
    ) -> Self {
        Self {
            candidate,
            authority,
            item_ledger: workspace.item_multiset(),
            outcomes: Vec::new(),
            failure: None,
        }
    }

    #[cfg(test)]
    pub(crate) fn capture_and_push(
        &mut self,
        capture: impl FnOnce(
            crate::transaction::TransactionCapture<'_>,
        ) -> Result<WorkspaceCommand, CommandError>,
    ) -> Result<(), TransactionError> {
        if let Some(error) = &self.failure {
            return Err(error.clone());
        }
        let command = match capture(crate::transaction::TransactionCapture::new(&self.candidate)) {
            Ok(command) => command,
            Err(source) => {
                let error = TransactionError::Command {
                    index: self.outcomes.len(),
                    source,
                };
                self.failure = Some(error.clone());
                return Err(error);
            }
        };
        self.push_command(&command)
    }

    fn push_command(&mut self, command: &WorkspaceCommand) -> Result<(), TransactionError> {
        if let Some(error) = &self.failure {
            return Err(error.clone());
        }
        let index = self.outcomes.len();
        let result = (|| {
            validate_declared_open_item(&self.item_ledger, command)?;
            let (applied, _) = apply_command(&mut self.candidate, self.authority, command)?;
            apply_item_delta(&mut self.item_ledger, applied.delta)?;
            // Only Move can leave a source/target body for this prefix to finish.
            // Other writers complete their concrete birth/open/resize scope directly.
            if let WorkspaceCommand::Move { payload, target } = command {
                let source = payload_root(payload);
                let target = target_root(target);
                self.candidate.complete_root_structure(source)?;
                if target != source {
                    self.candidate.complete_root_structure(target)?;
                }
            }
            self.outcomes.push(applied.outcome);
            Ok(())
        })()
        .map_err(|source| TransactionError::Command { index, source });
        if let Err(error) = &result {
            self.failure = Some(error.clone());
        }
        result
    }

    pub(crate) fn finish(
        mut self,
        workspace: &Workspace,
    ) -> Result<PreparedTransaction, TransactionError> {
        if let Some(error) = self.failure {
            return Err(error);
        }
        prune_vacant_surfaces(&mut self.candidate);
        canonicalize_draft(&mut self.candidate)?;
        // General batches may restore their initial durable values. Compare
        // once after normalization; authority issuance is deliberately separate.
        let changed = self.candidate != *workspace;
        if self.outcomes.len() == 1
            && let Some(CommandOutcome::Moved { changed: moved, .. }) = self.outcomes.first_mut()
        {
            *moved = changed;
            if !changed {
                self.candidate.retain_original_issuance(workspace);
            }
        }
        let actual_items = self.candidate.item_multiset();
        if actual_items != self.item_ledger {
            return Err(TransactionError::ItemReconciliation {
                expected: self.item_ledger,
                actual: actual_items,
            });
        }
        Ok(PreparedTransaction::new(
            self.candidate,
            self.outcomes,
            changed,
        ))
    }
}

/// Applies a follow-up command to an already discardable transaction draft.
/// A failure invalidates that whole draft; the caller must not publish it.
pub(crate) fn apply_owned_draft_command(
    workspace: &mut Workspace,
    policy: &DockPolicySnapshot,
    command: &WorkspaceCommand,
) -> Result<CommandOutcome, TransactionError> {
    // This candidate is already discardable. Keep only a valid empty value
    // observable if its owned follow-up fails and is abandoned by the caller.
    let mut candidate = std::mem::take(workspace);
    let outcome = apply_owned_draft_inner(&mut candidate, policy, command)?;
    *workspace = candidate;
    Ok(outcome)
}

fn apply_owned_draft_inner(
    draft: &mut Workspace,
    policy: &DockPolicySnapshot,
    command: &WorkspaceCommand,
) -> Result<CommandOutcome, TransactionError> {
    let mut expected = draft.item_multiset();
    validate_declared_open_item(&expected, command)
        .map_err(|source| TransactionError::Command { index: 0, source })?;
    let (applied, _) = apply_command(draft, TransactionAuthority::Policy(policy), command)
        .map_err(|source| TransactionError::Command { index: 0, source })?;
    apply_item_delta(&mut expected, applied.delta)
        .map_err(|source| TransactionError::Command { index: 0, source })?;
    prune_vacant_surfaces(draft);
    canonicalize_draft(draft)?;
    let actual = draft.item_multiset();
    if actual != expected {
        return Err(TransactionError::ItemReconciliation { expected, actual });
    }
    Ok(applied.outcome)
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

fn validate_declared_open_item(
    items: &BTreeMap<ItemId, usize>,
    command: &WorkspaceCommand,
) -> Result<(), CommandError> {
    if let Some(item) = command.opened_item()
        && items.contains_key(&item)
    {
        return Err(CommandError::ItemAlreadyOpen { item });
    }
    Ok(())
}

fn prune_vacant_surfaces(workspace: &mut Workspace) {
    workspace.storage.surfaces.retain(|_, presentation| {
        presentation.main_root.is_some() || !presentation.contained.is_empty()
    });
}

fn prune_vacant_surface(workspace: &mut Workspace, surface: SurfaceId) {
    if workspace.surface(surface).is_some_and(|presentation| {
        presentation.main_root.is_none() && presentation.contained.is_empty()
    }) {
        workspace.storage.surfaces.remove(&surface);
    }
}

fn tabs_len(workspace: &Workspace, tabs: NodeId) -> Result<usize, CommandError> {
    match workspace.storage.nodes.get(tabs) {
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
#[path = "operation/tests"]
mod tests {
    mod arrangement_lock;
    mod checked_move;
    mod local_item_move;
    mod local_selection;
    mod logical_surfaces;
    mod owner_reference;
    mod split_resize;

    use super::*;
    use crate::command::{ContainedRootSource, DockFraction, SurfaceRosterSource};
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

    fn scaled_open_item_transaction(command_count: usize) -> (Workspace, Vec<WorkspaceCommand>) {
        let mut builder = Workspace::builder();
        insert_root(&mut builder, MAIN, &[1]);
        builder.set_surface(SOURCE, SurfacePresentation::with_main(MAIN));
        let workspace = builder
            .build()
            .expect("scaled open workspace must be valid");
        let commands = (0..command_count)
            .map(|index| {
                let ordinal = u64::try_from(index).expect("fixture ordinal fits u64");
                WorkspaceCommand::CreateContainedRoot {
                    surface: SOURCE,
                    root: RootId::new(1_000 + ordinal),
                    floating: FloatingPresentationId::new(1_000 + ordinal),
                    rect: rect(ordinal as f64),
                    position: ContainedPosition::Front,
                    content: RootContent::OpenItem(ItemId::new(10_000 + ordinal)),
                }
            })
            .collect();
        (workspace, commands)
    }

    fn scaled_edge_move_transaction(
        command_count: usize,
    ) -> (Workspace, Vec<WorkspaceCommand>, Vec<bool>) {
        let mut builder = Workspace::builder();
        let mut roots = Vec::with_capacity(command_count);
        for index in 0..command_count {
            let ordinal = u64::try_from(index).expect("fixture ordinal fits u64");
            let left = builder.insert_node(Node::tabs([ItemId::new(ordinal * 2 + 1)]));
            let right = builder.insert_node(Node::tabs([ItemId::new(ordinal * 2 + 2)]));
            let split = builder.insert_node(
                Node::split(Axis::Horizontal, [left, right], [0.25, 0.75])
                    .expect("fixture split is valid"),
            );
            let root = RootId::new(10_000 + ordinal);
            let surface = SurfaceId::new(20_000 + ordinal);
            builder.set_root(root, RootRecord::new(split));
            builder.set_surface(surface, SurfacePresentation::with_main(root));
            roots.push((root, left, right));
        }
        let workspace = builder
            .build()
            .expect("scaled equivalent-move workspace must be valid");
        let mut expected_changes = Vec::with_capacity(command_count);
        let commands = roots
            .into_iter()
            .enumerate()
            .map(|(index, (root, left, right))| {
                let changed = index % 2 == 1;
                let reverse = index % 4 >= 2;
                let (source_node, target_node, edge, original_fraction) = if reverse {
                    (right, left, Edge::Right, 0.75)
                } else {
                    (left, right, Edge::Left, 0.25)
                };
                let fraction = if changed { 0.5 } else { original_fraction };
                expected_changes.push(changed);
                let payload = workspace
                    .capture_node_source(root, source_node)
                    .expect("fixture source exists");
                let target = workspace
                    .capture_inner_edge_target(
                        root,
                        target_node,
                        edge,
                        DockFraction::new(fraction).expect("fixture fraction is valid"),
                    )
                    .expect("fixture target exists");
                WorkspaceCommand::Move {
                    payload: MovePayload::Tabs(payload),
                    target: DockTarget::InnerEdge(target),
                }
            })
            .collect();
        (workspace, commands, expected_changes)
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

    #[test]
    fn open_item_transactions_use_constant_full_item_reconciliation_scans() {
        for command_count in [16, 128, 1_024] {
            let (mut workspace, commands) = scaled_open_item_transaction(command_count);

            crate::drop_resolver::structural_work::reset();
            let report = crate::transaction::WorkspaceTransaction::from_commands(commands)
                .apply(&mut workspace, &default_policy())
                .expect("scaled item opens publish atomically");

            let work = crate::drop_resolver::structural_work::snapshot();
            assert_eq!(report.outcomes().len(), command_count);
            assert_eq!(work.transaction_commands, command_count);
            assert!(work.item_multiset_scans <= 4);
            assert!(work.item_multiset_item_visits <= (command_count + 1) * 4);
        }
    }

    #[test]
    fn multi_moves_use_one_transaction_candidate_clone() {
        for command_count in [16, 128, 1_024] {
            let (mut workspace, commands, expected_changes) =
                scaled_edge_move_transaction(command_count);

            crate::drop_resolver::structural_work::reset();
            let report = crate::transaction::WorkspaceTransaction::from_commands(commands)
                .apply(&mut workspace, &default_policy())
                .expect("mixed moves publish atomically");

            let work = crate::drop_resolver::structural_work::snapshot();
            assert!(report.changed());
            assert_eq!(report.outcomes().len(), command_count);
            for (outcome, expected_changed) in report.outcomes().iter().zip(expected_changes) {
                assert!(matches!(
                    outcome,
                    CommandOutcome::Moved { changed, .. } if *changed == expected_changed
                ));
            }
            assert_eq!(work.workspace_deep_clones.transaction_candidates.calls, 1);
        }
    }

    fn append_item_to_root(workspace: &mut Workspace, root: RootId, item: ItemId) {
        let node = workspace.root(root).expect("test root must exist").node;
        let target = DockTarget::Center(
            workspace
                .capture_tab_target(root, node)
                .expect("test root must be a current tabs target"),
        );
        crate::transaction::WorkspaceTransaction::from_commands([WorkspaceCommand::Open {
            item,
            target,
        }])
        .apply(workspace, &default_policy())
        .expect("checked Open must append the test item and complete its owner");
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
            .storage
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
    fn changed_close_capability_is_an_expected_frozen_surface_close_rejection() {
        for (initial, replacement) in [
            (CloseCapability::Immediate, CloseCapability::DeferredAllowed),
            (CloseCapability::DeferredAllowed, CloseCapability::Immediate),
        ] {
            let workspace = workspace_with_surface(Some(MAIN));
            let mut policy = DockPolicy::default();
            policy.set_close_capability(initial);
            let prepared =
                capture_surface_close(&workspace, &policy.snapshot(PolicyRevision::new(1)));
            policy.set_close_capability(replacement);

            let error = prepare_surface_content_close(
                &workspace,
                &policy.snapshot(PolicyRevision::new(2)),
                &prepared,
            )
            .expect_err(
                "changed capabilities invalidate the exact approval without an invariant failure",
            );

            assert!(error.is_expected_rejection());
            assert!(matches!(
                error,
                TransactionError::Command {
                    index: 0,
                    source: CommandError::CloseCapabilityChanged { item, expected, actual },
                } if item == ItemId::new(1) && expected == initial && actual == replacement
            ));
            workspace.validate().unwrap();
        }
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

        assert!(!error.is_expected_rejection());
        assert!(matches!(
            error,
            TransactionError::Command {
                source: CommandError::Invariant {
                    stage: "validate frozen close-surface root item order",
                },
                ..
            }
        ));
        assert_eq!(workspace, unchanged);
    }
}
