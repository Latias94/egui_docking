//! Structured output from explicit egui docking host-frame boundaries.

use std::collections::BTreeMap;

use dockspace::backend::command::CloseCommitOutcome;
#[cfg(any(feature = "backend", test))]
use dockspace::backend::command::CommandOutcome;
use dockspace::backend::effect::{EffectId, EffectTransition};
use dockspace::backend::ids::{ItemId, ReducerCausalOrdinal, SurfaceId};
use dockspace::backend::ingress::BackendIngressOrdinal;
use dockspace::backend::interaction::InteractionOutcome;
use dockspace::backend::presentation_observation::HostPresentationObservationOutcome;
use dockspace::backend::transition::{EngineTransition, InputOutcome, SurfaceContributionOutcome};
use dockspace::error::CommandError;
use dockspace::model::{DockspaceActionOutcome, DockspaceActionRejection};
use dockspace::policy::CloseCapability;
use dockspace::runtime::WorkspaceVersion;
use dockspace::{
    CloseDecisionToken, CloseItemDecisionState, ClosePlan, ClosePlanPhase, ClosePlanTarget,
    CloseRequestId, CloseResolutionOutcome, DeferredCloseToken,
};

use dockspace::NativeCloseEdge;

/// Product-level summary of one atomic docking publication.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DockspaceMutation {
    before: WorkspaceVersion,
    after: WorkspaceVersion,
    workspace_changed: bool,
    published_state_changed: bool,
    affected_surfaces: Vec<SurfaceId>,
}

impl DockspaceMutation {
    pub(crate) fn from_transition(transition: &EngineTransition) -> Self {
        Self {
            before: transition.before(),
            after: transition.after(),
            workspace_changed: transition.changed(),
            published_state_changed: transition.published_state_changed(),
            affected_surfaces: transition.affected_surfaces().collect(),
        }
    }

    /// Returns the durable workspace version before publication.
    #[must_use]
    pub const fn before(&self) -> WorkspaceVersion {
        self.before
    }

    /// Returns the durable workspace version after publication.
    #[must_use]
    pub const fn after(&self) -> WorkspaceVersion {
        self.after
    }

    /// Returns whether durable workspace or policy state changed.
    #[must_use]
    pub const fn workspace_changed(&self) -> bool {
        self.workspace_changed
    }

    /// Returns whether any published topology, presentation, or interaction state changed.
    #[must_use]
    pub const fn published_state_changed(&self) -> bool {
        self.published_state_changed
    }

    /// Returns logical surfaces whose presentation authority changed.
    #[must_use]
    pub fn affected_surfaces(&self) -> &[SurfaceId] {
        &self.affected_surfaces
    }
}

/// Product-level result of one checked workspace command.
#[cfg(any(feature = "backend", test))]
#[derive(Clone, Debug, PartialEq)]
pub enum DockspaceCommandOutcome {
    /// The command committed or produced a valid no-op.
    Applied(CommandOutcome),
    /// The command was deterministically rejected without mutation.
    Rejected(CommandError),
    /// The command named an older workspace version and was consumed inertly.
    Stale {
        /// Version carried by the command input.
        expected: WorkspaceVersion,
        /// Version accepted by the reducer boundary.
        accepted: WorkspaceVersion,
    },
}

/// Atomic publication plus the exact result of one checked workspace command.
#[cfg(any(feature = "backend", test))]
#[derive(Clone, Debug, PartialEq)]
pub struct DockspaceCommandResult {
    mutation: DockspaceMutation,
    outcome: DockspaceCommandOutcome,
}

#[cfg(any(feature = "backend", test))]
impl DockspaceCommandResult {
    pub(crate) fn from_transition(transition: &EngineTransition) -> Option<Self> {
        let outcome =
            transition
                .reduced_inputs()
                .iter()
                .find_map(|input| match input.outcome() {
                    InputOutcome::CommandProcessed { outcome, .. } => {
                        Some(DockspaceCommandOutcome::Applied(outcome.clone()))
                    }
                    InputOutcome::CommandRejected { error, .. } => {
                        Some(DockspaceCommandOutcome::Rejected(error.clone()))
                    }
                    InputOutcome::StaleRejected {
                        expected,
                        accepted_base,
                    } => Some(DockspaceCommandOutcome::Stale {
                        expected: *expected,
                        accepted: *accepted_base,
                    }),
                    _ => None,
                })?;
        Some(Self {
            mutation: DockspaceMutation::from_transition(transition),
            outcome,
        })
    }

    /// Returns the atomic publication summary.
    #[must_use]
    pub const fn mutation(&self) -> &DockspaceMutation {
        &self.mutation
    }

    /// Returns the exact checked command result.
    #[must_use]
    pub const fn outcome(&self) -> &DockspaceCommandOutcome {
        &self.outcome
    }
}

/// Product-level terminal status of one revision-bound item action.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DockspaceActionStatus {
    /// The action committed or produced a valid product no-op.
    Applied(DockspaceActionOutcome),
    /// Current policy or topology deterministically rejected the action.
    Rejected(DockspaceActionRejection),
    /// The action was prepared from an older published workspace version.
    Stale {
        /// Version carried by the prepared action.
        expected: WorkspaceVersion,
        /// Version accepted by the reducer boundary.
        accepted: WorkspaceVersion,
    },
}

/// Atomic publication plus the exact result of one product item action.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DockspaceActionResult {
    mutation: DockspaceMutation,
    status: DockspaceActionStatus,
}

impl DockspaceActionResult {
    pub(crate) fn from_transition(transition: &EngineTransition) -> Option<Self> {
        let status =
            transition
                .reduced_inputs()
                .iter()
                .find_map(|input| match input.outcome() {
                    InputOutcome::ProductActionProcessed { outcome, .. } => {
                        Some(DockspaceActionStatus::Applied(outcome.clone()))
                    }
                    InputOutcome::ProductActionRejected { reason, .. } => {
                        Some(DockspaceActionStatus::Rejected(*reason))
                    }
                    InputOutcome::StaleRejected {
                        expected,
                        accepted_base,
                    } => Some(DockspaceActionStatus::Stale {
                        expected: *expected,
                        accepted: *accepted_base,
                    }),
                    _ => None,
                })?;
        Some(Self {
            mutation: DockspaceMutation::from_transition(transition),
            status,
        })
    }

    /// Returns the atomic publication summary.
    #[must_use]
    pub const fn mutation(&self) -> &DockspaceMutation {
        &self.mutation
    }

    /// Returns the exact product-action status.
    #[must_use]
    pub const fn status(&self) -> &DockspaceActionStatus {
        &self.status
    }
}

/// One application-facing item decision in a close plan.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DockspaceCloseItem {
    item: ItemId,
    capability: CloseCapability,
    token: CloseDecisionToken,
    state: CloseItemDecisionState,
}

impl DockspaceCloseItem {
    /// Returns the stable pane identity which must be decided.
    #[must_use]
    pub const fn item(self) -> ItemId {
        self.item
    }

    /// Returns the policy capability frozen into this request.
    #[must_use]
    pub const fn capability(self) -> CloseCapability {
        self.capability
    }

    /// Returns the initial decision token for this pane.
    #[must_use]
    pub const fn token(self) -> CloseDecisionToken {
        self.token
    }

    /// Returns the current decision state for this pane.
    #[must_use]
    pub const fn state(self) -> CloseItemDecisionState {
        self.state
    }

    /// Returns the deferred continuation when this pane is awaiting one.
    #[must_use]
    pub const fn deferred_token(self) -> Option<DeferredCloseToken> {
        match self.state {
            CloseItemDecisionState::Deferred { continuation } => Some(continuation),
            CloseItemDecisionState::Pending
            | CloseItemDecisionState::Allowed
            | CloseItemDecisionState::Vetoed => None,
        }
    }
}

/// Stable application-facing view of one core close plan.
///
/// Destruction proofs, cancellation state, authority domains, and reducer
/// diagnostics remain private to the core. The view retains only the identities
/// and tokens an application needs to decide pane closure.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DockspaceClosePlan {
    request: CloseRequestId,
    target: ClosePlanTarget,
    items: Box<[DockspaceCloseItem]>,
    phase: ClosePlanPhase,
}

impl DockspaceClosePlan {
    pub(crate) fn from_core(plan: &ClosePlan) -> Self {
        Self {
            request: plan.request(),
            target: plan.target(),
            items: plan
                .items()
                .iter()
                .copied()
                .map(|item| DockspaceCloseItem {
                    item: item.item(),
                    capability: item.capability(),
                    token: item.token(),
                    state: item.state(),
                })
                .collect(),
            phase: plan.phase(),
        }
    }

    /// Returns the unique close request identity.
    #[must_use]
    pub const fn request(&self) -> CloseRequestId {
        self.request
    }

    /// Returns the stable item, root, or surface target.
    #[must_use]
    pub const fn target(&self) -> ClosePlanTarget {
        self.target
    }

    /// Returns the required pane decisions in core order.
    #[must_use]
    pub fn items(&self) -> &[DockspaceCloseItem] {
        &self.items
    }

    /// Returns the current application-visible close phase.
    #[must_use]
    pub const fn phase(&self) -> ClosePlanPhase {
        self.phase
    }
}

/// One close request emitted by an egui interaction or semantic action.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DockspaceCloseRequest {
    plan: DockspaceClosePlan,
    reused: bool,
}

impl DockspaceCloseRequest {
    pub(crate) fn new(plan: &ClosePlan, reused: bool) -> Self {
        Self {
            plan: DockspaceClosePlan::from_core(plan),
            reused,
        }
    }

    /// Returns the stable close-plan view.
    #[must_use]
    pub const fn plan(&self) -> &DockspaceClosePlan {
        &self.plan
    }

    /// Returns whether this interaction reused an unresolved plan.
    #[must_use]
    pub const fn reused(&self) -> bool {
        self.reused
    }
}

/// Product-level result of one initial or deferred close decision.
#[derive(Clone, Debug, PartialEq)]
pub enum DockspaceCloseOutcome {
    /// The exact close token was consumed by the core-owned close workflow.
    Processed {
        /// Token-resolution result, including typed inert outcomes.
        resolution: CloseResolutionOutcome,
        /// Latest retained close plan, when the request exists.
        plan: Option<DockspaceClosePlan>,
        /// Checked topology result after the final allow decision.
        application: Option<Result<CloseCommitOutcome, CommandError>>,
    },
    /// The decision named an older workspace version and was consumed inertly.
    Stale {
        /// Version carried by the close input.
        expected: WorkspaceVersion,
        /// Version accepted by the reducer boundary.
        accepted: WorkspaceVersion,
    },
}

/// Atomic publication plus the exact result of one close decision.
#[derive(Clone, Debug, PartialEq)]
pub struct DockspaceCloseResult {
    mutation: DockspaceMutation,
    outcome: DockspaceCloseOutcome,
}

impl DockspaceCloseResult {
    pub(crate) fn from_transition(transition: &EngineTransition) -> Option<Self> {
        let outcome =
            transition
                .reduced_inputs()
                .iter()
                .find_map(|input| match input.outcome() {
                    InputOutcome::CloseDecisionProcessed {
                        resolution,
                        plan,
                        application,
                        ..
                    } => Some(DockspaceCloseOutcome::Processed {
                        resolution: *resolution,
                        plan: plan.as_ref().map(DockspaceClosePlan::from_core),
                        application: application.clone(),
                    }),
                    InputOutcome::StaleRejected {
                        expected,
                        accepted_base,
                    } => Some(DockspaceCloseOutcome::Stale {
                        expected: *expected,
                        accepted: *accepted_base,
                    }),
                    _ => None,
                })?;
        Some(Self {
            mutation: DockspaceMutation::from_transition(transition),
            outcome,
        })
    }

    /// Returns the atomic publication summary.
    #[must_use]
    pub const fn mutation(&self) -> &DockspaceMutation {
        &self.mutation
    }

    /// Returns the exact close-workflow result.
    #[must_use]
    pub const fn outcome(&self) -> &DockspaceCloseOutcome {
        &self.outcome
    }
}

/// Product-level terminal status of one surface contribution.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DockspaceSurfaceCommitStatus {
    /// A complete next presentation candidate was installed.
    Ready,
    /// The current ready candidate was retained unchanged.
    Retained,
    /// The host explicitly left the surface non-interactive.
    Unavailable,
    /// The prepared contribution was superseded before installation.
    Rejected,
}

impl DockspaceSurfaceCommitStatus {
    pub(crate) const fn from_contribution(outcome: &SurfaceContributionOutcome) -> Self {
        match outcome {
            SurfaceContributionOutcome::Ready { .. } => Self::Ready,
            SurfaceContributionOutcome::Retained { .. } => Self::Retained,
            SurfaceContributionOutcome::Unavailable { .. } => Self::Unavailable,
            SurfaceContributionOutcome::Rejected { .. } => Self::Rejected,
        }
    }
}

/// Truthful availability of one adapter operation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DockspaceCapability {
    /// Every application and platform prerequisite is currently available.
    Supported,
    /// The operation is disabled for an explicit, queryable reason.
    Unavailable(DockspaceUnavailableReason),
}

/// Lifecycle state of one logical surface painted during a host frame.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DockspaceSurfaceStatus {
    /// The surface exists and was projected into a sealed, ready scene.
    Ready,
    /// The surface retains a previous paint projection, but current facts cannot authorize input.
    ///
    /// The selected application's pane callback, when available, renders the retained projection
    /// through a disabled child UI while a new exact contribution is collected. Docking chrome
    /// hit testing remains unavailable until that contribution is compiled and acknowledged.
    Stale,
    /// The surface has neither current authority nor a previous paint projection.
    ///
    /// This includes first publication, workspace replacement, and measurement unavailability
    /// before any ready plan existed. A currently measurable pane candidate may still render
    /// through disabled child UIs, but geometry-derived docking input is unavailable.
    Bootstrap,
    /// The surface is not owned by the tick-start workspace roster.
    ///
    /// A strict host frame cannot paint an absent surface. This status remains available to
    /// report a surface removed by the one reducer tick which closed the frame.
    Absent,
}

/// Why an egui adapter operation is unavailable in the current frame.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DockspaceUnavailableReason {
    /// The current host cannot report terminal presentation settlement.
    ///
    /// Callback-only painting cannot authorize contained interaction because
    /// the core cannot prove that the corresponding receiver graph reached the
    /// screen. Use an explicit outer host that settles its final egui output.
    PresentationSettlementRequired,
    /// Application policy disables contained-floating presentation.
    ContainedPolicyDisabled,
    /// No ready sealed scene can authorize placement on this surface.
    SurfaceBoundsUnavailable,
    /// The requested logical surface is not owned by the current workspace.
    SurfaceAbsent,
    /// The surface has no exact current native binding eligible to report focus.
    PaneFocusBindingUnavailable,
    /// Global platform facts do not prove that this exact native binding is focused.
    PaneFocusWindowNotFocused,
    /// The application did not expose a stable focus target for this pane.
    PaneFocusTargetMissing { item: ItemId },
    /// The application could not authoritatively report this pane's focus state.
    PaneFocusStateUnknown { item: ItemId },
    /// More than one pane on the same surface claimed focus.
    ConflictingPaneFocus,
    /// The adapter exhausted its monotonic pane-focus observation identity domain.
    PaneFocusObservationGenerationExhausted,
}

/// Immediate paint result from one backend host-frame surface call.
///
/// This value deliberately contains no reducer transition. Semantic input and measurement facts
/// are staged until the enclosing host frame ends.
#[derive(Clone, Debug)]
pub struct SurfacePaintResponse {
    pub(crate) surface: SurfaceId,
    pub(crate) missing_panes: Vec<ItemId>,
    pub(crate) capture_errors: Vec<CommandError>,
    pub(crate) interaction_capabilities: DockspaceInteractionCapabilities,
    pub(crate) surface_status: DockspaceSurfaceStatus,
    pub(crate) contained_capability: DockspaceCapability,
    pub(crate) pane_focus_capability: DockspaceCapability,
}

/// Exact interaction lanes available for one painted surface.
///
/// Current-frame egui actions, retained presentation freshness, and core-owned
/// pointer receivers are distinct capabilities. The retained backend currently
/// promotes the latter two together, but callers must not infer either one from
/// the local-action capability.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct DockspaceInteractionCapabilities {
    local_actions_current: bool,
    retained_presentation_current: bool,
    pointer_receivers_current: bool,
}

impl DockspaceInteractionCapabilities {
    pub(crate) const fn new(
        local_actions_current: bool,
        retained_presentation_current: bool,
        pointer_receivers_current: bool,
    ) -> Self {
        Self {
            local_actions_current,
            retained_presentation_current,
            pointer_receivers_current,
        }
    }

    /// Returns whether current-pass egui responses may emit supported local actions.
    ///
    /// This covers the semantic and gesture actions already routed through the
    /// core reducer, including tab actions, tab docking, splitter resizing, and
    /// contained-floating gestures. It does not imply that a retained
    /// presentation was accepted or that core-owned pointer receivers are
    /// authoritative.
    #[must_use]
    pub const fn local_actions_current(self) -> bool {
        self.local_actions_current
    }

    /// Returns whether this paint matches the latest accepted retained presentation.
    #[must_use]
    pub const fn retained_presentation_current(self) -> bool {
        self.retained_presentation_current
    }

    /// Returns whether core-owned pointer receivers are authoritative for this paint.
    #[must_use]
    pub const fn pointer_receivers_current(self) -> bool {
        self.pointer_receivers_current
    }
}

impl SurfacePaintResponse {
    /// Returns the painted logical surface.
    #[must_use]
    pub const fn surface(&self) -> SurfaceId {
        self.surface
    }

    /// Returns stable item identities missing from the application pane registry.
    #[must_use]
    pub fn missing_panes(&self) -> &[ItemId] {
        &self.missing_panes
    }

    /// Returns exact source/target capture errors observed while registering widgets.
    #[must_use]
    pub fn capture_errors(&self) -> &[CommandError] {
        &self.capture_errors
    }

    /// Returns the exact independent interaction capabilities for this paint.
    #[must_use]
    pub const fn interaction_capabilities(&self) -> DockspaceInteractionCapabilities {
        self.interaction_capabilities
    }

    /// Returns the exact core lifecycle state used for this paint.
    #[must_use]
    pub const fn surface_status(&self) -> DockspaceSurfaceStatus {
        self.surface_status
    }

    /// Returns current contained-floating tear-off availability.
    #[must_use]
    pub const fn contained_capability(&self) -> DockspaceCapability {
        self.contained_capability
    }

    /// Returns whether this surface can publish exact pane-focus observations.
    #[must_use]
    pub const fn pane_focus_capability(&self) -> DockspaceCapability {
        self.pane_focus_capability
    }
}

/// One surface's paint result and terminal host-frame disposition.
#[derive(Debug)]
pub struct SurfaceCommitResponse {
    pub(crate) paint: Option<SurfacePaintResponse>,
    pub(crate) status: DockspaceSurfaceCommitStatus,
}

impl SurfaceCommitResponse {
    /// Returns the local paint result when this surface was painted by egui.
    #[must_use]
    pub const fn paint(&self) -> Option<&SurfacePaintResponse> {
        self.paint.as_ref()
    }

    /// Returns the terminal status of this contribution.
    #[must_use]
    pub const fn status(&self) -> DockspaceSurfaceCommitStatus {
        self.status
    }
}

/// Receipt for one backend ingress ordinal which the core reduced as an effect result.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BackendEffectReceipt {
    ordinal: BackendIngressOrdinal,
    accepted_effect: Option<EffectId>,
}

impl BackendEffectReceipt {
    /// Returns the exact backend ingress position consumed by the reducer.
    #[must_use]
    pub const fn ordinal(self) -> BackendIngressOrdinal {
        self.ordinal
    }

    /// Returns the effect identity when the result matched a known terminal transition.
    #[must_use]
    pub const fn accepted_effect(self) -> Option<EffectId> {
        self.accepted_effect
    }
}

/// Count-only summary of presentation observations reduced by one host frame.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PresentationObservationSummary {
    observed: usize,
    rejected: usize,
    captured_unknown: usize,
    retired: usize,
    retired_presented_eligible: usize,
    retired_presented_ineligible: usize,
    retired_dropped: usize,
}

impl PresentationObservationSummary {
    /// Returns the number of observation outcomes in the committed boundary.
    #[must_use]
    pub const fn observed(self) -> usize {
        self.observed
    }

    /// Returns the number of rejected observations.
    #[must_use]
    pub const fn rejected(self) -> usize {
        self.rejected
    }

    /// Returns the number of accepted captures without a known final output.
    #[must_use]
    pub const fn captured_unknown(self) -> usize {
        self.captured_unknown
    }

    /// Returns the number of terminally retired presentation streams.
    #[must_use]
    pub const fn retired(self) -> usize {
        self.retired
    }

    /// Returns the number of presented outputs eligible for authority promotion.
    #[must_use]
    pub const fn retired_presented_eligible(self) -> usize {
        self.retired_presented_eligible
    }

    /// Returns the number of presented outputs which settled but could not promote authority.
    #[must_use]
    pub const fn retired_presented_ineligible(self) -> usize {
        self.retired_presented_ineligible
    }

    /// Returns the number of explicit known-dropped terminal outputs.
    #[must_use]
    pub const fn retired_dropped(self) -> usize {
        self.retired_dropped
    }
}

fn close_request_events(transition: &EngineTransition) -> Vec<DockspaceCloseRequest> {
    let mut ordered = Vec::new();
    for input in transition.reduced_inputs() {
        if let InputOutcome::InteractionProcessed {
            outcome: InteractionOutcome::CloseRequested { plan, reused },
            ..
        } = input.outcome()
        {
            ordered.push(OrderedCloseRequest {
                ordinal: input.causal_ordinal(),
                sequence: input.sequence().get(),
                request: DockspaceCloseRequest::new(plan, *reused),
            });
        }
    }
    for edge in transition.reduced_pointer_edges() {
        for outcome in edge.interaction_outcomes() {
            if let InteractionOutcome::CloseRequested { plan, reused } = outcome {
                ordered.push(OrderedCloseRequest {
                    ordinal: edge.causal_ordinal(),
                    sequence: edge.ticket().sequence().get(),
                    request: DockspaceCloseRequest::new(plan, *reused),
                });
            }
        }
    }
    ordered.sort_by_key(|entry| (entry.ordinal, entry.sequence));
    ordered.into_iter().map(|entry| entry.request).collect()
}

struct OrderedCloseRequest {
    ordinal: ReducerCausalOrdinal,
    sequence: u64,
    request: DockspaceCloseRequest,
}

fn native_close_edges(transition: &EngineTransition) -> Vec<NativeCloseEdge> {
    transition
        .reduced_inputs()
        .iter()
        .flat_map(|input| match input.outcome() {
            InputOutcome::PlatformSnapshotPublished {
                native_close_edges, ..
            }
            | InputOutcome::NativeCloseObservationPublished {
                native_close_edges, ..
            } => native_close_edges.as_slice(),
            _ => &[],
        })
        .copied()
        .collect()
}

fn effect_receipts(transition: &EngineTransition) -> Vec<BackendEffectReceipt> {
    transition
        .reduced_inputs()
        .iter()
        .filter_map(|input| {
            let ordinal = input.backend_ingress_ordinal()?;
            let accepted_effect = match input.outcome() {
                InputOutcome::PlatformEffectReported {
                    effect,
                    transition:
                        EffectTransition::Applied
                        | EffectTransition::Duplicate
                        | EffectTransition::RetiredTerminal,
                    ..
                } => Some(*effect),
                _ => None,
            };
            Some(BackendEffectReceipt {
                ordinal,
                accepted_effect,
            })
        })
        .collect()
}

fn presentation_summary(transition: &EngineTransition) -> PresentationObservationSummary {
    let mut summary = PresentationObservationSummary::default();
    for outcome in transition.presentation_observations() {
        summary.observed += 1;
        summary.retired += usize::from(matches!(
            outcome,
            HostPresentationObservationOutcome::Retired { .. }
        ));
        match outcome {
            HostPresentationObservationOutcome::NoUpdate { .. } => {}
            HostPresentationObservationOutcome::CapturedUnknown { .. } => {
                summary.captured_unknown += 1;
            }
            HostPresentationObservationOutcome::Rejected { .. } => {
                summary.rejected += 1;
            }
            HostPresentationObservationOutcome::Retired {
                presented: dockspace::intent::Authority::Known(Some(_)),
                promotion_eligible: true,
                ..
            } => {
                summary.retired_presented_eligible += 1;
            }
            HostPresentationObservationOutcome::Retired {
                presented: dockspace::intent::Authority::Known(Some(_)),
                promotion_eligible: false,
                ..
            } => {
                summary.retired_presented_ineligible += 1;
            }
            HostPresentationObservationOutcome::Retired {
                presented: dockspace::intent::Authority::Known(None),
                ..
            } => {
                summary.retired_dropped += 1;
            }
            HostPresentationObservationOutcome::Retired { .. } => {}
        }
    }
    summary
}

impl HostFrameResponse {
    pub(crate) fn from_transition(
        transition: EngineTransition,
        surfaces: BTreeMap<SurfaceId, SurfaceCommitResponse>,
    ) -> Self {
        let close_requests = close_request_events(&transition);
        let mutation = DockspaceMutation::from_transition(&transition);
        Self {
            mutation,
            surfaces,
            close_requests,
            native_close_edges: native_close_edges(&transition),
            effect_receipts: effect_receipts(&transition),
            presentation_summary: presentation_summary(&transition),
        }
    }
}

/// One atomic host-frame result and every frozen surface's terminal disposition.
#[derive(Debug)]
pub struct HostFrameResponse {
    pub(crate) mutation: DockspaceMutation,
    pub(crate) surfaces: BTreeMap<SurfaceId, SurfaceCommitResponse>,
    pub(crate) close_requests: Vec<DockspaceCloseRequest>,
    native_close_edges: Vec<NativeCloseEdge>,
    effect_receipts: Vec<BackendEffectReceipt>,
    presentation_summary: PresentationObservationSummary,
}

impl HostFrameResponse {
    /// Returns the product-level summary of the atomic publication.
    #[must_use]
    pub const fn mutation(&self) -> &DockspaceMutation {
        &self.mutation
    }

    /// Returns one frozen surface's typed result.
    #[must_use]
    pub fn surface(&self, surface: SurfaceId) -> Option<&SurfaceCommitResponse> {
        self.surfaces.get(&surface)
    }

    /// Iterates every frozen surface result in stable surface identity order.
    #[must_use]
    pub fn surfaces(&self) -> impl ExactSizeIterator<Item = (SurfaceId, &SurfaceCommitResponse)> {
        self.surfaces
            .iter()
            .map(|(surface, response)| (*surface, response))
    }

    /// Iterates close plans requested by semantic controls in this host frame.
    #[must_use]
    pub fn close_requests(&self) -> impl Iterator<Item = &DockspaceClosePlan> {
        self.close_requests.iter().map(DockspaceCloseRequest::plan)
    }

    /// Returns close requests in their core causal order.
    #[must_use]
    pub fn close_request_events(&self) -> &[DockspaceCloseRequest] {
        &self.close_requests
    }

    /// Returns exact native close edges reduced by this host frame.
    #[must_use]
    pub fn native_close_edges(&self) -> &[NativeCloseEdge] {
        &self.native_close_edges
    }

    /// Returns backend effect receipts in reduced ingress order.
    #[must_use]
    pub fn effect_receipts(&self) -> &[BackendEffectReceipt] {
        &self.effect_receipts
    }

    /// Returns count-only presentation settlement facts for this boundary.
    #[must_use]
    pub const fn presentation_summary(&self) -> PresentationObservationSummary {
        self.presentation_summary
    }
}

/// Convenience result from one strict single-surface host frame.
///
/// This is not the legacy per-callback reducer response: it is produced by creating a complete
/// host frame, painting the one permitted surface slot, and ending that frame atomically.
#[derive(Debug)]
pub struct DockspaceResponse {
    pub(crate) mutation: DockspaceMutation,
    pub(crate) paint: SurfacePaintResponse,
    pub(crate) surface_commit_status: DockspaceSurfaceCommitStatus,
    pub(crate) close_requests: Vec<DockspaceCloseRequest>,
}

impl DockspaceResponse {
    /// Returns the product-level summary of the atomic publication.
    #[must_use]
    pub const fn mutation(&self) -> &DockspaceMutation {
        &self.mutation
    }

    /// Returns the product-level terminal status for the sole surface contribution.
    #[must_use]
    pub const fn surface_commit_status(&self) -> DockspaceSurfaceCommitStatus {
        self.surface_commit_status
    }

    /// Iterates close plans requested by semantic controls in this call.
    pub fn close_requests(&self) -> impl Iterator<Item = &DockspaceClosePlan> {
        self.close_requests.iter().map(DockspaceCloseRequest::plan)
    }

    /// Iterates close requests in core causal order, retaining whether each plan was reused.
    #[must_use]
    pub fn close_request_events(&self) -> &[DockspaceCloseRequest] {
        &self.close_requests
    }

    /// Returns stable item identities missing from the application pane registry.
    #[must_use]
    pub fn missing_panes(&self) -> &[ItemId] {
        self.paint.missing_panes()
    }

    /// Returns exact source/target capture errors observed while registering widgets.
    #[must_use]
    pub fn capture_errors(&self) -> &[CommandError] {
        self.paint.capture_errors()
    }

    /// Returns the exact independent interaction capabilities for this paint.
    #[must_use]
    pub const fn interaction_capabilities(&self) -> DockspaceInteractionCapabilities {
        self.paint.interaction_capabilities()
    }

    /// Returns the exact core lifecycle state used for this paint.
    #[must_use]
    pub const fn surface_status(&self) -> DockspaceSurfaceStatus {
        self.paint.surface_status()
    }

    /// Returns current contained-floating tear-off availability.
    #[must_use]
    pub const fn contained_capability(&self) -> DockspaceCapability {
        self.paint.contained_capability()
    }

    /// Returns whether this surface can publish exact pane-focus observations.
    #[must_use]
    pub const fn pane_focus_capability(&self) -> DockspaceCapability {
        self.paint.pane_focus_capability()
    }
}
