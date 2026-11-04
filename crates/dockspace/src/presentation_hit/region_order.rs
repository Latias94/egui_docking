//! Canonical hit ordering with small sort keys and linear record movement.

use super::PresentationHitRegion;

#[cfg(test)]
mod order_tests;

pub(super) fn canonicalize_regions(regions: &mut [PresentationHitRegion]) {
    if regions.is_sorted_by(|left, right| left.id <= right.id) {
        return;
    }

    let mut sources = (0..regions.len()).collect::<Vec<_>>();
    sources.sort_unstable_by(|&left, &right| regions[left].id.cmp(&regions[right].id));

    // sources[new_row] names its original row. Following that direction fixes
    // each row before advancing; the cycle's last row already holds its start.
    // Marking visited rows in this same permutation needs no second inventory.
    for start in 0..sources.len() {
        let mut current = start;
        while sources[current] != start {
            let source = sources[current];
            regions.swap(current, source);
            sources[current] = current;
            current = source;
        }
        sources[current] = current;
    }
}
