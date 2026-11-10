//! Small retained registry for deferred native viewport declarations.
//!
//! The registry is intentionally narrower than the core lifecycle saga: core
//! owns the binding, phase, and ownership transfer, while this adapter only
//! remembers which eframe viewport must be declared on the next root pass.

use dockspace::geometry::PhysicalRect;
use dockspace::runtime::{NativeSurfaceBinding, NativeSurfaceRole};
use eframe::egui::{self, ViewportBuilder, ViewportClass, ViewportId};

use crate::error::NativeHostProtocolError;
use crate::mailbox::DeferredViewportPaint;

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
        ViewportBuilder::default().with_visible(self.visible)
    }
}

#[derive(Debug, Default)]
pub(crate) struct DeferredViewportDriver {
    entries: Vec<DeferredViewportSpec>,
    entry_limit: Option<usize>,
}

impl DeferredViewportDriver {
    /// Reserves every finite deferred declaration before accepting a viewport.
    pub(crate) fn with_capacity_limit(entries: usize) -> Result<Self, NativeHostProtocolError> {
        Self::reservation_bytes(entries)
            .ok_or(NativeHostProtocolError::NativeReportStorageUnavailable)?;
        let mut backing = Vec::new();
        backing
            .try_reserve_exact(entries)
            .map_err(|_| NativeHostProtocolError::NativeReportStorageUnavailable)?;
        if backing.capacity() > entries {
            return Err(NativeHostProtocolError::NativeReportStorageUnavailable);
        }
        Ok(Self {
            entries: backing,
            entry_limit: Some(entries),
        })
    }

    /// Includes inline state and the selected backing, without allocator metadata.
    pub(crate) fn reservation_bytes(entries: usize) -> Option<usize> {
        size_of::<Self>().checked_add(entries.checked_mul(size_of::<DeferredViewportSpec>())?)
    }

    /// Counts inline state and actual capacity, including retired spare slots.
    pub(crate) fn owned_data_capacity_bytes(&self) -> usize {
        size_of::<Self>().saturating_add(
            self.entries
                .capacity()
                .saturating_mul(size_of::<DeferredViewportSpec>()),
        )
    }

    /// Checks exact identity and completes any backing allocation before a
    /// viewport effect or OS operation becomes irreversible. Repeated exact
    /// declarations remain admissible even when every finite slot is occupied.
    pub(crate) fn can_insert(
        &mut self,
        viewport: ViewportId,
        binding: NativeSurfaceBinding,
        placement: PhysicalRect,
        role: NativeSurfaceRole,
    ) -> bool {
        if let Ok(index) = self
            .entries
            .binary_search_by_key(&viewport, |spec| spec.viewport())
        {
            return self.entries.get(index).is_some_and(|existing| {
                existing.binding() == binding
                    && existing.placement() == placement
                    && existing.role() == role
            });
        }
        if self
            .entries
            .iter()
            .any(|existing| existing.binding() == binding)
            || self
                .entry_limit
                .is_some_and(|limit| self.entries.len() >= limit)
        {
            return false;
        }
        if self.entries.len() < self.entries.capacity() {
            return true;
        }
        self.entry_limit.is_none() && self.entries.try_reserve(1).is_ok()
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
        let index = match self
            .entries
            .binary_search_by_key(&viewport, |spec| spec.viewport())
        {
            Ok(_) => return true,
            Err(index) => index,
        };
        self.entries.insert(
            index,
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
        let spec = self
            .entries
            .iter_mut()
            .find(|spec| spec.binding() == binding)?;
        spec.set_visible(true);
        Some(spec.viewport())
    }

    pub(crate) fn remove(&mut self, binding: NativeSurfaceBinding) -> Option<ViewportId> {
        let index = self
            .entries
            .iter()
            .position(|spec| spec.binding() == binding)?;
        Some(self.entries.remove(index).viewport())
    }

    pub(crate) fn viewport_for(&self, binding: NativeSurfaceBinding) -> Option<ViewportId> {
        self.entries
            .iter()
            .find_map(|spec| (spec.binding() == binding).then_some(spec.viewport()))
    }

    /// Borrows declarations in viewport order without constructing a snapshot.
    pub(crate) fn specs(&self) -> impl Iterator<Item = DeferredViewportSpec> + '_ {
        self.entries.iter().copied()
    }

    #[cfg(test)]
    pub(crate) fn retained_specs(&self) -> Vec<DeferredViewportSpec> {
        self.specs().collect()
    }

    pub(crate) fn has_transitional_viewport(&self) -> bool {
        self.entries.iter().any(|spec| !spec.visible)
    }
}

pub(crate) fn viewport_id_for(binding: NativeSurfaceBinding) -> ViewportId {
    ViewportId::from_hash_of(("dockspace-deferred-surface", binding))
}

pub(crate) fn declare_deferred_viewports(
    context: &egui::Context,
    specs: impl IntoIterator<Item = DeferredViewportSpec>,
    title: impl Fn(DeferredViewportSpec) -> String,
    callback: impl Fn(DeferredViewportSpec, &mut egui::Ui, ViewportClass)
    + Clone
    + Send
    + Sync
    + 'static,
) {
    for spec in specs {
        let callback = callback.clone();
        context.show_viewport_deferred(
            spec.viewport(),
            spec.builder().with_title(title(spec)),
            move |ui, class| {
                callback(spec, ui, class);
            },
        );
    }
}

pub(crate) fn paint_placeholder(
    ui: &mut egui::Ui,
    class: ViewportClass,
    disposition: Option<DeferredViewportPaint>,
) {
    ui.painter()
        .rect_filled(ui.max_rect(), 0.0, egui::Color32::from_gray(24));
    ui.disable();
    if class != ViewportClass::Deferred {
        ui.centered_and_justified(|ui| {
            ui.label("Native dockspace viewports require an eframe multi-window backend");
        });
        return;
    }
    let label = match disposition {
        Some(DeferredViewportPaint::Created) => "Creating dockspace window…",
        Some(DeferredViewportPaint::Staging(request)) => match request.phase() {
            crate::mailbox::NativePreparationPaintPhase::PreShow => "Preparing dockspace window…",
            crate::mailbox::NativePreparationPaintPhase::PostShow => "Activating dockspace window…",
        },
        Some(DeferredViewportPaint::Semantic(_)) => "Waiting for semantic dockspace rendering…",
        Some(DeferredViewportPaint::Retain(_)) => "Retaining the last dockspace frame…",
        Some(DeferredViewportPaint::Waiting) | None => "Waiting for dockspace authority…",
    };
    ui.centered_and_justified(|ui| {
        ui.label(label);
    });
    if matches!(
        disposition,
        Some(DeferredViewportPaint::Created | DeferredViewportPaint::Staging(_))
    ) {
        ui.ctx().request_repaint_once_of(ViewportId::ROOT);
    }
}

#[cfg(test)]
mod capacity_tests;
