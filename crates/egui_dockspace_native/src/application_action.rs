//! Bounded application-owned docking, close and configuration ingress.

use dockspace::close::{
    CloseDecision, CloseDecisionToken, CloseRequestId, DeferredCloseDecision, DeferredCloseToken,
};
use dockspace::model::{DockspaceActionRejection, SurfaceId, WorkspaceVersion};
use dockspace::policy::DockPolicy;
#[cfg(feature = "serde")]
use dockspace::runtime::PreparedDockspaceDocumentRestore;
use dockspace::runtime::{
    DockPresentationConfig, DockspaceActionStatus, DockspaceCloseOutcome, DockspaceClosePlan,
    DockspaceCloseRejection, DockspaceCloseRequestRejection, DockspaceCloseResolution,
    DockspaceHostFrame, DockspacePresentationTransition, DockspacePresentationTransitionId,
    DockspacePresentationTransitionResult, DockspaceRuntimeError, DockspaceRuntimeErrorKind,
    DockspaceSubmittedAction, HostFrameReport, HostInputOutcome, NativeSurfaceBinding,
    PreparedCloseRequest, PreparedDockAction, PreparedDockspaceLayoutReset,
};
use egui_dockspace::{DockInputConfig, DockStyle};

use crate::NativeRuntimeErrorKind;

/// Why one native application request could not be queued.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum NativeActionRequestError {
    /// Another command, presentation terminal or unconsumed result owns the slot.
    Busy,
    /// The native runtime has entered terminal shutdown.
    Stopped,
    /// This fixed-host facade cannot remove its own logical surface.
    ///
    /// Keep an empty central root or another presentation on this surface.
    /// Native surface replacement and rebinding require a separate lifecycle.
    HostSurfaceRequired {
        /// The native application's fixed logical host surface.
        surface: SurfaceId,
    },
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
            Self::HostSurfaceRequired { .. } => {
                "keep a central dock area or another group on the application host surface"
            }
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

/// Exact committed result of one application command.
///
/// Close requests return the core plan without deciding on the application's
/// behalf. Consume this result before submitting a decision or another command.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NativeActionStatus {
    /// Current native state rejected one Root continuation before replacement.
    ///
    /// The frame may still commit its earlier ordered input. The command slot
    /// reaches this terminal result without stopping the native runtime.
    RootContinuationRejected {
        /// Exact Root binding frozen when this command was queued.
        predecessor: NativeSurfaceBinding,
        /// Incoming logical Root before presentation identity rebasing.
        incoming_root: SurfaceId,
        /// Stable core category of the preparation rejection.
        kind: DockspaceRuntimeErrorKind,
    },
    /// A docking, selection, or reopening command reached its terminal status.
    Dock(DockspaceActionStatus),
    /// A content-close request opened or reused one core-owned plan.
    CloseRequested {
        /// Current exact plan, including application decision tokens.
        plan: DockspaceClosePlan,
        /// Whether an unresolved plan already existed for this target.
        reused: bool,
    },
    /// Current policy or topology rejected a content-close request.
    CloseRejected(DockspaceCloseRequestRejection),
    /// One initial or deferred close decision was consumed.
    CloseDecisionProcessed {
        /// Exact resolution, including inert stale or duplicate tokens.
        resolution: DockspaceCloseResolution,
        /// Latest core plan when its retained detail remains available.
        plan: Option<DockspaceClosePlan>,
        /// Atomic close result after the final allow decision.
        application: Option<Result<DockspaceCloseOutcome, DockspaceCloseRejection>>,
        /// Whether this decision changed durable topology.
        changed: bool,
    },
    /// A complete runtime policy committed at its configuration boundary.
    PolicyChanged {
        /// Whether the policy differed from its previous value.
        changed: bool,
        /// Workspace version after the configuration input.
        version: WorkspaceVersion,
    },
    /// The arrangement lock committed while preserving all other policy rules.
    ArrangementLockChanged {
        /// Requested arrangement lock state.
        locked: bool,
        /// Whether the policy differed from its previous value.
        changed: bool,
        /// Workspace version after the configuration input.
        version: WorkspaceVersion,
    },
    /// Renderer style and its represented core settings committed together.
    StyleChanged {
        /// Whether the adapter style differed from its previous value.
        changed: bool,
        /// Whether core geometry or interaction timing changed.
        geometry_changed: bool,
    },
    /// Input bindings committed after all earlier native input was consumed.
    InputChanged {
        /// Whether the bindings differed from their previous value.
        changed: bool,
    },
    /// An application default or preset replaced the logical layout.
    ///
    /// Native bindings and output authority still require their fresh observations.
    LayoutReset {
        /// Candidate version immediately before the terminal replacement.
        previous: WorkspaceVersion,
        /// Workspace version published by the replacement.
        current: WorkspaceVersion,
    },
    /// A prepared document and its durable sidecars published atomically.
    #[cfg(feature = "serde")]
    DocumentRestored {
        /// Candidate version immediately before the terminal replacement.
        previous: WorkspaceVersion,
        /// Workspace version published by the replacement.
        current: WorkspaceVersion,
    },
    /// An application command was prepared from an obsolete workspace version.
    Stale {
        /// Version frozen when the command was queued.
        expected: WorkspaceVersion,
        /// Version accepted by the reducer.
        accepted: WorkspaceVersion,
    },
}

pub(crate) type NativeActionTerminal = Result<NativeActionStatus, NativeActionFailure>;

/// One affine command retained by the existing application slot.
#[derive(Debug)]
pub(crate) struct NativeApplicationAction {
    expected: WorkspaceVersion,
    pub(crate) command: NativeApplicationCommand,
}

#[derive(Debug)]
pub(crate) enum NativeApplicationCommand {
    Dock(PreparedDockAction),
    Close(PreparedCloseRequest),
    DecideClose {
        request: CloseRequestId,
        token: CloseDecisionToken,
        decision: CloseDecision,
    },
    ContinueClose {
        request: CloseRequestId,
        token: DeferredCloseToken,
        decision: DeferredCloseDecision,
    },
    Policy {
        policy: Box<DockPolicy>,
        arrangement_lock: Option<bool>,
    },
    Style {
        style: Box<DockStyle>,
        presentation: Box<DockPresentationConfig>,
        changed: bool,
    },
    Input {
        input: DockInputConfig,
        changed: bool,
    },
    LayoutReset(Box<PreparedDockspaceLayoutReset>),
    LayoutResetRebindingRoot {
        reset: Box<PreparedDockspaceLayoutReset>,
        existing_root: NativeSurfaceBinding,
        incoming_root: SurfaceId,
    },
    #[cfg(feature = "serde")]
    DocumentRestore {
        restore: Box<PreparedDockspaceDocumentRestore>,
        root_surface: SurfaceId,
        root_mappings: Vec<(NativeSurfaceBinding, SurfaceId)>,
    },
}

impl From<PreparedDockAction> for NativeApplicationAction {
    fn from(action: PreparedDockAction) -> Self {
        Self {
            expected: action.expected_version(),
            command: NativeApplicationCommand::Dock(action),
        }
    }
}

impl From<PreparedCloseRequest> for NativeApplicationAction {
    fn from(request: PreparedCloseRequest) -> Self {
        Self {
            expected: request.expected_version(),
            command: NativeApplicationCommand::Close(request),
        }
    }
}

impl NativeApplicationAction {
    pub(crate) const fn new(expected: WorkspaceVersion, command: NativeApplicationCommand) -> Self {
        Self { expected, command }
    }

    pub(crate) const fn expected_version(&self) -> WorkspaceVersion {
        self.expected
    }

    pub(crate) fn submit(
        self,
        frame: &mut DockspaceHostFrame<'_>,
        published: WorkspaceVersion,
    ) -> Result<NativeApplicationSubmission, DockspaceRuntimeError> {
        if self.is_replacement() && self.expected != published {
            return Ok(NativeApplicationSubmission::Terminal(
                NativeActionStatus::Stale {
                    expected: self.expected,
                    accepted: published,
                },
            ));
        }
        let submitted = match self.command {
            NativeApplicationCommand::Dock(action) => match frame.submit_prepared_action(action) {
                Ok(submitted) => submitted,
                Err(error) if error.is_foreign_prepared_action() => {
                    return Ok(NativeApplicationSubmission::Terminal(
                        NativeActionStatus::Dock(DockspaceActionStatus::Rejected(
                            DockspaceActionRejection::Conflict,
                        )),
                    ));
                }
                Err(error) => return Err(error),
            },
            NativeApplicationCommand::Close(request) => {
                frame.submit_prepared_close_request(request)?
            }
            NativeApplicationCommand::DecideClose {
                request,
                token,
                decision,
            } => frame.resolve_close(request, token, decision)?,
            NativeApplicationCommand::ContinueClose {
                request,
                token,
                decision,
            } => frame.continue_deferred_close(request, token, decision)?,
            NativeApplicationCommand::Policy { policy, .. } => {
                return Ok(NativeApplicationSubmission::Configuration(policy));
            }
            NativeApplicationCommand::Style {
                style,
                presentation,
                changed,
            } => {
                return Ok(NativeApplicationSubmission::Style {
                    style,
                    presentation,
                    changed,
                });
            }
            NativeApplicationCommand::Input { input, changed } => {
                return Ok(NativeApplicationSubmission::InputConfiguration { input, changed });
            }
            NativeApplicationCommand::LayoutReset(reset) => {
                return Ok(NativeApplicationSubmission::Replacement(
                    NativeWorkspaceReplacement::Layout(reset),
                ));
            }
            NativeApplicationCommand::LayoutResetRebindingRoot {
                reset,
                existing_root,
                incoming_root,
            } => {
                return Ok(NativeApplicationSubmission::Replacement(
                    NativeWorkspaceReplacement::LayoutRebindingRoot {
                        reset,
                        existing_root,
                        incoming_root,
                    },
                ));
            }
            #[cfg(feature = "serde")]
            NativeApplicationCommand::DocumentRestore {
                restore,
                root_surface,
                root_mappings,
            } => {
                return Ok(NativeApplicationSubmission::Replacement(
                    NativeWorkspaceReplacement::Document {
                        restore,
                        root_surface,
                        root_mappings,
                    },
                ));
            }
        };
        Ok(NativeApplicationSubmission::Input(submitted))
    }

    fn expectation(&self) -> NativeActionExpectation {
        match &self.command {
            NativeApplicationCommand::Dock(_) => NativeActionExpectation::Dock,
            NativeApplicationCommand::Close(_) => NativeActionExpectation::Close,
            NativeApplicationCommand::DecideClose { .. }
            | NativeApplicationCommand::ContinueClose { .. } => {
                NativeActionExpectation::CloseDecision
            }
            NativeApplicationCommand::Policy {
                arrangement_lock, ..
            } => NativeActionExpectation::Policy(*arrangement_lock),
            NativeApplicationCommand::Style { .. } => NativeActionExpectation::Style,
            NativeApplicationCommand::Input { .. } => NativeActionExpectation::Input,
            NativeApplicationCommand::LayoutReset(_)
            | NativeApplicationCommand::LayoutResetRebindingRoot { .. } => {
                NativeActionExpectation::LayoutReset
            }
            #[cfg(feature = "serde")]
            NativeApplicationCommand::DocumentRestore { .. } => {
                NativeActionExpectation::DocumentRestore
            }
        }
    }

    const fn is_replacement(&self) -> bool {
        match self.command {
            NativeApplicationCommand::LayoutReset(_)
            | NativeApplicationCommand::LayoutResetRebindingRoot { .. } => true,
            #[cfg(feature = "serde")]
            NativeApplicationCommand::DocumentRestore { .. } => true,
            _ => false,
        }
    }
}

/// Stack-local phase distinction within one rollbackable core frame.
pub(crate) enum NativeApplicationSubmission {
    Input(DockspaceSubmittedAction),
    Configuration(Box<DockPolicy>),
    Style {
        style: Box<DockStyle>,
        presentation: Box<DockPresentationConfig>,
        changed: bool,
    },
    InputConfiguration {
        input: DockInputConfig,
        changed: bool,
    },
    Replacement(NativeWorkspaceReplacement),
    Terminal(NativeActionStatus),
}

pub(crate) enum NativeWorkspaceReplacement {
    Layout(Box<PreparedDockspaceLayoutReset>),
    LayoutRebindingRoot {
        reset: Box<PreparedDockspaceLayoutReset>,
        existing_root: NativeSurfaceBinding,
        incoming_root: SurfaceId,
    },
    #[cfg(feature = "serde")]
    Document {
        restore: Box<PreparedDockspaceDocumentRestore>,
        root_surface: SurfaceId,
        root_mappings: Vec<(NativeSurfaceBinding, SurfaceId)>,
    },
}

/// Exact correlation within the committed frame; replacement is its unique terminal input.
pub(crate) enum NativeSubmittedApplicationAction {
    Input(DockspaceSubmittedAction),
    Style {
        style: Box<DockStyle>,
        submitted: Option<DockspaceSubmittedAction>,
        changed: bool,
    },
    InputConfiguration {
        input: DockInputConfig,
        changed: bool,
    },
    LayoutReset,
    #[cfg(feature = "serde")]
    DocumentRestore,
    Terminal(NativeActionStatus),
}

impl NativeSubmittedApplicationAction {
    pub(crate) fn commit_input(&mut self) -> Option<DockInputConfig> {
        let Self::InputConfiguration { input, changed } = self else {
            return None;
        };
        let input = *input;
        *self = Self::Terminal(NativeActionStatus::InputChanged { changed: *changed });
        Some(input)
    }

    /// Uses the candidate style for this update's encoding before publication.
    pub(crate) fn encoding_style(&self) -> Option<&DockStyle> {
        match self {
            Self::Style { style, .. } => Some(style),
            _ => None,
        }
    }

    /// Extracts adapter styling only after the matching core frame committed.
    pub(crate) fn commit_style(
        &mut self,
        report: &HostFrameReport,
    ) -> Result<Option<Box<DockStyle>>, ()> {
        let Self::Style {
            submitted, changed, ..
        } = self
        else {
            return Ok(None);
        };
        let geometry_changed = match submitted {
            Some(submitted) => match report.submitted_action_outcome(*submitted) {
                Some(HostInputOutcome::PresentationConfigReplaced { changed, .. }) => *changed,
                _ => return Err(()),
            },
            None => false,
        };
        let status = NativeActionStatus::StyleChanged {
            changed: *changed,
            geometry_changed,
        };
        let Self::Style { style, .. } = std::mem::replace(self, Self::Terminal(status)) else {
            unreachable!("the style submission was checked before replacement")
        };
        Ok(Some(style))
    }
}

impl NativeApplicationSubmission {
    /// Appends configuration or replacement after all earlier local and focus input.
    pub(crate) fn finish(
        self,
        frame: &mut DockspaceHostFrame<'_>,
    ) -> Result<NativeSubmittedApplicationAction, DockspaceRuntimeError> {
        match self {
            Self::Input(submitted) => Ok(NativeSubmittedApplicationAction::Input(submitted)),
            Self::Configuration(policy) => frame
                .replace_policy(*policy)
                .map(NativeSubmittedApplicationAction::Input),
            Self::Style {
                style,
                presentation,
                changed,
            } => {
                let submitted = if frame.presentation_config() == presentation.as_ref() {
                    None
                } else {
                    Some(frame.replace_presentation_config(*presentation)?)
                };
                Ok(NativeSubmittedApplicationAction::Style {
                    style,
                    submitted,
                    changed,
                })
            }
            Self::InputConfiguration { input, changed } => {
                if changed {
                    // A binding change ends the previous gesture through its
                    // existing semantic cancellation action. This frame keeps
                    // its original bindings while encoding captured raw input.
                    for surface in frame.surfaces() {
                        let cancel = frame
                            .paint_plan(surface)?
                            .and_then(|plan| plan.prepare_gesture_cancel());
                        if let Some(cancel) = cancel {
                            frame.submit_surface_action(cancel)?;
                            break;
                        }
                    }
                }
                Ok(NativeSubmittedApplicationAction::InputConfiguration { input, changed })
            }
            Self::Replacement(NativeWorkspaceReplacement::Layout(reset)) => {
                frame.submit_layout_reset(&reset)?;
                Ok(NativeSubmittedApplicationAction::LayoutReset)
            }
            Self::Replacement(NativeWorkspaceReplacement::LayoutRebindingRoot {
                reset,
                existing_root,
                incoming_root,
            }) => {
                if let Err(error) =
                    frame.submit_native_layout_reset(&reset, existing_root, incoming_root)
                {
                    return root_continuation_rejection(error, existing_root, incoming_root);
                }
                Ok(NativeSubmittedApplicationAction::LayoutReset)
            }
            #[cfg(feature = "serde")]
            Self::Replacement(NativeWorkspaceReplacement::Document {
                restore,
                root_surface,
                root_mappings,
            }) => {
                if let Some(&(existing_root, incoming_root)) = root_mappings.first() {
                    if let Err(error) =
                        frame.submit_native_document_restore_with_roots(&restore, root_mappings)
                    {
                        return root_continuation_rejection(error, existing_root, incoming_root);
                    }
                } else {
                    frame.submit_native_document_restore(&restore, root_surface)?;
                }
                Ok(NativeSubmittedApplicationAction::DocumentRestore)
            }
            Self::Terminal(status) => Ok(NativeSubmittedApplicationAction::Terminal(status)),
        }
    }
}

fn root_continuation_rejection(
    error: DockspaceRuntimeError,
    predecessor: NativeSurfaceBinding,
    incoming_root: SurfaceId,
) -> Result<NativeSubmittedApplicationAction, DockspaceRuntimeError> {
    // Preparation rejects before append. Protocol and publication failures still
    // propagate so a poisoned candidate can never be committed as a rejection.
    if matches!(
        error.kind(),
        DockspaceRuntimeErrorKind::OperationConflict
            | DockspaceRuntimeErrorKind::InvalidConfiguration
    ) {
        Ok(NativeSubmittedApplicationAction::Terminal(
            NativeActionStatus::RootContinuationRejected {
                predecessor,
                incoming_root,
                kind: error.kind(),
            },
        ))
    } else {
        Err(error)
    }
}

#[derive(Debug, Clone, Copy)]
enum NativeActionExpectation {
    Dock,
    Close,
    CloseDecision,
    Policy(Option<bool>),
    Style,
    Input,
    LayoutReset,
    #[cfg(feature = "serde")]
    DocumentRestore,
}

impl NativeActionExpectation {
    const fn is_replacement(self) -> bool {
        match self {
            Self::LayoutReset => true,
            #[cfg(feature = "serde")]
            Self::DocumentRestore => true,
            _ => false,
        }
    }
}

/// Single-slot queue consumed only by the final root pass.
#[derive(Debug, Default)]
pub(crate) struct NativeApplicationActions {
    state: NativeApplicationActionState,
}

#[derive(Debug)]
enum NativeApplicationActionState {
    Vacant,
    Queued(NativeApplicationAction),
    UncommittedFrame {
        expected: WorkspaceVersion,
        kind: NativeActionExpectation,
    },
    AwaitingPresentation {
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
    pub(crate) fn enqueue<T>(
        &mut self,
        command: T,
        prepare: impl FnOnce(T) -> NativeApplicationAction,
    ) -> Result<(), T> {
        if self.is_occupied() {
            return Err(command);
        }
        self.state = NativeApplicationActionState::Queued(prepare(command));
        Ok(())
    }

    pub(crate) const fn is_occupied(&self) -> bool {
        !matches!(self.state, NativeApplicationActionState::Vacant)
    }

    pub(crate) const fn has_queued(&self) -> bool {
        matches!(self.state, NativeApplicationActionState::Queued(_))
    }

    pub(crate) const fn has_queued_replacement(&self) -> bool {
        matches!(
            &self.state,
            NativeApplicationActionState::Queued(action) if action.is_replacement()
        )
    }

    pub(crate) const fn has_pending_replacement(&self) -> bool {
        self.has_queued_replacement()
            || matches!(
                self.state,
                NativeApplicationActionState::UncommittedFrame { kind, .. }
                    if kind.is_replacement()
            )
    }

    /// A terminal replacement waits for frozen placement geometry to commit.
    pub(crate) fn take_after_placements(
        &mut self,
        placements_unsettled: bool,
    ) -> Option<NativeApplicationAction> {
        if placements_unsettled && self.has_queued_replacement() {
            return None;
        }
        self.take()
    }

    pub(crate) fn take(&mut self) -> Option<NativeApplicationAction> {
        let state = std::mem::take(&mut self.state);
        let NativeApplicationActionState::Queued(action) = state else {
            self.state = state;
            return None;
        };
        self.state = NativeApplicationActionState::UncommittedFrame {
            expected: action.expected_version(),
            kind: action.expectation(),
        };
        Some(action)
    }

    pub(crate) fn settle_submission(
        &mut self,
        submitted: Option<NativeSubmittedApplicationAction>,
        report: &HostFrameReport,
    ) -> Result<bool, ()> {
        let input = match submitted {
            Some(NativeSubmittedApplicationAction::Input(submitted)) => {
                report.submitted_action_outcome(submitted)
            }
            Some(
                NativeSubmittedApplicationAction::Style { .. }
                | NativeSubmittedApplicationAction::InputConfiguration { .. },
            ) => return Err(()),
            Some(NativeSubmittedApplicationAction::LayoutReset) => report
                .inputs()
                .iter()
                .find(|input| matches!(input, HostInputOutcome::LayoutReset { .. })),
            #[cfg(feature = "serde")]
            Some(NativeSubmittedApplicationAction::DocumentRestore) => report
                .inputs()
                .iter()
                .find(|input| matches!(input, HostInputOutcome::DocumentRestored { .. })),
            Some(NativeSubmittedApplicationAction::Terminal(status)) => {
                let NativeApplicationActionState::UncommittedFrame { expected, kind } = self.state
                else {
                    return Err(());
                };
                let valid = match status {
                    NativeActionStatus::Dock(DockspaceActionStatus::Rejected(
                        DockspaceActionRejection::Conflict,
                    )) => matches!(kind, NativeActionExpectation::Dock),
                    NativeActionStatus::Stale {
                        expected: stale, ..
                    } => expected == stale && kind.is_replacement(),
                    NativeActionStatus::RootContinuationRejected { .. } => kind.is_replacement(),
                    NativeActionStatus::StyleChanged { .. } => {
                        matches!(kind, NativeActionExpectation::Style)
                    }
                    NativeActionStatus::InputChanged { .. } => {
                        matches!(kind, NativeActionExpectation::Input)
                    }
                    _ => false,
                };
                if !valid {
                    return Err(());
                }
                self.state = NativeApplicationActionState::Terminal(Ok(status));
                return Ok(true);
            }
            None => None,
        };
        self.settle(input, report.presentation_transitions())
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
            NativeApplicationActionState::UncommittedFrame { expected, kind } => {
                let Some(input) = input else {
                    self.set_terminal_failure(
                        NativeRuntimeErrorKind::Internal,
                        NativeActionInterruption::CommittedOutcomeUnavailable,
                    );
                    return Err(());
                };
                let settlement = match (kind, input) {
                    (
                        NativeActionExpectation::Dock,
                        HostInputOutcome::ProductActionApplied(outcome),
                    ) => ReductionSettlement::Terminal(DockspaceActionStatus::Applied(
                        outcome.clone(),
                    )),
                    (
                        NativeActionExpectation::Dock,
                        HostInputOutcome::ProductPresentationActionRequested { transition, .. },
                    ) => ReductionSettlement::Presentation {
                        transition: *transition,
                    },
                    (
                        NativeActionExpectation::Dock,
                        HostInputOutcome::ProductActionRejected(reason),
                    ) => ReductionSettlement::Terminal(DockspaceActionStatus::Rejected(*reason)),
                    (
                        _,
                        HostInputOutcome::StaleRejected {
                            expected: stale,
                            accepted,
                        },
                    ) if *stale == *expected => {
                        let status = if matches!(kind, NativeActionExpectation::Dock) {
                            NativeActionStatus::Dock(DockspaceActionStatus::Stale {
                                expected: *stale,
                                accepted: *accepted,
                            })
                        } else {
                            NativeActionStatus::Stale {
                                expected: *stale,
                                accepted: *accepted,
                            }
                        };
                        ReductionSettlement::ApplicationTerminal(status)
                    }
                    (
                        NativeActionExpectation::Close,
                        HostInputOutcome::CloseRequested { plan, reused, .. },
                    ) => ReductionSettlement::ApplicationTerminal(
                        NativeActionStatus::CloseRequested {
                            plan: plan.clone(),
                            reused: *reused,
                        },
                    ),
                    (NativeActionExpectation::Close, HostInputOutcome::CloseRejected(reason)) => {
                        ReductionSettlement::ApplicationTerminal(NativeActionStatus::CloseRejected(
                            *reason,
                        ))
                    }
                    (
                        NativeActionExpectation::CloseDecision,
                        HostInputOutcome::CloseDecisionProcessed {
                            resolution,
                            plan,
                            application,
                            changed,
                        },
                    ) => ReductionSettlement::ApplicationTerminal(
                        NativeActionStatus::CloseDecisionProcessed {
                            resolution: *resolution,
                            plan: plan.clone(),
                            application: application.clone(),
                            changed: *changed,
                        },
                    ),
                    (
                        NativeActionExpectation::Policy(arrangement_lock),
                        HostInputOutcome::PolicyReplaced { changed, version },
                    ) => ReductionSettlement::ApplicationTerminal(match arrangement_lock {
                        Some(locked) => NativeActionStatus::ArrangementLockChanged {
                            locked: *locked,
                            changed: *changed,
                            version: *version,
                        },
                        None => NativeActionStatus::PolicyChanged {
                            changed: *changed,
                            version: *version,
                        },
                    }),
                    (
                        NativeActionExpectation::LayoutReset,
                        HostInputOutcome::LayoutReset { previous, current },
                    ) => {
                        ReductionSettlement::ApplicationTerminal(NativeActionStatus::LayoutReset {
                            previous: *previous,
                            current: *current,
                        })
                    }
                    #[cfg(feature = "serde")]
                    (
                        NativeActionExpectation::DocumentRestore,
                        HostInputOutcome::DocumentRestored { previous, current },
                    ) => ReductionSettlement::ApplicationTerminal(
                        NativeActionStatus::DocumentRestored {
                            previous: *previous,
                            current: *current,
                        },
                    ),
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
                        self.state = NativeApplicationActionState::Terminal(Ok(
                            NativeActionStatus::Dock(status),
                        ));
                        Ok(true)
                    }
                    ReductionSettlement::ApplicationTerminal(status) => {
                        self.state = NativeApplicationActionState::Terminal(Ok(status));
                        Ok(true)
                    }
                    ReductionSettlement::Presentation { transition } => {
                        self.state = NativeApplicationActionState::AwaitingPresentation {
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
            transition: requested_transition,
            ..
        } = &self.state
        else {
            return Ok(false);
        };
        // Core validates the frozen command and its completed payload. The host
        // correlates only the opaque session-scoped request identity.
        let Some(transition) = transitions
            .iter()
            .find(|transition| transition.id() == *requested_transition)
        else {
            return Ok(false);
        };
        let status = if transition.result() == DockspacePresentationTransitionResult::Applied {
            let Some(outcome) = transition.outcome().cloned() else {
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
        self.state = NativeApplicationActionState::Terminal(Ok(NativeActionStatus::Dock(status)));
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
            | NativeApplicationActionState::UncommittedFrame { .. } => {
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
    ApplicationTerminal(NativeActionStatus),
    Presentation {
        transition: DockspacePresentationTransitionId,
    },
}
