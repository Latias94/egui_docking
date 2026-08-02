//! Atomic preparation and publication of checked workspace commands.

use crate::command::{CommandOutcome, WorkspaceCommand};
use crate::error::TransactionError;
use crate::graph::Workspace;
use crate::policy::DockPolicySnapshot;

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

    pub(crate) fn preflight(
        &self,
        workspace: &Workspace,
        policy: &DockPolicySnapshot,
    ) -> Result<(), TransactionError> {
        crate::operation::prepare_transaction(workspace, policy, &self.commands).map(drop)
    }
}

impl FromIterator<WorkspaceCommand> for WorkspaceTransaction {
    fn from_iter<T: IntoIterator<Item = WorkspaceCommand>>(iter: T) -> Self {
        Self::from_commands(iter)
    }
}

#[derive(Debug)]
pub(crate) struct PreparedTransaction {
    pub(crate) candidate: Workspace,
    pub(crate) outcomes: Vec<CommandOutcome>,
}

impl PreparedTransaction {
    pub(crate) fn publish(self, workspace: &mut Workspace) -> TransactionReport {
        let changed = self.candidate != *workspace;
        *workspace = self.candidate;
        TransactionReport {
            outcomes: self.outcomes,
            changed,
        }
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
