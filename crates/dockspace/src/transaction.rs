//! Atomic preparation and publication of checked workspace commands.

use crate::command::{CommandOutcome, WorkspaceCommand};
use crate::error::TransactionError;
use crate::graph::{Workspace, WorkspaceData};
use crate::operation::PreparedTransaction;
use crate::policy::DockPolicySnapshot;

/// Forks one complete transaction or atomic canonicalization draft. Slot/map
/// metadata is copied while node/MRU payloads share until their first write.
/// The existing ledger records logical inventory, not physical payload copies.
pub(crate) fn clone_workspace_draft(workspace: &Workspace) -> Workspace {
    #[cfg(test)]
    crate::drop_resolver::structural_work::record_transaction_candidate_clone(workspace);
    workspace.clone()
}

/// Copies an uncommitted draft without granting runtime workspace ownership.
pub(crate) fn clone_workspace_data_draft(workspace: &WorkspaceData) -> WorkspaceData {
    #[cfg(test)]
    crate::drop_resolver::structural_work::record_transaction_candidate_data_clone(workspace);
    workspace.clone()
}

/// Ordered command batch applied as one atomic workspace transaction.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct WorkspaceTransaction {
    commands: Vec<WorkspaceCommand>,
}

impl WorkspaceTransaction {
    /// Creates an empty transaction.
    pub fn new() -> Self {
        Self::default()
    }

    /// Creates a transaction from an ordered command sequence.
    pub fn from_commands(commands: impl IntoIterator<Item = WorkspaceCommand>) -> Self {
        Self {
            commands: commands.into_iter().collect(),
        }
    }

    /// Appends one command.
    pub fn push(&mut self, command: WorkspaceCommand) {
        self.commands.push(command);
    }

    /// Returns the ordered command sequence.
    pub fn commands(&self) -> &[WorkspaceCommand] {
        &self.commands
    }

    /// Edits one actual candidate and publishes it after one final normalization.
    ///
    /// Each push captures from its completed prefix. A failed push poisons the
    /// stage even when the editing closure ignores its Result. Dropping the
    /// stage or failing finalization leaves the live workspace unchanged.
    ///
    /// # Errors
    ///
    /// Returns the first capture/execution error, the editing error, or the
    /// existing final canonicalization/item reconciliation error.
    pub fn apply_with(
        workspace: &mut Workspace,
        policy: &DockPolicySnapshot,
        edit: impl FnOnce(&mut WorkspaceTransactionStage<'_>) -> Result<(), TransactionError>,
    ) -> Result<TransactionReport, TransactionError> {
        let prepared = crate::operation::prepare_transaction_with(workspace, policy, edit)?;
        Ok(prepared.publish(workspace))
    }

    /// Stages and atomically publishes this transaction.
    ///
    /// # Errors
    ///
    /// Returns [`TransactionError`] without changing `workspace` on any failure.
    pub fn apply(
        &self,
        workspace: &mut Workspace,
        policy: &DockPolicySnapshot,
    ) -> Result<TransactionReport, TransactionError> {
        let prepared = crate::operation::prepare_transaction(workspace, policy, &self.commands)?;
        Ok(prepared.publish(workspace))
    }
}

/// Incremental construction of one checked transaction candidate.
///
/// This stage exposes no mutable or canonical Workspace. Its only edit enters
/// the same checked executor used by a literal command batch.
pub struct WorkspaceTransactionStage<'policy> {
    draft: crate::operation::TransactionDraft<'policy>,
}

impl<'policy> WorkspaceTransactionStage<'policy> {
    pub(crate) fn new(draft: crate::operation::TransactionDraft<'policy>) -> Self {
        Self { draft }
    }

    /// Captures and executes one command against the actual current prefix.
    ///
    /// # Errors
    ///
    /// Remembers the first failure and refuses all further pushes.
    pub fn push(
        &mut self,
        capture: impl FnOnce(
            TransactionCapture<'_>,
        ) -> Result<WorkspaceCommand, crate::error::CommandError>,
    ) -> Result<(), TransactionError> {
        self.draft.capture_and_push(capture)
    }

    pub(crate) fn finish(
        self,
        original: &Workspace,
    ) -> Result<PreparedTransaction, TransactionError> {
        self.draft.finish(original)
    }
}

/// Read/capture facet of an actual, potentially not-yet-normalized prefix.
/// It cannot be passed to layout/scene compilation or converted into Workspace.
#[derive(Clone, Copy)]
pub struct TransactionCapture<'draft> {
    workspace: &'draft Workspace,
}

impl<'draft> TransactionCapture<'draft> {
    pub(crate) fn new(workspace: &'draft Workspace) -> Self {
        Self { workspace }
    }

    /// Reads one actual runtime node in this prefix.
    pub fn node(self, node: crate::ids::NodeId) -> Option<&'draft crate::graph::Node> {
        self.workspace.node(node)
    }
    /// Reads one actual root record in this prefix.
    pub fn root(self, root: crate::ids::RootId) -> Option<&'draft crate::graph::RootRecord> {
        self.workspace.root(root)
    }
    /// Reads one actual selected-first tab history in this prefix.
    pub fn tab_mru(self, tabs: crate::ids::NodeId) -> Option<&'draft [crate::ids::ItemId]> {
        self.workspace.tab_mru(tabs)
    }
    /// Captures one current item source.
    pub fn capture_item_source(
        self,
        root: crate::ids::RootId,
        tabs: crate::ids::NodeId,
        item: crate::ids::ItemId,
    ) -> Result<crate::command::ItemSource, crate::error::CommandError> {
        self.workspace.capture_item_source(root, tabs, item)
    }
    /// Captures one current node source.
    pub fn capture_node_source(
        self,
        root: crate::ids::RootId,
        node: crate::ids::NodeId,
    ) -> Result<crate::command::NodeSource, crate::error::CommandError> {
        self.workspace.capture_node_source(root, node)
    }
    /// Captures one current pane target and its actual selected-item rule.
    pub fn capture_tab_target(
        self,
        root: crate::ids::RootId,
        tabs: crate::ids::NodeId,
    ) -> Result<crate::command::TabTarget, crate::error::CommandError> {
        self.workspace.capture_tab_target(root, tabs)
    }
    /// Captures one current inner-edge target.
    pub fn capture_inner_edge_target(
        self,
        root: crate::ids::RootId,
        node: crate::ids::NodeId,
        edge: crate::command::Edge,
        fraction: crate::command::DockFraction,
    ) -> Result<crate::command::EdgeTarget, crate::error::CommandError> {
        self.workspace
            .capture_inner_edge_target(root, node, edge, fraction)
    }
    /// Captures one current outer-edge target.
    pub fn capture_outer_edge_target(
        self,
        root: crate::ids::RootId,
        edge: crate::command::Edge,
        fraction: crate::command::DockFraction,
    ) -> Result<crate::command::EdgeTarget, crate::error::CommandError> {
        self.workspace
            .capture_outer_edge_target(root, edge, fraction)
    }
}

impl FromIterator<WorkspaceCommand> for WorkspaceTransaction {
    fn from_iter<T: IntoIterator<Item = WorkspaceCommand>>(iter: T) -> Self {
        Self::from_commands(iter)
    }
}

/// Result published by one successful transaction.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TransactionReport {
    outcomes: Vec<CommandOutcome>,
    changed: bool,
}

impl TransactionReport {
    pub(crate) fn new(outcomes: Vec<CommandOutcome>, changed: bool) -> Self {
        Self { outcomes, changed }
    }

    /// Returns per-command outcomes in command order.
    pub fn outcomes(&self) -> &[CommandOutcome] {
        &self.outcomes
    }

    /// Returns whether the complete canonical workspace changed.
    pub const fn changed(&self) -> bool {
        self.changed
    }

    /// Consumes the report and returns per-command outcomes.
    pub fn into_outcomes(self) -> Vec<CommandOutcome> {
        self.outcomes
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::canonical::CanonicalizationError;
    use crate::command::{ContainedPosition, DockTarget, MovePayload, RootContent};
    use crate::drop_resolver::structural_work::{self, WorkspaceCloneVolume};
    use crate::geometry::LogicalRect;
    use crate::graph::{Axis, Node, RootRecord, SurfacePresentation};
    use crate::ids::{FloatingPresentationId, ItemId, NodeId, RootId, SurfaceId};
    use crate::operation::prepare_transaction;
    use crate::validation::WorkspaceValidationError;

    const SOURCE: RootId = RootId::new(1);
    const TARGET: RootId = RootId::new(2);
    const TARGET_SURFACE: SurfaceId = SurfaceId::new(2);

    fn moving_fixture() -> (Workspace, WorkspaceCommand, NodeId) {
        let mut builder = Workspace::builder();
        let source = builder.insert_node(Node::tabs([ItemId::new(1)]));
        let central = builder.insert_node(Node::tabs([]));
        let split = builder.insert_node(
            Node::equal_split(Axis::Horizontal, [source, central]).expect("valid split"),
        );
        let target = builder.insert_node(Node::tabs([ItemId::new(2)]));
        builder.set_root(SOURCE, RootRecord::new(split).with_central(central));
        builder.set_root(TARGET, RootRecord::new(target));
        builder.set_surface(SurfaceId::new(1), SurfacePresentation::with_main(SOURCE));
        builder.set_surface(TARGET_SURFACE, SurfacePresentation::with_main(TARGET));
        let workspace = builder.build().expect("valid move fixture");
        let source = workspace
            .capture_item_source(SOURCE, source, ItemId::new(1))
            .expect("source exists");
        let target = workspace
            .capture_tab_target(TARGET, target)
            .expect("target exists");
        let command = WorkspaceCommand::Move {
            payload: MovePayload::Item(source),
            target: DockTarget::Center(target),
        };
        (workspace, command, central)
    }

    #[test]
    fn prepared_candidate_can_be_discarded_and_retried_with_one_clone_each() {
        let (mut workspace, command, central) = moving_fixture();
        let original = workspace.clone();
        let volume = WorkspaceCloneVolume::capture(&workspace);
        let policy = DockPolicySnapshot::default();

        structural_work::reset();
        let prepared = prepare_transaction(&workspace, &policy, std::slice::from_ref(&command))
            .expect("move prepares");
        let (candidate, report) = prepared.into_parts();
        assert_eq!(workspace, original);
        assert_eq!(
            candidate.root(SOURCE),
            Some(&RootRecord::new(central).with_central(central))
        );
        assert_eq!(candidate.nodes().count(), 2);
        assert_eq!(candidate.item_multiset(), original.item_multiset());
        let target = candidate.root(TARGET).expect("target survives").node;
        assert_eq!(
            candidate.tab_mru(target),
            Some([ItemId::new(1), ItemId::new(2)].as_slice())
        );
        candidate
            .validate()
            .expect("prepared candidate is canonical");
        assert!(report.changed());
        assert!(matches!(
            report.outcomes(),
            [CommandOutcome::Moved { items, source_root: SOURCE, target_root: TARGET, changed: true }]
                if items == &[ItemId::new(1)]
        ));
        drop(candidate);
        let clones = structural_work::snapshot().workspace_deep_clones;
        assert_eq!(clones.transaction_candidates.calls, 1);
        assert_eq!(clones.transaction_candidates.volume, volume);

        structural_work::reset();
        let applied = WorkspaceTransaction::from_commands([command])
            .apply(&mut workspace, &policy)
            .expect("discarded preparation leaves the command retryable");
        assert_eq!(applied, report);
        assert_eq!(
            workspace.root(SOURCE),
            Some(&RootRecord::new(central).with_central(central))
        );
        let clones = structural_work::snapshot().workspace_deep_clones;
        assert_eq!(clones.transaction_candidates.calls, 1);
        assert_eq!(clones.transaction_candidates.volume, volume);
    }

    #[test]
    fn late_output_failure_rolls_back_topology_and_item_delta_before_retry() {
        let (mut workspace, movement, central) = moving_fixture();
        let original = workspace.clone();
        let volume = WorkspaceCloneVolume::capture(&workspace);
        let policy = DockPolicySnapshot::default();
        let floating = FloatingPresentationId::new(3);
        let mut commands = [
            movement,
            WorkspaceCommand::CreateContainedRoot {
                surface: TARGET_SURFACE,
                root: RootId::new(3),
                floating,
                rect: LogicalRect::new(0.0, 0.0, 0.0, 240.0).expect("zero width is representable"),
                position: ContainedPosition::Front,
                content: RootContent::OpenItem(ItemId::new(3)),
            },
        ];

        for _ in 0..2 {
            structural_work::reset();
            let error = WorkspaceTransaction::from_commands(commands.iter().cloned())
                .apply(&mut workspace, &policy)
                .expect_err("invalid geometry fails after topology normalization");
            assert!(matches!(
                error,
                TransactionError::Canonicalization(CanonicalizationError::InvalidOutput(errors))
                    if errors.errors().iter().any(|error| matches!(
                        error,
                        WorkspaceValidationError::NonPositiveContainedRect { floating: actual, .. }
                            if *actual == floating
                    ))
            ));
            assert_eq!(workspace, original);
            let clones = structural_work::snapshot().workspace_deep_clones;
            assert_eq!(clones.transaction_candidates.calls, 1);
            assert_eq!(clones.transaction_candidates.volume, volume);
        }

        let WorkspaceCommand::CreateContainedRoot { rect, .. } = &mut commands[1] else {
            unreachable!("second command creates the contained root");
        };
        *rect = LogicalRect::new(0.0, 0.0, 320.0, 240.0).expect("positive area is valid");
        structural_work::reset();
        let prepared =
            prepare_transaction(&workspace, &policy, &commands).expect("corrected batch prepares");
        let report = prepared.publish(&mut workspace);
        assert!(report.changed());
        assert!(matches!(
            report.outcomes(),
            [
                CommandOutcome::Moved { .. },
                CommandOutcome::ContainedRootCreated { .. }
            ]
        ));
        assert_eq!(
            workspace.root(SOURCE),
            Some(&RootRecord::new(central).with_central(central))
        );
        assert_eq!(
            workspace.item_multiset(),
            [
                (ItemId::new(1), 1),
                (ItemId::new(2), 1),
                (ItemId::new(3), 1)
            ]
            .into()
        );
        workspace
            .validate()
            .expect("retried batch publishes a strict graph");
        let clones = structural_work::snapshot().workspace_deep_clones;
        assert_eq!(clones.transaction_candidates.calls, 1);
        assert_eq!(clones.transaction_candidates.volume, volume);
    }

    #[test]
    fn consuming_a_prepared_noop_preserves_both_changed_flags() {
        let (workspace, _, _) = moving_fixture();
        let tabs = workspace.root(TARGET).expect("target exists").node;
        let command = WorkspaceCommand::Move {
            payload: MovePayload::Tabs(
                workspace
                    .capture_node_source(TARGET, tabs)
                    .expect("source exists"),
            ),
            target: DockTarget::Center(
                workspace
                    .capture_tab_target(TARGET, tabs)
                    .expect("target exists"),
            ),
        };

        structural_work::reset();
        let prepared = prepare_transaction(&workspace, &DockPolicySnapshot::default(), &[command])
            .expect("self-center move is a checked no-op");
        let (candidate, report) = prepared.into_parts();
        assert_eq!(candidate, workspace);
        assert!(!report.changed());
        assert!(matches!(
            report.outcomes(),
            [CommandOutcome::Moved { changed: false, .. }]
        ));
        assert_eq!(
            structural_work::snapshot()
                .workspace_deep_clones
                .transaction_candidates
                .calls,
            1
        );
    }
}
