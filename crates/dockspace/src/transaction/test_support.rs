//! Test-only command batch and capture conveniences over the production executor.

use super::*;
use crate::command::WorkspaceCommand;
use crate::error::TransactionError;
use crate::graph::WorkspaceData;
use crate::operation::PreparedTransaction;
use crate::policy::DockPolicySnapshot;

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
    /// Creates a transaction from an ordered command sequence.
    pub fn from_commands(commands: impl IntoIterator<Item = WorkspaceCommand>) -> Self {
        Self {
            commands: commands.into_iter().collect(),
        }
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
}

impl FromIterator<WorkspaceCommand> for WorkspaceTransaction {
    fn from_iter<T: IntoIterator<Item = WorkspaceCommand>>(iter: T) -> Self {
        Self::from_commands(iter)
    }
}
