//! Compact exact membership for terminal presentation-host identities.

use std::collections::BTreeMap;

use super::PresentationHostSerial;

/// Disjoint inclusive serial ranges for hosts whose detailed terminal state was reclaimed.
///
/// Every serial comes from the monotonic core allocator. Adjacent retirements therefore collapse
/// without weakening stale-capability rejection, while live serials remain exact holes.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(super) struct RetiredPresentationHostRanges {
    intervals: BTreeMap<PresentationHostSerial, PresentationHostSerial>,
}

impl RetiredPresentationHostRanges {
    pub(super) const fn new() -> Self {
        Self {
            intervals: BTreeMap::new(),
        }
    }

    pub(super) fn contains(&self, serial: PresentationHostSerial) -> bool {
        self.intervals
            .range(..=serial)
            .next_back()
            .is_some_and(|(_, end)| serial <= *end)
    }

    /// Inserts one exact serial and merges both adjacent intervals when possible.
    ///
    /// Returns `true` only when the serial was not already represented.
    pub(super) fn insert(&mut self, serial: PresentationHostSerial) -> bool {
        if self.contains(serial) {
            return false;
        }

        let predecessor = self
            .intervals
            .range(..serial)
            .next_back()
            .map(|(start, end)| (*start, *end));
        let successor = self
            .intervals
            .range(serial..)
            .next()
            .map(|(start, end)| (*start, *end));
        let joins_predecessor =
            predecessor.is_some_and(|(_, end)| end.checked_next() == Some(serial));
        let joins_successor =
            successor.is_some_and(|(start, _)| serial.checked_next() == Some(start));

        match (predecessor, successor, joins_predecessor, joins_successor) {
            (Some((left_start, _)), Some((right_start, right_end)), true, true) => {
                self.intervals.insert(left_start, right_end);
                self.intervals.remove(&right_start);
            }
            (Some((left_start, _)), _, true, false) => {
                self.intervals.insert(left_start, serial);
            }
            (_, Some((right_start, right_end)), false, true) => {
                self.intervals.remove(&right_start);
                self.intervals.insert(serial, right_end);
            }
            _ => {
                self.intervals.insert(serial, serial);
            }
        }
        true
    }

    pub(super) fn interval_count(&self) -> usize {
        self.intervals.len()
    }

    pub(super) fn logical_host_count(&self) -> u64 {
        self.intervals
            .iter()
            .map(|(start, end)| end.0 - start.0 + 1)
            .sum()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn serial(value: u64) -> PresentationHostSerial {
        PresentationHostSerial(value)
    }

    #[test]
    fn adjacent_and_bridging_serials_merge_into_one_interval() {
        let mut ranges = RetiredPresentationHostRanges::default();

        assert!(ranges.insert(serial(2)));
        assert!(ranges.insert(serial(4)));
        assert_eq!(ranges.interval_count(), 2);
        assert!(ranges.insert(serial(3)));
        assert_eq!(ranges.interval_count(), 1);
        assert!(ranges.insert(serial(1)));
        assert_eq!(ranges.interval_count(), 1);
        assert_eq!(ranges.logical_host_count(), 4);
        assert!(!ranges.insert(serial(3)));
        assert_eq!(ranges.logical_host_count(), 4);
        assert!((1..=4).all(|value| ranges.contains(serial(value))));
        assert!(!ranges.contains(serial(5)));
    }

    #[test]
    fn live_serial_holes_remain_exact() {
        let mut ranges = RetiredPresentationHostRanges::default();

        for value in 2..=10_000 {
            assert!(ranges.insert(serial(value)));
        }

        assert_eq!(ranges.interval_count(), 1);
        assert_eq!(ranges.logical_host_count(), 9_999);
        assert!(!ranges.contains(serial(1)));
        assert!(ranges.contains(serial(10_000)));
    }
}
