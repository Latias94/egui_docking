//! Fixed-size public projection of exact native lifecycle progress.

/// Monotonic lifecycle milestones observed by the native coordinator.
///
/// The counters expose only committed product facts. They are not an event log,
/// callback API, or substitute for inspecting the current dockspace view.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct NativeLifecycleProgress {
    first_live_admissions: u64,
    destroyed_observations: u64,
    quiescent_retirements: u64,
}

impl NativeLifecycleProgress {
    /// Returns how many exact child first-live admissions committed.
    #[must_use]
    pub const fn first_live_admissions(self) -> u64 {
        self.first_live_admissions
    }

    /// Returns how many exact native-window destruction observations committed.
    #[must_use]
    pub const fn destroyed_observations(self) -> u64 {
        self.destroyed_observations
    }

    /// Returns how many retired bindings reached core-confirmed quiescence.
    #[must_use]
    pub const fn quiescent_retirements(self) -> u64 {
        self.quiescent_retirements
    }

    pub(crate) fn record_first_live_admission(&mut self) {
        increment_exact(
            &mut self.first_live_admissions,
            "native first-live progress exhausted",
        );
    }

    pub(crate) fn record_destroyed_observation(&mut self) {
        increment_exact(
            &mut self.destroyed_observations,
            "native destruction progress exhausted",
        );
    }

    pub(crate) fn record_quiescent_retirement(&mut self) {
        increment_exact(
            &mut self.quiescent_retirements,
            "native quiescence progress exhausted",
        );
    }
}

fn increment_exact(counter: &mut u64, exhausted: &str) {
    *counter = counter
        .checked_add(1)
        .unwrap_or_else(|| panic!("{exhausted}"));
}
