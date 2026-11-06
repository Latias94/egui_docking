//! Atomic preparation and publication of checked workspace commands.

use crate::command::{CommandOutcome, WorkspaceCommand};
use crate::error::TransactionError;
use crate::graph::Workspace;
use crate::policy::DockPolicySnapshot;

/// Copies one complete transaction or atomic canonicalization draft, accounting
/// for its full structural volume in the existing test-only clone ledger.
/// Both use the `transaction_candidates` counter, including standalone wrappers.
pub(crate) fn clone_workspace_draft(workspace: &Workspace) -> Workspace {
    #[cfg(test)]
    crate::drop_resolver::structural_work::record_transaction_candidate_clone(workspace);
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

impl FromIterator<WorkspaceCommand> for WorkspaceTransaction {
    fn from_iter<T: IntoIterator<Item = WorkspaceCommand>>(iter: T) -> Self {
        Self::from_commands(iter)
    }
}

#[derive(Debug)]
pub(crate) struct PreparedTransaction {
    pub(crate) candidate: crate::operation::CanonicalWorkspace,
    pub(crate) outcomes: Vec<CommandOutcome>,
}

impl PreparedTransaction {
    /// Consumes the prepared candidate and its report without publishing it.
    ///
    /// `original` is the snapshot used for preparation. The engine can check its
    /// publication barriers against the candidate before moving it into place.
    pub(crate) fn into_parts(self, original: &Workspace) -> (Workspace, TransactionReport) {
        let (candidate, report) = self.into_canonical_parts(original);
        (candidate.into_workspace(), report)
    }

    pub(crate) fn into_snapshot_parts(
        self,
        original: &Workspace,
    ) -> (crate::operation::WorkspaceSnapshot, TransactionReport) {
        let (candidate, report) = self.into_canonical_parts(original);
        (candidate.into(), report)
    }

    fn into_canonical_parts(
        self,
        original: &Workspace,
    ) -> (crate::operation::CanonicalWorkspace, TransactionReport) {
        let report = TransactionReport {
            outcomes: self.outcomes,
            changed: *self.candidate != *original,
        };
        (self.candidate, report)
    }

    pub(crate) fn publish(self, workspace: &mut Workspace) -> TransactionReport {
        let (candidate, report) = self.into_parts(workspace);
        *workspace = candidate;
        report
    }
}

/// Result published by one successful transaction.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TransactionReport {
    outcomes: Vec<CommandOutcome>,
    changed: bool,
}

impl TransactionReport {
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
        let (candidate, report) = prepared.into_parts(&workspace);
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
        let (candidate, report) = prepared.into_parts(&workspace);
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
