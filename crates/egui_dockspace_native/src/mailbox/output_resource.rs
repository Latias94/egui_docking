//! One output token owns its dispatch budget, affine paint and renderer terminal.

use dockspace::runtime::{
    DockspaceSession, NativeInputBudget, NativeSurfaceBinding, PaintedNativeStagingOutput,
    PaintedSurfaceOutput,
};
use eframe::NativeOutputResult;

use super::journal::ReservedHostRecord;
use super::{HostRecord, OutputReservation};

pub(crate) enum PendingNativeOutput {
    Surface(PaintedSurfaceOutput),
    Staging(PaintedNativeStagingOutput),
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
                .field(&output.surface())
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
                        .with_presentation(NativeWindowPresentationState::Visible, None)
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
}
