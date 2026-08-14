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
    DockspaceSession, HostFrameReport, SurfaceUnavailableReason, WorkspaceVersion,
};
use egui::emath::GuiRounding;
use egui::{Id, Sense, Ui};

use crate::builder::DockspaceBuilder;
use crate::error::DockspaceError;
use crate::error_detail::DockspaceErrorSource;
use crate::pane::PaneView;
use crate::product_render;
use crate::response::{
    DockspaceActionResult, DockspaceCloseResult, DockspaceResponse, DockspaceSurfaceStatus,
};
use crate::style::DockStyle;

/// Stateful egui adapter whose headless session is the sole docking authority.
pub struct Dockspace {
    id: Id,
    session: DockspaceSession,
    style: DockStyle,
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
        Ok(Self { id, session, style })
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
        Ok(Self { id, session, style })
    }

    /// Strictly restores one complete product document into a new egui facade.
    ///
    /// `resolve_external_item` must return the application's expected item identity
    /// for every persisted key, including closed historical panes.
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
        resolve_external_item: impl Fn(DockspaceDocumentId, &str) -> Option<ItemId>,
    ) -> Result<Self, DockspaceError> {
        style.validate().map_err(DockspaceError::from_detail)?;
        let presentation = style
            .presentation_config()
            .map_err(DockspaceError::from_detail)?;
        let session = DockspaceSession::from_document_json_with_presentation_config(
            bytes,
            policy,
            presentation,
            resolve_external_item,
        )
        .map_err(DockspaceError::from_detail)?;
        Ok(Self {
            id: Id::new(("egui_dockspace", id_salt)),
            session,
            style,
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

    /// Measures, paints, and atomically advances the sole logical surface.
    ///
    /// Current-pass egui responses are reduced through opaque surface actions.
    /// This path deliberately does not fabricate retained renderer or native
    /// pointer authority from callback order.
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
        let dock_rect = ui.available_rect_before_wrap();
        let popup_rect = ui.ctx().input(egui::InputState::content_rect).round_ui();
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

        let mut missing = BTreeSet::new();
        let mut defer_measurement = false;
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
                product_render::PointerActionAuthority::LocalResponses,
            );
            missing.extend(paint.missing_items);
            defer_measurement = paint.defer_measurement;
            for action in paint
                .presentation_actions
                .into_iter()
                .chain(paint.local_actions)
            {
                frame
                    .submit_surface_action(action)
                    .map_err(DockspaceError::from_detail)?;
            }
            true
        } else {
            ui.allocate_rect(dock_rect, Sense::hover());
            ui.painter()
                .rect_filled(dock_rect, 0.0, self.style.workspace_fill);
            false
        };

        if defer_measurement {
            frame
                .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
                .map_err(DockspaceError::from_detail)?;
        } else {
            missing.extend(product_render::measure_surface(
                &mut frame,
                surface,
                ui,
                dock_rect,
                popup_rect,
                panes,
                &self.style,
            )?);
        }
        let report = frame.commit().map_err(DockspaceError::from_detail)?;
        if !had_plan || report.repaint_surfaces().contains(&surface) {
            ui.ctx().request_repaint();
        }
        product_response(&report, surface, missing, had_plan)
    }
}

fn product_response(
    report: &HostFrameReport,
    surface: SurfaceId,
    missing: BTreeSet<ItemId>,
    had_plan: bool,
) -> Result<DockspaceResponse, DockspaceError> {
    DockspaceResponse::from_product_report(
        report,
        surface,
        missing.into_iter().collect(),
        had_plan,
        if had_plan {
            DockspaceSurfaceStatus::Ready
        } else {
            DockspaceSurfaceStatus::Bootstrap
        },
    )
    .ok_or(DockspaceErrorSource::ApplicationOutcomeUnavailable {
        operation: "surface contribution",
    })
    .map_err(Into::into)
}
