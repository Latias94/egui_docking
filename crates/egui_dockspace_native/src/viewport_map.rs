//! Minimal product mapping between native windows, eframe viewports, and core bindings.

use std::collections::{BTreeMap, BTreeSet};

use dockspace::model::SurfaceId;
use dockspace::runtime::NativeSurfaceBinding;
use eframe::{NativeViewportCreateAttempt, egui::ViewportId};
use winit::window::WindowId;

use crate::error::NativeViewportBindingError;
use crate::retirement::CommittedRetirement;

mod create_receipt;
pub(crate) use create_receipt::{CreateAdmission, CreateTerminal, NativeCreateReceipt};
mod window_receipt;
pub(crate) use window_receipt::NativeWindowReceipt;

#[derive(Debug)]
struct NativeViewportRoute {
    window: Option<WindowId>,
    binding: NativeSurfaceBinding,
    create: Option<NativeCreateReceipt>,
    terminal: Option<NativeWindowReceipt>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct RouteSnapshot {
    window: Option<WindowId>,
    binding: NativeSurfaceBinding,
    create_attempt: Option<NativeViewportCreateAttempt>,
}

impl NativeViewportRoute {
    fn snapshot(&self) -> RouteSnapshot {
        RouteSnapshot {
            window: self.window,
            binding: self.binding,
            create_attempt: self
                .create
                .as_ref()
                .and_then(NativeCreateReceipt::attempt)
                .or_else(|| {
                    self.terminal
                        .as_ref()
                        .and_then(NativeWindowReceipt::attempt)
                }),
        }
    }
}

#[derive(Debug, Default)]
pub(crate) struct NativeViewportMap {
    viewports: BTreeMap<ViewportId, NativeViewportRoute>,
    windows: BTreeMap<WindowId, ViewportId>,
    surfaces: BTreeMap<SurfaceId, ViewportId>,
    focus_reportable: BTreeSet<NativeSurfaceBinding>,
    pointer_suppressed: BTreeSet<NativeSurfaceBinding>,
    prepared_retirements: BTreeSet<NativeSurfaceBinding>,
}

impl NativeViewportMap {
    pub(crate) fn reserve(
        &mut self,
        viewport: ViewportId,
        binding: NativeSurfaceBinding,
    ) -> Result<(), NativeViewportBindingError> {
        if let Some(existing) = self
            .viewports
            .get(&viewport)
            .map(NativeViewportRoute::snapshot)
        {
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
        let Some(current) = self
            .viewports
            .get(&viewport)
            .map(NativeViewportRoute::snapshot)
        else {
            return Err(NativeViewportBindingError::ViewportUnbound { viewport });
        };
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
        self.validate_not_retiring(viewport, current.binding)?;
        self.validate_window_unclaimed(viewport, window)?;
        let route = self
            .viewports
            .get_mut(&viewport)
            .expect("validated viewport route remains present");
        route.window = Some(window);
        route.terminal = Some(NativeWindowReceipt::attached(
            viewport,
            window,
            expected,
            current.create_attempt,
        ));
        self.windows.insert(window, viewport);
        Ok(())
    }

    pub(crate) fn attach_output(
        &mut self,
        viewport: ViewportId,
        expected: NativeSurfaceBinding,
        window: WindowId,
        create_attempt: Option<NativeViewportCreateAttempt>,
    ) -> Result<(), NativeViewportBindingError> {
        let Some(current) = self
            .viewports
            .get(&viewport)
            .map(NativeViewportRoute::snapshot)
        else {
            return Err(NativeViewportBindingError::ViewportUnbound { viewport });
        };
        if current.create_attempt != create_attempt {
            return Err(NativeViewportBindingError::BindingNotCurrent {
                viewport,
                surface: expected.surface(),
            });
        }
        self.attach(viewport, expected, window)?;
        let route = self
            .viewports
            .get(&viewport)
            .expect("an attached output retains its exact viewport route");
        debug_assert_eq!(route.binding, expected);
        debug_assert_eq!(route.window, Some(window));
        Ok(())
    }

    pub(crate) fn bind(
        &mut self,
        viewport: ViewportId,
        window: WindowId,
        binding: NativeSurfaceBinding,
    ) -> Result<(), NativeViewportBindingError> {
        if let Some(existing) = self
            .viewports
            .get(&viewport)
            .map(NativeViewportRoute::snapshot)
        {
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
    ) -> Result<Option<NativeWindowReceipt>, NativeViewportBindingError> {
        let current = self.validate_replacement(viewport, expected, successor)?;
        if current.binding == successor {
            return Ok(None);
        }
        let terminal = self.take_window_receipt(viewport, expected);
        if let Some(window) = current.window {
            self.windows.remove(&window);
        }
        self.pointer_suppressed.remove(&expected);
        self.insert(viewport, None, successor);
        Ok(terminal)
    }

    pub(crate) fn reserve_retired_replacement(
        &mut self,
        viewport: ViewportId,
        expected: NativeSurfaceBinding,
        successor: NativeSurfaceBinding,
    ) -> Result<Option<NativeWindowReceipt>, NativeViewportBindingError> {
        let Some(current) = self
            .viewports
            .get(&viewport)
            .map(NativeViewportRoute::snapshot)
        else {
            return Err(NativeViewportBindingError::ViewportUnbound { viewport });
        };
        Self::validate_binding(viewport, expected, current)?;
        self.validate_create_settled(viewport, expected)?;
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
        let terminal = self.take_window_receipt(viewport, expected);
        self.pointer_suppressed.remove(&expected);
        self.insert(viewport, None, successor);
        Ok(terminal)
    }

    pub(crate) fn replace(
        &mut self,
        viewport: ViewportId,
        expected: NativeSurfaceBinding,
        successor_window: WindowId,
        successor: NativeSurfaceBinding,
    ) -> Result<Option<NativeWindowReceipt>, NativeViewportBindingError> {
        let same_window = self.window(viewport) == Some(successor_window);
        let current = if same_window {
            let current = self
                .viewports
                .get(&viewport)
                .map(NativeViewportRoute::snapshot)
                .ok_or(NativeViewportBindingError::ViewportUnbound { viewport })?;
            Self::validate_binding(viewport, expected, current)?;
            self.validate_create_settled(viewport, expected)?;
            if successor.surface() != expected.surface()
                || self.prepared_retirements.contains(&expected)
                || !self.viewports.get(&viewport).is_some_and(|route| {
                    route
                        .terminal
                        .as_ref()
                        .is_some_and(NativeWindowReceipt::is_awaiting)
                })
            {
                return Err(NativeViewportBindingError::BindingNotCurrent {
                    viewport,
                    surface: expected.surface(),
                });
            }
            current
        } else {
            self.validate_replacement(viewport, expected, successor)?
        };
        self.validate_window_unclaimed(viewport, successor_window)?;
        if current.window == Some(successor_window) {
            let route = self
                .viewports
                .get_mut(&viewport)
                .expect("validated replacement retains its route");
            route.binding = successor;
            if let Some(receipt) = route.terminal.as_mut() {
                receipt.rebind(successor);
            }
            self.focus_reportable.remove(&expected);
            if self.pointer_suppressed.remove(&expected) {
                self.pointer_suppressed.insert(successor);
            }
            return Ok(None);
        }
        if expected == successor {
            return Err(NativeViewportBindingError::BindingNotCurrent {
                viewport,
                surface: expected.surface(),
            });
        }
        let terminal = self.take_window_receipt(viewport, expected);
        if let Some(window) = current.window {
            self.windows.remove(&window);
        }
        self.insert(viewport, Some(successor_window), successor);
        Ok(terminal)
    }

    pub(crate) fn remove_viewport(
        &mut self,
        viewport: ViewportId,
        expected: NativeSurfaceBinding,
    ) -> Result<(NativeSurfaceBinding, Option<NativeWindowReceipt>), NativeViewportBindingError>
    {
        let Some(current) = self
            .viewports
            .get(&viewport)
            .map(NativeViewportRoute::snapshot)
        else {
            return Err(NativeViewportBindingError::ViewportUnbound { viewport });
        };
        if self.prepared_retirements.contains(&current.binding) {
            return Err(NativeViewportBindingError::BindingNotCurrent {
                viewport,
                surface: current.binding.surface(),
            });
        }
        self.validate_create_settled(viewport, current.binding)?;
        if current.binding != expected {
            return Err(NativeViewportBindingError::BindingMismatch {
                viewport,
                expected: expected.surface(),
                current: current.binding.surface(),
            });
        }
        let receipt = self.take_window_receipt(viewport, expected);
        self.remove_route_unchecked(viewport, current);
        Ok((current.binding, receipt))
    }

    pub(crate) fn prepare_retirements(
        &mut self,
        retirements: &[CommittedRetirement],
    ) -> Result<(), NativeSurfaceBinding> {
        for retirement in retirements {
            let viewport = retirement.viewport();
            let binding = retirement.binding();
            let current = self
                .viewports
                .get(&viewport)
                .filter(|route| route.binding == binding)
                .map(NativeViewportRoute::snapshot);
            // A successor route is independent of the predecessor's retirement.
            if current.is_some() && self.validate_create_settled(viewport, binding).is_err()
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
                .filter(|route| route.binding == binding)
                .map(NativeViewportRoute::snapshot);
            if let Some(current) = current {
                self.remove_route_unchecked(viewport, current);
            }
            self.pointer_suppressed.remove(&binding);
            self.prepared_retirements.remove(&binding);
        }
    }

    pub(crate) fn suppress_pointer_route(
        &mut self,
        viewport: ViewportId,
        binding: NativeSurfaceBinding,
    ) -> bool {
        let Some(current) = self
            .viewports
            .get(&viewport)
            .map(NativeViewportRoute::snapshot)
        else {
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

    pub(crate) fn window(&self, viewport: ViewportId) -> Option<WindowId> {
        self.viewports.get(&viewport).and_then(|route| route.window)
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

    /// Returns the exact binding whose last committed roster made focus reportable.
    pub(crate) fn focus_binding_for_event(
        &self,
        window: WindowId,
        viewport: Option<ViewportId>,
    ) -> Option<NativeSurfaceBinding> {
        let (_, binding) = self.route_for_event(window, viewport)?;
        (self.focus_reportable.contains(&binding)
            && !self.pointer_suppressed.contains(&binding)
            && !self.prepared_retirements.contains(&binding))
        .then_some(binding)
    }

    /// Replaces the exact focus-reportable roster after a platform snapshot commits.
    pub(crate) fn set_focus_reportable(
        &mut self,
        bindings: impl IntoIterator<Item = NativeSurfaceBinding>,
    ) {
        self.focus_reportable = bindings
            .into_iter()
            .filter(|binding| {
                self.surfaces
                    .get(&binding.surface())
                    .and_then(|viewport| self.viewports.get(viewport))
                    .is_some_and(|route| {
                        route.binding == *binding
                            && !self.pointer_suppressed.contains(binding)
                            && !self.prepared_retirements.contains(binding)
                    })
            })
            .collect();
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
    /// window exists but before the adapter has attached the callback window.
    /// That staged output is accepted only when eframe echoes the exact create
    /// attempt admitted for this route. A logical reservation without that
    /// physical incarnation remains unavailable.
    pub(crate) fn binding_for_output(
        &self,
        viewport: ViewportId,
        window: WindowId,
        create_attempt: Option<NativeViewportCreateAttempt>,
    ) -> Option<NativeSurfaceBinding> {
        let route = self.viewports.get(&viewport)?;
        if route.create.as_ref().and_then(NativeCreateReceipt::attempt) != create_attempt
            || route
                .create
                .as_ref()
                .is_some_and(|receipt| !receipt.permits_output(window))
        {
            return None;
        }
        if route.window.is_none() && create_attempt.is_none() {
            return None;
        }
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

    pub(crate) fn admit_create_attempt(
        &mut self,
        viewport: ViewportId,
        expected: NativeSurfaceBinding,
        attempt: NativeViewportCreateAttempt,
    ) -> bool {
        let Some(route) = self.viewports.get_mut(&viewport) else {
            return false;
        };
        if attempt.viewport_id() != viewport
            || route.binding != expected
            || route.window.is_some()
            || self.pointer_suppressed.contains(&expected)
            || self.prepared_retirements.contains(&expected)
            || route
                .create
                .as_ref()
                .is_some_and(|current| current.attempt() != Some(attempt) || !current.is_awaiting())
        {
            return false;
        }
        route
            .create
            .get_or_insert_with(|| NativeCreateReceipt::new(Some(attempt)));
        true
    }

    pub(crate) fn binding_for_create_attempt(
        &self,
        attempt: NativeViewportCreateAttempt,
    ) -> Option<NativeSurfaceBinding> {
        let route = self.viewports.get(&attempt.viewport_id())?;
        (route.window.is_none()
            && route.create.as_ref().and_then(NativeCreateReceipt::attempt) == Some(attempt)
            && !self.pointer_suppressed.contains(&route.binding)
            && !self.prepared_retirements.contains(&route.binding))
        .then_some(route.binding)
    }

    pub(crate) fn has_admitted_create_attempt(
        &self,
        viewport: ViewportId,
        binding: NativeSurfaceBinding,
    ) -> bool {
        self.viewports.get(&viewport).is_some_and(|route| {
            route.binding == binding
                && route.window.is_none()
                && route.create.is_some()
                && !self.pointer_suppressed.contains(&binding)
                && !self.prepared_retirements.contains(&binding)
        })
    }

    pub(crate) fn viewport(&self, surface: SurfaceId) -> Option<ViewportId> {
        self.surfaces.get(&surface).copied()
    }

    pub(crate) fn receive_create_terminal(
        &mut self,
        viewport: ViewportId,
        binding: NativeSurfaceBinding,
        attempt: Option<NativeViewportCreateAttempt>,
        terminal: CreateTerminal,
    ) -> CreateAdmission {
        let Some(route) = self.viewports.get_mut(&viewport) else {
            return CreateAdmission::Conflict;
        };
        if route.binding != binding {
            return CreateAdmission::Conflict;
        }
        route
            .create
            .as_mut()
            .map_or(CreateAdmission::Conflict, |receipt| {
                receipt.receive(attempt, terminal)
            })
    }

    pub(crate) fn transfer_create_terminal(
        &mut self,
        viewport: ViewportId,
        binding: NativeSurfaceBinding,
        attempt: Option<NativeViewportCreateAttempt>,
        terminal: CreateTerminal,
    ) -> bool {
        self.viewports
            .get_mut(&viewport)
            .filter(|route| route.binding == binding)
            .and_then(|route| route.create.as_mut())
            .is_some_and(|receipt| receipt.transfer(attempt, terminal))
    }

    /// Removes only the failed route whose already received receipt is transferred.
    pub(crate) fn take_failed_create(
        &mut self,
        viewport: ViewportId,
        binding: NativeSurfaceBinding,
    ) -> Option<NativeCreateReceipt> {
        let route = self.viewports.get(&viewport)?;
        if route.binding != binding
            || route.window.is_some()
            || !route
                .create
                .as_ref()
                .is_some_and(NativeCreateReceipt::is_transferred_failure)
        {
            return None;
        }
        let receipt = self.viewports.get_mut(&viewport)?.create.take()?;
        let snapshot = self.viewports.get(&viewport)?.snapshot();
        self.remove_route_unchecked(viewport, snapshot);
        Some(receipt)
    }

    pub(crate) fn commit_create_boundary(&mut self) {
        for route in self.viewports.values_mut() {
            if let Some(receipt) = route.create.as_mut() {
                receipt.commit_created_boundary();
            }
        }
    }

    pub(crate) fn has_pending_create_receipts(&self) -> bool {
        self.viewports.values().any(|route| {
            route
                .create
                .as_ref()
                .is_some_and(|receipt| !receipt.is_committed())
        })
    }

    #[cfg(test)]
    pub(crate) fn admit_test_create(
        &mut self,
        viewport: ViewportId,
        binding: NativeSurfaceBinding,
    ) -> bool {
        let Some(route) = self.viewports.get_mut(&viewport) else {
            return false;
        };
        if route.binding != binding || route.window.is_some() {
            return false;
        }
        route
            .create
            .get_or_insert_with(|| NativeCreateReceipt::new(None));
        true
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
        current: RouteSnapshot,
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
    ) -> Result<RouteSnapshot, NativeViewportBindingError> {
        let Some(current) = self
            .viewports
            .get(&viewport)
            .map(NativeViewportRoute::snapshot)
        else {
            return Err(NativeViewportBindingError::ViewportUnbound { viewport });
        };
        if self.prepared_retirements.contains(&current.binding) {
            return Err(NativeViewportBindingError::BindingNotCurrent {
                viewport,
                surface: current.binding.surface(),
            });
        }
        self.validate_create_settled(viewport, current.binding)?;
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

    fn validate_create_settled(
        &self,
        viewport: ViewportId,
        binding: NativeSurfaceBinding,
    ) -> Result<(), NativeViewportBindingError> {
        if self.viewports.get(&viewport).is_some_and(|route| {
            route
                .create
                .as_ref()
                .is_some_and(|receipt| !receipt.is_committed())
        }) {
            return Err(NativeViewportBindingError::BindingNotCurrent {
                viewport,
                surface: binding.surface(),
            });
        }
        Ok(())
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
        let previous = self.viewports.insert(
            viewport,
            NativeViewportRoute {
                window,
                binding,
                create: None,
                terminal: window
                    .map(|window| NativeWindowReceipt::attached(viewport, window, binding, None)),
            },
        );
        if let Some(previous) = previous {
            self.focus_reportable.remove(&previous.binding);
        }
        if let Some(window) = window {
            self.windows.insert(window, viewport);
        }
        self.surfaces.insert(binding.surface(), viewport);
    }

    fn remove_route_unchecked(&mut self, viewport: ViewportId, route: RouteSnapshot) {
        let removed = self.viewports.remove(&viewport);
        debug_assert_eq!(
            removed.as_ref().map(NativeViewportRoute::snapshot),
            Some(route)
        );
        if let Some(window) = route.window {
            self.windows.remove(&window);
        }
        self.surfaces.remove(&route.binding.surface());
        self.focus_reportable.remove(&route.binding);
    }

    pub(crate) fn take_window_receipt(
        &mut self,
        viewport: ViewportId,
        binding: NativeSurfaceBinding,
    ) -> Option<NativeWindowReceipt> {
        self.viewports
            .get_mut(&viewport)
            .filter(|route| route.binding == binding)?
            .terminal
            .take()
    }

    pub(crate) fn has_window_receipt(
        &self,
        viewport: ViewportId,
        binding: NativeSurfaceBinding,
    ) -> bool {
        self.viewports
            .get(&viewport)
            .is_some_and(|route| route.binding == binding && route.terminal.is_some())
    }

    pub(crate) fn take_window_receipt_for_event(
        &mut self,
        window: WindowId,
        viewport: Option<ViewportId>,
    ) -> Option<NativeWindowReceipt> {
        let (viewport, binding) = self.route_for_event(window, viewport)?;
        self.take_window_receipt(viewport, binding)
    }
}
