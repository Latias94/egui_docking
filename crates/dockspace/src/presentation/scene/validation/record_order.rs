//! One row order for duplicate detection, semantic joins and final publication.

pub(super) struct RecordOrder<K> {
    entries: Vec<(K, usize)>,
}

impl<K: Copy + Ord> RecordOrder<K> {
    pub(super) fn new(keys: impl IntoIterator<Item = K>) -> Result<Self, K> {
        let mut entries = keys
            .into_iter()
            .enumerate()
            .map(|(row, key)| (key, row))
            .collect::<Vec<_>>();
        entries.sort_unstable_by_key(|entry| entry.0);
        if let Some(key) = entries
            .windows(2)
            .find_map(|pair| (pair[0].0 == pair[1].0).then_some(pair[0].0))
        {
            return Err(key);
        }
        Ok(Self { entries })
    }

    pub(super) fn entries(&self) -> &[(K, usize)] {
        &self.entries
    }

    pub(super) fn get<'a, T>(&self, records: &'a [T], key: K) -> Option<&'a T> {
        self.entries
            .binary_search_by_key(&key, |entry| entry.0)
            .ok()
            .map(|index| &records[self.entries[index].1])
    }

    pub(super) fn iter<'a, T>(
        &'a self,
        records: &'a [T],
    ) -> impl DoubleEndedIterator<Item = (K, &'a T)> + ExactSizeIterator {
        self.entries.iter().map(|&(key, row)| (key, &records[row]))
    }

    pub(super) fn apply<T>(mut self, records: &mut [T]) {
        debug_assert_eq!(records.len(), self.entries.len());
        // Validation keeps input order until success. Consume its row order
        // without sorting the records again or allocating another permutation.
        for start in 0..self.entries.len() {
            let mut current = start;
            while self.entries[current].1 != start {
                let source = self.entries[current].1;
                records.swap(current, source);
                self.entries[current].1 = current;
                current = source;
            }
            self.entries[current].1 = current;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    proptest! {
        #[test]
        fn row_order_matches_sorted_records_and_smallest_duplicate(
            keys in prop::collection::vec(any::<u16>(), 0..256),
        ) {
            let mut sorted = keys.clone();
            sorted.sort_unstable();
            let duplicate = sorted.windows(2)
                .find_map(|pair| (pair[0] == pair[1]).then_some(pair[0]));
            match (RecordOrder::new(keys.iter().copied()), duplicate) {
                (Err(actual), Some(expected)) => prop_assert_eq!(actual, expected),
                (Ok(order), None) => {
                    let records = keys.iter().copied().enumerate()
                        .map(|(row, key)| (key, row)).collect::<Vec<_>>();
                    for &(key, row) in &records {
                        prop_assert_eq!(order.get(&records, key), Some(&(key, row)));
                    }
                    let mut expected = records.clone();
                    expected.sort_unstable_by_key(|record| record.0);
                    let mut actual = records;
                    order.apply(&mut actual);
                    prop_assert_eq!(actual, expected);
                }
                _ => prop_assert!(false, "row directory and sorted duplicate oracle disagree"),
            }
        }
    }
}
