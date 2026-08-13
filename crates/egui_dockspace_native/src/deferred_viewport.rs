//! Small retained registry for deferred native viewport declarations.
//!
//! The registry is intentionally narrower than the core lifecycle saga: core
//! owns the binding, phase, and ownership transfer, while this adapter only
//! remembers which eframe viewport must be declared on the next root pass.

use std::collections::BTreeMap;
use std::sync::Arc;

use dockspace::geometry::PhysicalRect;
use dockspace::runtime::{NativeSurfaceBinding, NativeSurfaceRole};
use eframe::egui::{self, ViewportBuilder, ViewportClass, ViewportId};

use crate::mailbox::{DeferredViewportPaint, NativeHostBridge};

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct DeferredViewportSpec {
    viewport: ViewportId,
    binding: NativeSurfaceBinding,
    placement: PhysicalRect,
    role: NativeSurfaceRole,
    visible: bool,
}

impl DeferredViewportSpec {
    pub(crate) const fn viewport(self) -> ViewportId {
        self.viewport
    }

    pub(crate) const fn binding(self) -> NativeSurfaceBinding {
        self.binding
    }

    pub(crate) const fn placement(self) -> PhysicalRect {
        self.placement
    }

    pub(crate) const fn role(self) -> NativeSurfaceRole {
        self.role
    }

    pub(crate) fn set_visible(&mut self, visible: bool) {
        self.visible = visible;
    }

    pub(crate) fn builder(self) -> ViewportBuilder {
        ViewportBuilder::default()
            .with_visible(self.visible)
            .with_title(match self.role {
                NativeSurfaceRole::Root => "Dockspace",
                NativeSurfaceRole::Child => "Dockspace panel",
            })
    }
}

#[derive(Debug, Default)]
pub(crate) struct DeferredViewportDriver {
    entries: BTreeMap<ViewportId, DeferredViewportSpec>,
}

impl DeferredViewportDriver {
    pub(crate) fn can_insert(
        &self,
        viewport: ViewportId,
        binding: NativeSurfaceBinding,
        placement: PhysicalRect,
        role: NativeSurfaceRole,
    ) -> bool {
        match self.entries.get(&viewport).copied() {
            Some(existing) => {
                existing.binding() == binding
                    && existing.placement() == placement
                    && existing.role() == role
            }
            None => !self
                .entries
                .values()
                .any(|existing| existing.binding() == binding),
        }
    }

    pub(crate) fn insert(
        &mut self,
        viewport: ViewportId,
        binding: NativeSurfaceBinding,
        placement: PhysicalRect,
        role: NativeSurfaceRole,
    ) -> bool {
        if !self.can_insert(viewport, binding, placement, role) {
            return false;
        }
        if self.entries.contains_key(&viewport) {
            return true;
        }
        self.entries.insert(
            viewport,
            DeferredViewportSpec {
                viewport,
                binding,
                placement,
                role,
                visible: false,
            },
        );
        true
    }

    pub(crate) fn set_visible(&mut self, binding: NativeSurfaceBinding) -> Option<ViewportId> {
        let (viewport, spec) = self
            .entries
            .iter_mut()
            .find(|(_, spec)| spec.binding() == binding)?;
        spec.set_visible(true);
        Some(*viewport)
    }

    pub(crate) fn remove(&mut self, binding: NativeSurfaceBinding) -> Option<ViewportId> {
        let viewport = self
            .entries
            .iter()
            .find_map(|(viewport, spec)| (spec.binding() == binding).then_some(*viewport))?;
        self.entries.remove(&viewport);
        Some(viewport)
    }

    pub(crate) fn viewport_for(&self, binding: NativeSurfaceBinding) -> Option<ViewportId> {
        self.entries
            .iter()
            .find_map(|(viewport, spec)| (spec.binding() == binding).then_some(*viewport))
    }

    pub(crate) fn specs(&self) -> impl Iterator<Item = DeferredViewportSpec> + '_ {
        self.entries.values().copied()
    }

    pub(crate) fn declare(&self, context: &egui::Context, bridge: &Arc<NativeHostBridge>) {
        for spec in self.specs() {
            let bridge = Arc::clone(bridge);
            context.show_viewport_deferred(spec.viewport(), spec.builder(), move |ui, class| {
                paint_placeholder(ui, class, &bridge);
            });
        }
    }
}

pub(crate) fn viewport_id_for(binding: NativeSurfaceBinding) -> ViewportId {
    ViewportId::from_hash_of(("dockspace-deferred-surface", binding))
}

fn paint_placeholder(ui: &mut egui::Ui, class: ViewportClass, bridge: &NativeHostBridge) {
    ui.painter()
        .rect_filled(ui.max_rect(), 0.0, egui::Color32::from_gray(24));
    let token = eframe::current_native_output_token();
    if class != ViewportClass::Deferred {
        if let Some(token) = token {
            bridge.abandon_output(token);
        }
        ui.centered_and_justified(|ui| {
            ui.label("Native dockspace viewports require an eframe multi-window backend");
        });
        return;
    }
    let Some(token) = token else {
        ui.centered_and_justified(|ui| {
            ui.label("Waiting for native output authority…");
        });
        return;
    };
    let disposition = bridge.record_deferred_viewport_paint(token);
    let label = match disposition {
        DeferredViewportPaint::Created => "Creating dockspace window…",
        DeferredViewportPaint::Staging(request) => match request.phase() {
            dockspace::runtime::NativeStagingPresentationPhase::PreShow => {
                "Preparing dockspace window…"
            }
            dockspace::runtime::NativeStagingPresentationPhase::PostShow => {
                "Activating dockspace window…"
            }
        },
        DeferredViewportPaint::Waiting => "Waiting for dockspace authority…",
    };
    ui.centered_and_justified(|ui| {
        ui.label(label);
    });
    if !matches!(disposition, DeferredViewportPaint::Waiting) {
        ui.ctx().request_repaint_of(ViewportId::ROOT);
    }
}
