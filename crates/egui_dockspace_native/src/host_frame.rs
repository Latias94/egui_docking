//! Native host-frame adapter over the renderer-neutral session frame.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex, PoisonError};

use dockspace::model::SurfaceId;
use dockspace::runtime::{
    DockspaceHostFrame, HostFrameReport, NativeStagingPaintRequest, PreparedDockAction,
    PreparedSurfaceAction, SurfaceUnavailableReason,
};
use eframe::egui;
use egui_dockspace::{DockStyle, PaneView};

use crate::error::NativeHostProtocolError;
use crate::error::NativeRuntimeError;
use crate::mailbox::NativeHostBridge;
use crate::viewport_map::NativeViewportMap;

/// Final-pass paint metadata plus fork-owned scroll identities from that same pass.
pub(crate) struct NativeSurfacePaint {
    inner: egui_dockspace::native_support::NativeSurfacePaint,
    scroll_identities: Vec<egui::WidgetHitIdentity>,
}

impl std::ops::Deref for NativeSurfacePaint {
    type Target = egui_dockspace::native_support::NativeSurfacePaint;

    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}

impl std::ops::DerefMut for NativeSurfacePaint {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.inner
    }
}

impl NativeSurfacePaint {
    pub(crate) fn scroll_identities(&self) -> &[egui::WidgetHitIdentity] {
        &self.scroll_identities
    }
}

/// Affine native host frame which commits the output barrier and only then
/// releases later callback records.
pub(crate) struct NativeHostFrame<'session> {
    pub(crate) frame: DockspaceHostFrame<'session>,
    bridge: Arc<NativeHostBridge>,
    viewports: Arc<Mutex<NativeViewportMap>>,
    repaint_context: Option<egui::Context>,
}

impl std::fmt::Debug for NativeHostFrame<'_> {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("NativeHostFrame")
            .finish_non_exhaustive()
    }
}

impl<'session> NativeHostFrame<'session> {
    pub(crate) fn new(
        frame: DockspaceHostFrame<'session>,
        bridge: Arc<NativeHostBridge>,
        viewports: Arc<Mutex<NativeViewportMap>>,
        repaint_context: Option<egui::Context>,
    ) -> Self {
        NativeHostFrame {
            frame,
            bridge,
            viewports,
            repaint_context,
        }
    }

    pub(crate) fn complete_unpainted_surfaces(
        &mut self,
        reason: SurfaceUnavailableReason,
    ) -> Result<(), NativeRuntimeError> {
        self.frame
            .complete_unpainted_surfaces(reason)
            .map_err(Into::into)
    }

    pub(crate) fn paint_surface(
        &mut self,
        instance_id: egui::Id,
        surface: SurfaceId,
        ui: &mut egui::Ui,
        panes: &mut dyn PaneView,
        style: &DockStyle,
    ) -> Result<NativeSurfacePaint, NativeRuntimeError> {
        let mut scroll_identities = Vec::new();
        let mut register_scroll_candidate = |ui: &egui::Ui, rect: egui::Rect, id: egui::Id| {
            let identity = ui.register_scroll_hit_candidate(rect, id);
            scroll_identities.push(identity);
            (identity.id(), identity.layer_id())
        };
        let paint = egui_dockspace::native_support::paint_surface(
            &mut self.frame,
            instance_id,
            surface,
            ui,
            panes,
            style,
            &mut register_scroll_candidate,
        )
        .map_err(NativeRuntimeError::from)?;
        let receiver_count = paint.scroll_receivers().len();
        if receiver_count != scroll_identities.len()
            || !paint
                .scroll_receivers()
                .zip(scroll_identities.iter().copied())
                .all(|(receiver, identity)| {
                    receiver.widget_id() == identity.id()
                        && receiver.layer_id() == identity.layer_id()
                })
        {
            return Err(NativeHostProtocolError::ScrollReceiverIdentityMismatch.into());
        }
        Ok(NativeSurfacePaint {
            inner: paint,
            scroll_identities,
        })
    }

    pub(crate) fn measure_surface(
        &mut self,
        surface: SurfaceId,
        ui: &egui::Ui,
        dock_rect: egui::Rect,
        popup_rect: egui::Rect,
        panes: &dyn PaneView,
        style: &DockStyle,
    ) -> Result<(), NativeRuntimeError> {
        egui_dockspace::native_support::measure_surface(
            &mut self.frame,
            surface,
            ui,
            dock_rect,
            popup_rect,
            panes,
            style,
        )
        .map(|_| ())
        .map_err(Into::into)
    }

    pub(crate) fn submit_surface_action(
        &mut self,
        action: PreparedSurfaceAction,
    ) -> Result<(), NativeRuntimeError> {
        self.frame.submit_surface_action(action).map_err(Into::into)
    }

    pub(crate) fn submit_prepared_action(
        &mut self,
        action: PreparedDockAction,
    ) -> Result<(), NativeRuntimeError> {
        self.frame
            .submit_prepared_action(action)
            .map_err(Into::into)
    }

    pub(crate) fn confirm_surface_painted(
        &mut self,
        surface: SurfaceId,
    ) -> Result<(), NativeRuntimeError> {
        self.frame
            .confirm_surface_painted(surface)
            .map_err(Into::into)
    }

    /// Commits the core frame and releases all presentation records included in
    /// that committed causal boundary.
    ///
    /// # Errors
    ///
    /// Returns an error without releasing the callback-order barrier when the
    /// core rejects the candidate frame.
    pub fn commit(self) -> Result<HostFrameReport, NativeRuntimeError> {
        let staging_requests = self.compile_staging_requests()?;
        let staging_viewports = staging_requests.keys().copied().collect::<Vec<_>>();
        let Self {
            frame,
            bridge,
            viewports: _,
            repaint_context,
        } = self;
        let report = frame.commit()?;
        bridge.replace_staging_requests(staging_requests);
        bridge.commit_frame_boundary();
        if let Some(context) = repaint_context {
            for viewport in staging_viewports {
                context.request_repaint_of(viewport);
            }
        }
        Ok(report)
    }

    fn compile_staging_requests(
        &self,
    ) -> Result<BTreeMap<egui::ViewportId, NativeStagingPaintRequest>, NativeRuntimeError> {
        let viewports = self
            .viewports
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        let mut requests = BTreeMap::new();
        for request in self.frame.native_staging_paints() {
            let Some(viewport) = viewports.viewport(request.surface()) else {
                return Err(NativeHostProtocolError::OutputRouteAttachmentFailed.into());
            };
            if viewports.binding(viewport) != Some(request.binding())
                || requests.insert(viewport, request).is_some()
            {
                return Err(NativeHostProtocolError::OutputRouteAttachmentFailed.into());
            }
        }
        Ok(requests)
    }
}
