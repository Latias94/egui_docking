//! Bounded application-owned product action ingress.

use dockspace::model::{DockspaceActionOutcome, DockspaceView, WorkspaceVersion};
use dockspace::runtime::{
    DockspacePresentationTransition, DockspacePresentationTransitionResult, HostInputOutcome,
    PreparedDockAction,
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
    Presentation(DockspaceActionOutcome),
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
        view: DockspaceView<'_>,
    ) -> Result<bool, ()> {
        let Some(awaiting) = self.awaiting.as_ref() else {
            return Ok(false);
        };
        match awaiting {
            AwaitingApplicationAction::Reduction(expected) => {
                let Some(status) = inputs.iter().find_map(|input| match input {
                    HostInputOutcome::ProductActionApplied(outcome) => {
                        Some(DockspaceActionStatus::Applied(outcome.clone()))
                    }
                    HostInputOutcome::ProductActionRejected(reason) => {
                        Some(DockspaceActionStatus::Rejected(*reason))
                    }
                    HostInputOutcome::StaleRejected {
                        expected: stale,
                        accepted,
                    } if *stale == *expected => Some(DockspaceActionStatus::Stale {
                        expected: *stale,
                        accepted: *accepted,
                    }),
                    _ => None,
                }) else {
                    return Err(());
                };
                let DockspaceActionStatus::Applied(
                    requested @ (DockspaceActionOutcome::RootDockRequested { .. }
                    | DockspaceActionOutcome::RootFloatRequested { .. }),
                ) = status
                else {
                    self.awaiting = None;
                    self.result = Some(status);
                    return Ok(true);
                };
                self.awaiting = Some(AwaitingApplicationAction::Presentation(requested));
                self.settle_presentation(transitions, view).map(|_| true)
            }
            AwaitingApplicationAction::Presentation(_) => {
                self.settle_presentation(transitions, view)
            }
        }
    }

    pub(crate) fn settle_presentation_transitions(
        &mut self,
        transitions: &[DockspacePresentationTransition],
        view: DockspaceView<'_>,
    ) -> Result<bool, ()> {
        if !matches!(
            self.awaiting,
            Some(AwaitingApplicationAction::Presentation(_))
        ) {
            return Ok(false);
        }
        self.settle_presentation(transitions, view)
    }

    fn settle_presentation(
        &mut self,
        transitions: &[DockspacePresentationTransition],
        view: DockspaceView<'_>,
    ) -> Result<bool, ()> {
        let Some(AwaitingApplicationAction::Presentation(requested)) = self.awaiting.as_ref()
        else {
            return Ok(false);
        };
        let Some(transition) = transitions
            .iter()
            .copied()
            .find(|transition| transition_matches_request(*transition, requested))
        else {
            return Ok(false);
        };
        let status = if transition.result() == DockspacePresentationTransitionResult::Applied {
            DockspaceActionStatus::Applied(
                completed_presentation_outcome(requested, transition, view).ok_or(())?,
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

fn transition_matches_request(
    transition: DockspacePresentationTransition,
    requested: &DockspaceActionOutcome,
) -> bool {
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
    transition: DockspacePresentationTransition,
    view: DockspaceView<'_>,
) -> Option<DockspaceActionOutcome> {
    match requested {
        DockspaceActionOutcome::RootDockRequested {
            root,
            target_root,
            items,
            ..
        } => Some(DockspaceActionOutcome::RootDocked {
            root: *root,
            target_root: *target_root,
            items: items.clone(),
            changed: true,
        }),
        DockspaceActionOutcome::RootFloatRequested { root, items, .. } => {
            let first = view.item(*items.first()?)?;
            let floating = first.contained()?;
            if first.root() != *root
                || first.surface() != transition.target_surface()
                || !items.iter().copied().all(|item| {
                    view.item(item).is_some_and(|location| {
                        location.root() == *root
                            && location.surface() == transition.target_surface()
                            && location.contained() == Some(floating)
                    })
                })
            {
                return None;
            }
            Some(DockspaceActionOutcome::RootFloated {
                root: *root,
                surface: transition.target_surface(),
                floating,
                items: items.clone(),
                changed: true,
            })
        }
        _ => None,
    }
}
