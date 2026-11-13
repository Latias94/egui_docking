//! Single-use touch requests for the existing root presentation menu.

use dockspace::model::{RootId, SurfaceId};
use dockspace::runtime::DockspaceContextMenuRequest;
use egui::{Id, Ui, ViewportId};

#[derive(Clone, Copy)]
struct StagedTouchContextMenu {
    viewport: ViewportId,
    pass: u64,
    request: DockspaceContextMenuRequest,
}

/// Stages at most one request per instance for this exact paint pass.
///
/// The caller must check the request against the current workspace version
/// before staging it. An unmatched request expires after this pass.
pub(crate) fn stage_touch_context_menu(
    ui: &Ui,
    instance: Id,
    request: DockspaceContextMenuRequest,
) {
    let staged = StagedTouchContextMenu {
        viewport: ui.ctx().viewport_id(),
        pass: ui.ctx().cumulative_pass_nr(),
        request,
    };
    ui.ctx().data_mut(|data| {
        data.insert_temp(instance.with("touch-context-menu"), staged);
    });
}

pub(crate) fn take_touch_context_menu(
    ui: &Ui,
    instance: Id,
    surface: SurfaceId,
    root: RootId,
) -> Option<DockspaceContextMenuRequest> {
    let viewport = ui.ctx().viewport_id();
    let pass = ui.ctx().cumulative_pass_nr();
    ui.ctx().data_mut(|data| {
        let id = instance.with("touch-context-menu");
        let staged = data.get_temp::<StagedTouchContextMenu>(id)?;
        if staged.viewport != viewport {
            return None;
        }
        if staged.pass != pass {
            data.remove::<StagedTouchContextMenu>(id);
            return None;
        }
        if staged.request.surface() != surface || staged.request.root() != root {
            return None;
        }
        data.remove::<StagedTouchContextMenu>(id);
        Some(staged.request)
    })
}
