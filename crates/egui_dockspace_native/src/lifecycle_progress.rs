//! Fixed-size projections of native lifecycle progress and output ownership.

/// Current retained native output owners, without tokens or lifecycle authority.
///
/// Counts occupancy rather than allocated capacity or completed output history.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct NativeOutputBacklog {
    ordinary: usize,
    control: usize,
    protected_staging: usize,
    awaiting_staging: usize,
}

impl NativeOutputBacklog {
    pub(crate) const fn new(
        ordinary: usize,
        control: usize,
        protected_staging: usize,
        awaiting_staging: usize,
    ) -> Self {
        Self {
            ordinary,
            control,
            protected_staging,
            awaiting_staging,
        }
    }

    /// Returns the number of retained ordinary output owners.
    #[must_use]
    pub const fn ordinary(self) -> usize {
        self.ordinary
    }

    /// Returns retained root and child control owners, including protected staging.
    #[must_use]
    pub const fn control(self) -> usize {
        self.control
    }

    /// Returns protected child staging owners, a subset of the control backlog.
    #[must_use]
    pub const fn protected_staging(self) -> usize {
        self.protected_staging
    }

    /// Returns existing staging requests still waiting for their first paint callback.
    ///
    /// Requests are distinct from retained output owners and confer no paint authority.
    #[must_use]
    pub const fn awaiting_staging(self) -> usize {
        self.awaiting_staging
    }
}

/// Bounded, read-only native close progress without window identities or history.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct NativeCloseProgress {
    requests_retained: u64,
    cancellations_committed: u64,
}

impl NativeCloseProgress {
    pub(crate) fn record_request(&mut self) {
        self.requests_retained = self.requests_retained.saturating_add(1);
    }

    pub(crate) fn record_cancellations(&mut self, count: usize) {
        self.cancellations_committed = self
            .cancellations_committed
            .saturating_add(u64::try_from(count).unwrap_or(u64::MAX));
    }

    /// Returns how many native close requests were successfully retained at ingress.
    ///
    /// Retention does not imply a committed close-policy transition.
    #[must_use]
    pub const fn requests_retained(self) -> u64 {
        self.requests_retained
    }

    /// Returns how many pending close bindings were cleared by committed core transitions.
    #[must_use]
    pub const fn cancellations_committed(self) -> u64 {
        self.cancellations_committed
    }
}

/// Monotonic lifecycle milestones observed by the native coordinator.
///
/// The counters expose only committed product facts. They are not an event log,
/// callback API, or substitute for inspecting the current dockspace view.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub(crate) struct NativeLifecycleProgress {
    first_live_admissions: u64,
    destroyed_observations: u64,
    quiescent_retirements: u64,
}

impl NativeLifecycleProgress {
    /// Returns how many exact child first-live admissions committed.
    #[cfg(test)]
    #[must_use]
    pub(crate) const fn first_live_admissions(self) -> u64 {
        self.first_live_admissions
    }

    /// Returns how many exact native-window destruction observations committed.
    #[cfg(test)]
    #[must_use]
    pub(crate) const fn destroyed_observations(self) -> u64 {
        self.destroyed_observations
    }

    /// Returns how many retired bindings reached core-confirmed quiescence.
    #[cfg(test)]
    #[must_use]
    pub(crate) const fn quiescent_retirements(self) -> u64 {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn close_progress_distinguishes_retained_requests_from_committed_cancellations() {
        let mut progress = NativeCloseProgress::default();
        let baseline = progress;
        assert_eq!(baseline.requests_retained(), 0);
        assert_eq!(baseline.cancellations_committed(), 0);

        progress.record_request();
        assert_eq!(progress.requests_retained(), 1);
        assert_eq!(progress.cancellations_committed(), 0);
        progress.record_cancellations(0);
        assert_eq!(progress.cancellations_committed(), 0);
        progress.record_cancellations(2);
        assert_eq!(progress.requests_retained(), 1);
        assert_eq!(progress.cancellations_committed(), 2);
        assert_eq!(baseline, NativeCloseProgress::default());
    }

    #[test]
    fn close_progress_saturates_without_wrapping() {
        let mut progress = NativeCloseProgress {
            requests_retained: u64::MAX - 1,
            cancellations_committed: u64::MAX - 1,
        };
        progress.record_request();
        progress.record_request();
        progress.record_cancellations(2);
        progress.record_cancellations(usize::MAX);
        assert_eq!(progress.requests_retained(), u64::MAX);
        assert_eq!(progress.cancellations_committed(), u64::MAX);
        assert_eq!(std::mem::size_of::<NativeCloseProgress>(), 16);
    }
}
