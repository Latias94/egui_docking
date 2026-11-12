//! Default egui facade backed by one renderer-neutral [`DockspaceSession`].

use std::collections::BTreeSet;
use std::{fmt::Debug, hash::Hash};

use dockspace::close::{
    CloseDecision, CloseDecisionToken, CloseRequestId, DeferredCloseDecision, DeferredCloseToken,
};
use dockspace::model::{
    DockAnchor, DockPlacement, DockspaceActionRejection, DockspaceLayout, DockspaceView, ItemId,
    PreparedDockAction, PreparedTabsGroup, RootId, SurfaceId,
};
use dockspace::policy::DockPolicy;
#[cfg(feature = "serde")]
use dockspace::runtime::{DockspaceDocumentBootstrap, DockspaceDocumentId};
use dockspace::runtime::{
    DockspaceSession, HostFrameReport, PreparedCloseRequest, SurfaceUnavailableReason, SurfaceWork,
    SurfaceWorkDeferred, WorkspaceVersion,
};
use egui::emath::GuiRounding;
use egui::{Id, Sense, Ui};

use crate::DockInputConfig;
use crate::builder::DockspaceBuilder;
use crate::error::DockspaceError;
use crate::error_detail::DockspaceErrorSource;
use crate::pane::PaneView;
use crate::pass_settlement::{
    PresentationSettlementFailure, ProductPassEnrollment, ProductPassSettlement,
    ProductPassSettlementBatch,
};
use crate::product_render;
use crate::response::DockspaceMutation;
use crate::response::{
    DockspaceActionResult, DockspaceCapability, DockspaceCloseRequestResult, DockspaceCloseResult,
    DockspaceResponse, DockspaceSurfaceStatus, DockspaceUnavailableReason,
};
use crate::style::DockStyle;

#[path = "product_owned_run.rs"]
mod owned_run;

#[cfg(test)]
#[path = "product_chrome_tests.rs"]
mod chrome_tests;

/// Stateful egui adapter whose headless session is the sole docking authority.
pub struct Dockspace {
    id: Id,
    session: DockspaceSession,
    style: DockStyle,
    pass_settlement: ProductPassSettlement,
    measurement_deferred: bool,
    pending_focus: crate::product_focus::PendingFocusObservations,
    pointer: crate::product_pointer::ProductPointer,
    ui_actions: owned_run::UiActionQueue,
    #[cfg(test)]
    drop_target_observations: Vec<product_render::RegisteredDropTargetObservation>,
}

impl Dockspace {
    /// Starts a builder for a stable egui instance and product docking layout.
    pub fn builder(id_salt: impl Hash + Debug, layout: DockspaceLayout) -> DockspaceBuilder {
        DockspaceBuilder::new(id_salt, layout)
    }

    fn from_session(id: Id, session: DockspaceSession, style: DockStyle) -> Self {
        Self {
            id,
            session,
            style,
            pass_settlement: ProductPassSettlement::default(),
            measurement_deferred: false,
            pending_focus: crate::product_focus::PendingFocusObservations::default(),
            pointer: crate::product_pointer::ProductPointer::default(),
            ui_actions: owned_run::UiActionQueue::default(),
            #[cfg(test)]
            drop_target_observations: Vec::new(),
        }
    }

    pub(crate) fn from_layout_parts(
        id: Id,
        layout: DockspaceLayout,
        policy: DockPolicy,
        style: DockStyle,
    ) -> Result<Self, DockspaceError> {
        let presentation = style
            .presentation_config()
            .map_err(DockspaceError::from_detail)?;
        let session =
            DockspaceSession::from_layout_with_presentation_config(layout, policy, presentation)
                .map_err(DockspaceError::from_detail)?;
        Ok(Self::from_session(id, session, style))
    }

    #[cfg(feature = "serde")]
    pub(crate) fn from_persistent_layout_parts(
        id: Id,
        layout: DockspaceLayout,
        policy: DockPolicy,
        style: DockStyle,
        bootstrap: DockspaceDocumentBootstrap,
    ) -> Result<Self, DockspaceError> {
        let presentation = style
            .presentation_config()
            .map_err(DockspaceError::from_detail)?;
        let session = DockspaceSession::from_persistent_layout_with_presentation_config(
            layout,
            policy,
            presentation,
            bootstrap,
        )
        .map_err(DockspaceError::from_detail)?;
        Ok(Self::from_session(id, session, style))
    }

    /// Strictly restores one complete product document into a new egui facade.
    ///
    /// `recognize_external_item` must recognize every persisted application key,
    /// including closed historical panes. Numeric core identities are restored
    /// from the validated document and cannot be remapped by the application.
    ///
    /// # Errors
    ///
    /// Returns an error for invalid style, malformed or unsupported JSON, rejected
    /// pane identity associations, or a document that cannot initialize the core.
    #[cfg(feature = "serde")]
    pub fn from_document_json(
        id_salt: impl Hash + Debug,
        bytes: &[u8],
        policy: DockPolicy,
        style: DockStyle,
        recognize_external_item: impl Fn(DockspaceDocumentId, &str) -> bool,
    ) -> Result<Self, DockspaceError> {
        style.validate().map_err(DockspaceError::from_detail)?;
        let presentation = style
            .presentation_config()
            .map_err(DockspaceError::from_detail)?;
        let session = DockspaceSession::from_document_json_with_presentation_config(
            bytes,
            policy,
            presentation,
            recognize_external_item,
        )
        .map_err(DockspaceError::from_detail)?;
        Ok(Self::from_session(
            Id::unique(("egui_dockspace", id_salt)),
            session,
            style,
        ))
    }

    /// Returns the stable egui identity used to scope adapter widgets.
    #[must_use]
    pub const fn id(&self) -> Id {
        self.id
    }

    /// Returns the published item/surface-centric workspace view.
    #[must_use]
    pub fn view(&self) -> DockspaceView<'_> {
        self.session.view()
    }

    /// Returns the durable workspace version.
    #[must_use]
    pub const fn version(&self) -> WorkspaceVersion {
        self.session.version()
    }

    /// Returns the current declarative docking policy.
    #[must_use]
    pub const fn policy(&self) -> &DockPolicy {
        self.session.policy()
    }

    /// Returns the current renderer style.
    #[must_use]
    pub const fn style(&self) -> &DockStyle {
        &self.style
    }

    /// Replaces the complete declarative docking policy atomically.
    ///
    /// # Errors
    ///
    /// Returns an error without changing the published policy when the host frame cannot commit.
    pub fn set_policy(&mut self, policy: DockPolicy) -> Result<DockspaceMutation, DockspaceError> {
        let mut frame = self
            .session
            .begin_host_frame()
            .map_err(DockspaceError::from_detail)?;
        frame
            .replace_policy(policy)
            .map_err(DockspaceError::from_detail)?;
        frame
            .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
            .map_err(DockspaceError::from_detail)?;
        let report = frame.commit().map_err(DockspaceError::from_detail)?;
        Ok(DockspaceMutation::from_runtime_report(&report))
    }

    /// Atomically applies an application-owned default or preset layout.
    ///
    /// This path needs no serialization or document lineage. Pane data, item-key
    /// history, configured policy and renderer style remain application/session
    /// owned. Retired presentation identities are rebased by core before commit.
    ///
    /// # Errors
    ///
    /// Returns an error without partial publication when the arrangement is
    /// locked, a document-bound item is unknown, or the replacement cannot commit.
    pub fn reset_layout(
        &mut self,
        layout: DockspaceLayout,
    ) -> Result<DockspaceMutation, DockspaceError> {
        let reset = self
            .session
            .prepare_layout_reset(layout)
            .map_err(DockspaceError::from_detail)?;
        let mut frame = self
            .session
            .begin_layout_reset_frame(&reset)
            .map_err(DockspaceError::from_detail)?;
        frame
            .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
            .map_err(DockspaceError::from_detail)?;
        let report = frame.commit().map_err(DockspaceError::from_detail)?;
        self.pending_focus.clear();
        Ok(DockspaceMutation::from_runtime_report(&report))
    }

    /// Returns the adapter's docking activation and cancellation bindings.
    #[must_use]
    pub const fn input(&self) -> DockInputConfig {
        self.pointer.input()
    }

    /// Replaces gesture bindings at the next input boundary.
    ///
    /// An active pointer retains its last observed modifiers. The next run
    /// recomputes docking intent without inventing pointer motion or restoring
    /// a previously cleared target.
    pub fn set_input(&mut self, input: DockInputConfig) {
        self.pointer.set_input(input);
    }

    /// Replaces renderer geometry and visual styling atomically.
    ///
    /// Geometry changes commit through core before the adapter publishes the new style. A
    /// visual-only change remains adapter-local, so it cannot disturb an in-flight gesture or
    /// presentation settlement. Failed validation or geometry publication leaves both
    /// authorities unchanged.
    ///
    /// # Errors
    ///
    /// Returns an error when the style is invalid or the host frame cannot commit.
    pub fn set_style(&mut self, style: DockStyle) -> Result<DockspaceMutation, DockspaceError> {
        style.validate().map_err(DockspaceError::from_detail)?;
        let presentation = style
            .presentation_config()
            .map_err(DockspaceError::from_detail)?;
        let style_changed = self.style != style;
        let presentation_changed = self.session.presentation_config() != &presentation;
        if !presentation_changed {
            self.style = style;
            let mut mutation = DockspaceMutation::unchanged(self.session.version());
            if style_changed {
                mutation.include_adapter_presentation_change(
                    self.session.view().surfaces().map(|surface| surface.id()),
                );
            }
            return Ok(mutation);
        }
        let mut frame = self
            .session
            .begin_host_frame()
            .map_err(DockspaceError::from_detail)?;
        frame
            .replace_presentation_config(presentation)
            .map_err(DockspaceError::from_detail)?;
        frame
            .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
            .map_err(DockspaceError::from_detail)?;
        let report = frame.commit().map_err(DockspaceError::from_detail)?;
        self.style = style;
        let mut mutation = DockspaceMutation::from_runtime_report(&report);
        if style_changed {
            mutation.include_adapter_presentation_change(
                self.session.view().surfaces().map(|surface| surface.id()),
            );
        }
        Ok(mutation)
    }

    /// Returns the current durable document lineage, when configured.
    #[cfg(feature = "serde")]
    #[must_use]
    pub fn document_id(&self) -> Option<DockspaceDocumentId> {
        self.session.document_id()
    }

    /// Returns the generation assigned to the next successful save.
    #[cfg(feature = "serde")]
    #[must_use]
    pub fn next_document_generation(&self) -> Option<u64> {
        self.session.next_document_generation()
    }

    /// Resolves one session-owned application pane key.
    #[cfg(feature = "serde")]
    #[must_use]
    pub fn item_id_for_external_key(&self, external_key: &str) -> Option<ItemId> {
        self.session.item_id_for_external_key(external_key)
    }

    /// Resolves one item to its exact session-owned application key.
    #[cfg(feature = "serde")]
    #[must_use]
    pub fn external_key_for_item(&self, item: ItemId) -> Option<&str> {
        self.session.external_key_for_item(item)
    }

    /// Allocates one append-only application pane identity.
    ///
    /// # Errors
    ///
    /// Returns a persistence error when the session is not document-bound, the
    /// key is invalid, or the item identity space is exhausted.
    #[cfg(feature = "serde")]
    pub fn ensure_external_item(
        &mut self,
        external_key: impl Into<String>,
    ) -> Result<ItemId, DockspaceError> {
        self.session
            .ensure_external_item(external_key)
            .map_err(DockspaceError::from_detail)
    }

    /// Captures and encodes the complete session-owned document as JSON bytes.
    ///
    /// # Errors
    ///
    /// Returns a persistence error without advancing the document generation when
    /// strict capture or JSON encoding fails.
    #[cfg(feature = "serde")]
    pub fn save_document_json(&mut self) -> Result<Vec<u8>, DockspaceError> {
        self.session
            .save_document_json()
            .map_err(DockspaceError::from_detail)
    }

    /// Restores a complete document into this session at one host-frame boundary.
    ///
    /// The restored surfaces are initially deferred, so the next egui frame can
    /// measure and paint their exact product plans before exposing interaction.
    ///
    /// # Errors
    ///
    /// Returns a persistence or host-frame error without publishing a partial
    /// document when decoding, identity recognition, or candidate validation
    /// fails.
    #[cfg(feature = "serde")]
    pub fn restore_document_json(
        &mut self,
        bytes: &[u8],
        recognize_external_item: impl Fn(DockspaceDocumentId, &str) -> bool,
    ) -> Result<DockspaceMutation, DockspaceError> {
        let restore = self
            .session
            .prepare_document_restore_json(bytes, recognize_external_item)
            .map_err(DockspaceError::from_detail)?;
        let mut frame = self
            .session
            .begin_document_restore_frame(&restore)
            .map_err(DockspaceError::from_detail)?;
        frame
            .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
            .map_err(DockspaceError::from_detail)?;
        let report = frame.commit().map_err(DockspaceError::from_detail)?;
        self.pending_focus.clear();
        Ok(DockspaceMutation::from_runtime_report(&report))
    }

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

    /// Measures, paints, and publishes the sole surface from an existing egui callback.
    ///
    /// Renderer observations, focus, queued UI intent, and raw pointer input
    /// enter one frame before measurement and encoding. This callback publishes
    /// once. UI actions discovered while encoding enter a bounded queue for the
    /// next original egui run; repeated passes never replay those actions.
    ///
    /// [`Self::run_single_surface`] is the recommended entry point: it owns the
    /// complete run and can validate terminal output hooks before publication.
    /// This callback does not own that boundary. A later output hook failure or
    /// discard cannot roll back docking state already published here, or pane
    /// side effects. Inside [`crate::run_ui`], exact renderer reports can still
    /// authorize subsequent input. An unowned callback reports presentation
    /// settlement as unavailable and defers response-driven actions too.
    ///
    /// # Errors
    ///
    /// Returns an error when this is not the sole surface, measurements or UI
    /// intent admission fail, or the frame cannot publish. Before publication,
    /// an error drops the complete docking candidate.
    pub fn show_single_surface(
        &mut self,
        surface: SurfaceId,
        ui: &mut Ui,
        panes: &mut dyn PaneView,
    ) -> Result<DockspaceResponse, DockspaceError> {
        let mut surfaces = self.session.view().surfaces();
        let first = surfaces.next().map(|surface| surface.id());
        let count = usize::from(first.is_some()) + surfaces.count();
        if count != 1 {
            return Err(DockspaceErrorSource::SingleSurfaceRequiresOne { count }.into());
        }
        if first != Some(surface) {
            return Err(DockspaceErrorSource::SurfaceOutsideRoster { surface }.into());
        }
        #[cfg(test)]
        self.drop_target_observations.clear();
        let dock_rect = ui.available_rect_before_wrap();
        let popup_rect = ui.ctx().input(egui::InputState::content_rect).round_ui();
        let settlement_claim = match self.pass_settlement.enroll(ui) {
            ProductPassEnrollment::Acquired(claim) => Some(claim),
            ProductPassEnrollment::Unowned => None,
            ProductPassEnrollment::Conflict => {
                return Err(DockspaceErrorSource::PassSettlementConflict.into());
            }
        };
        let run_sequence = self.ui_actions.enter_run(ui)?;
        let ready_settlement = self.pass_settlement.take_ready(ui);
        let mut preceding_feedback = Vec::new();
        let mut retirement_repaint = false;
        if ready_settlement.lane_changed {
            if let Some(retired) = self
                .session
                .disable_host_pointer()
                .map_err(DockspaceError::from_detail)?
            {
                preceding_feedback.extend_from_slice(retired.manipulation_feedback());
                retirement_repaint |= retired.repaint_required();
            }
            self.pending_focus.clear();
            self.pointer.clear();
        }
        let presentation_failure = ready_settlement.presentation_failure;
        let presentation_settlement_available =
            settlement_claim.is_some() && presentation_failure.is_none();
        for presentation in ready_settlement.presentations {
            let artifact = presentation.output.presentation_artifact();
            if self
                .session
                .report_surface_presentation(presentation.output, presentation.result)
                .is_ok()
                && let Some(focus) = presentation.pane_focus_observation
            {
                self.pending_focus.push(artifact, focus);
            }
        }
        if presentation_settlement_available {
            self.session
                .enable_host_pointer()
                .map_err(DockspaceError::from_detail)?;
        } else {
            if let Some(retired) = self
                .session
                .disable_host_pointer()
                .map_err(DockspaceError::from_detail)?
            {
                preceding_feedback.extend_from_slice(retired.manipulation_feedback());
                retirement_repaint |= retired.repaint_required();
            }
            self.pointer.clear();
        }
        let mut pointer = self.pointer.clone();
        let mut pending_actions = None;
        let mut observed_at = None;
        let mut deferred_focus = Vec::new();
        let mut accepted_focus = Vec::new();
        let mut missing = BTreeSet::new();
        let mut defer_measurement = false;
        let mut pane_focus_observation = None;
        let mut pane_focus_pending = None;
        let mut pane_focus_capability =
            DockspaceCapability::Unavailable(DockspaceUnavailableReason::SurfaceBoundsUnavailable);
        let mut pointer_receivers = Vec::new();
        let mut focus_bindings = Vec::new();
        let mut had_plan = false;
        let mut proof = None;
        let result = (|| -> Result<HostFrameReport, DockspaceError> {
            let mut frame = self
                .session
                .begin_host_frame()
                .map_err(DockspaceError::from_detail)?;
            let actual_focus = frame.view().surface(surface).map(|view| {
                crate::product_focus::sample_host_focus(
                    ui.ctx(),
                    view,
                    panes,
                    ui.make_persistent_id((self.id, "workspace-focus", surface)),
                    ui.ctx()
                        .layer_transform_to_global(ui.layer_id())
                        .map_or(dock_rect, |transform| transform * dock_rect),
                    pointer.presented_focus_bindings(&frame, surface),
                )
            });
            accepted_focus = self
                .pending_focus
                .submit_current(&mut frame, surface)
                .map_err(DockspaceError::from_detail)?;
            if let Some(actual_focus) = actual_focus {
                frame
                    .report_host_focus(surface, actual_focus)
                    .map_err(DockspaceError::from_detail)?;
            }
            for action in self.ui_actions.before(run_sequence) {
                frame
                    .submit_deferred_surface_action(action)
                    .map_err(DockspaceError::from_detail)?;
            }
            if presentation_settlement_available {
                let input = pointer.prepare(&frame, surface, ui);
                observed_at = input.observed_at();
                self.pointer.record_rejected_run(&pointer);
                deferred_focus = input
                    .submit_into(&mut frame)
                    .map_err(DockspaceError::from_detail)?;
            }
            frame
                .begin_encoding_pass()
                .map_err(DockspaceError::from_detail)?;
            let (measure, backpressured) = match frame
                .surface_work(surface)
                .map_err(DockspaceError::from_detail)?
            {
                SurfaceWork::Ready(plan) => (
                    plan.measurement_refresh().is_allowed() && !self.measurement_deferred,
                    false,
                ),
                SurfaceWork::Deferred(SurfaceWorkDeferred::Unmeasured) => (true, false),
                SurfaceWork::Deferred(SurfaceWorkDeferred::OutputCapacity) => (false, true),
            };
            let mut resources = product_render::PaintResources::default();
            if measure {
                missing.extend(product_render::measure_surface(
                    &mut frame,
                    surface,
                    ui,
                    dock_rect,
                    popup_rect,
                    panes,
                    &self.style,
                    Some(&mut resources),
                )?);
            }
            self.ui_actions
                .stage_touch_menu(run_sequence, ui, self.id, surface, frame.version());
            let capture = settlement_claim
                .as_ref()
                .map(|_| crate::output_proof::SurfaceOutputCapture::begin_multilayer(ui.painter()));
            if backpressured {
                crate::product_focus::retain_presented_pane_focus(
                    ui.ctx(),
                    pointer.presented_focus_bindings(&frame, surface),
                );
            }
            if !backpressured
                && let Some(plan) = frame
                    .paint_plan(surface)
                    .map_err(DockspaceError::from_detail)?
            {
                let paint = product_render::paint_surface(
                    ui,
                    self.id,
                    plan,
                    panes,
                    &self.style,
                    &self.pointer.input(),
                    resources,
                    if presentation_settlement_available {
                        product_render::PointerActionAuthority::HostJournal
                    } else {
                        product_render::PointerActionAuthority::LocalResponses
                    },
                    #[cfg(feature = "native-render-support")]
                    None,
                )
                .map_err(DockspaceError::from_detail)?;
                missing.extend(paint.missing_items);
                defer_measurement = paint.defer_measurement;
                pane_focus_observation = paint.pane_focus_observation;
                pane_focus_pending = paint.pane_focus_pending;
                pane_focus_capability = paint.pane_focus_capability;
                pointer_receivers = paint.receivers;
                focus_bindings = paint.pane_focus_bindings;
                self.ui_actions
                    .capture(&mut pending_actions, run_sequence, paint.local_actions)?;
                #[cfg(test)]
                {
                    self.drop_target_observations = paint.drop_target_observations;
                }
                if settlement_claim.is_some()
                    && let Some(receipt) = paint.execution_receipt
                {
                    frame
                        .confirm_surface_painted(receipt)
                        .map_err(DockspaceError::from_detail)?;
                }
                had_plan = true;
            } else {
                ui.allocate_rect(dock_rect, Sense::hover());
                let visuals = self.style.resolved_visuals(ui.visuals());
                ui.painter()
                    .rect_filled(dock_rect, 0.0, visuals.workspace_fill);
            }
            proof = capture.and_then(|capture| capture.finish(ui.painter()));
            frame
                .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
                .map_err(DockspaceError::from_detail)?;
            frame.commit().map_err(DockspaceError::from_detail)
        })();
        let mut report = match result {
            Ok(report) => report,
            Err(error) => {
                self.pointer.record_rejected_run(&pointer);
                return Err(error);
            }
        };
        self.ui_actions
            .publish(pending_actions, run_sequence, &report);
        let output = report.take_painted_outputs().pop();
        pointer.observe(report.surface_presentation_commits());
        if let Some(output) = output.as_ref() {
            pointer.emitted(
                output.presentation_artifact(),
                ui.layer_id(),
                ui.ctx().cumulative_pass_nr().saturating_add(1),
                ui.ctx()
                    .layer_transform_to_global(ui.layer_id())
                    .is_none_or(|transform| transform == egui::emath::TSTransform::IDENTITY),
                pointer_receivers,
                focus_bindings,
            );
        }
        self.pointer = pointer;
        self.pending_focus.settle(&accepted_focus, &report);
        for id in deferred_focus {
            let target = ui
                .ctx()
                .viewport(|viewport| viewport.this_pass.widgets.get(id).copied());
            if let Some(target) = target
                && target.enabled
                && target.visible
                && target.sense.is_focusable()
                && ui.memory(|memory| memory.allows_interaction(target.layer_id))
            {
                ui.memory_mut(|memory| memory.request_focus(id));
            }
        }
        crate::product_pointer::schedule_pointer_continuation(&self.session, ui.ctx(), observed_at);
        self.measurement_deferred = defer_measurement;
        let needs_presentation_settlement =
            defer_measurement || pane_focus_observation.is_some() || pane_focus_pending.is_some();
        let continuation_awaits_presentation = self
            .session
            .pointer_continuation_awaits_presentation(surface);
        let settlement_staged = settlement_claim.is_some_and(|claim| {
            claim.stage(ProductPassSettlementBatch {
                emission_incomplete: had_plan && output.is_none(),
                output,
                proof,
                pane_focus_observation,
                pane_focus_pending,
                needs_repaint: needs_presentation_settlement || continuation_awaits_presentation,
            })
        });
        let presentation_settlement_capability = match presentation_failure {
            Some(PresentationSettlementFailure::OutputMismatch) => {
                DockspaceCapability::Unavailable(
                    DockspaceUnavailableReason::PresentationOutputMismatch,
                )
            }
            Some(PresentationSettlementFailure::Abandoned) => DockspaceCapability::Unavailable(
                DockspaceUnavailableReason::PresentationSettlementAbandoned,
            ),
            None if settlement_staged => DockspaceCapability::Supported,
            None => DockspaceCapability::Unavailable(
                DockspaceUnavailableReason::PresentationSettlementRequired,
            ),
        };
        if !settlement_staged {
            pane_focus_capability = DockspaceCapability::Unavailable(
                DockspaceUnavailableReason::PresentationSettlementRequired,
            );
        }
        if !had_plan
            || report.repaint_surfaces().contains(&surface)
            || !self.ui_actions.is_empty()
            || (needs_presentation_settlement && settlement_staged)
            || retirement_repaint
        {
            ui.ctx().request_repaint();
        }
        let mut response = product_response(
            &report,
            surface,
            missing,
            had_plan,
            had_plan,
            presentation_settlement_capability,
            pane_focus_capability,
        )?;
        response.include_preceding_feedback(preceding_feedback);
        Ok(response)
    }

    #[cfg(test)]
    fn registered_drop_target_observations(
        &self,
    ) -> &[product_render::RegisteredDropTargetObservation] {
        &self.drop_target_observations
    }
}

#[cfg(test)]
#[path = "product_dockspace_tests.rs"]
mod tests;

fn product_response(
    report: &HostFrameReport,
    surface: SurfaceId,
    missing: BTreeSet<ItemId>,
    local_actions_current: bool,
    had_plan: bool,
    presentation_settlement_capability: DockspaceCapability,
    pane_focus_capability: DockspaceCapability,
) -> Result<DockspaceResponse, DockspaceError> {
    DockspaceResponse::from_product_report(
        report,
        surface,
        missing.into_iter().collect(),
        local_actions_current,
        if had_plan {
            DockspaceSurfaceStatus::Ready
        } else {
            DockspaceSurfaceStatus::Bootstrap
        },
        presentation_settlement_capability,
        pane_focus_capability,
    )
    .ok_or(DockspaceErrorSource::ApplicationOutcomeUnavailable {
        operation: "surface contribution",
    })
    .map_err(Into::into)
}
