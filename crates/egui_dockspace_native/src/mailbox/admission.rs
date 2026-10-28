//! Fixed overload admission boundary, independent of fatal and shutdown modes.

use std::time::Duration;

pub(super) const RECORD_LIMIT: usize = 4_096;
pub(super) const AGE_LIMIT: Duration = Duration::from_millis(250);

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) enum NativeAdmission {
    #[default]
    Running,
    Draining {
        boundary: u64,
    },
    Recovering {
        boundary: u64,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum NativeDispatch {
    ContinueBoundary,
    Replay { through: Option<u64> },
    Cancel { boundary: u64 },
    Idle,
}

impl NativeAdmission {
    /// Freezes the accepted prefix once, retaining B throughout recovery.
    pub(crate) fn freeze(&mut self, captured_through: u64) -> u64 {
        match *self {
            Self::Running => {
                *self = Self::Draining {
                    boundary: captured_through,
                };
                captured_through
            }
            Self::Draining { boundary } | Self::Recovering { boundary } => boundary,
        }
    }

    /// Selects work from the existing owners without waiting for parked records.
    ///
    /// The caller combines core commit and roster/clear settlement barriers in
    /// `boundary_pending`. Awaiting an external callback is not such a barrier.
    pub(crate) fn dispatch(
        &self,
        boundary_pending: bool,
        head_ordinal: Option<u64>,
    ) -> NativeDispatch {
        if boundary_pending {
            return NativeDispatch::ContinueBoundary;
        }
        match *self {
            Self::Draining { boundary } => {
                if head_ordinal.is_some_and(|ordinal| ordinal <= boundary) {
                    NativeDispatch::Replay {
                        through: Some(boundary),
                    }
                } else {
                    NativeDispatch::Cancel { boundary }
                }
            }
            Self::Running | Self::Recovering { .. } => {
                if head_ordinal.is_some() {
                    NativeDispatch::Replay { through: None }
                } else {
                    NativeDispatch::Idle
                }
            }
        }
    }

    /// Advances only after the exact cancellation has successfully committed.
    pub(crate) fn finish_cancellation(&mut self, boundary: u64) -> bool {
        if *self != (Self::Draining { boundary }) {
            return false;
        }
        *self = Self::Recovering { boundary };
        true
    }

    /// Resumes ordinary admission after source count and age recover.
    pub(crate) fn try_resume(
        &mut self,
        retained_count: usize,
        oldest_age: Option<Duration>,
    ) -> bool {
        if !matches!(self, Self::Recovering { .. })
            || retained_count >= RECORD_LIMIT
            || oldest_age.is_some_and(|age| age >= AGE_LIMIT)
        {
            return false;
        }
        *self = Self::Running;
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn freeze_keeps_the_first_boundary_through_recovery() {
        let mut admission = NativeAdmission::default();
        assert_eq!(admission.freeze(17), 17);
        assert_eq!(admission.freeze(23), 17);
        assert_eq!(admission, NativeAdmission::Draining { boundary: 17 });
        assert!(admission.finish_cancellation(17));
        assert_eq!(admission.freeze(29), 17);
        assert_eq!(admission, NativeAdmission::Recovering { boundary: 17 });
        assert!(admission.try_resume(0, None));
        assert_eq!(admission.freeze(31), 31);
        assert_eq!(admission, NativeAdmission::Draining { boundary: 31 });
    }

    #[test]
    fn draining_replays_only_the_ready_prefix_through_the_fixed_boundary() {
        let admission = NativeAdmission::Draining { boundary: 17 };
        for (head, expected) in [
            (Some(1), NativeDispatch::Replay { through: Some(17) }),
            (Some(17), NativeDispatch::Replay { through: Some(17) }),
            (Some(18), NativeDispatch::Cancel { boundary: 17 }),
            (Some(u64::MAX), NativeDispatch::Cancel { boundary: 17 }),
            (None, NativeDispatch::Cancel { boundary: 17 }),
        ] {
            assert_eq!(admission.dispatch(false, head), expected, "head={head:?}");
        }
        assert_eq!(
            NativeAdmission::Draining { boundary: 0 }.dispatch(false, Some(1)),
            NativeDispatch::Cancel { boundary: 0 },
        );
    }

    #[test]
    fn full_boundary_settlement_precedes_replay_and_cancellation_in_every_state() {
        for admission in [
            NativeAdmission::Running,
            NativeAdmission::Draining { boundary: 17 },
            NativeAdmission::Recovering { boundary: 17 },
        ] {
            for head in [None, Some(1), Some(17), Some(18)] {
                assert_eq!(
                    admission.dispatch(true, head),
                    NativeDispatch::ContinueBoundary,
                    "admission={admission:?}, head={head:?}",
                );
            }
        }
    }

    #[test]
    fn parked_reservations_do_not_delay_cancel_or_lose_late_terminal_replay() {
        let mut admission = NativeAdmission::Running;
        assert_eq!(admission.freeze(17), 17);
        assert_eq!(
            admission.dispatch(false, Some(17)),
            NativeDispatch::Replay { through: Some(17) },
        );
        // Retained owners may remain parked while the ready prefix is empty.
        assert_eq!(
            admission.dispatch(false, None),
            NativeDispatch::Cancel { boundary: 17 },
        );
        assert!(admission.finish_cancellation(17));
        assert_eq!(admission.dispatch(false, None), NativeDispatch::Idle);
        // The original parked ordinal and its post-B dependency use the journal.
        for head in [Some(9), Some(18)] {
            assert_eq!(
                admission.dispatch(false, head),
                NativeDispatch::Replay { through: None },
            );
        }
        assert_eq!(admission, NativeAdmission::Recovering { boundary: 17 });
    }

    #[test]
    fn cancellation_retries_until_the_exact_commit_and_finishes_once() {
        let mut admission = NativeAdmission::Draining { boundary: 17 };
        assert!(!admission.finish_cancellation(16));
        assert!(!admission.finish_cancellation(18));
        assert!(!admission.try_resume(0, None));
        // A dropped cancellation candidate leaves dispatch and admission intact.
        for _ in 0..2 {
            assert_eq!(
                admission.dispatch(false, Some(18)),
                NativeDispatch::Cancel { boundary: 17 },
            );
            assert_eq!(admission, NativeAdmission::Draining { boundary: 17 });
        }
        assert!(admission.finish_cancellation(17));
        assert!(!admission.finish_cancellation(17));
        assert_eq!(
            admission.dispatch(false, Some(18)),
            NativeDispatch::Replay { through: None },
        );
        assert!(admission.try_resume(0, None));
        assert!(!admission.finish_cancellation(17));
        assert_eq!(admission, NativeAdmission::Running);
    }

    #[test]
    fn source_recovery_requires_cancellation_and_both_measured_caps() {
        let mut admission = NativeAdmission::Draining { boundary: 17 };
        assert!(!admission.try_resume(0, None));
        assert!(admission.finish_cancellation(17));
        assert!(!admission.try_resume(RECORD_LIMIT, None));
        assert!(!admission.try_resume(0, Some(AGE_LIMIT)));
        assert_eq!(admission, NativeAdmission::Recovering { boundary: 17 });
        assert!(admission.try_resume(RECORD_LIMIT - 1, Some(AGE_LIMIT - Duration::from_nanos(1)),));
        assert_eq!(admission, NativeAdmission::Running);
        assert!(!admission.try_resume(0, None));
    }
}
