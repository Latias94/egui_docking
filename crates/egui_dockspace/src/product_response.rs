//! Stable responses produced by the renderer-neutral product facade.

use dockspace::model::{
    DockspaceActionOutcome, DockspaceActionRejection, ItemId, SurfaceId, WorkspaceVersion,
};
pub use dockspace::runtime::{
    DockspaceCloseItem, DockspaceCloseOutcome as DockspaceAppliedClose, DockspaceClosePlan,
    DockspaceCloseRejection as DockspaceCloseApplicationRejection, DockspaceCloseRequestRejection,
    DockspaceCloseResolution,
};
use dockspace::runtime::{
    HostCloseRequestOrigin, HostFrameReport, HostInputOutcome, HostSurfaceCommitStatus,
};

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
    pub(crate) fn unchanged(version: WorkspaceVersion) -> Self {
        Self {
            before: version,
            after: version,
            workspace_changed: false,
            published_state_changed: false,
            affected_surfaces: Vec::new(),
        }
    }

    pub(crate) fn from_runtime_report(report: &HostFrameReport) -> Self {
        Self {
            before: report.before(),
            after: report.after(),
            workspace_changed: report.workspace_changed(),
            published_state_changed: report.published_state_changed(),
            affected_surfaces: report.affected_surfaces().to_vec(),
        }
    }

    pub(crate) fn include_adapter_presentation_change(
        &mut self,
        surfaces: impl IntoIterator<Item = SurfaceId>,
    ) {
        self.published_state_changed = true;
        self.affected_surfaces.extend(surfaces);
        self.affected_surfaces.sort_unstable();
        self.affected_surfaces.dedup();
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

    /// Returns whether durable workspace state changed.
    #[must_use]
    pub const fn workspace_changed(&self) -> bool {
        self.workspace_changed
    }

    /// Returns whether any published scene or interaction state changed.
    #[must_use]
    pub const fn published_state_changed(&self) -> bool {
        self.published_state_changed
    }

    /// Returns logical surfaces whose published state changed.
    #[must_use]
    pub fn affected_surfaces(&self) -> &[SurfaceId] {
        &self.affected_surfaces
    }
}

/// Product-level terminal status of one revision-bound action.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DockspaceActionStatus {
    /// The action committed or produced a valid no-op.
    Applied(DockspaceActionOutcome),
    /// Current policy or topology rejected the action.
    Rejected(DockspaceActionRejection),
    /// The action was prepared from an older workspace revision.
    Stale {
        /// Version carried by the prepared action.
        expected: WorkspaceVersion,
        /// Version accepted by the reducer boundary.
        accepted: WorkspaceVersion,
    },
}

/// Atomic publication plus one exact product-action result.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DockspaceActionResult {
    mutation: DockspaceMutation,
    status: DockspaceActionStatus,
}

impl DockspaceActionResult {
    pub(crate) fn from_runtime_report(report: &HostFrameReport) -> Option<Self> {
        let status = report.inputs().iter().find_map(|input| match input {
            HostInputOutcome::ProductActionApplied(outcome) => {
                Some(DockspaceActionStatus::Applied(outcome.clone()))
            }
            HostInputOutcome::ProductActionRejected(reason) => {
                Some(DockspaceActionStatus::Rejected(*reason))
            }
            HostInputOutcome::StaleRejected { expected, accepted } => {
                Some(DockspaceActionStatus::Stale {
                    expected: *expected,
                    accepted: *accepted,
                })
            }
            _ => None,
        })?;
        Some(Self {
            mutation: DockspaceMutation::from_runtime_report(report),
            status,
        })
    }

    /// Returns the atomic publication summary.
    #[must_use]
    pub const fn mutation(&self) -> &DockspaceMutation {
        &self.mutation
    }

    /// Returns the exact action status.
    #[must_use]
    pub const fn status(&self) -> &DockspaceActionStatus {
        &self.status
    }
}

/// One product close request opened or reused by an interaction or application action.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DockspaceCloseRequest {
    plan: DockspaceClosePlan,
    reused: bool,
}

impl DockspaceCloseRequest {
    pub(crate) fn new(plan: &DockspaceClosePlan, reused: bool) -> Self {
        Self {
            plan: plan.clone(),
            reused,
        }
    }

    /// Returns the stable close-plan view.
    #[must_use]
    pub const fn plan(&self) -> &DockspaceClosePlan {
        &self.plan
    }

    /// Returns whether this request reused an unresolved plan.
    #[must_use]
    pub const fn reused(&self) -> bool {
        self.reused
    }
}

/// Product-level terminal status of one programmatic close request.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DockspaceCloseRequestStatus {
    /// The request opened or reused one core-owned close plan.
    Requested(DockspaceCloseRequest),
    /// Current policy or topology rejected the item or root target.
    Rejected(DockspaceCloseRequestRejection),
    /// The request was prepared from an older workspace revision.
    Stale {
        /// Version carried by the prepared request.
        expected: WorkspaceVersion,
        /// Version accepted by the reducer boundary.
        accepted: WorkspaceVersion,
    },
}

/// Atomic publication plus one exact programmatic close-request result.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DockspaceCloseRequestResult {
    mutation: DockspaceMutation,
    status: DockspaceCloseRequestStatus,
}

impl DockspaceCloseRequestResult {
    pub(crate) fn from_runtime_report(report: &HostFrameReport) -> Option<Self> {
        let status = report.inputs().iter().find_map(|input| match input {
            HostInputOutcome::CloseRequested {
                plan,
                reused,
                origin: HostCloseRequestOrigin::Application,
            } => Some(DockspaceCloseRequestStatus::Requested(
                DockspaceCloseRequest::new(plan, *reused),
            )),
            HostInputOutcome::CloseRejected(reason) => {
                Some(DockspaceCloseRequestStatus::Rejected(*reason))
            }
            HostInputOutcome::StaleRejected { expected, accepted } => {
                Some(DockspaceCloseRequestStatus::Stale {
                    expected: *expected,
                    accepted: *accepted,
                })
            }
            _ => None,
        })?;
        Some(Self {
            mutation: DockspaceMutation::from_runtime_report(report),
            status,
        })
    }

    /// Returns the atomic publication summary.
    #[must_use]
    pub const fn mutation(&self) -> &DockspaceMutation {
        &self.mutation
    }

    /// Returns the exact request status.
    #[must_use]
    pub const fn status(&self) -> &DockspaceCloseRequestStatus {
        &self.status
    }
}

/// Product-level result of one initial or deferred close decision.
#[derive(Clone, Debug, PartialEq)]
pub enum DockspaceCloseOutcome {
    /// The exact decision token was consumed by the close workflow.
    Processed {
        /// Token-resolution result, including typed inert outcomes.
        resolution: DockspaceCloseResolution,
        /// Latest retained close plan, when the request exists.
        plan: Option<DockspaceClosePlan>,
        /// Checked topology result after the final allow decision.
        application: Option<Result<DockspaceAppliedClose, DockspaceCloseApplicationRejection>>,
    },
    /// The decision named an older workspace revision.
    Stale {
        /// Version carried by the close input.
        expected: WorkspaceVersion,
        /// Version accepted by the reducer boundary.
        accepted: WorkspaceVersion,
    },
}

/// Atomic publication plus one close-decision result.
#[derive(Clone, Debug, PartialEq)]
pub struct DockspaceCloseResult {
    mutation: DockspaceMutation,
    outcome: DockspaceCloseOutcome,
}

impl DockspaceCloseResult {
    pub(crate) fn from_runtime_report(report: &HostFrameReport) -> Option<Self> {
        let outcome = report.inputs().iter().find_map(|input| match input {
            HostInputOutcome::CloseDecisionProcessed {
                resolution,
                plan,
                application,
                ..
            } => Some(DockspaceCloseOutcome::Processed {
                resolution: *resolution,
                plan: plan.clone(),
                application: application.clone(),
            }),
            HostInputOutcome::StaleRejected { expected, accepted } => {
                Some(DockspaceCloseOutcome::Stale {
                    expected: *expected,
                    accepted: *accepted,
                })
            }
            _ => None,
        })?;
        Some(Self {
            mutation: DockspaceMutation::from_runtime_report(report),
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
    /// The current candidate was retained unchanged.
    Retained,
    /// The host explicitly left the surface non-interactive.
    Unavailable,
    /// The prepared contribution was superseded.
    Rejected,
}

/// Truthful availability of one adapter operation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DockspaceCapability {
    /// Every prerequisite is currently available.
    Supported,
    /// The operation is disabled for an explicit reason.
    Unavailable(DockspaceUnavailableReason),
}

/// Lifecycle state used for one product paint.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DockspaceSurfaceStatus {
    /// The surface has a complete current plan.
    Ready,
    /// The surface retains old geometry while current facts are unavailable.
    Stale,
    /// The surface has no prior ready plan.
    Bootstrap,
    /// The surface is outside the session roster.
    Absent,
}

/// Why one product operation is unavailable.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DockspaceUnavailableReason {
    /// The callback-only path cannot prove final renderer presentation.
    PresentationSettlementRequired,
    /// Current policy disables contained floating.
    ContainedPolicyDisabled,
    /// No ready scene can authorize placement.
    SurfaceBoundsUnavailable,
    /// The surface is absent.
    SurfaceAbsent,
    /// No exact native focus binding exists.
    PaneFocusBindingUnavailable,
    /// The exact native window is not focused.
    PaneFocusWindowNotFocused,
    /// The pane did not expose a stable focus target.
    PaneFocusTargetMissing { item: ItemId },
    /// The pane focus state is not authoritative.
    PaneFocusStateUnknown { item: ItemId },
    /// More than one pane claimed focus.
    ConflictingPaneFocus,
    /// The focus observation identity domain was exhausted.
    PaneFocusObservationGenerationExhausted,
}

/// Exact interaction lanes available for one product paint.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct DockspaceInteractionCapabilities {
    local_actions_current: bool,
    retained_presentation_current: bool,
    pointer_receivers_current: bool,
}

impl DockspaceInteractionCapabilities {
    const fn new(local_actions_current: bool) -> Self {
        Self {
            local_actions_current,
            retained_presentation_current: false,
            pointer_receivers_current: false,
        }
    }

    /// Returns whether current-pass egui responses may emit local actions.
    #[must_use]
    pub const fn local_actions_current(self) -> bool {
        self.local_actions_current
    }

    /// Returns whether this paint matches a retained presented output.
    #[must_use]
    pub const fn retained_presentation_current(self) -> bool {
        self.retained_presentation_current
    }

    /// Returns whether core-owned pointer receivers are authoritative.
    #[must_use]
    pub const fn pointer_receivers_current(self) -> bool {
        self.pointer_receivers_current
    }
}

#[derive(Debug)]
struct SurfacePaintResponse {
    missing_panes: Vec<ItemId>,
    interaction_capabilities: DockspaceInteractionCapabilities,
    surface_status: DockspaceSurfaceStatus,
    contained_capability: DockspaceCapability,
    pane_focus_capability: DockspaceCapability,
}

/// Convenience result from one strict single-surface product frame.
#[derive(Debug)]
pub struct DockspaceResponse {
    mutation: DockspaceMutation,
    paint: SurfacePaintResponse,
    surface_commit_status: DockspaceSurfaceCommitStatus,
    close_requests: Vec<DockspaceCloseRequest>,
}

impl DockspaceResponse {
    pub(crate) fn from_product_report(
        report: &HostFrameReport,
        surface: SurfaceId,
        missing_panes: Vec<ItemId>,
        local_actions_current: bool,
        surface_status: DockspaceSurfaceStatus,
    ) -> Option<Self> {
        let mut surface_commits = report
            .surface_commits()
            .iter()
            .copied()
            .filter(|commit| commit.surface() == surface);
        let status = match surface_commits.next()?.status() {
            HostSurfaceCommitStatus::Ready => DockspaceSurfaceCommitStatus::Ready,
            HostSurfaceCommitStatus::Retained => DockspaceSurfaceCommitStatus::Retained,
            HostSurfaceCommitStatus::Unavailable => DockspaceSurfaceCommitStatus::Unavailable,
            HostSurfaceCommitStatus::Rejected => DockspaceSurfaceCommitStatus::Rejected,
        };
        if surface_commits.next().is_some() {
            return None;
        }
        let close_requests = report
            .inputs()
            .iter()
            .filter_map(|input| match input {
                HostInputOutcome::CloseRequested {
                    plan,
                    reused,
                    origin: HostCloseRequestOrigin::Interaction,
                } => Some(DockspaceCloseRequest::new(plan, *reused)),
                _ => None,
            })
            .collect();
        Some(Self {
            mutation: DockspaceMutation::from_runtime_report(report),
            paint: SurfacePaintResponse {
                missing_panes,
                interaction_capabilities: DockspaceInteractionCapabilities::new(
                    local_actions_current,
                ),
                surface_status,
                contained_capability: DockspaceCapability::Unavailable(
                    DockspaceUnavailableReason::PresentationSettlementRequired,
                ),
                pane_focus_capability: DockspaceCapability::Unavailable(
                    DockspaceUnavailableReason::PaneFocusBindingUnavailable,
                ),
            },
            surface_commit_status: status,
            close_requests,
        })
    }

    /// Returns the product-level publication summary.
    #[must_use]
    pub const fn mutation(&self) -> &DockspaceMutation {
        &self.mutation
    }

    /// Returns the sole surface contribution status.
    #[must_use]
    pub const fn surface_commit_status(&self) -> DockspaceSurfaceCommitStatus {
        self.surface_commit_status
    }

    /// Iterates close plans requested by local controls in this call.
    pub fn close_requests(&self) -> impl Iterator<Item = &DockspaceClosePlan> {
        self.close_requests.iter().map(DockspaceCloseRequest::plan)
    }

    /// Returns close requests in causal order.
    #[must_use]
    pub fn close_request_events(&self) -> &[DockspaceCloseRequest] {
        &self.close_requests
    }

    /// Returns stable pane identities missing from the application registry.
    #[must_use]
    pub fn missing_panes(&self) -> &[ItemId] {
        &self.paint.missing_panes
    }

    /// Returns independent interaction capabilities for this paint.
    #[must_use]
    pub const fn interaction_capabilities(&self) -> DockspaceInteractionCapabilities {
        self.paint.interaction_capabilities
    }

    /// Returns the core lifecycle state used for this paint.
    #[must_use]
    pub const fn surface_status(&self) -> DockspaceSurfaceStatus {
        self.paint.surface_status
    }

    /// Returns contained-floating availability.
    #[must_use]
    pub const fn contained_capability(&self) -> DockspaceCapability {
        self.paint.contained_capability
    }

    /// Returns pane-focus observation availability.
    #[must_use]
    pub const fn pane_focus_capability(&self) -> DockspaceCapability {
        self.paint.pane_focus_capability
    }
}
