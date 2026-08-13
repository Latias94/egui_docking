//! Native host-frame adapter over the renderer-neutral session frame.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex, PoisonError};

use dockspace::model::SurfaceId;
use dockspace::runtime::{
    DockspaceHostFrame, HostFrameReport, NativeStagingPaintRequest, PreparedSurfaceAction,
    SurfaceUnavailableReason,
};
use eframe::egui;
use egui_dockspace::{DockStyle, PaneView};

use crate::error::NativeRuntimeError;
use crate::error::NativeHostProtocolError;
use crate::mailbox::NativeHostBridge;
use crate::viewport_map::NativeViewportMap;

/// Affine native host frame which commits the output barrier and only then
/// releases later callback records.
pub(crate) struct NativeHostFrame<'session> {
    pub(crate) frame: DockspaceHostFrame<'session>,
    bridge: Arc<NativeHostBridge>,
    viewports: Arc<Mutex<NativeViewportMap>>,
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
    ) -> Self {
        NativeHostFrame {
            frame,
            bridge,
            viewports,
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
    ) -> Result<egui_dockspace::native_support::NativeSurfacePaint, NativeRuntimeError> {
        egui_dockspace::native_support::paint_surface(
            &mut self.frame,
            instance_id,
            surface,
            ui,
            panes,
            style,
        )
        .map_err(Into::into)
    }

    pub(crate) fn measure_surface(
        &mut self,
        surface: SurfaceId,
        ui: &egui::Ui,
        panes: &dyn PaneView,
        style: &DockStyle,
    ) -> Result<(), NativeRuntimeError> {
        egui_dockspace::native_support::measure_surface(&mut self.frame, surface, ui, panes, style)
            .map(|_| ())
            .map_err(Into::into)
    }

    pub(crate) fn submit_surface_action(
        &mut self,
        action: PreparedSurfaceAction,
    ) -> Result<(), NativeRuntimeError> {
        self.frame.submit_surface_action(action).map_err(Into::into)
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
        let Self {
            frame,
            bridge,
            viewports: _,
        } = self;
        let report = frame.commit()?;
        bridge.replace_staging_requests(staging_requests);
        bridge.commit_frame_boundary();
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
