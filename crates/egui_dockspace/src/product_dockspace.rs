//! Default egui facade backed by one renderer-neutral [`DockspaceSession`].

use std::collections::BTreeSet;
use std::{fmt::Debug, hash::Hash};

use dockspace::close::{
    CloseDecision, CloseDecisionToken, CloseRequestId, DeferredCloseDecision, DeferredCloseToken,
};
use dockspace::model::{
    DockPlacement, DockspaceLayout, DockspaceView, ItemId, PreparedDockAction, RootId, SurfaceId,
};
use dockspace::policy::DockPolicy;
#[cfg(feature = "serde")]
use dockspace::runtime::{DockspaceDocumentBootstrap, DockspaceDocumentId};
use dockspace::runtime::{
    DockspaceSession, HostFrameReport, HostSurfaceCommitStatus, PreparedCloseRequest,
    PreparedPaneFocusObservation, SurfacePresentationArtifact, SurfaceUnavailableReason,
    WorkspaceVersion,
};
use egui::emath::GuiRounding;
use egui::{Id, Sense, Ui};

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

/// Stateful egui adapter whose headless session is the sole docking authority.
pub struct Dockspace {
    id: Id,
    session: DockspaceSession,
    style: DockStyle,
    pass_settlement: ProductPassSettlement,
    presentation_settlement_was_available: bool,
    measurement_deferred: bool,
    pending_focus: Vec<(SurfacePresentationArtifact, PreparedPaneFocusObservation)>,
    #[cfg(test)]
    drop_target_observations: Vec<product_render::RegisteredDropTargetObservation>,
}

impl Dockspace {
    /// Starts a builder for a stable egui instance and product docking layout.
    pub fn builder(id_salt: impl Hash + Debug, layout: DockspaceLayout) -> DockspaceBuilder {
        DockspaceBuilder::new(id_salt, layout)
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
        Ok(Self {
            id,
            session,
            style,
            pass_settlement: ProductPassSettlement::default(),
            presentation_settlement_was_available: false,
            measurement_deferred: false,
            pending_focus: Vec::new(),
            #[cfg(test)]
            drop_target_observations: Vec::new(),
        })
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
        Ok(Self {
            id,
            session,
            style,
            pass_settlement: ProductPassSettlement::default(),
            presentation_settlement_was_available: false,
            measurement_deferred: false,
            pending_focus: Vec::new(),
            #[cfg(test)]
            drop_target_observations: Vec::new(),
        })
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
        Ok(Self {
            id: Id::unique(("egui_dockspace", id_salt)),
            session,
            style,
            pass_settlement: ProductPassSettlement::default(),
            presentation_settlement_was_available: false,
            measurement_deferred: false,
            pending_focus: Vec::new(),
            #[cfg(test)]
            drop_target_observations: Vec::new(),
        })
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

    /// Prepares a revision-bound complete-root docking action.
    pub const fn prepare_dock_root(
        &self,
        root: RootId,
        placement: DockPlacement,
    ) -> PreparedDockAction {
        self.session.prepare_dock_root(root, placement)
    }

    /// Prepares a revision-bound action that moves one item into contained floating content.
    pub const fn prepare_float_item(&self, item: ItemId, surface: SurfaceId) -> PreparedDockAction {
        self.session.prepare_float_item(item, surface)
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
    /// # Errors
    ///
    /// Returns an error when the action belongs to another session, its source
    /// revision is invalid, or the atomic host frame cannot be committed.
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
    /// # Errors
    ///
    /// Returns an error when the placement is invalid or the action cannot be
    /// reduced atomically.
    pub fn open_item_current(
        &mut self,
        item: ItemId,
        placement: DockPlacement,
    ) -> Result<DockspaceActionResult, DockspaceError> {
        self.submit_prepared_action(self.prepare_open_item(item, placement))
    }

    /// Docks one item at a stable placement against the current revision.
    ///
    /// # Errors
    ///
    /// Returns an error when the placement is invalid or the action cannot be
    /// reduced atomically.
    pub fn dock_item_current(
        &mut self,
        item: ItemId,
        placement: DockPlacement,
    ) -> Result<DockspaceActionResult, DockspaceError> {
        self.submit_prepared_action(self.prepare_dock_item(item, placement))
    }

    /// Docks one complete root at a stable placement against the current revision.
    ///
    /// # Errors
    ///
    /// Returns an error when the placement is invalid or the action cannot be
    /// reduced atomically.
    pub fn dock_root_current(
        &mut self,
        root: RootId,
        placement: DockPlacement,
    ) -> Result<DockspaceActionResult, DockspaceError> {
        self.submit_prepared_action(self.prepare_dock_root(root, placement))
    }

    /// Moves one open item into a contained presentation on an existing surface.
    ///
    /// A complete singleton root preserves its root identity. Moving one item out
    /// of a larger root allocates a fresh root and contained-presentation identity.
    ///
    /// # Errors
    ///
    /// Returns an error when the item or surface is unavailable, current policy
    /// rejects contained floating, or the action cannot be reduced atomically.
    pub fn float_item_current(
        &mut self,
        item: ItemId,
        surface: SurfaceId,
    ) -> Result<DockspaceActionResult, DockspaceError> {
        self.submit_prepared_action(self.prepare_float_item(item, surface))
    }

    /// Raises the contained presentation owning one item to the front.
    ///
    /// # Errors
    ///
    /// Returns an error when the item is not contained, current policy rejects
    /// the update, or the action cannot be reduced atomically.
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

    /// Measures, paints, and advances the sole logical surface.
    ///
    /// Current-pass egui responses are reduced through opaque surface actions.
    /// This path deliberately does not fabricate retained renderer or native
    /// pointer authority from callback order.
    ///
    /// Local actions commit in the current call. Presentation acknowledgements
    /// and pane-focus observations become eligible only when this call is inside
    /// [`crate::run_ui`], whose private guard owns the complete egui run.
    /// Direct calls from an unmodified `eframe::App` remain useful for rendering,
    /// selection, close, and other actions that do not require terminal proof,
    /// but report presentation settlement as unavailable.
    ///
    /// Output observations, measurement, complete emission, and current local
    /// input commit at separate core boundaries. A later error does not roll
    /// back an earlier successful boundary or application pane side effects.
    ///
    /// # Errors
    ///
    /// Returns an error when the session does not contain exactly this surface,
    /// egui measurements are invalid, or the atomic host frame cannot commit.
    pub fn show_single_surface(
        &mut self,
        surface: SurfaceId,
        ui: &mut Ui,
        panes: &mut dyn PaneView,
    ) -> Result<DockspaceResponse, DockspaceError> {
        #[cfg(test)]
        self.drop_target_observations.clear();
        let dock_rect = ui.available_rect_before_wrap();
        let popup_rect = ui.ctx().input(egui::InputState::content_rect).round_ui();
        let enrollment = self.pass_settlement.enroll(ui);
        let settlement_claim = match enrollment {
            ProductPassEnrollment::Acquired(claim) => Some(claim),
            ProductPassEnrollment::Unowned => None,
            ProductPassEnrollment::Conflict => {
                return Err(DockspaceErrorSource::PassSettlementConflict.into());
            }
        };
        let ready_settlement = self.pass_settlement.take_ready(ui);
        if ready_settlement.lane_changed {
            self.pending_focus.clear();
        }
        let presentation_failure = ready_settlement.presentation_failure;
        let presentation_settlement_available =
            settlement_claim.is_some() && presentation_failure.is_none();
        let cancel_active_gesture = presentation_failure.is_some()
            || (self.presentation_settlement_was_available && !presentation_settlement_available);

        // Submit exact renderer facts first. Focus is eligible only after core
        // accepts the matching artifact, including when reports arrive out of order.
        let mut preceding_reports = Vec::new();
        let has_reports =
            !ready_settlement.presentations.is_empty() || !self.pending_focus.is_empty();
        for presentation in ready_settlement.presentations {
            let artifact = presentation.output.presentation_artifact();
            if self
                .session
                .report_surface_presentation(presentation.output, presentation.result)
                .is_ok()
                && let Some(focus) = presentation.pane_focus_observation
            {
                self.pending_focus.push((artifact, focus));
            }
        }
        let mut accepted_focus = Vec::new();
        if has_reports {
            let mut observation_frame = self
                .session
                .begin_host_frame()
                .map_err(DockspaceError::from_detail)?;
            observation_frame
                .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
                .map_err(DockspaceError::from_detail)?;
            let observation_report = observation_frame
                .commit()
                .map_err(DockspaceError::from_detail)?;
            for (artifact, focus) in std::mem::take(&mut self.pending_focus) {
                let commits = observation_report.surface_presentation_commits();
                if commits.iter().any(|commit| commit.presents(artifact)) {
                    accepted_focus.push(focus);
                } else if !commits.iter().any(|commit| commit.settles(artifact)) {
                    self.pending_focus.push((artifact, focus));
                }
            }
            preceding_reports.push(observation_report);
        }
        if !accepted_focus.is_empty() {
            let mut focus_frame = self
                .session
                .begin_host_frame()
                .map_err(DockspaceError::from_detail)?;
            for focus in accepted_focus {
                focus_frame
                    .submit_pane_focus_observation(focus)
                    .map_err(DockspaceError::from_detail)?;
            }
            focus_frame
                .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
                .map_err(DockspaceError::from_detail)?;
            preceding_reports.push(focus_frame.commit().map_err(DockspaceError::from_detail)?);
        }

        let mut frame = self
            .session
            .begin_host_frame()
            .map_err(DockspaceError::from_detail)?;
        let surfaces = frame.surfaces();
        if surfaces.len() != 1 {
            return Err(DockspaceErrorSource::SingleSurfaceRequiresOne {
                count: surfaces.len(),
            }
            .into());
        }
        if surfaces.first().copied() != Some(surface) {
            return Err(DockspaceErrorSource::SurfaceOutsideRoster { surface }.into());
        }
        let should_measure = !self.measurement_deferred
            || frame
                .paint_plan(surface)
                .map_err(DockspaceError::from_detail)?
                .is_none();
        let mut missing = BTreeSet::new();
        if should_measure {
            missing.extend(product_render::measure_surface(
                &mut frame,
                surface,
                ui,
                dock_rect,
                popup_rect,
                panes,
                &self.style,
            )?);
        } else {
            frame
                .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
                .map_err(DockspaceError::from_detail)?;
        }
        let measurement_report = frame.commit().map_err(DockspaceError::from_detail)?;
        let mut frame = self
            .session
            .begin_host_frame()
            .map_err(DockspaceError::from_detail)?;
        let capture = settlement_claim
            .as_ref()
            .map(|_| crate::output_proof::SurfaceOutputCapture::begin(ui.painter()));
        let mut defer_measurement = false;
        let mut pane_focus_observation = None;
        let mut pane_focus_pending = None;
        let mut pane_focus_capability =
            DockspaceCapability::Unavailable(DockspaceUnavailableReason::SurfaceBoundsUnavailable);
        let mut local_actions = Vec::new();
        let had_plan = if let Some(plan) = frame
            .paint_plan(surface)
            .map_err(DockspaceError::from_detail)?
        {
            let paint = product_render::paint_surface(
                ui,
                self.id,
                plan,
                panes,
                &self.style,
                product_render::PointerActionAuthority::LocalResponses {
                    local_actions: true,
                    presentation_settlement: presentation_settlement_available,
                    cancel_active_gesture,
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
            #[cfg(test)]
            {
                self.drop_target_observations = paint.drop_target_observations;
            }
            local_actions = paint.local_actions;
            if settlement_claim.is_some()
                && let Some(receipt) = paint.execution_receipt
            {
                frame
                    .confirm_surface_painted(receipt)
                    .map_err(DockspaceError::from_detail)?;
            }
            true
        } else {
            ui.allocate_rect(dock_rect, Sense::hover());
            let visuals = self.style.resolved_visuals(ui.visuals());
            ui.painter()
                .rect_filled(dock_rect, 0.0, visuals.workspace_fill);
            false
        };
        let proof = capture.and_then(|capture| capture.finish(ui.painter()));
        frame
            .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
            .map_err(DockspaceError::from_detail)?;
        let mut paint_report = frame.commit().map_err(DockspaceError::from_detail)?;
        let output = paint_report.take_painted_outputs().into_iter().next();

        // Input cannot mutate the interaction captured by an already executed
        // paint plan. It commits in a subsequent core frame, preserving exact
        // execution-receipt validation at the paint boundary.
        let mut frame = self
            .session
            .begin_host_frame()
            .map_err(DockspaceError::from_detail)?;
        let has_local_actions = !local_actions.is_empty();
        for action in local_actions {
            frame
                .submit_surface_action(action)
                .map_err(DockspaceError::from_detail)?;
        }
        frame
            .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
            .map_err(DockspaceError::from_detail)?;
        self.measurement_deferred = defer_measurement;
        let report = frame.commit().map_err(DockspaceError::from_detail)?;
        if has_local_actions && defer_measurement && report.published_state_changed() {
            let marker = ui.make_persistent_id((self.id, "surface-input-projection", surface));
            product_render::request_discard_once(
                ui,
                marker,
                "egui_dockspace: paint the current interaction projection",
            );
        }
        let recovered_ready = !had_plan
            && report.surface_commits().iter().any(|commit| {
                commit.surface() == surface && commit.status() == HostSurfaceCommitStatus::Ready
            });
        if recovered_ready {
            let marker = ui.make_persistent_id((self.id, "ready-surface-discard-frame", surface));
            product_render::request_discard_once(
                ui,
                marker,
                "egui_dockspace: paint newly ready surface in the terminal pass",
            );
        }
        let needs_presentation_settlement =
            defer_measurement || pane_focus_observation.is_some() || pane_focus_pending.is_some();
        let settlement_staged = settlement_claim.is_some_and(|claim| {
            claim.stage(ProductPassSettlementBatch {
                emission_incomplete: had_plan && output.is_none(),
                output,
                proof,
                pane_focus_observation,
                pane_focus_pending,
                needs_repaint: needs_presentation_settlement,
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
        self.presentation_settlement_was_available =
            presentation_settlement_available && settlement_staged;
        if needs_presentation_settlement && settlement_staged {
            ui.ctx().request_repaint();
        }
        if !had_plan
            || report.repaint_surfaces().contains(&surface)
            || measurement_report.repaint_surfaces().contains(&surface)
        {
            ui.ctx().request_repaint();
        }
        preceding_reports.push(measurement_report);
        preceding_reports.push(paint_report);
        let mut response = product_response(
            &report,
            surface,
            missing,
            had_plan,
            had_plan,
            presentation_settlement_capability,
            pane_focus_capability,
        )?;
        response.include_preceding_reports(&preceding_reports);
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
