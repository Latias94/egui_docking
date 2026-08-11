//! Private cross-crate rendering seam for the fork-backed native adapter.
//!
//! The native crate owns the `DockspaceSession`; this module only reuses the
//! ordinary product renderer over an externally owned host frame. Keeping this
//! seam here prevents the native crate from copying a second projection or
//! pane/tab renderer.

use std::collections::BTreeSet;

use dockspace::model::{ItemId, SurfaceId};
use dockspace::runtime::{
    DockspaceHostFrame, DockspaceReceiverDescriptor, SurfaceUnavailableReason,
};
use egui::emath::GuiRounding;
use egui::{Id, Ui};

use crate::error::DockspaceError;
use crate::pane::PaneView;
use crate::product_render;
use crate::style::DockStyle;

/// Result of painting one surface against a core-owned host frame.
#[derive(Debug)]
pub struct NativeSurfacePaint {
    viewport_id: egui::ViewportId,
    cumulative_pass_nr: u64,
    had_ready_plan: bool,
    deferred_measurement: bool,
    missing_items: BTreeSet<ItemId>,
    receivers: Vec<NativePaintReceiver>,
}

/// One egui widget identity bound to an exact core receiver in the same pass.
#[derive(Debug, Clone, Copy)]
pub struct NativePaintReceiver {
    widget_id: egui::Id,
    layer_id: egui::LayerId,
    receiver: DockspaceReceiverDescriptor,
}

impl NativePaintReceiver {
    /// Returns the egui widget identity registered during painting.
    #[must_use]
    pub const fn widget_id(self) -> egui::Id {
        self.widget_id
    }

    /// Returns the exact egui layer containing the widget.
    #[must_use]
    pub const fn layer_id(self) -> egui::LayerId {
        self.layer_id
    }

    /// Returns the output-bound core receiver descriptor.
    #[must_use]
    pub const fn receiver(self) -> DockspaceReceiverDescriptor {
        self.receiver
    }
}

impl NativeSurfacePaint {
    /// Returns the viewport whose current pass produced these bindings.
    #[must_use]
    pub const fn viewport_id(&self) -> egui::ViewportId {
        self.viewport_id
    }

    /// Returns the completed-pass generation expected after this pass ends.
    #[must_use]
    pub const fn cumulative_pass_nr(&self) -> u64 {
        self.cumulative_pass_nr
    }

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

    /// Returns every egui widget bound to one exact core receiver in paint order.
    pub fn receivers(&self) -> impl ExactSizeIterator<Item = NativePaintReceiver> + '_ {
        self.receivers.iter().copied()
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
    let viewport_id = ui.ctx().viewport_id();
    let cumulative_pass_nr = ui
        .ctx()
        .cumulative_pass_nr_for(viewport_id)
        .saturating_add(1);
    let dock_rect = ui.available_rect_before_wrap();
    let Some(plan) = frame
        .paint_plan(surface)
        .map_err(DockspaceError::from_detail)?
    else {
        ui.allocate_rect(dock_rect, egui::Sense::hover());
        ui.painter()
            .rect_filled(dock_rect, 0.0, style.workspace_fill);
        return Ok(NativeSurfacePaint {
            viewport_id,
            cumulative_pass_nr,
            had_ready_plan: false,
            deferred_measurement: false,
            missing_items: BTreeSet::new(),
            receivers: Vec::new(),
        });
    };

    let painted = product_render::paint_surface(
        ui,
        instance_id,
        plan,
        panes,
        style,
        product_render::PointerActionAuthority::ExternalJournal,
    );
    for action in painted.actions {
        frame
            .submit_surface_action(action)
            .map_err(DockspaceError::from_detail)?;
    }
    Ok(NativeSurfacePaint {
        viewport_id,
        cumulative_pass_nr,
        had_ready_plan: true,
        deferred_measurement: painted.defer_measurement,
        missing_items: painted.missing_items,
        receivers: painted
            .receivers
            .into_iter()
            .map(|binding| NativePaintReceiver {
                widget_id: binding.widget_id,
                layer_id: binding.layer_id,
                receiver: binding.receiver,
            })
            .collect(),
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
