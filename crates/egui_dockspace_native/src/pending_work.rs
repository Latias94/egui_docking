//! Allocation-free diagnostics for the current native settlement boundary.

/// Why the native runtime has not reached quiescence.
///
/// Reasons describe current work, not failures, completion guarantees or
/// permission to bypass a presentation or lifecycle barrier.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum NativeWaitReason {
    /// A suspended update or native service turn must finish.
    Service,
    /// Fatal shutdown prevents ordinary runtime quiescence.
    Stopped,
    /// The application root lacks an exact currently presented output.
    RootPresentation,
    /// An application command or its unconsumed result owns the command slot.
    ApplicationAction,
    /// A local UI action remains retained across output passes.
    LocalAction,
    /// Native child creation has not reached its terminal boundary.
    WindowCreation,
    /// A presentation result or ownership transition remains unsettled.
    Presentation,
    /// Host callbacks, platform observations or callback errors await settlement.
    HostEvents,
    /// A native lifecycle output still owns work beyond ordinary repainting.
    LifecycleOutput,
    /// A managed viewport is being created, shown or replaced.
    WindowTransition,
    /// A platform effect or its result remains outstanding.
    PlatformEffect,
    /// An obsolete native window binding has not finished retiring.
    WindowRetirement,
    /// A native focus request awaits dispatch or its exact callback.
    Focus,
    /// Hit-test publication or input-control work remains unsettled.
    InputRouting,
    /// A stationary pointer observation or capture operation remains outstanding.
    PointerObservation,
}

impl NativeWaitReason {
    const ALL: [Self; 15] = [
        Self::Service,
        Self::Stopped,
        Self::RootPresentation,
        Self::ApplicationAction,
        Self::LocalAction,
        Self::WindowCreation,
        Self::Presentation,
        Self::HostEvents,
        Self::LifecycleOutput,
        Self::WindowTransition,
        Self::PlatformEffect,
        Self::WindowRetirement,
        Self::Focus,
        Self::InputRouting,
        Self::PointerObservation,
    ];

    const fn bit(self) -> u16 {
        1 << self as u16
    }
}

/// Fixed-size snapshot of all current native wait reasons.
///
/// Reading it does not consume work or retain history. Ordinary stable repaint
/// outputs do not make the runtime busy. Debug formatting lists active reasons.
#[derive(Clone, Copy, Default, PartialEq, Eq, Hash)]
pub struct NativePendingWork {
    reasons: u16,
}

impl NativePendingWork {
    /// Returns whether no work prevents ordinary runtime quiescence.
    #[must_use]
    pub const fn is_empty(self) -> bool {
        self.reasons == 0
    }

    /// Returns whether this snapshot includes one wait reason.
    #[must_use]
    pub const fn contains(self, reason: NativeWaitReason) -> bool {
        self.reasons & reason.bit() != 0
    }

    /// Iterates active reasons without allocating or querying the runtime again.
    pub fn iter(self) -> impl Iterator<Item = NativeWaitReason> {
        NativeWaitReason::ALL
            .into_iter()
            .filter(move |reason| self.contains(*reason))
    }

    pub(crate) fn include(&mut self, reason: NativeWaitReason, pending: bool) {
        if pending {
            self.reasons |= reason.bit();
        }
    }
}

impl std::fmt::Debug for NativePendingWork {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.debug_set().entries(self.iter()).finish()
    }
}
