//! Minimal product mapping between native windows, eframe viewports, and core bindings.

use std::mem::size_of;

use dockspace::model::SurfaceId;
use dockspace::runtime::NativeSurfaceBinding;
use eframe::{NativeViewportCreateAttempt, egui::ViewportId};
use winit::window::WindowId;

use crate::error::{NativeHostProtocolError, NativeViewportBindingError};
use crate::mailbox::journal::ReservedHostRecord;
use crate::retirement::CommittedRetirement;

mod create_receipt;
pub(crate) use create_receipt::{CreateAdmission, CreateTerminal, NativeCreateReceipt};
mod window_receipt;
pub(crate) use window_receipt::NativeWindowReceipt;

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
enum SemanticWake {
    #[default]
    Idle,
    Blocked,
    Ready,
}

#[derive(Debug)]
struct NativeViewportRoute {
    viewport: ViewportId,
    window: Option<WindowId>,
    window_published: bool,
    binding: NativeSurfaceBinding,
    create: Option<NativeCreateReceipt>,
    terminal: Option<NativeWindowReceipt>,
    pointer_suppressed: bool,
    focus_reportable: bool,
    semantic_wake: SemanticWake,
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
    viewports: Vec<NativeViewportRoute>,
    prepared_retirements: Vec<NativeSurfaceBinding>,
    slot_limit: Option<usize>,
}

impl NativeViewportMap {
    /// Installs both finite backing stores before any route or retirement exists.
    pub(crate) fn with_capacity_limit(slots: usize) -> Result<Self, NativeHostProtocolError> {
        let error = NativeHostProtocolError::NativeReportStorageUnavailable;
        Self::reservation_bytes(slots).ok_or(error)?;
        let mut viewports = Vec::new();
        let mut prepared_retirements = Vec::new();
        viewports.try_reserve_exact(slots).map_err(|_| error)?;
        prepared_retirements
            .try_reserve_exact(slots)
            .map_err(|_| error)?;
        if viewports.capacity() > slots || prepared_retirements.capacity() > slots {
            return Err(error);
        }
        Ok(Self {
            viewports,
            prepared_retirements,
            slot_limit: Some(slots),
        })
    }

    /// Counts inline state and W routes plus W independent exact retirement markers.
    pub(crate) fn reservation_bytes(slots: usize) -> Option<usize> {
        let per_slot =
            size_of::<NativeViewportRoute>().checked_add(size_of::<NativeSurfaceBinding>())?;
        size_of::<Self>().checked_add(slots.checked_mul(per_slot)?)
    }

    /// The installer separately accounts for overlap with any empty spare backing.
    pub(crate) fn can_configure_capacity_limit(&self) -> bool {
        self.slot_limit.is_none()
            && self.viewports.is_empty()
            && self.prepared_retirements.is_empty()
    }

    /// Counts inline state and actual backing capacities, including unused slots.
    /// Route receipts and their supported ingress payloads are entirely inline.
    pub(crate) fn owned_data_capacity_bytes(&self) -> usize {
        size_of::<Self>()
            .saturating_add(
                self.viewports
                    .capacity()
                    .saturating_mul(size_of::<NativeViewportRoute>()),
            )
            .saturating_add(
                self.prepared_retirements
                    .capacity()
                    .saturating_mul(size_of::<NativeSurfaceBinding>()),
            )
    }

    pub(crate) fn bindings(&self) -> impl Iterator<Item = NativeSurfaceBinding> + '_ {
        self.viewports.iter().map(|route| route.binding)
    }

    /// Retains a semantic repaint only on an existing viewport route.
    /// Reblocking a ready route revokes its previous claim eligibility.
    /// Returns whether the route exists, including an already blocked route.
    pub(crate) fn block_semantic_viewport(&mut self, viewport: ViewportId) -> bool {
        let Some(route) = self.route_mut(viewport) else {
            return false;
        };
        route.semantic_wake = SemanticWake::Blocked;
        true
    }

    /// The caller releases blocked paints only after the complete ordered
    /// boundary commits and no semantic prelude remains pending.
    pub(crate) fn release_blocked_semantic_viewports(&mut self) {
        for route in &mut self.viewports {
            if route.semantic_wake == SemanticWake::Blocked {
                route.semantic_wake = SemanticWake::Ready;
            }
        }
    }

    /// Transfers one ready repaint in viewport order without owning a snapshot.
    /// The caller requests the repaint after releasing its host-state lock.
    pub(crate) fn claim_ready_semantic_viewport(&mut self) -> Option<ViewportId> {
        let route = self
            .viewports
            .iter_mut()
            .find(|route| route.semantic_wake == SemanticWake::Ready)?;
        route.semantic_wake = SemanticWake::Idle;
        Some(route.viewport)
    }

    /// Clears only the exact route's repaint, preserving any successor owner.
    /// Returns whether the exact route matched, even if its repaint was idle.
    pub(crate) fn clear_semantic_wake(
        &mut self,
        viewport: ViewportId,
        binding: NativeSurfaceBinding,
    ) -> bool {
        let Some(route) = self.route_mut(viewport) else {
            return false;
        };
        if route.binding != binding {
            return false;
        }
        route.semantic_wake = SemanticWake::Idle;
        true
    }

    /// Quarantine relinquishes repaint obligations without retiring any route.
    pub(crate) fn clear_semantic_wakes(&mut self) {
        for route in &mut self.viewports {
            route.semantic_wake = SemanticWake::Idle;
        }
    }

    pub(crate) fn has_semantic_wake(&self) -> bool {
        self.viewports
            .iter()
            .any(|route| route.semantic_wake != SemanticWake::Idle)
    }

    #[cfg(test)]
    pub(crate) fn semantic_viewport_blocked(&self, viewport: ViewportId) -> bool {
        self.route(viewport)
            .is_some_and(|route| route.semantic_wake == SemanticWake::Blocked)
    }

    #[cfg(test)]
    pub(crate) fn semantic_viewport_ready(&self, viewport: ViewportId) -> bool {
        self.route(viewport)
            .is_some_and(|route| route.semantic_wake == SemanticWake::Ready)
    }

    pub(crate) fn reserve(
        &mut self,
        viewport: ViewportId,
        binding: NativeSurfaceBinding,
    ) -> Result<(), NativeViewportBindingError> {
        if let Some(existing) = self.route(viewport).map(NativeViewportRoute::snapshot) {
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
        self.prepare_route_slot(viewport)?;
        self.insert(viewport, None, binding);
        Ok(())
    }

    pub(crate) fn attach(
        &mut self,
        viewport: ViewportId,
        expected: NativeSurfaceBinding,
        window: WindowId,
    ) -> Result<(), NativeViewportBindingError> {
        let Some(current) = self.route(viewport).map(NativeViewportRoute::snapshot) else {
            return Err(NativeViewportBindingError::ViewportUnbound { viewport });
        };
        Self::validate_binding(viewport, expected, current)?;
        if let Some(existing) = current.window {
            if existing == window {
                self.route_mut(viewport)
                    .expect("validated viewport route remains present")
                    .window_published = true;
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
            .route_mut(viewport)
            .expect("validated viewport route remains present");
        route.window = Some(window);
        route.window_published = true;
        route.terminal = Some(NativeWindowReceipt::attached(
            viewport,
            window,
            expected,
            current.create_attempt,
        ));
        Ok(())
    }

    /// Captures a created window in its existing route without allocating an index entry.
    /// The reducer later calls `attach_output` to publish the window index.
    pub(crate) fn capture_output_window(
        &mut self,
        viewport: ViewportId,
        expected: NativeSurfaceBinding,
        window: WindowId,
        create_attempt: Option<NativeViewportCreateAttempt>,
    ) -> Result<(), NativeViewportBindingError> {
        let current = self
            .route(viewport)
            .map(NativeViewportRoute::snapshot)
            .ok_or(NativeViewportBindingError::ViewportUnbound { viewport })?;
        if current.create_attempt != create_attempt {
            return Err(NativeViewportBindingError::BindingNotCurrent {
                viewport,
                surface: expected.surface(),
            });
        }
        Self::validate_binding(viewport, expected, current)?;
        if let Some(existing) = current.window {
            return if existing == window {
                Ok(())
            } else {
                Err(NativeViewportBindingError::ViewportWindowAlreadyAttached {
                    viewport,
                    existing,
                })
            };
        }
        self.validate_not_retiring(viewport, current.binding)?;
        self.validate_window_unclaimed(viewport, window)?;
        let route = self
            .route_mut(viewport)
            .expect("validated capture retains its route");
        route.window = Some(window);
        route.window_published = false;
        route.terminal = Some(NativeWindowReceipt::attached(
            viewport,
            window,
            expected,
            create_attempt,
        ));
        Ok(())
    }

    pub(crate) fn attach_output(
        &mut self,
        viewport: ViewportId,
        expected: NativeSurfaceBinding,
        window: WindowId,
        create_attempt: Option<NativeViewportCreateAttempt>,
    ) -> Result<(), NativeViewportBindingError> {
        let Some(current) = self.route(viewport).map(NativeViewportRoute::snapshot) else {
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
            .route(viewport)
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
        if let Some(existing) = self.route(viewport).map(NativeViewportRoute::snapshot) {
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
        self.prepare_route_slot(viewport)?;
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
        self.prepare_route_slot(viewport)?;
        let terminal = self.take_window_receipt(viewport, expected);
        self.insert(viewport, None, successor);
        Ok(terminal)
    }

    pub(crate) fn reserve_retired_replacement(
        &mut self,
        viewport: ViewportId,
        expected: NativeSurfaceBinding,
        successor: NativeSurfaceBinding,
    ) -> Result<Option<NativeWindowReceipt>, NativeViewportBindingError> {
        let Some(current) = self.route(viewport).map(NativeViewportRoute::snapshot) else {
            return Err(NativeViewportBindingError::ViewportUnbound { viewport });
        };
        Self::validate_binding(viewport, expected, current)?;
        self.validate_create_settled(viewport, expected)?;
        if !self.is_pointer_suppressed(expected) || self.prepared_retirements.contains(&expected) {
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
        self.prepare_route_slot(viewport)?;
        let terminal = self.take_window_receipt(viewport, expected);
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
                .route(viewport)
                .map(NativeViewportRoute::snapshot)
                .ok_or(NativeViewportBindingError::ViewportUnbound { viewport })?;
            Self::validate_binding(viewport, expected, current)?;
            self.validate_create_settled(viewport, expected)?;
            if successor.surface() != expected.surface()
                || self.prepared_retirements.contains(&expected)
                || !self.route(viewport).is_some_and(|route| {
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
        self.prepare_route_slot(viewport)?;
        if current.window == Some(successor_window) {
            let route = self
                .route_mut(viewport)
                .expect("validated replacement retains its route");
            route.binding = successor;
            if let Some(receipt) = route.terminal.as_mut() {
                receipt.rebind(successor);
            }
            route.focus_reportable = false;
            return Ok(None);
        }
        if expected == successor {
            return Err(NativeViewportBindingError::BindingNotCurrent {
                viewport,
                surface: expected.surface(),
            });
        }
        let terminal = self.take_window_receipt(viewport, expected);
        self.insert(viewport, Some(successor_window), successor);
        Ok(terminal)
    }

    pub(crate) fn remove_viewport(
        &mut self,
        viewport: ViewportId,
        expected: NativeSurfaceBinding,
    ) -> Result<(NativeSurfaceBinding, Option<NativeWindowReceipt>), NativeViewportBindingError>
    {
        let Some(current) = self.route(viewport).map(NativeViewportRoute::snapshot) else {
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

    /// Rejects duplicate exact bindings, including duplicates within one batch.
    /// Every validation and allocation completes before any marker is installed.
    pub(crate) fn prepare_retirements(
        &mut self,
        retirements: &[CommittedRetirement],
    ) -> Result<(), NativeSurfaceBinding> {
        for (index, retirement) in retirements.iter().enumerate() {
            let viewport = retirement.viewport();
            let binding = retirement.binding();
            let current = self
                .route(viewport)
                .filter(|route| route.binding == binding)
                .map(NativeViewportRoute::snapshot);
            // A successor route is independent of the predecessor's retirement.
            if current.is_some() && self.validate_create_settled(viewport, binding).is_err()
                || self.prepared_retirements.contains(&binding)
                || retirements[..index]
                    .iter()
                    .any(|previous| previous.binding() == binding)
                || self.slot_limit.is_some_and(|limit| {
                    self.prepared_retirements
                        .len()
                        .checked_add(index + 1)
                        .is_none_or(|required| required > limit)
                })
            {
                return Err(binding);
            }
        }
        if let Some(first) = retirements.first() {
            self.prepared_retirements
                .try_reserve_exact(retirements.len())
                .map_err(|_| first.binding())?;
        }
        self.prepared_retirements
            .extend(retirements.iter().map(|retirement| retirement.binding()));
        self.prepared_retirements.sort_unstable();
        Ok(())
    }

    pub(crate) fn abort_retirements(&mut self, retirements: &[CommittedRetirement]) {
        for retirement in retirements {
            self.remove_prepared_retirement(retirement.binding());
        }
    }

    pub(crate) fn commit_retirements(&mut self, retirements: &[CommittedRetirement]) {
        for retirement in retirements {
            let viewport = retirement.viewport();
            let binding = retirement.binding();
            let current = self
                .route(viewport)
                .filter(|route| route.binding == binding)
                .map(NativeViewportRoute::snapshot);
            if let Some(current) = current {
                self.remove_route_unchecked(viewport, current);
            }
            self.remove_prepared_retirement(binding);
        }
    }

    pub(crate) fn suppress_pointer_route(
        &mut self,
        viewport: ViewportId,
        binding: NativeSurfaceBinding,
    ) -> bool {
        let Some(route) = self.route_mut(viewport) else {
            return false;
        };
        if route.binding != binding {
            return false;
        }
        route.pointer_suppressed = true;
        true
    }

    pub(crate) fn binding(&self, viewport: ViewportId) -> Option<NativeSurfaceBinding> {
        self.route(viewport).map(|route| route.binding)
    }

    pub(crate) fn window(&self, viewport: ViewportId) -> Option<WindowId> {
        self.route(viewport).and_then(|route| route.window)
    }

    pub(crate) fn pointer_binding_for_window(
        &self,
        window: WindowId,
    ) -> Option<NativeSurfaceBinding> {
        let (_, binding) = self.route_for_event(window, None)?;
        (!self.is_pointer_suppressed(binding)).then_some(binding)
    }

    pub(crate) fn pointer_binding_for_event(
        &self,
        window: WindowId,
        viewport: Option<ViewportId>,
    ) -> Option<NativeSurfaceBinding> {
        let (_, binding) = self.route_for_event(window, viewport)?;
        (!self.is_pointer_suppressed(binding)).then_some(binding)
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
        let (viewport, binding) = self.route_for_event(window, viewport)?;
        (self.route(viewport)?.focus_reportable
            && !self.is_pointer_suppressed(binding)
            && !self.prepared_retirements.contains(&binding))
        .then_some(binding)
    }

    /// Replaces the exact focus-reportable roster after a platform snapshot commits.
    pub(crate) fn set_focus_reportable(
        &mut self,
        bindings: impl IntoIterator<Item = NativeSurfaceBinding>,
    ) {
        for route in &mut self.viewports {
            route.focus_reportable = false;
        }
        for binding in bindings {
            if self.prepared_retirements.contains(&binding) {
                continue;
            }
            if let Some(route) = self
                .viewports
                .iter_mut()
                .find(|route| route.binding == binding && !route.pointer_suppressed)
            {
                route.focus_reportable = true;
            }
        }
    }

    pub(crate) fn route_for_event(
        &self,
        window: WindowId,
        viewport: Option<ViewportId>,
    ) -> Option<(ViewportId, NativeSurfaceBinding)> {
        // Published windows retain index priority; captures remain visible only
        // through the same fallback used before the reducer published the index.
        let route = self
            .viewports
            .iter()
            .find(|route| route.window_published && route.window == Some(window))
            .or_else(|| {
                self.viewports
                    .iter()
                    .find(|route| route.window == Some(window))
            })?;
        let mapped_viewport = route.viewport;
        if viewport.is_some_and(|viewport| viewport != mapped_viewport) {
            return None;
        }
        Some((mapped_viewport, route.binding))
    }

    /// Captures Destroyed in its attached route receipt without moving the owner.
    pub(crate) fn receive_window_destroyed(
        &mut self,
        window: WindowId,
        viewport: Option<ViewportId>,
        event_ordinal: u64,
        reserve: impl FnOnce(ViewportId, NativeSurfaceBinding) -> ReservedHostRecord,
    ) -> Option<(ViewportId, NativeSurfaceBinding)> {
        let (viewport, binding) = self.route_for_event(window, viewport)?;
        let receipt = self.route_mut(viewport)?.terminal.as_mut()?;
        if receipt.binding() != binding || !receipt.matches_window(window, Some(viewport)) {
            return None;
        }
        receipt
            .receive_destroyed(event_ordinal, || reserve(viewport, binding))
            .then_some((viewport, binding))
    }

    pub(crate) fn window_ingress(&self) -> impl Iterator<Item = &ReservedHostRecord> {
        self.viewports.iter().filter_map(|route| {
            route
                .terminal
                .as_ref()
                .and_then(NativeWindowReceipt::ingress)
        })
    }

    pub(crate) fn take_window_ingress(&mut self, ordinal: u64) -> Option<ReservedHostRecord> {
        self.viewports.iter_mut().find_map(|route| {
            route
                .terminal
                .as_mut()
                .and_then(|receipt| receipt.take_ingress(ordinal))
        })
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
        let route = self.route(viewport)?;
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
            .route_for_event(window, None)
            .is_some_and(|(current, _)| current != viewport)
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
        let retiring = self.prepared_retirements.contains(&expected);
        let Some(route) = self.route_mut(viewport) else {
            return false;
        };
        if attempt.viewport_id() != viewport
            || route.binding != expected
            || route.window.is_some()
            || route.pointer_suppressed
            || retiring
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
        let route = self.route(attempt.viewport_id())?;
        (route.window.is_none()
            && route.create.as_ref().and_then(NativeCreateReceipt::attempt) == Some(attempt)
            && !route.pointer_suppressed
            && !self.prepared_retirements.contains(&route.binding))
        .then_some(route.binding)
    }

    pub(crate) fn has_admitted_create_attempt(
        &self,
        viewport: ViewportId,
        binding: NativeSurfaceBinding,
    ) -> bool {
        self.route(viewport).is_some_and(|route| {
            route.binding == binding
                && route.window.is_none()
                && route.create.is_some()
                && !route.pointer_suppressed
                && !self.prepared_retirements.contains(&binding)
        })
    }

    /// A captured or admitted physical create keeps its original effect owner
    /// through fatal drain, including before the Created callback is reduced.
    pub(crate) fn has_create_lifetime_owner(
        &self,
        viewport: ViewportId,
        binding: NativeSurfaceBinding,
    ) -> bool {
        self.route(viewport).is_some_and(|route| {
            route.binding == binding
                && (route.create.is_some() || route.terminal.is_some() || route.window.is_some())
        })
    }

    pub(crate) fn viewport(&self, surface: SurfaceId) -> Option<ViewportId> {
        self.viewports
            .iter()
            .find_map(|route| (route.binding.surface() == surface).then_some(route.viewport))
    }

    pub(crate) fn receive_create_terminal(
        &mut self,
        viewport: ViewportId,
        binding: NativeSurfaceBinding,
        attempt: Option<NativeViewportCreateAttempt>,
        terminal: CreateTerminal,
        reserve: impl FnOnce() -> ReservedHostRecord,
    ) -> CreateAdmission {
        let Some(route) = self.route_mut(viewport) else {
            return CreateAdmission::Conflict;
        };
        if route.binding != binding {
            return CreateAdmission::Conflict;
        }
        route
            .create
            .as_mut()
            .map_or(CreateAdmission::Conflict, |receipt| {
                receipt.receive(attempt, terminal, reserve)
            })
    }

    pub(crate) fn create_ingress(&self) -> impl Iterator<Item = &ReservedHostRecord> {
        self.viewports
            .iter()
            .filter_map(|route| route.create.as_ref().and_then(NativeCreateReceipt::ingress))
    }

    pub(crate) fn take_create_ingress(&mut self, ordinal: u64) -> Option<ReservedHostRecord> {
        self.viewports.iter_mut().find_map(|route| {
            route
                .create
                .as_mut()
                .and_then(|receipt| receipt.take_ingress(ordinal))
        })
    }

    pub(crate) fn transfer_create_terminal(
        &mut self,
        viewport: ViewportId,
        binding: NativeSurfaceBinding,
        attempt: Option<NativeViewportCreateAttempt>,
        terminal: CreateTerminal,
    ) -> bool {
        self.route_mut(viewport)
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
        let route = self.route(viewport)?;
        if route.binding != binding
            || route.window.is_some()
            || !route
                .create
                .as_ref()
                .is_some_and(NativeCreateReceipt::is_transferred_failure)
        {
            return None;
        }
        let receipt = self.route_mut(viewport)?.create.take()?;
        let snapshot = self.route(viewport)?.snapshot();
        self.remove_route_unchecked(viewport, snapshot);
        Some(receipt)
    }

    pub(crate) fn commit_create_boundary(&mut self) {
        for route in &mut self.viewports {
            if let Some(receipt) = route.create.as_mut() {
                receipt.commit_created_boundary();
            }
        }
    }

    pub(crate) fn has_pending_create_receipts(&self) -> bool {
        self.viewports.iter().any(|route| {
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
        let Some(route) = self.route_mut(viewport) else {
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
        if let Some((existing, _)) = self.route_for_event(window, None)
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
        if let Some(existing) = self.viewport(binding.surface())
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
        let Some(current) = self.route(viewport).map(NativeViewportRoute::snapshot) else {
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
            self.viewport(successor.surface()),
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
        if self.route(viewport).is_some_and(|route| {
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
        if self.is_pointer_suppressed(binding) || self.prepared_retirements.contains(&binding) {
            return Err(NativeViewportBindingError::BindingNotCurrent {
                viewport,
                surface: binding.surface(),
            });
        }
        Ok(())
    }

    fn is_pointer_suppressed(&self, binding: NativeSurfaceBinding) -> bool {
        self.viewports
            .iter()
            .find(|route| route.binding == binding)
            .is_some_and(|route| route.pointer_suppressed)
    }

    fn route(&self, viewport: ViewportId) -> Option<&NativeViewportRoute> {
        let index = self
            .viewports
            .binary_search_by_key(&viewport, |route| route.viewport)
            .ok()?;
        self.viewports.get(index)
    }

    fn route_mut(&mut self, viewport: ViewportId) -> Option<&mut NativeViewportRoute> {
        let index = self
            .viewports
            .binary_search_by_key(&viewport, |route| route.viewport)
            .ok()?;
        self.viewports.get_mut(index)
    }

    /// A replacement reuses its original slot before any receipt is moved.
    fn prepare_route_slot(
        &mut self,
        viewport: ViewportId,
    ) -> Result<(), NativeViewportBindingError> {
        if self.route(viewport).is_some() {
            return Ok(());
        }
        let error = NativeViewportBindingError::CapacityExceeded {
            limit: self.slot_limit.unwrap_or(self.viewports.len()),
        };
        if self
            .slot_limit
            .is_some_and(|limit| self.viewports.len() >= limit)
        {
            return Err(error);
        }
        if self.viewports.len() == self.viewports.capacity() {
            self.viewports.try_reserve(1).map_err(|_| error)?;
        }
        Ok(())
    }

    fn remove_prepared_retirement(&mut self, binding: NativeSurfaceBinding) {
        if let Ok(index) = self.prepared_retirements.binary_search(&binding) {
            self.prepared_retirements.remove(index);
        }
    }

    fn insert(
        &mut self,
        viewport: ViewportId,
        window: Option<WindowId>,
        binding: NativeSurfaceBinding,
    ) {
        let mut route = NativeViewportRoute {
            viewport,
            window,
            window_published: window.is_some(),
            binding,
            create: None,
            terminal: window
                .map(|window| NativeWindowReceipt::attached(viewport, window, binding, None)),
            pointer_suppressed: false,
            focus_reportable: false,
            semantic_wake: SemanticWake::Idle,
        };
        match self
            .viewports
            .binary_search_by_key(&viewport, |route| route.viewport)
        {
            Ok(index) => {
                // A pending repaint belongs to the logical viewport across its
                // physical replacement, including a reservation without a window.
                route.semantic_wake = self.viewports[index].semantic_wake;
                self.viewports[index] = route;
            }
            Err(index) => {
                assert!(self.viewports.len() < self.viewports.capacity());
                self.viewports.insert(index, route);
            }
        }
    }

    fn remove_route_unchecked(&mut self, viewport: ViewportId, route: RouteSnapshot) {
        let index = self
            .viewports
            .binary_search_by_key(&viewport, |route| route.viewport)
            .expect("the removed viewport retains its exact route");
        let removed = self.viewports.remove(index);
        debug_assert_eq!(removed.snapshot(), route);
    }

    pub(crate) fn take_window_receipt(
        &mut self,
        viewport: ViewportId,
        binding: NativeSurfaceBinding,
    ) -> Option<NativeWindowReceipt> {
        self.route_mut(viewport)
            .filter(|route| route.binding == binding)?
            .terminal
            .take()
    }

    pub(crate) fn has_window_receipt(
        &self,
        viewport: ViewportId,
        binding: NativeSurfaceBinding,
    ) -> bool {
        self.route(viewport)
            .is_some_and(|route| route.binding == binding && route.terminal.is_some())
    }

    pub(crate) fn has_received_destroyed(
        &self,
        viewport: ViewportId,
        binding: NativeSurfaceBinding,
        window: WindowId,
        event_ordinal: u64,
    ) -> bool {
        self.route(viewport).is_some_and(|route| {
            route.binding == binding
                && route.window == Some(window)
                && route.terminal.as_ref().is_some_and(|receipt| {
                    receipt.binding() == binding
                        && receipt.matches_window(window, Some(viewport))
                        && receipt.received_destroyed(event_ordinal)
                })
        })
    }

    pub(crate) fn received_destroyed_receipt_mut(
        &mut self,
        viewport: ViewportId,
        binding: NativeSurfaceBinding,
        window: WindowId,
        event_ordinal: u64,
    ) -> Option<&mut NativeWindowReceipt> {
        let route = self.route_mut(viewport)?;
        if route.binding != binding || route.window != Some(window) {
            return None;
        }
        route.terminal.as_mut().filter(|receipt| {
            receipt.binding() == binding
                && receipt.matches_window(window, Some(viewport))
                && receipt.received_destroyed(event_ordinal)
        })
    }
}

#[cfg(test)]
mod storage_tests;

#[cfg(test)]
mod wake_tests;

#[cfg(test)]
mod tests {
    use dockspace::model::{
        DockspaceLayout, DockspaceNode, DockspaceRootLayout, DockspaceSurfaceLayout, ItemId, RootId,
    };
    use dockspace::policy::DockPolicy;
    use dockspace::runtime::{
        DockspaceSession, HostInputOutcome, HostWindowToken, NativePointerRoster,
        SurfaceUnavailableReason,
    };

    use super::*;

    pub(super) fn bindings() -> (NativeSurfaceBinding, NativeSurfaceBinding) {
        let layout = DockspaceLayout::new([1, 2].map(|id| {
            DockspaceSurfaceLayout::new(
                SurfaceId::new(id),
                DockspaceRootLayout::new(
                    RootId::new(id),
                    DockspaceNode::central_tabs([ItemId::new(id)]),
                ),
            )
        }))
        .expect("test layout validates");
        let mut session = DockspaceSession::from_layout(layout, DockPolicy::default())
            .expect("test session initializes");
        session
            .enable_managed_native_host(NativePointerRoster::Exact(Vec::new()))
            .expect("native host enrolls");
        for id in [1, 2] {
            session
                .register_native_root(SurfaceId::new(id), HostWindowToken::new(id))
                .expect("native root registers");
        }
        let mut frame = session
            .begin_host_frame()
            .expect("registration frame begins");
        frame
            .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
            .expect("registration surfaces settle");
        let report = frame.commit().expect("registration frame commits");
        let mut bindings = report.inputs().iter().filter_map(|input| match input {
            HostInputOutcome::NativeSurfaceRegistered { binding } => Some(*binding),
            _ => None,
        });
        (bindings.next().unwrap(), bindings.next().unwrap())
    }

    #[test]
    fn owned_data_capacity_retains_backing_through_route_and_focus_updates() {
        let (first, second) = bindings();
        let first_viewport = ViewportId::ROOT;
        let second_viewport = ViewportId::from_hash_of("capacity-peer");
        let mut routes = NativeViewportMap::with_capacity_limit(2).unwrap();
        let empty = routes.owned_data_capacity_bytes();
        assert_eq!(empty, NativeViewportMap::reservation_bytes(2).unwrap());
        routes.reserve(first_viewport, first).unwrap();
        let reserved = routes.owned_data_capacity_bytes();
        assert_eq!(reserved, empty);
        routes.reserve(second_viewport, second).unwrap();
        let two_reserved = routes.owned_data_capacity_bytes();
        assert_eq!(two_reserved, reserved);

        routes
            .attach(first_viewport, first, WindowId::from(81))
            .unwrap();
        let attached = routes.owned_data_capacity_bytes();
        assert_eq!(attached, two_reserved);
        routes
            .attach(second_viewport, second, WindowId::from(82))
            .unwrap();
        let two_attached = routes.owned_data_capacity_bytes();
        assert_eq!(two_attached, attached);
        let mut journal = crate::mailbox::journal::HostIngressJournal::new(
            dockspace::runtime::NativeMonotonicClock::system(),
        );
        assert_eq!(
            routes.receive_window_destroyed(
                WindowId::from(81),
                Some(first_viewport),
                1,
                |viewport, binding| journal.reserve(crate::mailbox::HostRecord::WindowEvent(
                    crate::event::NativeWindowEventRecord::for_test(
                        1,
                        WindowId::from(81),
                        Some(viewport),
                        Some(binding),
                        winit::event::WindowEvent::Destroyed,
                    ),
                )),
            ),
            Some((first_viewport, first)),
        );
        assert_eq!(routes.window_ingress().count(), 1);
        assert_eq!(routes.owned_data_capacity_bytes(), two_attached);
        routes.set_focus_reportable([first, second]);
        let two_focused = routes.owned_data_capacity_bytes();
        assert_eq!(two_focused, two_attached);
        assert_eq!(
            routes.focus_binding_for_event(WindowId::from(81), None),
            Some(first)
        );
        routes.set_focus_reportable([second]);
        let one_focused = routes.owned_data_capacity_bytes();
        assert_eq!(one_focused, two_focused);
        assert_eq!(
            routes.focus_binding_for_event(WindowId::from(81), None),
            None
        );
        assert!(routes.suppress_pointer_route(first_viewport, first));
        assert_eq!(routes.owned_data_capacity_bytes(), two_focused);
        routes.set_focus_reportable([]);
        assert_eq!(routes.owned_data_capacity_bytes(), two_attached);
        drop(routes.remove_viewport(first_viewport, first).unwrap());
        drop(routes.remove_viewport(second_viewport, second).unwrap());
        assert_eq!(routes.owned_data_capacity_bytes(), empty);
    }

    #[test]
    fn captured_output_window_routes_and_excludes_peers_before_reducer_indexes_it() {
        let (binding, other_binding) = bindings();
        let viewport = ViewportId::ROOT;
        let other_viewport = ViewportId::from_hash_of("unindexed-window-peer");
        let window = WindowId::from(71);
        let mut routes = NativeViewportMap::default();
        routes.reserve(viewport, binding).unwrap();
        let reserved = routes.owned_data_capacity_bytes();
        assert!(routes.admit_test_create(viewport, binding));
        assert_eq!(routes.owned_data_capacity_bytes(), reserved);

        // This route model uses the admitted tokenless test attempt, not a
        // fabricated renderer output token or a full Created callback fixture.
        routes
            .capture_output_window(viewport, binding, window, None)
            .unwrap();
        assert_eq!(routes.owned_data_capacity_bytes(), reserved);
        assert!(!routes.route(viewport).unwrap().window_published);
        assert!(routes.has_window_receipt(viewport, binding));
        assert_eq!(routes.window(viewport), Some(window));
        assert_eq!(
            routes.route_for_event(window, None),
            Some((viewport, binding))
        );
        assert_eq!(
            routes.route_for_event(window, Some(viewport)),
            Some((viewport, binding))
        );
        assert_eq!(routes.route_for_event(window, Some(other_viewport)), None);
        assert_eq!(routes.pointer_binding_for_window(window), Some(binding));
        assert_eq!(
            routes.binding_for_output(viewport, window, None),
            Some(binding)
        );
        assert_eq!(
            routes.bind(other_viewport, window, other_binding),
            Err(NativeViewportBindingError::WindowAlreadyBound {
                window,
                existing: viewport
            }),
        );
        routes.reserve(other_viewport, other_binding).unwrap();
        assert!(routes.admit_test_create(other_viewport, other_binding));
        assert_eq!(
            routes.capture_output_window(other_viewport, other_binding, window, None),
            Err(NativeViewportBindingError::WindowAlreadyBound {
                window,
                existing: viewport
            }),
        );
        assert_eq!(routes.window(other_viewport), None);
        assert!(!routes.has_window_receipt(other_viewport, other_binding));

        routes
            .capture_output_window(viewport, binding, window, None)
            .unwrap();
        assert!(
            !routes.route(viewport).unwrap().window_published,
            "duplicate capture cannot publish the window index"
        );
        let captured = routes.owned_data_capacity_bytes();
        routes
            .attach_output(viewport, binding, window, None)
            .unwrap();
        assert_eq!(routes.owned_data_capacity_bytes(), captured);
        assert!(routes.route(viewport).unwrap().window_published);
        assert_eq!(
            routes.route_for_event(window, None),
            Some((viewport, binding))
        );
        assert_eq!(routes.pointer_binding_for_window(window), Some(binding));
        assert_eq!(
            routes.binding_for_output(viewport, window, None),
            Some(binding)
        );
    }
}
