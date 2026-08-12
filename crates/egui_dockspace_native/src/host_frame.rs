//! Native host-frame adapter over the renderer-neutral session frame.

use std::sync::Arc;

use dockspace::model::SurfaceId;
use dockspace::runtime::{
    DockspaceHostFrame, HostFrameReport, PreparedSurfaceAction, SurfaceUnavailableReason,
};
use eframe::egui;
use egui_dockspace::{DockStyle, PaneView};

use crate::error::NativeRuntimeError;
use crate::mailbox::NativeHostBridge;

/// Affine native host frame which commits the output barrier and only then
/// releases later callback records.
pub(crate) struct NativeHostFrame<'session> {
    pub(crate) frame: DockspaceHostFrame<'session>,
    bridge: Arc<NativeHostBridge>,
}

impl std::fmt::Debug for NativeHostFrame<'_> {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("NativeHostFrame")
            .finish_non_exhaustive()
    }
}

impl<'session> NativeHostFrame<'session> {
    pub(crate) fn new(frame: DockspaceHostFrame<'session>, bridge: Arc<NativeHostBridge>) -> Self {
        NativeHostFrame { frame, bridge }
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
        let Self { frame, bridge } = self;
        let report = frame.commit()?;
        bridge.commit_frame_boundary();
        Ok(report)
    }
}
