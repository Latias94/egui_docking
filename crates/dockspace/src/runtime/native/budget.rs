//! Cooperative record-boundary limits for native input processing.

use std::fmt;
use std::sync::Arc;
use std::time::{Duration, Instant};

const MAX_RECORDS: usize = 256;
const MAX_DURATION: Duration = Duration::from_millis(2);

/// A cloneable clock whose source returns nondecreasing durations.
#[derive(Clone)]
pub struct NativeMonotonicClock {
    source: Arc<dyn Fn() -> Duration + Send + Sync>,
}

impl fmt::Debug for NativeMonotonicClock {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("NativeMonotonicClock")
            .finish_non_exhaustive()
    }
}

impl NativeMonotonicClock {
    /// Creates a clock from a source that must return nondecreasing durations.
    #[must_use]
    pub fn new(source: impl Fn() -> Duration + Send + Sync + 'static) -> Self {
        Self {
            source: Arc::new(source),
        }
    }

    /// Creates a system clock whose clones share one captured instant origin.
    #[must_use]
    pub fn system() -> Self {
        let origin = Instant::now();
        Self::new(move || origin.elapsed())
    }

    /// Returns the current duration from the shared clock source.
    #[must_use]
    pub fn now(&self) -> Duration {
        (self.source)()
    }
}

/// A cooperative budget of 256 completed records or two elapsed milliseconds.
///
/// Checks happen at record boundaries, so one record may run past the deadline.
/// This is not a hard real-time limit.
#[derive(Debug)]
pub struct NativeInputBudget {
    clock: NativeMonotonicClock,
    started_at: Duration,
    deadline: Duration,
    completed_records: usize,
}

impl NativeInputBudget {
    /// Starts the budget immediately; call before claiming an input prefix.
    #[must_use]
    pub fn new(clock: &NativeMonotonicClock) -> Self {
        let started_at = clock.now();
        Self {
            clock: clock.clone(),
            started_at,
            deadline: started_at.saturating_add(MAX_DURATION),
            completed_records: 0,
        }
    }

    /// Returns whether the record limit or inclusive deadline has been reached.
    #[must_use]
    pub fn is_exhausted(&self) -> bool {
        self.completed_records >= MAX_RECORDS || self.clock.now() >= self.deadline
    }

    /// Returns the remaining record allowance, regardless of elapsed time.
    #[must_use]
    pub fn remaining_records(&self) -> usize {
        MAX_RECORDS.saturating_sub(self.completed_records)
    }

    /// Returns the time elapsed since construction using the current clock value.
    #[must_use]
    pub fn elapsed(&self) -> Duration {
        self.clock.now().saturating_sub(self.started_at)
    }

    /// Counts one completed record without overflowing, even after exhaustion.
    pub(crate) fn complete_record(&mut self) {
        self.completed_records = self.completed_records.saturating_add(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    fn clock_at(origin: Duration) -> (NativeMonotonicClock, Arc<AtomicU64>) {
        let nanos = Arc::new(AtomicU64::new(0));
        let source = Arc::clone(&nanos);
        let clock = NativeMonotonicClock::new(move || {
            origin.saturating_add(Duration::from_nanos(source.load(Ordering::SeqCst)))
        });
        (clock, nanos)
    }

    #[test]
    fn deadline_is_inclusive_and_starts_at_construction() {
        let (clock, nanos) = clock_at(Duration::from_secs(7));
        nanos.store(100, Ordering::SeqCst);
        let budget = NativeInputBudget::new(&clock);
        assert_eq!(budget.elapsed(), Duration::ZERO);
        assert!(!budget.is_exhausted());

        nanos.store(2_000_099, Ordering::SeqCst);
        assert!(!budget.is_exhausted());
        assert_eq!(budget.elapsed(), MAX_DURATION - Duration::from_nanos(1));

        nanos.store(2_000_100, Ordering::SeqCst);
        assert!(budget.is_exhausted());
        assert_eq!(budget.elapsed(), MAX_DURATION);
        assert_eq!(budget.remaining_records(), 256);
    }

    #[test]
    fn record_256_exhausts_the_budget() {
        let (clock, _) = clock_at(Duration::ZERO);
        let mut budget = NativeInputBudget::new(&clock);
        for completed in 0..256 {
            assert!(!budget.is_exhausted());
            assert_eq!(budget.remaining_records(), 256 - completed);
            budget.complete_record();
        }
        assert!(budget.is_exhausted());
        assert_eq!(budget.remaining_records(), 0);
        assert_eq!(budget.elapsed(), Duration::ZERO);

        budget.complete_record();
        assert!(budget.is_exhausted());
        assert_eq!(budget.remaining_records(), 0);
    }

    #[test]
    fn completed_records_saturate_without_wrapping() {
        let (clock, _) = clock_at(Duration::ZERO);
        let mut budget = NativeInputBudget::new(&clock);
        budget.completed_records = usize::MAX;
        budget.complete_record();
        assert_eq!(budget.completed_records, usize::MAX);
        assert!(budget.is_exhausted());
        assert_eq!(budget.remaining_records(), 0);
    }

    #[test]
    fn clock_clones_share_the_source() {
        let (clock, nanos) = clock_at(Duration::from_secs(3));
        let cloned = clock.clone();
        assert!(Arc::ptr_eq(&clock.source, &cloned.source));
        for value in [1, 42, 2_000_000] {
            nanos.store(value, Ordering::SeqCst);
            let expected = Duration::from_secs(3) + Duration::from_nanos(value);
            assert_eq!(clock.now(), expected);
            assert_eq!(cloned.now(), expected);
        }
    }

    #[test]
    fn system_clock_clones_share_the_origin() {
        let clock = NativeMonotonicClock::system();
        let cloned = clock.clone();
        assert!(Arc::ptr_eq(&clock.source, &cloned.source));
        let before = clock.now();
        assert!(cloned.now() >= before);
    }

    #[test]
    fn resolver_time_is_visible_before_record_completion() {
        let (clock, nanos) = clock_at(Duration::ZERO);
        let mut budget = NativeInputBudget::new(&clock);
        assert!(!budget.is_exhausted());

        let resolve_record = || nanos.store(3_000_000, Ordering::SeqCst);
        resolve_record();
        assert!(budget.is_exhausted());
        assert_eq!(budget.elapsed(), Duration::from_millis(3));
        assert_eq!(budget.remaining_records(), 256);

        budget.complete_record();
        assert_eq!(budget.remaining_records(), 255);
        assert!(budget.is_exhausted());
    }

    #[test]
    fn deadline_saturates_near_duration_max() {
        let (clock, nanos) = clock_at(Duration::MAX - Duration::from_nanos(1));
        let budget = NativeInputBudget::new(&clock);
        assert_eq!(budget.deadline, Duration::MAX);
        assert!(!budget.is_exhausted());

        nanos.store(1, Ordering::SeqCst);
        assert!(budget.is_exhausted());
        assert_eq!(budget.elapsed(), Duration::from_nanos(1));
    }
}
