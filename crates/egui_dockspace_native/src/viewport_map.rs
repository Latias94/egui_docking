//! Minimal product mapping between native windows, eframe viewports, and core bindings.

use std::collections::BTreeMap;

use dockspace::model::SurfaceId;
use dockspace::runtime::NativeSurfaceBinding;
use eframe::egui::ViewportId;
use winit::window::WindowId;

use crate::NativeViewportBindingError;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct NativeViewportRoute {
    window: WindowId,
    binding: NativeSurfaceBinding,
}

#[derive(Debug, Default)]
pub(crate) struct NativeViewportMap {
    viewports: BTreeMap<ViewportId, NativeViewportRoute>,
    windows: BTreeMap<WindowId, ViewportId>,
    surfaces: BTreeMap<SurfaceId, ViewportId>,
}

impl NativeViewportMap {
    pub(crate) fn bind(
        &mut self,
        viewport: ViewportId,
        window: WindowId,
        binding: NativeSurfaceBinding,
    ) -> Result<(), NativeViewportBindingError> {
        if let Some(existing) = self.viewports.get(&viewport).copied() {
            if existing.window == window && existing.binding == binding {
                return Ok(());
            }
            return Err(NativeViewportBindingError::ViewportAlreadyBound {
                viewport,
                existing: existing.binding.surface(),
            });
        }
        self.validate_unclaimed_peers(viewport, window, binding)?;
        self.insert(viewport, window, binding);
        Ok(())
    }

    pub(crate) fn replace(
        &mut self,
        viewport: ViewportId,
        expected: NativeSurfaceBinding,
        successor_window: WindowId,
        successor: NativeSurfaceBinding,
    ) -> Result<(), NativeViewportBindingError> {
        let Some(current) = self.viewports.get(&viewport).copied() else {
            return Err(NativeViewportBindingError::ViewportUnbound { viewport });
        };
        if current.binding != expected {
            return Err(NativeViewportBindingError::BindingMismatch {
                viewport,
                expected: expected.surface(),
                current: current.binding.surface(),
            });
        }
        if successor.surface() != expected.surface() {
            return Err(NativeViewportBindingError::ReplacementSurfaceMismatch {
                viewport,
                expected: expected.surface(),
                successor: successor.surface(),
            });
        }
        if let Some(existing) = self.windows.get(&successor_window).copied()
            && existing != viewport
        {
            return Err(NativeViewportBindingError::WindowAlreadyBound {
                window: successor_window,
                existing,
            });
        }
        debug_assert_eq!(
            self.surfaces.get(&successor.surface()).copied(),
            Some(viewport),
            "viewport and surface indexes must remain symmetric",
        );

        self.windows.remove(&current.window);
        self.insert(viewport, successor_window, successor);
        Ok(())
    }

    pub(crate) fn remove_viewport(
        &mut self,
        viewport: ViewportId,
        expected: NativeSurfaceBinding,
    ) -> Result<NativeSurfaceBinding, NativeViewportBindingError> {
        let Some(current) = self.viewports.get(&viewport).copied() else {
            return Err(NativeViewportBindingError::ViewportUnbound { viewport });
        };
        if current.binding != expected {
            return Err(NativeViewportBindingError::BindingMismatch {
                viewport,
                expected: expected.surface(),
                current: current.binding.surface(),
            });
        }
        self.viewports.remove(&viewport);
        self.windows.remove(&current.window);
        self.surfaces.remove(&current.binding.surface());
        Ok(current.binding)
    }

    pub(crate) fn binding(&self, viewport: ViewportId) -> Option<NativeSurfaceBinding> {
        self.viewports.get(&viewport).map(|route| route.binding)
    }

    pub(crate) fn binding_for_event(
        &self,
        window: WindowId,
        viewport: Option<ViewportId>,
    ) -> Option<NativeSurfaceBinding> {
        let mapped_viewport = self.windows.get(&window).copied()?;
        if viewport.is_some_and(|viewport| viewport != mapped_viewport) {
            return None;
        }
        self.binding(mapped_viewport)
    }

    pub(crate) fn viewport(&self, surface: SurfaceId) -> Option<ViewportId> {
        self.surfaces.get(&surface).copied()
    }

    fn validate_unclaimed_peers(
        &self,
        viewport: ViewportId,
        window: WindowId,
        binding: NativeSurfaceBinding,
    ) -> Result<(), NativeViewportBindingError> {
        if let Some(existing) = self.windows.get(&window).copied()
            && existing != viewport
        {
            return Err(NativeViewportBindingError::WindowAlreadyBound { window, existing });
        }
        if let Some(existing) = self.surfaces.get(&binding.surface()).copied()
            && existing != viewport
        {
            return Err(NativeViewportBindingError::SurfaceAlreadyBound {
                surface: binding.surface(),
                existing,
            });
        }
        Ok(())
    }

    fn insert(&mut self, viewport: ViewportId, window: WindowId, binding: NativeSurfaceBinding) {
        self.viewports
            .insert(viewport, NativeViewportRoute { window, binding });
        self.windows.insert(window, viewport);
        self.surfaces.insert(binding.surface(), viewport);
    }
}
