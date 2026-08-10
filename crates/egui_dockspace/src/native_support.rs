//! Private cross-crate rendering seam for the fork-backed native adapter.
//!
//! The native crate owns the `DockspaceSession`; this module only reuses the
//! ordinary product renderer over an externally owned host frame. Keeping this
//! seam here prevents the native crate from copying a second projection or
//! pane/tab renderer.

use std::collections::BTreeSet;

use dockspace::model::{ItemId, SurfaceId};
use dockspace::runtime::{DockspaceHostFrame, SurfaceUnavailableReason};
use egui::emath::GuiRounding;
use egui::{Id, Ui};

use crate::error::DockspaceError;
use crate::pane::PaneView;
use crate::product_render;
use crate::style::DockStyle;

/// Result of painting one surface against a core-owned host frame.
#[derive(Debug)]
pub struct NativeSurfacePaint {
    had_ready_plan: bool,
    deferred_measurement: bool,
    missing_items: BTreeSet<ItemId>,
}

impl NativeSurfacePaint {
    /// Returns whether a ready core plan was painted.
    #[must_use]
    pub const fn had_ready_plan(&self) -> bool {
        self.had_ready_plan
    }

    /// Returns whether the frame should retain the current contribution rather
    /// than publish a new measured contribution.
    #[must_use]
    pub const fn deferred_measurement(&self) -> bool {
        self.deferred_measurement
    }

    /// Returns pane identities which were missing from the application catalog.
    #[must_use]
    pub fn missing_items(&self) -> impl Iterator<Item = ItemId> + '_ {
        self.missing_items.iter().copied()
    }
}

/// Paints one ready surface using the same renderer as the ordinary product
/// facade. Actions are submitted to the supplied core host frame before this
/// function returns; no second docking authority is created.
pub fn paint_surface(
    frame: &mut DockspaceHostFrame<'_>,
    instance_id: Id,
    surface: SurfaceId,
    ui: &mut Ui,
    panes: &mut dyn PaneView,
    style: &DockStyle,
) -> Result<NativeSurfacePaint, DockspaceError> {
    let dock_rect = ui.available_rect_before_wrap();
    let Some(plan) = frame
        .paint_plan(surface)
        .map_err(DockspaceError::from_detail)?
    else {
        ui.allocate_rect(dock_rect, egui::Sense::hover());
        ui.painter()
            .rect_filled(dock_rect, 0.0, style.workspace_fill);
        return Ok(NativeSurfacePaint {
            had_ready_plan: false,
            deferred_measurement: false,
            missing_items: BTreeSet::new(),
        });
    };

    let painted = product_render::paint_surface(ui, instance_id, plan, panes, style);
    for action in painted.actions {
        frame
            .submit_surface_action(action)
            .map_err(DockspaceError::from_detail)?;
    }
    Ok(NativeSurfacePaint {
        had_ready_plan: true,
        deferred_measurement: painted.defer_measurement,
        missing_items: painted.missing_items,
    })
}

/// Supplies the egui measurements required to compile the next core surface
/// contribution. This is intentionally separate from painting: a native host
/// can paint a previously accepted plan and acknowledge its output while a
/// later frame measures the next plan.
pub fn measure_surface(
    frame: &mut DockspaceHostFrame<'_>,
    surface: SurfaceId,
    ui: &Ui,
    panes: &dyn PaneView,
    style: &DockStyle,
) -> Result<BTreeSet<ItemId>, DockspaceError> {
    let dock_rect = ui.available_rect_before_wrap();
    let popup_rect = ui.ctx().input(egui::InputState::content_rect).round_ui();
    product_render::measure_surface(frame, surface, ui, dock_rect, popup_rect, panes, style)
}

/// Explicitly settles every surface not painted by the native host callback.
pub fn defer_unpainted_surfaces(frame: &mut DockspaceHostFrame<'_>) -> Result<(), DockspaceError> {
    frame
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .map_err(DockspaceError::from_detail)
}
