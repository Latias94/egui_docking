//! Bounded application-owned product action ingress.

use dockspace::model::{DockspaceActionOutcome, WorkspaceVersion};
use dockspace::runtime::{
    DockspacePresentationTransition, DockspacePresentationTransitionId,
    DockspacePresentationTransitionResult, HostInputOutcome, PreparedDockAction,
};
use egui_dockspace::DockspaceActionStatus;

use crate::NativeRuntimeErrorKind;

/// Failure to queue one application-owned product action.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum NativeActionRequestError {
    /// Another application action is waiting for the next final root pass.
    Busy,
    /// The native runtime has entered terminal shutdown.
    Stopped,
}

impl NativeActionRequestError {
    pub(crate) const fn busy() -> Self {
        Self::Busy
    }

    pub(crate) const fn stopped() -> Self {
        Self::Stopped
    }
}

impl std::fmt::Display for NativeActionRequestError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let message = match self {
            Self::Busy => "a native dockspace application action is already pending",
            Self::Stopped => "the native dockspace runtime is stopped",
        };
        formatter.write_str(message)
    }
}

impl std::error::Error for NativeActionRequestError {}

/// Stage at which one accepted native application action was interrupted.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum NativeActionInterruption {
    /// The action never committed one core host-frame boundary.
    NotCommitted,
    /// The core frame committed without the exact submitted-action outcome.
    CommittedOutcomeUnavailable,
    /// Shutdown drained without the exact presentation transition terminal.
    PresentationUnresolved,
}

/// Terminal native-host failure of one previously accepted application action.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NativeActionFailure {
    kind: NativeRuntimeErrorKind,
    interruption: NativeActionInterruption,
}

impl NativeActionFailure {
    const fn new(kind: NativeRuntimeErrorKind, interruption: NativeActionInterruption) -> Self {
        Self { kind, interruption }
    }

    /// Returns the stable native runtime failure category.
    #[must_use]
    pub const fn kind(self) -> NativeRuntimeErrorKind {
        self.kind
    }

    /// Returns the exact action stage interrupted by the native failure.
    #[must_use]
    pub const fn interruption(self) -> NativeActionInterruption {
        self.interruption
    }
}

impl std::fmt::Display for NativeActionFailure {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let stage = match self.interruption {
            NativeActionInterruption::NotCommitted => "before the action committed",
            NativeActionInterruption::CommittedOutcomeUnavailable => {
                "after commit omitted the exact action outcome"
            }
            NativeActionInterruption::PresentationUnresolved => {
                "while the presentation transition remained unresolved"
            }
        };
        write!(
            formatter,
            "native dockspace action stopped {stage} ({:?})",
            self.kind
        )
    }
}

impl std::error::Error for NativeActionFailure {}

pub(crate) type NativeActionTerminal = Result<DockspaceActionStatus, NativeActionFailure>;

/// Single-slot queue consumed only by the final root pass.
#[derive(Debug, Default)]
pub(crate) struct NativeApplicationActions {
    state: NativeApplicationActionState,
}

#[derive(Debug)]
enum NativeApplicationActionState {
    Vacant,
    Queued(PreparedDockAction),
    UncommittedFrame(WorkspaceVersion),
    AwaitingPresentation {
        requested: DockspaceActionOutcome,
        transition: DockspacePresentationTransitionId,
        shutdown_kind: Option<NativeRuntimeErrorKind>,
    },
    Terminal(NativeActionTerminal),
}

impl Default for NativeApplicationActionState {
    fn default() -> Self {
        Self::Vacant
    }
}

impl NativeApplicationActions {
    pub(crate) fn enqueue(
        &mut self,
        action: PreparedDockAction,
    ) -> Result<(), NativeActionRequestError> {
        if self.is_occupied() {
            return Err(NativeActionRequestError::busy());
        }
        self.state = NativeApplicationActionState::Queued(action);
        Ok(())
    }

    pub(crate) const fn is_occupied(&self) -> bool {
        !matches!(self.state, NativeApplicationActionState::Vacant)
    }

    pub(crate) const fn has_queued(&self) -> bool {
        matches!(self.state, NativeApplicationActionState::Queued(_))
    }

    pub(crate) fn take(&mut self) -> Option<PreparedDockAction> {
        let state = std::mem::take(&mut self.state);
        let NativeApplicationActionState::Queued(action) = state else {
            self.state = state;
            return None;
        };
        self.state = NativeApplicationActionState::UncommittedFrame(action.expected_version());
        Some(action)
    }

    pub(crate) fn settle(
        &mut self,
        input: Option<&HostInputOutcome>,
        transitions: &[DockspacePresentationTransition],
    ) -> Result<bool, ()> {
        match &self.state {
            NativeApplicationActionState::Vacant
            | NativeApplicationActionState::Queued(_)
            | NativeApplicationActionState::Terminal(_) => Ok(false),
            NativeApplicationActionState::UncommittedFrame(expected) => {
                let Some(input) = input else {
                    self.set_terminal_failure(
                        NativeRuntimeErrorKind::Internal,
                        NativeActionInterruption::CommittedOutcomeUnavailable,
                    );
                    return Err(());
                };
                let settlement = match input {
                    HostInputOutcome::ProductActionApplied(outcome) => {
                        ReductionSettlement::Terminal(DockspaceActionStatus::Applied(
                            outcome.clone(),
                        ))
                    }
                    HostInputOutcome::ProductPresentationActionRequested {
                        outcome,
                        transition,
                    } => ReductionSettlement::Presentation {
                        requested: outcome.clone(),
                        transition: *transition,
                    },
                    HostInputOutcome::ProductActionRejected(reason) => {
                        ReductionSettlement::Terminal(DockspaceActionStatus::Rejected(*reason))
                    }
                    HostInputOutcome::StaleRejected {
                        expected: stale,
                        accepted,
                    } if *stale == *expected => {
                        ReductionSettlement::Terminal(DockspaceActionStatus::Stale {
                            expected: *stale,
                            accepted: *accepted,
                        })
                    }
                    _ => {
                        self.set_terminal_failure(
                            NativeRuntimeErrorKind::Internal,
                            NativeActionInterruption::CommittedOutcomeUnavailable,
                        );
                        return Err(());
                    }
                };
                match settlement {
                    ReductionSettlement::Terminal(status) => {
                        if matches!(
                            status,
                            DockspaceActionStatus::Applied(
                                DockspaceActionOutcome::RootDockRequested { .. }
                                    | DockspaceActionOutcome::RootFloatRequested { .. }
                            )
                        ) {
                            self.set_terminal_failure(
                                NativeRuntimeErrorKind::Internal,
                                NativeActionInterruption::CommittedOutcomeUnavailable,
                            );
                            return Err(());
                        }
                        self.state = NativeApplicationActionState::Terminal(Ok(status));
                        Ok(true)
                    }
                    ReductionSettlement::Presentation {
                        requested,
                        transition,
                    } => {
                        if !matches!(
                            requested,
                            DockspaceActionOutcome::RootDockRequested { .. }
                                | DockspaceActionOutcome::RootFloatRequested { .. }
                        ) {
                            self.set_terminal_failure(
                                NativeRuntimeErrorKind::Internal,
                                NativeActionInterruption::CommittedOutcomeUnavailable,
                            );
                            return Err(());
                        }
                        self.state = NativeApplicationActionState::AwaitingPresentation {
                            requested,
                            transition,
                            shutdown_kind: None,
                        };
                        self.settle_presentation(transitions).map(|_| true)
                    }
                }
            }
            NativeApplicationActionState::AwaitingPresentation { .. } => {
                self.settle_presentation(transitions)
            }
        }
    }

    pub(crate) fn settle_presentation_transitions(
        &mut self,
        transitions: &[DockspacePresentationTransition],
    ) -> Result<bool, ()> {
        if !matches!(
            self.state,
            NativeApplicationActionState::AwaitingPresentation { .. }
        ) {
            return Ok(false);
        }
        self.settle_presentation(transitions)
    }

    fn settle_presentation(
        &mut self,
        transitions: &[DockspacePresentationTransition],
    ) -> Result<bool, ()> {
        let NativeApplicationActionState::AwaitingPresentation {
            requested,
            transition: requested_transition,
            ..
        } = &self.state
        else {
            return Ok(false);
        };
        let Some(transition) = transitions.iter().find(|transition| {
            transition_matches_request(transition, *requested_transition, requested)
        }) else {
            return Ok(false);
        };
        let status = if transition.result() == DockspacePresentationTransitionResult::Applied {
            let Some(outcome) = completed_presentation_outcome(requested, transition) else {
                self.set_terminal_failure(
                    NativeRuntimeErrorKind::Internal,
                    NativeActionInterruption::CommittedOutcomeUnavailable,
                );
                return Err(());
            };
            DockspaceActionStatus::Applied(outcome)
        } else {
            DockspaceActionStatus::PresentationFailed {
                root: transition.root(),
                source_surface: transition.source_surface(),
                target_surface: transition.target_surface(),
                reason: transition.result(),
            }
        };
        self.state = NativeApplicationActionState::Terminal(Ok(status));
        Ok(true)
    }

    pub(crate) fn take_result(&mut self) -> Option<NativeActionTerminal> {
        let state = std::mem::take(&mut self.state);
        let NativeApplicationActionState::Terminal(result) = state else {
            self.state = state;
            return None;
        };
        Some(result)
    }

    pub(crate) fn begin_shutdown(&mut self, kind: NativeRuntimeErrorKind) -> bool {
        if let NativeApplicationActionState::AwaitingPresentation { shutdown_kind, .. } =
            &mut self.state
        {
            if shutdown_kind.is_none() {
                *shutdown_kind = Some(kind);
                return true;
            }
            return false;
        }
        match self.state {
            NativeApplicationActionState::Queued(_)
            | NativeApplicationActionState::UncommittedFrame(_) => {
                self.set_terminal_failure(kind, NativeActionInterruption::NotCommitted);
                true
            }
            NativeApplicationActionState::AwaitingPresentation { .. } => unreachable!(
                "awaiting presentation shutdown state is handled before terminalization"
            ),
            NativeApplicationActionState::Vacant | NativeApplicationActionState::Terminal(_) => {
                false
            }
        }
    }

    pub(crate) fn finish_shutdown_exhausted(&mut self) -> bool {
        let kind = match &self.state {
            NativeApplicationActionState::AwaitingPresentation {
                shutdown_kind: Some(kind),
                ..
            } => *kind,
            _ => return false,
        };
        self.set_terminal_failure(kind, NativeActionInterruption::PresentationUnresolved);
        true
    }

    fn set_terminal_failure(
        &mut self,
        kind: NativeRuntimeErrorKind,
        interruption: NativeActionInterruption,
    ) {
        debug_assert!(!matches!(
            self.state,
            NativeApplicationActionState::Terminal(_)
        ));
        self.state = NativeApplicationActionState::Terminal(Err(NativeActionFailure::new(
            kind,
            interruption,
        )));
    }
}

enum ReductionSettlement {
    Terminal(DockspaceActionStatus),
    Presentation {
        requested: DockspaceActionOutcome,
        transition: DockspacePresentationTransitionId,
    },
}

fn transition_matches_request(
    transition: &DockspacePresentationTransition,
    requested_transition: DockspacePresentationTransitionId,
    requested: &DockspaceActionOutcome,
) -> bool {
    if transition.id() != requested_transition {
        return false;
    }
    match requested {
        DockspaceActionOutcome::RootDockRequested {
            root,
            source_surface,
            ..
        } => transition.root() == *root && transition.source_surface() == *source_surface,
        DockspaceActionOutcome::RootFloatRequested {
            root,
            source_surface,
            target_surface,
            ..
        } => {
            transition.root() == *root
                && transition.source_surface() == *source_surface
                && transition.target_surface() == *target_surface
        }
        _ => false,
    }
}

fn completed_presentation_outcome(
    requested: &DockspaceActionOutcome,
    transition: &DockspacePresentationTransition,
) -> Option<DockspaceActionOutcome> {
    let completed = transition.outcome()?;
    match (requested, completed) {
        (
            DockspaceActionOutcome::RootDockRequested {
                root,
                target_root,
                items,
                ..
            },
            DockspaceActionOutcome::RootDocked {
                root: completed_root,
                target_root: completed_target,
                items: completed_items,
                ..
            },
        ) if completed_root == root
            && completed_target == target_root
            && completed_items == items =>
        {
            Some(completed.clone())
        }
        (
            DockspaceActionOutcome::RootFloatRequested { root, items, .. },
            DockspaceActionOutcome::RootFloated {
                root: completed_root,
                surface,
                items: completed_items,
                ..
            },
        ) if completed_root == root
            && *surface == transition.target_surface()
            && completed_items == items =>
        {
            Some(completed.clone())
        }
        _ => None,
    }
}
