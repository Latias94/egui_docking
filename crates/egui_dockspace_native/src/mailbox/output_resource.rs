//! One output token owns its dispatch budget, affine paint and renderer terminal.

use dockspace::runtime::{
    DockspaceSession, NativeInputBudget, NativeSurfaceBinding, PaintedSurfaceOutput,
};
use eframe::NativeOutputResult;
use eframe::{NativeOutputToken, egui::ViewportId};

use super::journal::ReservedHostRecord;
use super::{HostRecord, OutputReservation};
use crate::error::NativeHostProtocolError;
use crate::lifecycle_progress::NativeOutputBacklog;

const ORDINARY_OUTPUT_CAPACITY: usize = 256;

pub(crate) enum PendingNativeOutput {
    Surface(PaintedSurfaceOutput),
    Staging(super::NativePreparationPaint),
}

impl std::fmt::Debug for PendingNativeOutput {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Surface(output) => formatter
                .debug_tuple("Surface")
                .field(&output.surface())
                .finish(),
            Self::Staging(output) => formatter
                .debug_tuple("Staging")
                .field(&output.binding())
                .field(&output.phase())
                .finish(),
        }
    }
}

impl PendingNativeOutput {
    pub(super) fn matches_binding(&self, binding: NativeSurfaceBinding) -> bool {
        match self {
            Self::Surface(output) => output.matches_native_binding(binding),
            Self::Staging(output) => output.binding() == binding,
        }
    }

    pub(super) fn requires_lifecycle_settlement(&self, session: &DockspaceSession) -> bool {
        match self {
            Self::Staging(_) => true,
            Self::Surface(output) => session.presented_surface(output.surface()).is_none(),
        }
    }
}

#[derive(Debug)]
pub(super) struct NativeOutputResource {
    pub(super) reservation: OutputReservation,
    pub(super) paint: Option<PendingNativeOutput>,
    pub(super) terminal: Option<NativeOutputResult>,
    // Keep the exact callback order and age through submission until full commit.
    terminal_ingress: Option<ReservedHostRecord>,
    pub(super) submitted: bool,
    budget: Option<NativeInputBudget>,
}

/// Ordinary output owners and prepaid control passes use the same protocol.
///
/// The root slot is inline and independent of the bounded child backing, so
/// nested child passes cannot consume the root's recovery capacity. Control
/// diagnostic terminals remove their exact owner without joining semantic
/// ingress. An already promised child staging output may retain its affine
/// paint and terminal in that same prepaid backing through full commit.
#[derive(Debug)]
pub(super) struct NativeOutputResources {
    ordinary: Vec<(NativeOutputToken, NativeOutputResource)>,
    root_control: Option<(NativeOutputToken, NativeOutputResource)>,
    child_controls: Vec<(NativeOutputToken, NativeOutputResource)>,
}

impl Default for NativeOutputResources {
    fn default() -> Self {
        Self::try_new().expect("native output backing can be prepaid")
    }
}

impl NativeOutputResources {
    /// Prepays ordinary and child control backing before admitting any output.
    pub(super) fn try_new() -> Result<Self, NativeHostProtocolError> {
        let error = NativeHostProtocolError::NativeReportStorageUnavailable;
        Self::reservation_bytes().ok_or(error)?;
        let mut ordinary = Vec::new();
        ordinary
            .try_reserve_exact(ORDINARY_OUTPUT_CAPACITY)
            .map_err(|_| error)?;
        if ordinary.capacity() > ORDINARY_OUTPUT_CAPACITY {
            return Err(error);
        }
        let mut child_controls = Vec::new();
        child_controls
            .try_reserve_exact(super::terminal_roster::DEFAULT_WINDOW_CAPACITY)
            .map_err(|_| error)?;
        if child_controls.capacity() > super::terminal_roster::DEFAULT_WINDOW_CAPACITY {
            return Err(error);
        }
        Ok(Self {
            ordinary,
            root_control: None,
            child_controls,
        })
    }

    /// Includes inline state and both fixed backings, without allocator metadata.
    pub(super) fn reservation_bytes() -> Option<usize> {
        let slots = ORDINARY_OUTPUT_CAPACITY
            .checked_add(super::terminal_roster::DEFAULT_WINDOW_CAPACITY)?;
        size_of::<Self>().checked_add(slots.checked_mul(Self::ordinary_entry_data_bytes())?)
    }

    pub(super) const fn maximum_token_count(&self) -> usize {
        ORDINARY_OUTPUT_CAPACITY + 1 + self.child_controls.capacity()
    }

    pub(super) const fn default_maximum_token_count() -> usize {
        ORDINARY_OUTPUT_CAPACITY + 1 + super::terminal_roster::DEFAULT_WINDOW_CAPACITY
    }

    pub(super) const fn ordinary_entry_data_bytes() -> usize {
        size_of::<(NativeOutputToken, NativeOutputResource)>()
    }

    pub(super) fn owned_descendant_capacity_bytes(&self) -> usize {
        self.ordinary
            .capacity()
            .saturating_mul(Self::ordinary_entry_data_bytes())
            .saturating_add(
                self.child_controls
                    .capacity()
                    .saturating_mul(size_of::<(NativeOutputToken, NativeOutputResource)>()),
            )
    }

    pub(super) fn output_backlog(&self, awaiting_staging: usize) -> NativeOutputBacklog {
        NativeOutputBacklog::new(
            self.ordinary.len(),
            usize::from(self.root_control.is_some()) + self.child_controls.len(),
            self.child_controls
                .iter()
                .filter(|(_, resource)| !resource.reservation.abandoned)
                .count(),
            awaiting_staging,
        )
    }

    pub(super) fn contains_key(&self, token: &NativeOutputToken) -> bool {
        self.get(token).is_some()
    }

    pub(super) fn can_reserve_ordinary(&self) -> bool {
        self.ordinary.len() < ORDINARY_OUTPUT_CAPACITY
    }

    pub(super) fn is_control(&self, token: NativeOutputToken) -> bool {
        self.root_control
            .as_ref()
            .is_some_and(|(key, _)| *key == token)
            || self
                .get(&token)
                .is_some_and(|resource| resource.reservation.abandoned)
    }

    pub(super) fn get(&self, token: &NativeOutputToken) -> Option<&NativeOutputResource> {
        self.ordinary
            .binary_search_by_key(token, |(key, _)| *key)
            .ok()
            .map(|index| &self.ordinary[index].1)
            .or_else(|| {
                self.root_control
                    .iter()
                    .chain(self.child_controls.iter())
                    .find_map(|(key, resource)| (key == token).then_some(resource))
            })
    }

    pub(super) fn get_mut(
        &mut self,
        token: &NativeOutputToken,
    ) -> Option<&mut NativeOutputResource> {
        if let Ok(index) = self.ordinary.binary_search_by_key(token, |(key, _)| *key) {
            return Some(&mut self.ordinary[index].1);
        }
        self.root_control
            .iter_mut()
            .chain(self.child_controls.iter_mut())
            .find_map(|(key, resource)| (*key == *token).then_some(resource))
    }

    /// Refusal preserves the incoming affine resource and every retained owner.
    pub(super) fn insert(
        &mut self,
        token: NativeOutputToken,
        resource: NativeOutputResource,
    ) -> Result<(), NativeOutputResource> {
        if !self.can_reserve_ordinary()
            || self
                .root_control
                .iter()
                .chain(self.child_controls.iter())
                .any(|(key, _)| *key == token)
        {
            return Err(resource);
        }
        let Err(index) = self.ordinary.binary_search_by_key(&token, |(key, _)| *key) else {
            return Err(resource);
        };
        self.ordinary.insert(index, (token, resource));
        Ok(())
    }

    pub(super) fn insert_control(
        &mut self,
        token: NativeOutputToken,
        resource: NativeOutputResource,
    ) -> bool {
        if self.contains_key(&token) {
            return false;
        }
        if token.viewport_id() == ViewportId::ROOT {
            debug_assert!(resource.reservation.abandoned);
            if self.root_control.is_some() {
                return false;
            }
            self.root_control = Some((token, resource));
        } else {
            if self.child_controls.len() == self.child_controls.capacity()
                || self.child_controls.iter().any(|(current, _)| {
                    current.same_context(token)
                        && current.viewport_id() == token.viewport_id()
                        && current.window_id() == token.window_id()
                })
            {
                return false;
            }
            self.child_controls.push((token, resource));
        }
        true
    }

    pub(super) fn remove(&mut self, token: &NativeOutputToken) -> Option<NativeOutputResource> {
        if self
            .root_control
            .as_ref()
            .is_some_and(|(key, _)| key == token)
        {
            return self.root_control.take().map(|(_, resource)| resource);
        }
        if let Some(index) = self.child_controls.iter().position(|(key, _)| key == token) {
            return Some(self.child_controls.swap_remove(index).1);
        }
        let index = self
            .ordinary
            .binary_search_by_key(token, |(key, _)| *key)
            .ok()?;
        Some(self.ordinary.remove(index).1)
    }

    pub(super) fn values(&self) -> impl Iterator<Item = &NativeOutputResource> {
        self.iter().map(|(_, resource)| resource)
    }

    pub(super) fn terminal_ingress(
        &self,
    ) -> impl Iterator<Item = (&NativeOutputResource, &ReservedHostRecord)> {
        self.values().filter_map(|resource| {
            resource
                .terminal_ingress()
                .map(|ingress| (resource, ingress))
        })
    }

    pub(super) fn iter(&self) -> impl Iterator<Item = (&NativeOutputToken, &NativeOutputResource)> {
        self.ordinary
            .iter()
            .chain(self.root_control.iter())
            .chain(self.child_controls.iter())
            .map(|(token, resource)| (token, resource))
    }

    pub(super) fn iter_mut(
        &mut self,
    ) -> impl Iterator<Item = (&NativeOutputToken, &mut NativeOutputResource)> {
        self.ordinary
            .iter_mut()
            .chain(self.root_control.iter_mut())
            .chain(self.child_controls.iter_mut())
            .map(|(token, resource)| (&*token, resource))
    }
}

impl NativeOutputResource {
    pub(super) fn new(reservation: OutputReservation, budget: NativeInputBudget) -> Self {
        Self {
            reservation,
            paint: None,
            terminal: None,
            terminal_ingress: None,
            submitted: false,
            budget: Some(budget),
        }
    }

    pub(super) fn take_budget(&mut self) -> Option<NativeInputBudget> {
        if self.reservation.terminal_recorded {
            return None;
        }
        self.budget.take()
    }

    pub(super) fn defer_before_input(&mut self) -> bool {
        if self.submitted || self.paint.is_some() || self.reservation.terminal_recorded {
            return false;
        }
        // The original scope still supplies its terminal without invalidating
        // the retained framebuffer. Lifecycle settlement may already have
        // abandoned this owner; preserve its existing durable wake in that case.
        self.reservation.abandon_silently();
        true
    }

    pub(super) fn terminal_requires_repaint(&self, status: eframe::NativeOutputStatus) -> bool {
        self.reservation.wakes_after_current()
            || status != eframe::NativeOutputStatus::Presented
            || !matches!(self.paint, Some(PendingNativeOutput::Surface(_)))
    }

    pub(super) fn return_budget(&mut self, budget: NativeInputBudget) {
        // A late lease return must not reopen an output after its renderer terminal.
        if !self.reservation.terminal_recorded {
            self.budget = Some(budget);
        }
    }

    pub(super) fn record_terminal(&mut self, result: NativeOutputResult) {
        self.reservation.terminal_recorded = true;
        self.terminal = Some(result);
        self.budget = None;
    }

    pub(super) fn terminal_ingress(&self) -> Option<&ReservedHostRecord> {
        self.terminal_ingress.as_ref()
    }

    /// Retains one exact terminal envelope after its metadata has been recorded.
    /// Submission leaves this envelope in place until the full commit removes its owner.
    pub(super) fn retain_terminal_ingress(&mut self, ingress: ReservedHostRecord) -> bool {
        if self.reservation.abandoned
            || !self.reservation.terminal_recorded
            || self.submitted
            || self.terminal_ingress.is_some()
        {
            return false;
        }
        if !matches!(
            (self.terminal, ingress.record()),
            (Some(result), HostRecord::Output { token }) if result.token() == *token
        ) {
            return false;
        }
        self.terminal_ingress = Some(ingress);
        true
    }

    pub(super) fn bind_paint(
        &mut self,
        paint: PendingNativeOutput,
    ) -> Result<(), PendingNativeOutput> {
        if self.paint.is_some()
            || self.submitted
            || self.reservation.abandoned
            || !self
                .reservation
                .binding()
                .is_some_and(|binding| paint.matches_binding(binding))
        {
            return Err(paint);
        }
        self.paint = Some(paint);
        Ok(())
    }
}

#[cfg(test)]
mod storage_tests;

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use dockspace::geometry::{LogicalRect, LogicalSize, PhysicalRect, ScaleFactor};
    use dockspace::model::{
        DockspaceLayout, DockspaceNode, DockspaceRootLayout, DockspaceSurfaceLayout, ItemId,
        RootId, SurfaceId,
    };
    use dockspace::policy::DockPolicy;
    use dockspace::runtime::{
        HostInputOutcome, HostWindowToken, NativeCloseState, NativeMonotonicClock,
        NativePointerRoster, NativeReceiverAnswer, NativeWindowFacts, NativeWindowInputState,
        NativeWindowPresentationState, NativeWorkAreaRoster, SurfaceUnavailableReason,
        UniformSurfaceMetrics,
    };

    use super::*;

    fn budget() -> NativeInputBudget {
        NativeInputBudget::new(&NativeMonotonicClock::new(|| Duration::ZERO))
    }

    fn painted_output() -> (DockspaceSession, NativeSurfaceBinding, PaintedSurfaceOutput) {
        let surface = SurfaceId::new(1);
        let layout = DockspaceLayout::new([DockspaceSurfaceLayout::new(
            surface,
            DockspaceRootLayout::new(
                RootId::new(1),
                DockspaceNode::central_tabs([ItemId::new(1)]),
            ),
        )])
        .unwrap();
        let mut session = DockspaceSession::from_layout(layout, DockPolicy::default()).unwrap();
        session
            .enable_managed_native_host(NativePointerRoster::Exact(Vec::new()))
            .unwrap();
        session
            .configure_managed_native_capabilities(crate::capabilities::capabilities_for_backend(
                eframe::NativeWindowingBackend::Windows,
            ))
            .unwrap();
        session
            .register_native_root(surface, HostWindowToken::new(1))
            .unwrap();
        let mut registration = session
            .begin_native_host_frame(|_| NativeReceiverAnswer::Unknown)
            .unwrap();
        registration
            .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
            .unwrap();
        let report = registration.commit().unwrap();
        let binding = report
            .inputs()
            .iter()
            .find_map(|outcome| match outcome {
                HostInputOutcome::NativeSurfaceRegistered { binding } => Some(*binding),
                _ => None,
            })
            .unwrap();
        let bounds = PhysicalRect::new(0.0, 0.0, 640.0, 480.0).unwrap();
        let scale = ScaleFactor::new(1.0).unwrap();
        session
            .report_managed_native_snapshot(
                [(
                    binding,
                    NativeWindowFacts::live()
                        .with_content_bounds(bounds)
                        .with_outer_bounds(bounds)
                        .with_native_scale_factor(scale)
                        .with_presentation_scale_factor(scale)
                        .with_input(NativeWindowInputState::ReceivesInput, None)
                        .with_presentation(NativeWindowPresentationState::Visible)
                        .with_close(NativeCloseState::Clear, None),
                )],
                NativeWorkAreaRoster::Unknown,
            )
            .unwrap();
        let mut observation = session
            .begin_native_host_frame(|_| NativeReceiverAnswer::Unknown)
            .unwrap();
        observation
            .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
            .unwrap();
        observation.commit().unwrap();
        let mut measurement = session
            .begin_native_host_frame(|_| NativeReceiverAnswer::Unknown)
            .unwrap();
        measurement
            .measure_surface(
                surface,
                UniformSurfaceMetrics::new(
                    LogicalRect::new(0.0, 0.0, 640.0, 480.0).unwrap(),
                    LogicalSize::new(32.0, 24.0).unwrap(),
                    80.0,
                )
                .unwrap(),
            )
            .unwrap();
        measurement.commit().unwrap();
        let mut paint = session
            .begin_native_host_frame(|_| NativeReceiverAnswer::Unknown)
            .unwrap();
        let receipt = paint
            .paint_plan(surface)
            .unwrap()
            .unwrap()
            .execute_surface_program(|_| Ok::<(), ()>(()))
            .unwrap();
        paint.confirm_surface_painted(receipt).unwrap();
        let output = paint
            .commit()
            .unwrap()
            .take_painted_outputs()
            .pop()
            .unwrap();
        (session, binding, output)
    }

    #[test]
    fn output_resource_returns_budget_credits_and_terminal_closes_late_return() {
        let mut resource =
            NativeOutputResource::new(OutputReservation::unbound_for_test(), budget());
        let mut borrowed = resource.take_budget().unwrap();
        assert!(
            resource.take_budget().is_none(),
            "the dispatch has one borrower"
        );
        borrowed.complete_unsequenced_records(7);
        resource.return_budget(borrowed);
        let borrowed = resource.take_budget().unwrap();
        assert_eq!(borrowed.remaining_records(), 249);
        resource.reservation.terminal_recorded = true;
        resource.return_budget(borrowed);
        assert!(resource.budget.is_none());
        assert!(resource.take_budget().is_none());
    }

    #[test]
    fn output_resource_holds_one_exact_affine_paint_and_preserves_rejected_output() {
        let (session, binding, output) = painted_output();
        let mut reservation = OutputReservation::unbound_for_test();
        assert!(reservation.attach(binding));
        let mut resource = NativeOutputResource::new(reservation, budget());
        // Completion metadata is not an early presentation proof. Late paint
        // attachment remains possible until an exact core submission takes it.
        resource.reservation.terminal_recorded = true;
        assert!(
            resource
                .bind_paint(PendingNativeOutput::Surface(output))
                .is_ok()
        );
        assert!(
            resource
                .paint
                .as_ref()
                .unwrap()
                .requires_lifecycle_settlement(&session)
        );
        let output = resource.paint.take().unwrap();
        assert!(resource.paint.is_none());
        resource.submitted = true;
        let rejected = resource
            .bind_paint(output)
            .expect_err("submitted output cannot be rebound");
        assert!(rejected.matches_binding(binding));
        assert!(
            resource.paint.is_none(),
            "a rejected attachment stays caller-owned"
        );
        drop(rejected);
        drop(resource);
    }

    #[test]
    fn input_deferral_after_lifecycle_abandonment_preserves_the_original_owner_and_budget() {
        let mut resource =
            NativeOutputResource::new(OutputReservation::unbound_for_test(), budget());
        let mut borrowed = resource.take_budget().unwrap();
        borrowed.complete_unsequenced_records(7);
        assert!(resource.reservation.require_wake_after_current());
        assert!(resource.reservation.abandon());

        assert!(resource.defer_before_input());
        assert!(resource.defer_before_input());
        assert!(resource.reservation.wakes_on_terminal());
        assert!(resource.reservation.wakes_after_current());
        assert!(!resource.submitted);
        assert!(!resource.reservation.terminal_recorded);
        resource.return_budget(borrowed);
        assert_eq!(resource.take_budget().unwrap().remaining_records(), 249);

        resource.reservation.terminal_recorded = true;
        assert!(!resource.defer_before_input());
    }

    #[test]
    fn input_deferral_rejects_an_attached_affine_paint_without_losing_it() {
        let (_, binding, output) = painted_output();
        let mut reservation = OutputReservation::unbound_for_test();
        assert!(reservation.attach(binding));
        let mut resource = NativeOutputResource::new(reservation, budget());
        resource
            .bind_paint(PendingNativeOutput::Surface(output))
            .unwrap();
        assert!(!resource.defer_before_input());
        assert!(!resource.reservation.abandoned);
        assert!(resource.paint.as_ref().unwrap().matches_binding(binding));
    }

    #[test]
    fn ordinary_presentation_uses_host_service_but_first_live_and_dropped_outputs_require_repaint()
    {
        use eframe::NativeOutputStatus::{NotPresented, Presented};

        let (_, binding, output) = painted_output();
        let mut reservation = OutputReservation::unbound_for_test();
        assert!(reservation.attach(binding));
        let mut resource = NativeOutputResource::new(reservation, budget());
        assert!(resource.terminal_requires_repaint(Presented));
        resource
            .bind_paint(PendingNativeOutput::Surface(output))
            .unwrap();
        assert!(!resource.terminal_requires_repaint(Presented));
        assert!(resource.terminal_requires_repaint(NotPresented));
        assert!(resource.reservation.require_wake_after_current());
        assert!(resource.terminal_requires_repaint(Presented));
    }
}
