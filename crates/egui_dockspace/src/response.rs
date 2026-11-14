//! Stable responses produced by the single-surface egui facade.

use dockspace::model::{ItemId, SurfaceId, WorkspaceVersion};
pub use dockspace::runtime::{
    DockspaceActionStatus, DockspaceCloseItem, DockspaceCloseOutcome as DockspaceAppliedClose,
    DockspaceClosePlan, DockspaceCloseRejection as DockspaceCloseApplicationRejection,
    DockspaceCloseRequestRejection, DockspaceCloseResolution, DockspaceEvent,
    DockspaceLayoutChange, DockspaceManipulationFeedback, DockspaceNotification,
};
use dockspace::runtime::{
    DockspaceSubmittedAction, DockspaceTouchEvent, HostCloseRequestOrigin, HostFrameReport,
    HostInputOutcome, HostSurfaceCommitStatus,
};

/// Product-level summary of committed docking publications.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DockspaceMutation {
    before: WorkspaceVersion,
    after: WorkspaceVersion,
    published_state_changed: bool,
    affected_surfaces: Vec<SurfaceId>,
    notification: DockspaceNotification,
    manipulation_feedback: Vec<DockspaceManipulationFeedback>,
}

impl DockspaceMutation {
    pub(crate) fn unchanged(version: WorkspaceVersion) -> Self {
        Self {
            before: version,
            after: version,
            published_state_changed: false,
            affected_surfaces: Vec::new(),
            notification: DockspaceNotification::default(),
            manipulation_feedback: Vec::new(),
        }
    }

    pub(crate) fn from_runtime_report(report: &HostFrameReport) -> Self {
        Self {
            before: report.before(),
            after: report.after(),
            published_state_changed: report.published_state_changed(),
            affected_surfaces: report.affected_surfaces().to_vec(),
            notification: report.notification().clone(),
            manipulation_feedback: report.manipulation_feedback().to_vec(),
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

    /// Returns whether the durable docking layout changed.
    ///
    /// Native-window placement is tracked separately. Use
    /// [`DockspaceNotification::document_changed`] through [`Self::notification`]
    /// when deciding whether to save a complete document.
    #[must_use]
    pub const fn workspace_changed(&self) -> bool {
        self.notification.layout_change().is_some()
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

    /// Returns this publication's committed product changes in core event order.
    ///
    /// Consume application notifications after a successful docking call,
    /// outside repeated pane/tab painting callbacks. Unchanged and rejected
    /// actions, close requests and pending native transfers are not completion
    /// events. Rereading or cloning this value preserves event identities.
    #[must_use]
    pub fn events(&self) -> &[DockspaceEvent] {
        self.notification.events()
    }

    /// Returns the shared core notification for this successful publication.
    #[must_use]
    pub const fn notification(&self) -> &DockspaceNotification {
        &self.notification
    }

    /// Returns one summary for all durable docking-layout changes in this call.
    ///
    /// Includes split resizing even when no detailed lifecycle event is emitted.
    /// Applications saving a complete document should debounce
    /// [`DockspaceNotification::document_changed`] through [`Self::notification`]
    /// to include native-window placement as well.
    #[must_use]
    pub const fn layout_change(&self) -> Option<DockspaceLayoutChange> {
        self.notification.layout_change()
    }

    /// Returns manipulation feedback committed by this exact publication.
    ///
    /// Direct commands, policy updates, close decisions and document restore may
    /// cancel a pending release. Their original feedback is returned here once;
    /// a later `show_single_surface` invocation does not replay it.
    #[must_use]
    pub fn manipulation_feedback(&self) -> &[DockspaceManipulationFeedback] {
        &self.manipulation_feedback
    }
}

/// Atomic publication plus one exact product-action result.
///
/// Receiving this value means the host update published successfully. Inspect
/// [`Self::status`] to distinguish an applied action from policy/topology rejection
/// or a stale prepared revision.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DockspaceActionResult {
    mutation: DockspaceMutation,
    status: DockspaceActionStatus,
}

impl DockspaceActionResult {
    pub(crate) fn from_runtime_report(
        report: &HostFrameReport,
        submitted: DockspaceSubmittedAction,
    ) -> Option<Self> {
        let status = match report.submitted_action_outcome(submitted)? {
            HostInputOutcome::ProductActionApplied(outcome) => {
                DockspaceActionStatus::Applied(outcome.clone())
            }
            HostInputOutcome::ProductPresentationActionRequested { outcome, .. } => {
                DockspaceActionStatus::Applied(outcome.clone())
            }
            HostInputOutcome::ProductActionRejected(reason) => {
                DockspaceActionStatus::Rejected(*reason)
            }
            HostInputOutcome::StaleRejected { expected, accepted } => {
                DockspaceActionStatus::Stale {
                    expected: *expected,
                    accepted: *accepted,
                }
            }
            _ => return None,
        };
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
    pub(crate) fn from_runtime_report(
        report: &HostFrameReport,
        submitted: DockspaceSubmittedAction,
    ) -> Option<Self> {
        let status = match report.submitted_action_outcome(submitted)? {
            HostInputOutcome::CloseRequested {
                plan,
                reused,
                origin: HostCloseRequestOrigin::Application,
            } => DockspaceCloseRequestStatus::Requested(DockspaceCloseRequest::new(plan, *reused)),
            HostInputOutcome::CloseRejected(reason) => {
                DockspaceCloseRequestStatus::Rejected(*reason)
            }
            HostInputOutcome::StaleRejected { expected, accepted } => {
                DockspaceCloseRequestStatus::Stale {
                    expected: *expected,
                    accepted: *accepted,
                }
            }
            _ => return None,
        };
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
#[derive(Clone, Debug, PartialEq, Eq)]
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
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DockspaceCloseResult {
    mutation: DockspaceMutation,
    outcome: DockspaceCloseOutcome,
}

impl DockspaceCloseResult {
    pub(crate) fn from_runtime_report(
        report: &HostFrameReport,
        submitted: DockspaceSubmittedAction,
    ) -> Option<Self> {
        let outcome = match report.submitted_action_outcome(submitted)? {
            HostInputOutcome::CloseDecisionProcessed {
                resolution,
                plan,
                application,
                ..
            } => DockspaceCloseOutcome::Processed {
                resolution: *resolution,
                plan: plan.clone(),
                application: application.clone(),
            },
            HostInputOutcome::StaleRejected { expected, accepted } => {
                DockspaceCloseOutcome::Stale {
                    expected: *expected,
                    accepted: *accepted,
                }
            }
            _ => return None,
        };
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
    /// This update published input without producing a surface contribution.
    Deferred,
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
    /// The callback-only path cannot prove terminal-output emission.
    PresentationSettlementRequired,
    /// The exact Dockspace emission was missing or duplicated in terminal output.
    PresentationOutputMismatch,
    /// The exact caller-owned run was abandoned or poisoned before settlement.
    PresentationSettlementAbandoned,
    /// No ready scene can authorize placement.
    SurfaceBoundsUnavailable,
    /// The surface is absent.
    SurfaceAbsent,
    /// No exact native focus binding exists.
    PaneFocusBindingUnavailable,
    /// The exact native window is not focused.
    PaneFocusWindowNotFocused,
    /// The pane did not expose a stable focus target.
    PaneFocusTargetMissing {
        /// Pane which did not expose a focus target.
        item: ItemId,
    },
    /// The pane focus state is not authoritative.
    PaneFocusStateUnknown {
        /// Pane whose actual focus state could not be observed.
        item: ItemId,
    },
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
    scroll_receivers_current: bool,
}

impl DockspaceInteractionCapabilities {
    const fn new(local_actions_current: bool) -> Self {
        Self {
            local_actions_current,
            retained_presentation_current: false,
            pointer_receivers_current: false,
            scroll_receivers_current: false,
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

    /// Returns whether ordered scroll events have an exact delivery receiver.
    ///
    /// Official egui exposes only a frame aggregate and final hover
    /// position, so this remains `false` on the callback-only product path.
    /// Native hosts may use their separate event journal and receiver reports.
    #[must_use]
    pub const fn scroll_receivers_current(self) -> bool {
        self.scroll_receivers_current
    }
}

#[derive(Debug)]
struct SurfacePaintResponse {
    missing_panes: Vec<ItemId>,
    interaction_capabilities: DockspaceInteractionCapabilities,
    surface_status: DockspaceSurfaceStatus,
    presentation_settlement_capability: DockspaceCapability,
    pane_focus_capability: DockspaceCapability,
}

/// Committed publication summary from one single-surface egui invocation.
#[derive(Debug)]
pub struct DockspaceResponse {
    mutation: DockspaceMutation,
    paint: SurfacePaintResponse,
    surface_commit_status: DockspaceSurfaceCommitStatus,
    close_requests: Vec<DockspaceCloseRequest>,
    touch_events: Vec<DockspaceTouchEvent>,
}

impl DockspaceResponse {
    pub(crate) fn include_preceding_feedback(
        &mut self,
        mut feedback: Vec<DockspaceManipulationFeedback>,
    ) {
        feedback.append(&mut self.mutation.manipulation_feedback);
        self.mutation.manipulation_feedback = feedback;
    }
    pub(crate) fn from_product_report(
        report: &HostFrameReport,
        surface: SurfaceId,
        missing_panes: Vec<ItemId>,
        local_actions_current: bool,
        surface_status: DockspaceSurfaceStatus,
        presentation_settlement_capability: DockspaceCapability,
        pane_focus_capability: DockspaceCapability,
    ) -> Option<Self> {
        let mut surface_commits = report
            .surface_commits()
            .iter()
            .copied()
            .filter(|commit| commit.surface() == surface);
        let status = match surface_commits.next().map(|commit| commit.status()) {
            Some(HostSurfaceCommitStatus::Ready) => DockspaceSurfaceCommitStatus::Ready,
            Some(HostSurfaceCommitStatus::Retained) => DockspaceSurfaceCommitStatus::Retained,
            Some(HostSurfaceCommitStatus::Unavailable) => DockspaceSurfaceCommitStatus::Unavailable,
            Some(HostSurfaceCommitStatus::Rejected) => DockspaceSurfaceCommitStatus::Rejected,
            None => DockspaceSurfaceCommitStatus::Deferred,
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
        let touch_events = report
            .inputs()
            .iter()
            .filter_map(|input| match input {
                HostInputOutcome::Touch(event) => Some(*event),
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
                presentation_settlement_capability,
                pane_focus_capability,
            },
            surface_commit_status: status,
            close_requests,
            touch_events,
        })
    }

    /// Returns the product-level publication summary.
    #[must_use]
    pub const fn mutation(&self) -> &DockspaceMutation {
        &self.mutation
    }

    /// Returns the terminal surface contribution status for this invocation.
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

    /// Returns touch events in committed report and input order.
    #[must_use]
    pub fn touch_events(&self) -> &[DockspaceTouchEvent] {
        &self.touch_events
    }

    /// Returns core manipulation feedback in committed report and event order.
    ///
    /// Feedback identities are preserved for application-owned deduplication;
    /// observing these inert statuses never changes focus.
    #[must_use]
    pub fn manipulation_feedback(&self) -> &[DockspaceManipulationFeedback] {
        self.mutation.manipulation_feedback()
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

    /// Returns whether this host can terminally settle preview-qualified output.
    #[must_use]
    pub const fn presentation_settlement_capability(&self) -> DockspaceCapability {
        self.paint.presentation_settlement_capability
    }

    /// Returns pane-focus observation availability.
    #[must_use]
    pub const fn pane_focus_capability(&self) -> DockspaceCapability {
        self.paint.pane_focus_capability
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use dockspace::model::{
        DockspaceActionOutcome, DockspaceLayout, DockspaceNode, DockspaceRootLayout,
        DockspaceSurfaceLayout, ItemId, RootId, SurfaceId,
    };
    use dockspace::policy::DockPolicy;
    use dockspace::runtime::{DockspaceSession, SurfaceUnavailableReason};

    const SURFACE: SurfaceId = SurfaceId::new(1);
    const OTHER_SURFACE: SurfaceId = SurfaceId::new(2);
    const ROOT: RootId = RootId::new(1);
    const OTHER_ROOT: RootId = RootId::new(2);
    const FIRST: ItemId = ItemId::new(1);
    const SECOND: ItemId = ItemId::new(2);

    fn session() -> DockspaceSession {
        DockspaceSession::from_layout(
            DockspaceLayout::new([
                DockspaceSurfaceLayout::new(
                    SURFACE,
                    DockspaceRootLayout::new(ROOT, DockspaceNode::tabs([FIRST, SECOND])),
                ),
                DockspaceSurfaceLayout::new(
                    OTHER_SURFACE,
                    DockspaceRootLayout::new(OTHER_ROOT, DockspaceNode::tabs([ItemId::new(3)])),
                ),
            ])
            .unwrap(),
            DockPolicy::default(),
        )
        .unwrap()
    }

    fn commit(mut frame: dockspace::runtime::DockspaceHostFrame<'_>) -> HostFrameReport {
        frame
            .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
            .unwrap();
        frame.commit().unwrap()
    }

    #[test]
    fn action_response_uses_the_submitted_action_when_a_frame_has_multiple_actions() {
        let mut session = session();
        let first_prepared = session.prepare_select_item(FIRST);
        let second_prepared = session.prepare_select_item(SECOND);
        let mut frame = session.begin_host_frame().unwrap();
        let first = frame.submit_prepared_action(first_prepared).unwrap();
        let second = frame.submit_prepared_action(second_prepared).unwrap();
        let report = commit(frame);

        assert!(matches!(
            DockspaceActionResult::from_runtime_report(&report, first)
                .unwrap()
                .status(),
            DockspaceActionStatus::Applied(DockspaceActionOutcome::Selected { item: FIRST, .. })
        ));
        assert!(matches!(
            DockspaceActionResult::from_runtime_report(&report, second)
                .unwrap()
                .status(),
            DockspaceActionStatus::Applied(DockspaceActionOutcome::Selected { item: SECOND, .. })
        ));
    }

    #[test]
    fn close_request_response_uses_the_submitted_request_when_a_frame_has_multiple_requests() {
        let mut session = session();
        let first_prepared = session.prepare_close_item(FIRST);
        let root_prepared = session.prepare_close_root(OTHER_ROOT);
        let mut frame = session.begin_host_frame().unwrap();
        let first = frame.submit_prepared_close_request(first_prepared).unwrap();
        let root = frame.submit_prepared_close_request(root_prepared).unwrap();
        let report = commit(frame);

        assert!(matches!(
            DockspaceCloseRequestResult::from_runtime_report(&report, first)
                .unwrap()
                .status(),
            DockspaceCloseRequestStatus::Requested(request)
                if request.plan().target() == dockspace::close::ClosePlanTarget::Item { item: FIRST }
        ));
        assert!(matches!(
            DockspaceCloseRequestResult::from_runtime_report(&report, root)
                .unwrap()
                .status(),
            DockspaceCloseRequestStatus::Requested(request)
                if request.plan().target() == dockspace::close::ClosePlanTarget::Root { root: OTHER_ROOT }
        ));
    }
}
