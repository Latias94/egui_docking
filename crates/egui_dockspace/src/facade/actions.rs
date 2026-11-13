//! Application actions and close decisions enter the existing session.

use super::Dockspace;
use crate::error_detail::DockspaceErrorSource;
use crate::{
    DockspaceActionResult, DockspaceCloseRequestResult, DockspaceCloseResult, DockspaceError,
};
use dockspace::close::{
    CloseDecision, CloseDecisionToken, CloseRequestId, DeferredCloseDecision, DeferredCloseToken,
};
use dockspace::model::{
    DockAnchor, DockPlacement, DockspaceActionRejection, ItemId, PreparedDockAction,
    PreparedTabsGroup, RootId, SurfaceId,
};
use dockspace::runtime::{PreparedCloseRequest, SurfaceUnavailableReason};

impl Dockspace {
    /// Prepares a revision-bound selection action.
    pub const fn prepare_select_item(&self, item: ItemId) -> PreparedDockAction {
        self.session.prepare_select_item(item)
    }

    /// Prepares a revision-bound open action.
    pub const fn prepare_open_item(
        &self,
        item: ItemId,
        placement: DockPlacement,
    ) -> PreparedDockAction {
        self.session.prepare_open_item(item, placement)
    }

    /// Prepares a revision-bound item docking action.
    pub const fn prepare_dock_item(
        &self,
        item: ItemId,
        placement: DockPlacement,
    ) -> PreparedDockAction {
        self.session.prepare_dock_item(item, placement)
    }

    /// Captures an exact nonempty tabs group without exposing runtime node identities.
    ///
    /// Membership, order, selection, session and version remain bound to this
    /// capture even if its anchor subsequently moves.
    ///
    /// # Errors
    ///
    /// Returns a stable rejection when the anchor is absent or its group is empty.
    pub fn capture_tabs_group(
        &self,
        anchor: DockAnchor,
    ) -> Result<PreparedTabsGroup, DockspaceActionRejection> {
        self.session.capture_tabs_group(anchor)
    }

    /// Prepares one atomic docking action from a previously captured tabs group.
    ///
    /// Preparing again preserves the capture's original session and version.
    pub fn prepare_dock_tabs(
        &self,
        group: PreparedTabsGroup,
        placement: DockPlacement,
    ) -> PreparedDockAction {
        self.session.prepare_dock_tabs(group, placement)
    }

    /// Prepares a revision-bound complete-root docking action.
    pub const fn prepare_dock_root(
        &self,
        root: RootId,
        placement: DockPlacement,
    ) -> PreparedDockAction {
        self.session.prepare_dock_root(root, placement)
    }

    /// Prepares a revision-bound complete-root Dock Back action.
    pub const fn prepare_dock_root_back(&self, root: RootId) -> PreparedDockAction {
        self.session.prepare_dock_root_back(root)
    }

    /// Prepares a revision-bound action that moves one item into contained floating content.
    pub const fn prepare_float_item(&self, item: ItemId, surface: SurfaceId) -> PreparedDockAction {
        self.session.prepare_float_item(item, surface)
    }

    /// Prepares one captured tabs group for contained floating as an atomic payload.
    pub fn prepare_float_tabs(
        &self,
        group: PreparedTabsGroup,
        surface: SurfaceId,
    ) -> PreparedDockAction {
        self.session.prepare_float_tabs(group, surface)
    }

    /// Prepares a revision-bound contained raise addressed by item identity.
    pub const fn prepare_raise_contained(&self, item: ItemId) -> PreparedDockAction {
        self.session.prepare_raise_contained(item)
    }

    /// Prepares a revision-bound contained bring-into-view action.
    pub const fn prepare_bring_contained_into_view(&self, item: ItemId) -> PreparedDockAction {
        self.session.prepare_bring_contained_into_view(item)
    }

    /// Prepares a revision-bound item close request.
    pub const fn prepare_close_item(&self, item: ItemId) -> PreparedCloseRequest {
        self.session.prepare_close_item(item)
    }

    /// Prepares a revision-bound complete-root close request.
    pub const fn prepare_close_root(&self, root: RootId) -> PreparedCloseRequest {
        self.session.prepare_close_root(root)
    }

    /// Submits one core-issued product action against its source revision.
    ///
    /// A successful publication can still return a rejected or stale action.
    /// Inspect [`DockspaceActionResult::status`] before reporting application success.
    ///
    /// # Errors
    ///
    /// Returns an error when the action belongs to another session or the atomic
    /// host frame cannot be committed. Product rejection and stale revisions
    /// are returned as [`crate::DockspaceActionStatus`] inside the successful result.
    pub fn submit_prepared_action(
        &mut self,
        prepared: PreparedDockAction,
    ) -> Result<DockspaceActionResult, DockspaceError> {
        let mut frame = self
            .session
            .begin_host_frame()
            .map_err(DockspaceError::from_detail)?;
        frame
            .submit_prepared_action(prepared)
            .map_err(DockspaceError::from_detail)?;
        frame
            .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
            .map_err(DockspaceError::from_detail)?;
        let report = frame.commit().map_err(DockspaceError::from_detail)?;
        DockspaceActionResult::from_runtime_report(&report)
            .ok_or(DockspaceErrorSource::ApplicationOutcomeUnavailable {
                operation: "product action",
            })
            .map_err(Into::into)
    }

    /// Selects one currently open item against the current revision.
    ///
    /// Inspect [`DockspaceActionResult::status`] for the product outcome.
    ///
    /// # Errors
    ///
    /// Returns an error when the selection cannot be reduced atomically.
    pub fn select_item_current(
        &mut self,
        item: ItemId,
    ) -> Result<DockspaceActionResult, DockspaceError> {
        self.submit_prepared_action(self.prepare_select_item(item))
    }

    /// Opens one item at a stable placement against the current revision.
    ///
    /// New content becomes selected and requests pane focus. Existing content
    /// keeps its location, selection and focus without another request.
    ///
    /// Inspect [`DockspaceActionResult::status`] for placement or policy rejection.
    ///
    /// # Errors
    ///
    /// Returns an error when the action cannot be published atomically.
    pub fn open_item_current(
        &mut self,
        item: ItemId,
        placement: DockPlacement,
    ) -> Result<DockspaceActionResult, DockspaceError> {
        self.submit_prepared_action(self.prepare_open_item(item, placement))
    }

    /// Docks one item at a stable placement against the current revision.
    ///
    /// Inspect [`DockspaceActionResult::status`] for placement or policy rejection.
    ///
    /// # Errors
    ///
    /// Returns an error when the action cannot be published atomically.
    pub fn dock_item_current(
        &mut self,
        item: ItemId,
        placement: DockPlacement,
    ) -> Result<DockspaceActionResult, DockspaceError> {
        self.submit_prepared_action(self.prepare_dock_item(item, placement))
    }

    /// Docks a captured complete tabs group without refreshing its source authority.
    ///
    /// Inspect [`DockspaceActionResult::status`] for stale capture or policy rejection.
    ///
    /// # Errors
    ///
    /// Returns an error for a foreign capture or when the action cannot be published.
    pub fn dock_tabs_current(
        &mut self,
        group: PreparedTabsGroup,
        placement: DockPlacement,
    ) -> Result<DockspaceActionResult, DockspaceError> {
        self.submit_prepared_action(self.prepare_dock_tabs(group, placement))
    }

    /// Docks one complete root at a stable placement against the current revision.
    ///
    /// Inspect [`DockspaceActionResult::status`] for placement or policy rejection.
    ///
    /// # Errors
    ///
    /// Returns an error when the action cannot be published atomically.
    pub fn dock_root_current(
        &mut self,
        root: RootId,
        placement: DockPlacement,
    ) -> Result<DockspaceActionResult, DockspaceError> {
        self.submit_prepared_action(self.prepare_dock_root(root, placement))
    }

    /// Returns a complete floating root to its current docking destination.
    ///
    /// Inspect [`DockspaceActionResult::status`] for the product outcome.
    ///
    /// # Errors
    ///
    /// Returns an error when the revision-bound action cannot be committed.
    pub fn dock_root_back_current(
        &mut self,
        root: RootId,
    ) -> Result<DockspaceActionResult, DockspaceError> {
        self.submit_prepared_action(self.prepare_dock_root_back(root))
    }

    /// Moves one open item into a contained presentation on an existing surface.
    ///
    /// A complete singleton root preserves its root identity. Moving one item out
    /// of a larger root allocates a fresh root and contained-presentation identity.
    ///
    /// Inspect [`DockspaceActionResult::status`] for unavailable content or policy rejection.
    ///
    /// # Errors
    ///
    /// Returns an error when the action cannot be published atomically.
    pub fn float_item_current(
        &mut self,
        item: ItemId,
        surface: SurfaceId,
    ) -> Result<DockspaceActionResult, DockspaceError> {
        self.submit_prepared_action(self.prepare_float_item(item, surface))
    }

    /// Floats a captured complete tabs group while preserving its order and selection.
    ///
    /// The capture retains its original session and version. Inspect
    /// [`DockspaceActionResult::status`] for stale capture or policy rejection.
    ///
    /// # Errors
    ///
    /// Returns an error for a foreign capture or when the action cannot be published.
    pub fn float_tabs_current(
        &mut self,
        group: PreparedTabsGroup,
        surface: SurfaceId,
    ) -> Result<DockspaceActionResult, DockspaceError> {
        self.submit_prepared_action(self.prepare_float_tabs(group, surface))
    }

    /// Raises the contained presentation owning one item to the front.
    ///
    /// Inspect [`DockspaceActionResult::status`] for unavailable content or policy rejection.
    ///
    /// # Errors
    ///
    /// Returns an error when the action cannot be published atomically.
    pub fn raise_contained_current(
        &mut self,
        item: ItemId,
    ) -> Result<DockspaceActionResult, DockspaceError> {
        self.submit_prepared_action(self.prepare_raise_contained(item))
    }

    /// Clamps the contained presentation owning one item into current visible bounds.
    ///
    /// Passive resize and minimum-size changes preserve durable geometry. This
    /// explicit action is the product boundary which may commit a clamped rectangle.
    ///
    /// Inspect [`DockspaceActionResult::status`] for the product outcome.
    ///
    /// # Errors
    ///
    /// Returns an error when the atomic action frame cannot be committed.
    pub fn bring_contained_into_view_current(
        &mut self,
        item: ItemId,
    ) -> Result<DockspaceActionResult, DockspaceError> {
        self.submit_prepared_action(self.prepare_bring_contained_into_view(item))
    }

    /// Submits one session- and revision-bound programmatic close request.
    ///
    /// # Errors
    ///
    /// Returns an error when the request belongs to another session or the
    /// atomic host frame cannot be committed. Stale targets and policy
    /// rejections are returned in [`DockspaceCloseRequestResult`].
    pub fn submit_prepared_close_request(
        &mut self,
        prepared: PreparedCloseRequest,
    ) -> Result<DockspaceCloseRequestResult, DockspaceError> {
        let mut frame = self
            .session
            .begin_host_frame()
            .map_err(DockspaceError::from_detail)?;
        frame
            .submit_prepared_close_request(prepared)
            .map_err(DockspaceError::from_detail)?;
        frame
            .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
            .map_err(DockspaceError::from_detail)?;
        let report = frame.commit().map_err(DockspaceError::from_detail)?;
        DockspaceCloseRequestResult::from_runtime_report(&report)
            .ok_or(DockspaceErrorSource::ApplicationOutcomeUnavailable {
                operation: "close request",
            })
            .map_err(Into::into)
    }

    /// Opens or reuses one close plan for an item at the current revision.
    ///
    /// # Errors
    ///
    /// Returns an error when the atomic request frame cannot be committed.
    pub fn request_close_item_current(
        &mut self,
        item: ItemId,
    ) -> Result<DockspaceCloseRequestResult, DockspaceError> {
        self.submit_prepared_close_request(self.prepare_close_item(item))
    }

    /// Opens or reuses one close plan for a complete root at the current revision.
    ///
    /// # Errors
    ///
    /// Returns an error when the atomic request frame cannot be committed.
    pub fn request_close_root_current(
        &mut self,
        root: RootId,
    ) -> Result<DockspaceCloseRequestResult, DockspaceError> {
        self.submit_prepared_close_request(self.prepare_close_root(root))
    }

    /// Resolves one initial pane-close decision.
    ///
    /// # Errors
    ///
    /// Returns an error when the token cannot be submitted or the atomic host
    /// frame cannot be committed.
    pub fn resolve_close(
        &mut self,
        request: CloseRequestId,
        token: CloseDecisionToken,
        decision: CloseDecision,
    ) -> Result<DockspaceCloseResult, DockspaceError> {
        let mut frame = self
            .session
            .begin_host_frame()
            .map_err(DockspaceError::from_detail)?;
        frame
            .resolve_close(request, token, decision)
            .map_err(DockspaceError::from_detail)?;
        frame
            .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
            .map_err(DockspaceError::from_detail)?;
        let report = frame.commit().map_err(DockspaceError::from_detail)?;
        DockspaceCloseResult::from_runtime_report(&report)
            .ok_or(DockspaceErrorSource::ApplicationOutcomeUnavailable {
                operation: "close decision",
            })
            .map_err(Into::into)
    }

    /// Resolves one deferred pane-close continuation.
    ///
    /// # Errors
    ///
    /// Returns an error when the continuation cannot be submitted or the atomic
    /// host frame cannot be committed.
    pub fn resolve_deferred_close(
        &mut self,
        request: CloseRequestId,
        token: DeferredCloseToken,
        decision: DeferredCloseDecision,
    ) -> Result<DockspaceCloseResult, DockspaceError> {
        let mut frame = self
            .session
            .begin_host_frame()
            .map_err(DockspaceError::from_detail)?;
        frame
            .continue_deferred_close(request, token, decision)
            .map_err(DockspaceError::from_detail)?;
        frame
            .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
            .map_err(DockspaceError::from_detail)?;
        let report = frame.commit().map_err(DockspaceError::from_detail)?;
        DockspaceCloseResult::from_runtime_report(&report)
            .ok_or(DockspaceErrorSource::ApplicationOutcomeUnavailable {
                operation: "deferred close decision",
            })
            .map_err(Into::into)
    }
}
