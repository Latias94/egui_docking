//! Minimal product mapping between native windows, eframe viewports, and core bindings.

use std::collections::{BTreeMap, BTreeSet};

use dockspace::model::SurfaceId;
use dockspace::runtime::NativeSurfaceBinding;
use eframe::egui::ViewportId;
use winit::window::WindowId;

use crate::retirement::CommittedRetirement;
use crate::NativeViewportBindingError;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct NativeViewportRoute {
    window: Option<WindowId>,
    binding: NativeSurfaceBinding,
}

#[derive(Debug, Default)]
pub(crate) struct NativeViewportMap {
    viewports: BTreeMap<ViewportId, NativeViewportRoute>,
    windows: BTreeMap<WindowId, ViewportId>,
    surfaces: BTreeMap<SurfaceId, ViewportId>,
    pointer_suppressed: BTreeSet<NativeSurfaceBinding>,
    prepared_retirements: BTreeSet<NativeSurfaceBinding>,
}

impl NativeViewportMap {
    pub(crate) fn reserve(
        &mut self,
        viewport: ViewportId,
        binding: NativeSurfaceBinding,
    ) -> Result<(), NativeViewportBindingError> {
        if let Some(existing) = self.viewports.get(&viewport).copied() {
            self.validate_not_retiring(viewport, existing.binding)?;
            if existing.binding == binding {
                return Ok(());
            }
            return Err(NativeViewportBindingError::ViewportAlreadyBound {
                viewport,
                existing: existing.binding.surface(),
            });
        }
        self.validate_surface_unclaimed(viewport, binding)?;
        self.insert(viewport, None, binding);
        Ok(())
    }

    pub(crate) fn attach(
        &mut self,
        viewport: ViewportId,
        expected: NativeSurfaceBinding,
        window: WindowId,
    ) -> Result<(), NativeViewportBindingError> {
        let Some(current) = self.viewports.get(&viewport).copied() else {
            return Err(NativeViewportBindingError::ViewportUnbound { viewport });
        };
        self.validate_not_retiring(viewport, current.binding)?;
        Self::validate_binding(viewport, expected, current)?;
        if let Some(existing) = current.window {
            if existing == window {
                return Ok(());
            }
            return Err(NativeViewportBindingError::ViewportWindowAlreadyAttached {
                viewport,
                existing,
            });
        }
        self.validate_window_unclaimed(viewport, window)?;
        self.viewports.insert(
            viewport,
            NativeViewportRoute {
                window: Some(window),
                binding: current.binding,
            },
        );
        self.windows.insert(window, viewport);
        Ok(())
    }

    pub(crate) fn bind(
        &mut self,
        viewport: ViewportId,
        window: WindowId,
        binding: NativeSurfaceBinding,
    ) -> Result<(), NativeViewportBindingError> {
        if let Some(existing) = self.viewports.get(&viewport).copied() {
            self.validate_not_retiring(viewport, existing.binding)?;
            if existing.window == Some(window) && existing.binding == binding {
                return Ok(());
            }
            return Err(NativeViewportBindingError::ViewportAlreadyBound {
                viewport,
                existing: existing.binding.surface(),
            });
        }
        self.validate_unclaimed_peers(viewport, window, binding)?;
        self.insert(viewport, Some(window), binding);
        Ok(())
    }

    pub(crate) fn reserve_replacement(
        &mut self,
        viewport: ViewportId,
        expected: NativeSurfaceBinding,
        successor: NativeSurfaceBinding,
    ) -> Result<(), NativeViewportBindingError> {
        let current = self.validate_replacement(viewport, expected, successor)?;
        if current.binding == successor {
            return Ok(());
        }
        if let Some(window) = current.window {
            self.windows.remove(&window);
        }
        self.insert(viewport, None, successor);
        Ok(())
    }

    pub(crate) fn reserve_retired_replacement(
        &mut self,
        viewport: ViewportId,
        expected: NativeSurfaceBinding,
        successor: NativeSurfaceBinding,
    ) -> Result<(), NativeViewportBindingError> {
        let Some(current) = self.viewports.get(&viewport).copied() else {
            return Err(NativeViewportBindingError::ViewportUnbound { viewport });
        };
        Self::validate_binding(viewport, expected, current)?;
        if !self.pointer_suppressed.contains(&expected)
            || self.prepared_retirements.contains(&expected)
        {
            return Err(NativeViewportBindingError::BindingNotCurrent {
                viewport,
                surface: expected.surface(),
            });
        }
        if successor.surface() != expected.surface() {
            return Err(NativeViewportBindingError::ReplacementSurfaceMismatch {
                viewport,
                expected: expected.surface(),
                successor: successor.surface(),
            });
        }
        if let Some(window) = current.window {
            self.windows.remove(&window);
        }
        self.pointer_suppressed.remove(&expected);
        self.insert(viewport, None, successor);
        Ok(())
    }

    pub(crate) fn replace(
        &mut self,
        viewport: ViewportId,
        expected: NativeSurfaceBinding,
        successor_window: WindowId,
        successor: NativeSurfaceBinding,
    ) -> Result<(), NativeViewportBindingError> {
        let current = self.validate_replacement(viewport, expected, successor)?;
        self.validate_window_unclaimed(viewport, successor_window)?;
        if let Some(window) = current.window {
            self.windows.remove(&window);
        }
        self.insert(viewport, Some(successor_window), successor);
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
        self.validate_not_retiring(viewport, current.binding)?;
        if current.binding != expected {
            return Err(NativeViewportBindingError::BindingMismatch {
                viewport,
                expected: expected.surface(),
                current: current.binding.surface(),
            });
        }
        self.remove_route_unchecked(viewport, current);
        Ok(current.binding)
    }

    pub(crate) fn prepare_retirements(
        &mut self,
        retirements: &[CommittedRetirement],
    ) -> Result<(), NativeSurfaceBinding> {
        for retirement in retirements {
            let viewport = retirement.viewport();
            let binding = retirement.binding();
            let Some(current) = self.viewports.get(&viewport).copied() else {
                return Err(binding);
            };
            if Self::validate_binding(viewport, binding, current).is_err()
                || self.prepared_retirements.contains(&binding)
            {
                return Err(binding);
            }
        }
        self.prepared_retirements
            .extend(retirements.iter().map(|retirement| retirement.binding()));
        Ok(())
    }

    pub(crate) fn abort_retirements(&mut self, retirements: &[CommittedRetirement]) {
        for retirement in retirements {
            self.prepared_retirements.remove(&retirement.binding());
        }
    }

    pub(crate) fn commit_retirements(&mut self, retirements: &[CommittedRetirement]) {
        for retirement in retirements {
            let viewport = retirement.viewport();
            let binding = retirement.binding();
            let current = self
                .viewports
                .get(&viewport)
                .copied()
                .expect("prepared retirement retains its exact viewport route");
            debug_assert_eq!(current.binding, binding);
            self.remove_route_unchecked(viewport, current);
            self.pointer_suppressed.remove(&binding);
            self.prepared_retirements.remove(&binding);
        }
    }

    pub(crate) fn suppress_pointer_route(
        &mut self,
        viewport: ViewportId,
        binding: NativeSurfaceBinding,
    ) -> bool {
        let Some(current) = self.viewports.get(&viewport).copied() else {
            return false;
        };
        if current.binding != binding {
            return false;
        }
        self.pointer_suppressed.insert(binding);
        true
    }

    pub(crate) fn binding(&self, viewport: ViewportId) -> Option<NativeSurfaceBinding> {
        self.viewports.get(&viewport).map(|route| route.binding)
    }

    pub(crate) fn pointer_binding_for_window(
        &self,
        window: WindowId,
    ) -> Option<NativeSurfaceBinding> {
        let binding = self
            .windows
            .get(&window)
            .and_then(|viewport| self.binding(*viewport))?;
        (!self.pointer_suppressed.contains(&binding)).then_some(binding)
    }

    pub(crate) fn pointer_binding_for_event(
        &self,
        window: WindowId,
        viewport: Option<ViewportId>,
    ) -> Option<NativeSurfaceBinding> {
        let (_, binding) = self.route_for_event(window, viewport)?;
        (!self.pointer_suppressed.contains(&binding)).then_some(binding)
    }

    pub(crate) fn binding_for_event(
        &self,
        window: WindowId,
        viewport: Option<ViewportId>,
    ) -> Option<NativeSurfaceBinding> {
        self.route_for_event(window, viewport)
            .map(|(_, binding)| binding)
    }

    pub(crate) fn route_for_event(
        &self,
        window: WindowId,
        viewport: Option<ViewportId>,
    ) -> Option<(ViewportId, NativeSurfaceBinding)> {
        let mapped_viewport = self.windows.get(&window).copied()?;
        if viewport.is_some_and(|viewport| viewport != mapped_viewport) {
            return None;
        }
        self.binding(mapped_viewport)
            .map(|binding| (mapped_viewport, binding))
    }

    /// Returns the exact binding of a window already attached to a roster entry.
    ///
    /// Unlike [`Self::binding_for_output`], this never accepts a staged route
    /// whose successor window has not completed its first output callback. A
    /// root roster can still contain the predecessor window during that gap;
    /// treating it as the successor would create an incarnation ABA.
    pub(crate) fn binding_for_roster(
        &self,
        viewport: ViewportId,
        window: WindowId,
    ) -> Option<NativeSurfaceBinding> {
        self.binding_for_event(window, Some(viewport))
    }

    /// Returns the exact binding which may own an output callback.
    ///
    /// A deferred viewport can receive its first output after the native
    /// window exists but before the adapter has attached the callback window
    /// to the reserved route. In that interval the viewport route is still
    /// authoritative, while the reverse window index is intentionally empty.
    /// This query accepts that one staged state without inferring ownership
    /// from a window rectangle or callback order.
    pub(crate) fn binding_for_output(
        &self,
        viewport: ViewportId,
        window: WindowId,
    ) -> Option<NativeSurfaceBinding> {
        let route = self.viewports.get(&viewport).copied()?;
        if route.window.is_some_and(|current| current != window) {
            return None;
        }
        if self
            .windows
            .get(&window)
            .is_some_and(|current| *current != viewport)
        {
            return None;
        }
        Some(route.binding)
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
        self.validate_window_unclaimed(viewport, window)?;
        self.validate_surface_unclaimed(viewport, binding)
    }

    fn validate_window_unclaimed(
        &self,
        viewport: ViewportId,
        window: WindowId,
    ) -> Result<(), NativeViewportBindingError> {
        if let Some(existing) = self.windows.get(&window).copied()
            && existing != viewport
        {
            return Err(NativeViewportBindingError::WindowAlreadyBound { window, existing });
        }
        Ok(())
    }

    fn validate_surface_unclaimed(
        &self,
        viewport: ViewportId,
        binding: NativeSurfaceBinding,
    ) -> Result<(), NativeViewportBindingError> {
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

    fn validate_binding(
        viewport: ViewportId,
        expected: NativeSurfaceBinding,
        current: NativeViewportRoute,
    ) -> Result<(), NativeViewportBindingError> {
        if current.binding != expected {
            return Err(NativeViewportBindingError::BindingMismatch {
                viewport,
                expected: expected.surface(),
                current: current.binding.surface(),
            });
        }
        Ok(())
    }

    fn validate_replacement(
        &self,
        viewport: ViewportId,
        expected: NativeSurfaceBinding,
        successor: NativeSurfaceBinding,
    ) -> Result<NativeViewportRoute, NativeViewportBindingError> {
        let Some(current) = self.viewports.get(&viewport).copied() else {
            return Err(NativeViewportBindingError::ViewportUnbound { viewport });
        };
        self.validate_not_retiring(viewport, current.binding)?;
        Self::validate_binding(viewport, expected, current)?;
        if successor.surface() != expected.surface() {
            return Err(NativeViewportBindingError::ReplacementSurfaceMismatch {
                viewport,
                expected: expected.surface(),
                successor: successor.surface(),
            });
        }
        debug_assert_eq!(
            self.surfaces.get(&successor.surface()).copied(),
            Some(viewport),
            "viewport and surface indexes must remain symmetric",
        );
        Ok(current)
    }

    fn validate_not_retiring(
        &self,
        viewport: ViewportId,
        binding: NativeSurfaceBinding,
    ) -> Result<(), NativeViewportBindingError> {
        if self.pointer_suppressed.contains(&binding)
            || self.prepared_retirements.contains(&binding)
        {
            return Err(NativeViewportBindingError::BindingNotCurrent {
                viewport,
                surface: binding.surface(),
            });
        }
        Ok(())
    }

    fn insert(
        &mut self,
        viewport: ViewportId,
        window: Option<WindowId>,
        binding: NativeSurfaceBinding,
    ) {
        self.viewports
            .insert(viewport, NativeViewportRoute { window, binding });
        if let Some(window) = window {
            self.windows.insert(window, viewport);
        }
        self.surfaces.insert(binding.surface(), viewport);
    }

    fn remove_route_unchecked(&mut self, viewport: ViewportId, route: NativeViewportRoute) {
        let removed = self.viewports.remove(&viewport);
        debug_assert_eq!(removed, Some(route));
        if let Some(window) = route.window {
            self.windows.remove(&window);
        }
        self.surfaces.remove(&route.binding.surface());
    }
}
