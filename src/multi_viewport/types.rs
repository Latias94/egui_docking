use std::collections::BTreeMap;

use egui::{Modifiers, Pos2, Vec2, ViewportBuilder, ViewportId};
use egui_tiles::{InsertionPoint, TileId, Tree};

use super::surface::DockSurface;
use super::host::WindowHost;

#[derive(Clone, Copy, Debug)]
pub(super) struct DockPayload {
    pub(super) bridge_id: egui::Id,
    pub(super) source_viewport: ViewportId,
    pub(super) source_floating: Option<u64>,
    pub(super) tile_id: Option<TileId>,
}

impl DockPayload {
    pub(super) fn source_host(&self) -> WindowHost {
        if let Some(floating) = self.source_floating {
            return WindowHost::Floating {
                viewport: self.source_viewport,
                floating,
            };
        }

        if self.tile_id.is_none() && self.source_viewport != ViewportId::ROOT {
            return WindowHost::NativeViewport {
                viewport: self.source_viewport,
            };
        }

        WindowHost::DockTree {
            viewport: self.source_viewport,
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub(super) struct ResolvedDropTarget {
    pub(super) target_surface: DockSurface,
    pub(super) target_host: WindowHost,
    pub(super) pointer_local: Pos2,
    pub(super) insertion: Option<InsertionPoint>,
}

#[derive(Clone, Copy, Debug)]
pub(super) struct PendingDrop {
    pub(super) payload: DockPayload,
    pub(super) pointer_global: Pos2,
    pub(super) modifiers: Modifiers,
}

#[derive(Clone, Copy, Debug)]
pub(super) struct ResolvedDrop {
    pub(super) payload: DockPayload,
    pub(super) pointer_global: Pos2,
    pub(super) target: ResolvedDropTarget,
}

#[derive(Clone, Copy, Debug)]
pub(super) struct PendingInternalDrop {
    pub(super) viewport: ViewportId,
    pub(super) tile_id: TileId,
    pub(super) insertion: InsertionPoint,
}

pub(super) type FloatingId = u64;

#[derive(Clone, Copy, Debug)]
pub(super) struct PendingLocalDrop {
    pub(super) payload: DockPayload,
    pub(super) target_surface: DockSurface,
    pub(super) target_host: WindowHost,
    pub(super) pointer_local: Pos2,
    pub(super) modifiers: Modifiers,
}

#[derive(Clone, Copy, Debug)]
pub(super) enum GhostDragMode {
    Contained {
        viewport: ViewportId,
        floating: FloatingId,
    },
    Native {
        viewport: ViewportId,
    },
}

#[derive(Clone, Copy, Debug)]
pub(super) struct GhostDrag {
    pub(super) mode: GhostDragMode,
    pub(super) grab_offset: Vec2,
}

#[derive(Debug)]
pub(super) struct DetachedDock<Pane> {
    #[allow(dead_code)]
    pub(super) serial: u64,
    pub(super) tree: Tree<Pane>,
    pub(super) builder: ViewportBuilder,
}

#[derive(Clone, Copy, Debug)]
pub(super) struct FloatingDragState {
    pub(super) pointer_start: Pos2,
    pub(super) offset_start: Vec2,
}

#[derive(Clone, Copy, Debug)]
pub(super) struct FloatingResizeState {
    pub(super) pointer_start: Pos2,
    pub(super) size_start: Vec2,
}

#[derive(Debug)]
pub(super) struct FloatingDockWindow<Pane> {
    pub(super) tree: Tree<Pane>,
    pub(super) offset_in_dock: Vec2,
    pub(super) size: Vec2,
    pub(super) collapsed: bool,
    pub(super) drag: Option<FloatingDragState>,
    pub(super) resize: Option<FloatingResizeState>,
}

#[derive(Debug)]
pub(super) struct FloatingManager<Pane> {
    pub(super) windows: BTreeMap<FloatingId, FloatingDockWindow<Pane>>,
    pub(super) z_order: Vec<FloatingId>,
}

impl<Pane> Default for FloatingManager<Pane> {
    fn default() -> Self {
        Self {
            windows: BTreeMap::new(),
            z_order: Vec::new(),
        }
    }
}

impl<Pane> FloatingManager<Pane> {
    pub(super) fn bring_to_front(&mut self, id: FloatingId) {
        self.z_order.retain(|&x| x != id);
        self.z_order.push(id);
    }
}
