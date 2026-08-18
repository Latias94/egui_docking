//! Bounded application-owned product action ingress.

use dockspace::model::{DockspaceActionOutcome, WorkspaceVersion};
use dockspace::runtime::{
    DockspacePresentationTransition, DockspacePresentationTransitionId,
    DockspacePresentationTransitionResult, HostInputOutcome, PreparedDockAction,
};
use egui_dockspace::DockspaceActionStatus;

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

/// Single-slot queue consumed only by the final root pass.
#[derive(Debug, Default)]
pub(crate) struct NativeApplicationActions {
    pending: Option<PreparedDockAction>,
    awaiting: Option<AwaitingApplicationAction>,
    result: Option<DockspaceActionStatus>,
}

#[derive(Debug)]
enum AwaitingApplicationAction {
    Reduction(WorkspaceVersion),
    Presentation {
        requested: DockspaceActionOutcome,
        transition: DockspacePresentationTransitionId,
    },
}

impl NativeApplicationActions {
    pub(crate) fn enqueue(
        &mut self,
        action: PreparedDockAction,
    ) -> Result<(), NativeActionRequestError> {
        if self.is_occupied() {
            return Err(NativeActionRequestError::busy());
        }
        self.pending = Some(action);
        Ok(())
    }

    pub(crate) const fn is_occupied(&self) -> bool {
        self.pending.is_some() || self.awaiting.is_some() || self.result.is_some()
    }

    pub(crate) const fn has_queued(&self) -> bool {
        self.pending.is_some()
    }

    pub(crate) fn take(&mut self) -> Option<PreparedDockAction> {
        let action = self.pending.take()?;
        self.awaiting = Some(AwaitingApplicationAction::Reduction(
            action.expected_version(),
        ));
        Some(action)
    }

    pub(crate) fn settle(
        &mut self,
        inputs: &[HostInputOutcome],
        transitions: &[DockspacePresentationTransition],
    ) -> Result<bool, ()> {
        let Some(awaiting) = self.awaiting.as_ref() else {
            return Ok(false);
        };
        match awaiting {
            AwaitingApplicationAction::Reduction(expected) => {
                let Some(settlement) = inputs.iter().find_map(|input| match input {
                    HostInputOutcome::ProductActionApplied(outcome) => {
                        Some(ReductionSettlement::Terminal(
                            DockspaceActionStatus::Applied(outcome.clone()),
                        ))
                    }
                    HostInputOutcome::ProductPresentationActionRequested {
                        outcome,
                        transition,
                    } => Some(ReductionSettlement::Presentation {
                        requested: outcome.clone(),
                        transition: *transition,
                    }),
                    HostInputOutcome::ProductActionRejected(reason) => Some(
                        ReductionSettlement::Terminal(DockspaceActionStatus::Rejected(*reason)),
                    ),
                    HostInputOutcome::StaleRejected {
                        expected: stale,
                        accepted,
                    } if *stale == *expected => Some(ReductionSettlement::Terminal(
                        DockspaceActionStatus::Stale {
                            expected: *stale,
                            accepted: *accepted,
                        },
                    )),
                    _ => None,
                }) else {
                    return Err(());
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
                            return Err(());
                        }
                        self.awaiting = None;
                        self.result = Some(status);
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
                            return Err(());
                        }
                        self.awaiting = Some(AwaitingApplicationAction::Presentation {
                            requested,
                            transition,
                        });
                        self.settle_presentation(transitions).map(|_| true)
                    }
                }
            }
            AwaitingApplicationAction::Presentation { .. } => self.settle_presentation(transitions),
        }
    }

    pub(crate) fn settle_presentation_transitions(
        &mut self,
        transitions: &[DockspacePresentationTransition],
    ) -> Result<bool, ()> {
        if !matches!(
            self.awaiting,
            Some(AwaitingApplicationAction::Presentation { .. })
        ) {
            return Ok(false);
        }
        self.settle_presentation(transitions)
    }

    fn settle_presentation(
        &mut self,
        transitions: &[DockspacePresentationTransition],
    ) -> Result<bool, ()> {
        let Some(AwaitingApplicationAction::Presentation {
            requested,
            transition: requested_transition,
        }) = self.awaiting.as_ref()
        else {
            return Ok(false);
        };
        let Some(transition) = transitions.iter().find(|transition| {
            transition_matches_request(transition, *requested_transition, requested)
        }) else {
            return Ok(false);
        };
        let status = if transition.result() == DockspacePresentationTransitionResult::Applied {
            DockspaceActionStatus::Applied(
                completed_presentation_outcome(requested, transition).ok_or(())?,
            )
        } else {
            DockspaceActionStatus::PresentationFailed {
                root: transition.root(),
                source_surface: transition.source_surface(),
                target_surface: transition.target_surface(),
                reason: transition.result(),
            }
        };
        self.awaiting = None;
        self.result = Some(status);
        Ok(true)
    }

    pub(crate) fn take_result(&mut self) -> Option<DockspaceActionStatus> {
        self.result.take()
    }

    pub(crate) fn abandon_unsettled(&mut self) {
        self.pending = None;
        self.awaiting = None;
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
