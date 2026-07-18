//! Single-writer input queue and atomic headless state reducer.

use thiserror::Error;

use crate::command::WorkspaceCommand;
use crate::error::TransactionError;
use crate::event::{WorkspaceEvent, WorkspaceEventKind};
use crate::graph::Workspace;
use crate::ids::{InputSequence, WorkspaceRevision};
use crate::policy::DockPolicy;
use crate::transaction::WorkspaceTransaction;
use crate::transition::{
    EngineTransition, InputOutcome, InputPriority, ReducedInput, WorkspaceVersion,
};
use crate::validation::WorkspaceValidationErrors;

/// Input accepted by the U3 engine boundary.
#[derive(Debug, Clone, PartialEq)]
pub enum EngineInput {
    /// Authoritatively replace the complete workspace.
    ReplaceWorkspace(Workspace),
    /// Apply one checked command derived from an exact engine version.
    WorkspaceCommand {
        /// Version from which source and target references were captured.
        expected: WorkspaceVersion,
        /// Checked durable mutation.
        command: WorkspaceCommand,
    },
    /// Replace application policy if the input is still current.
    ReplacePolicy {
        /// Version observed when the application chose the policy.
        expected: WorkspaceVersion,
        /// Complete replacement policy.
        policy: DockPolicy,
    },
    /// Re-run strict validation without changing state.
    ValidateWorkspace,
}

impl EngineInput {
    /// Returns the fixed source-class priority used during reduction.
    #[must_use]
    pub const fn priority(&self) -> InputPriority {
        match self {
            Self::ReplaceWorkspace(_) => InputPriority::LifecycleControl,
            Self::WorkspaceCommand { .. } | Self::ReplacePolicy { .. } => {
                InputPriority::ApplicationCommand
            }
            Self::ValidateWorkspace => InputPriority::Maintenance,
        }
    }
}

/// Input after assignment by the engine's sole sequence writer.
#[derive(Debug, Clone, PartialEq)]
pub struct SequencedInput {
    sequence: InputSequence,
    input: EngineInput,
}

impl SequencedInput {
    /// Returns the monotonic writer sequence.
    #[must_use]
    pub const fn sequence(&self) -> InputSequence {
        self.sequence
    }

    /// Returns the typed input payload.
    #[must_use]
    pub const fn input(&self) -> &EngineInput {
        &self.input
    }
}

/// Failure to construct or atomically reduce engine state.
#[derive(Debug, Clone, PartialEq, Error)]
pub enum EngineError {
    /// Initial or replacement workspace violated a durable invariant.
    #[error("workspace is invalid: {0}")]
    InvalidWorkspace(#[source] WorkspaceValidationErrors),
    /// The single-writer input counter cannot advance without wrapping.
    #[error("engine input sequence is exhausted")]
    InputSequenceExhausted,
    /// A workspace replacement epoch cannot advance without wrapping.
    #[error("workspace epoch is exhausted while reducing input {input}")]
    WorkspaceEpochExhausted {
        /// Input which attempted replacement.
        input: InputSequence,
    },
    /// A state revision cannot advance without wrapping.
    #[error("workspace revision is exhausted while reducing input {input}")]
    WorkspaceRevisionExhausted {
        /// Input which attempted mutation.
        input: InputSequence,
    },
    /// A checked command transaction failed; no engine state was published.
    #[error("workspace command input {input} failed: {source}")]
    Command {
        /// Failing sequenced input.
        input: InputSequence,
        /// Atomic transaction failure.
        source: crate::error::TransactionError,
    },
    /// A successful one-command transaction omitted its required outcome.
    #[error("workspace command input {input} produced no command outcome")]
    MissingCommandOutcome {
        /// Input whose transaction violated the engine contract.
        input: InputSequence,
    },
}

/// Authoritative renderer-neutral docking engine.
#[derive(Debug, PartialEq)]
pub struct DockEngine {
    workspace: Workspace,
    policy: DockPolicy,
    version: WorkspaceVersion,
    last_input: InputSequence,
    pending: Vec<SequencedInput>,
}

impl DockEngine {
    /// Creates an engine from a strictly validated workspace and explicit policy.
    ///
    /// # Errors
    ///
    /// Returns [`EngineError::InvalidWorkspace`] if `workspace` is corrupted.
    pub fn new(workspace: Workspace, policy: DockPolicy) -> Result<Self, EngineError> {
        workspace
            .validate()
            .map_err(EngineError::InvalidWorkspace)?;
        Ok(Self {
            workspace,
            policy,
            version: WorkspaceVersion::default(),
            last_input: InputSequence::default(),
            pending: Vec::new(),
        })
    }

    /// Returns the published workspace.
    #[must_use]
    pub const fn workspace(&self) -> &Workspace {
        &self.workspace
    }

    /// Returns current application policy.
    #[must_use]
    pub const fn policy(&self) -> &DockPolicy {
        &self.policy
    }

    /// Returns the version required by state-derived inputs.
    #[must_use]
    pub const fn version(&self) -> WorkspaceVersion {
        self.version
    }

    /// Returns queued inputs in writer order.
    #[must_use]
    pub fn pending_inputs(&self) -> &[SequencedInput] {
        &self.pending
    }

    /// Assigns the next monotonic sequence and queues an input.
    ///
    /// # Errors
    ///
    /// Returns [`EngineError::InputSequenceExhausted`] instead of wrapping.
    pub fn enqueue(&mut self, input: EngineInput) -> Result<InputSequence, EngineError> {
        let sequence = self
            .last_input
            .checked_next()
            .ok_or(EngineError::InputSequenceExhausted)?;
        self.last_input = sequence;
        self.pending.push(SequencedInput { sequence, input });
        Ok(sequence)
    }

    /// Queues a command against the currently published state version.
    ///
    /// # Errors
    ///
    /// Returns [`EngineError::InputSequenceExhausted`] instead of wrapping.
    pub fn enqueue_command(
        &mut self,
        command: WorkspaceCommand,
    ) -> Result<InputSequence, EngineError> {
        self.enqueue(EngineInput::WorkspaceCommand {
            expected: self.version,
            command,
        })
    }

    /// Queues a complete authoritative workspace replacement.
    ///
    /// # Errors
    ///
    /// Returns [`EngineError::InputSequenceExhausted`] instead of wrapping.
    pub fn enqueue_workspace_replacement(
        &mut self,
        workspace: Workspace,
    ) -> Result<InputSequence, EngineError> {
        self.enqueue(EngineInput::ReplaceWorkspace(workspace))
    }

    /// Discards all queued inputs without changing published state.
    pub fn discard_pending(&mut self) -> Vec<SequencedInput> {
        std::mem::take(&mut self.pending)
    }

    /// Reduces queued inputs into one candidate and publishes it atomically.
    ///
    /// Inputs are ordered by [`InputPriority`] and then writer sequence. Events
    /// are returned only if the complete candidate commits. Expected command
    /// rejections are consumed and recorded in the transition. A fatal internal
    /// transaction error leaves the engine, including its pending queue, unchanged.
    ///
    /// # Errors
    ///
    /// Returns [`EngineError`] on counter exhaustion, invalid replacement state,
    /// or an internal transaction failure. No engine state is changed on failure.
    pub fn reduce_pending(&mut self) -> Result<EngineTransition, EngineError> {
        let before = self.version;
        let mut candidate = self.candidate();
        let mut inputs = std::mem::take(&mut candidate.pending);
        inputs.sort_by_key(|input| (input.input.priority(), input.sequence));

        let mut reduced = Vec::with_capacity(inputs.len());
        let mut events = Vec::new();
        let mut application_base = before;
        for input in inputs {
            let priority = input.input.priority();
            let outcome = candidate.reduce_one(&input, &mut application_base, &mut events)?;
            reduced.push(ReducedInput::new(input.sequence, priority, outcome));
        }

        let transition = EngineTransition::new(before, candidate.version, reduced, events);
        *self = candidate;
        Ok(transition)
    }

    fn reduce_one(
        &mut self,
        input: &SequencedInput,
        application_base: &mut WorkspaceVersion,
        events: &mut Vec<WorkspaceEvent>,
    ) -> Result<InputOutcome, EngineError> {
        match &input.input {
            EngineInput::ReplaceWorkspace(workspace) => {
                workspace
                    .validate()
                    .map_err(EngineError::InvalidWorkspace)?;
                let before = self.version;
                let epoch =
                    before
                        .epoch()
                        .checked_next()
                        .ok_or(EngineError::WorkspaceEpochExhausted {
                            input: input.sequence,
                        })?;
                self.workspace = workspace.clone();
                self.version = WorkspaceVersion::new(epoch, WorkspaceRevision::default());
                *application_base = self.version;
                events.push(WorkspaceEvent::new(
                    input.sequence,
                    self.version,
                    WorkspaceEventKind::WorkspaceReplaced,
                ));
                Ok(InputOutcome::WorkspaceReplaced {
                    before,
                    after: self.version,
                })
            }
            EngineInput::WorkspaceCommand { expected, command } => self.reduce_workspace_command(
                input.sequence,
                *expected,
                *application_base,
                command,
                events,
            ),
            EngineInput::ReplacePolicy { expected, policy } => {
                if *expected != *application_base {
                    return Ok(InputOutcome::StaleRejected {
                        expected: *expected,
                        accepted_base: *application_base,
                    });
                }
                let changed = self.policy != *policy;
                if changed {
                    self.policy = policy.clone();
                    self.advance_revision(input.sequence)?;
                    events.push(WorkspaceEvent::new(
                        input.sequence,
                        self.version,
                        WorkspaceEventKind::PolicyReplaced,
                    ));
                }
                Ok(InputOutcome::PolicyReplaced {
                    changed,
                    version: self.version,
                })
            }
            EngineInput::ValidateWorkspace => {
                self.workspace
                    .validate()
                    .map_err(EngineError::InvalidWorkspace)?;
                Ok(InputOutcome::WorkspaceValidated {
                    version: self.version,
                })
            }
        }
    }

    fn advance_revision(&mut self, input: InputSequence) -> Result<(), EngineError> {
        let revision = self
            .version
            .revision()
            .checked_next()
            .ok_or(EngineError::WorkspaceRevisionExhausted { input })?;
        self.version = WorkspaceVersion::new(self.version.epoch(), revision);
        Ok(())
    }

    fn reduce_workspace_command(
        &mut self,
        input: InputSequence,
        expected: WorkspaceVersion,
        application_base: WorkspaceVersion,
        command: &WorkspaceCommand,
        events: &mut Vec<WorkspaceEvent>,
    ) -> Result<InputOutcome, EngineError> {
        if expected != application_base {
            return Ok(InputOutcome::StaleRejected {
                expected,
                accepted_base: application_base,
            });
        }

        let report = match WorkspaceTransaction::from_commands([command.clone()])
            .apply(&mut self.workspace, &self.policy)
        {
            Ok(report) => report,
            Err(TransactionError::Command { index: 0, source })
                if source.is_expected_rejection() =>
            {
                return Ok(InputOutcome::CommandRejected {
                    error: source,
                    version: self.version,
                });
            }
            Err(source) => return Err(EngineError::Command { input, source }),
        };
        let changed = report.changed();
        if changed {
            self.advance_revision(input)?;
        }
        let outcome = report
            .into_outcomes()
            .into_iter()
            .next()
            .ok_or(EngineError::MissingCommandOutcome { input })?;
        if changed {
            events.push(WorkspaceEvent::new(
                input,
                self.version,
                WorkspaceEventKind::CommandCommitted(outcome.clone()),
            ));
        }
        Ok(InputOutcome::CommandProcessed {
            outcome,
            changed,
            version: self.version,
        })
    }

    fn candidate(&self) -> Self {
        Self {
            workspace: self.workspace.clone(),
            policy: self.policy.clone(),
            version: self.version,
            last_input: self.last_input,
            pending: self.pending.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::{Node, RootRecord, SurfacePresentation};
    use crate::ids::{ItemId, RootId, SurfaceId, WorkspaceEpoch};

    #[test]
    fn fatal_counter_exhaustion_rolls_back_the_complete_boundary() {
        let root = RootId::new(1);
        let surface = SurfaceId::new(1);
        let mut builder = Workspace::builder();
        let tabs = builder.insert_node(Node::tabs([ItemId::new(1), ItemId::new(2)]));
        builder.set_root(root, RootRecord::new(tabs));
        builder.set_surface(surface, SurfacePresentation::new(root));
        let workspace = builder.build().expect("test workspace must be valid");
        let command = WorkspaceCommand::Select {
            source: workspace
                .capture_item_source(root, tabs, ItemId::new(2))
                .expect("source must be capturable"),
        };
        let mut engine = DockEngine::new(workspace.clone(), DockPolicy::default())
            .expect("engine must be valid");
        engine.version = WorkspaceVersion::new(
            WorkspaceEpoch::default(),
            WorkspaceRevision::new(u64::MAX - 1),
        );
        let expected = engine.version;
        let mut policy = engine.policy.clone();
        policy.set_allow_native_surfaces(true);
        engine
            .enqueue(EngineInput::ReplacePolicy { expected, policy })
            .expect("first sequence must be available");
        engine
            .enqueue(EngineInput::WorkspaceCommand { expected, command })
            .expect("second sequence must be available");
        let pending = engine.pending.clone();

        assert!(matches!(
            engine.reduce_pending(),
            Err(EngineError::WorkspaceRevisionExhausted { .. })
        ));
        assert_eq!(engine.workspace, workspace);
        assert!(!engine.policy.allows_native_surfaces());
        assert_eq!(engine.version, expected);
        assert_eq!(engine.pending, pending);
    }
}
