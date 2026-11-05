//! Canonical hit ordering with small sort keys and linear record movement.

use super::{PresentationHitRegion, geometry_reuse::HitGeometry};

#[cfg(test)]
mod order_tests;

pub(super) fn canonicalize_reusing_order(
    regions: &mut [PresentationHitRegion],
    previous: Option<&HitGeometry>,
) -> Box<[usize]> {
    if let Some(previous) = previous
        && let Some(emission_rows) = &previous.emission_rows
        && emission_rows.len() == regions.len()
        && regions.iter().zip(emission_rows).all(|(region, &row)| {
            previous
                .regions
                .get(row)
                .is_some_and(|old| region.id == old.id)
        })
    {
        // The exact identity replay proves the old permutation still sorts
        // this new inventory. Geometry and touch fragments are rebuilt below.
        let mut sources = vec![0; regions.len()];
        for (emission, &canonical) in emission_rows.iter().enumerate() {
            sources[canonical] = emission;
        }
        permute_regions(regions, &mut sources);
        return emission_rows.clone();
    }
    canonicalize_regions(regions)
}

pub(super) fn canonicalize_regions(regions: &mut [PresentationHitRegion]) -> Box<[usize]> {
    if regions.is_sorted_by(|left, right| left.id <= right.id) {
        return (0..regions.len()).collect();
    }

    let mut sources = (0..regions.len()).collect::<Vec<_>>();
    sources.sort_unstable_by(|&left, &right| regions[left].id.cmp(&regions[right].id));
    let mut emission_rows = vec![0; regions.len()];
    for (canonical, &emission) in sources.iter().enumerate() {
        emission_rows[emission] = canonical;
    }

    permute_regions(regions, &mut sources);
    emission_rows.into_boxed_slice()
}

fn permute_regions(regions: &mut [PresentationHitRegion], sources: &mut [usize]) {
    // sources[new_row] names its original row. Following that direction fixes
    // each row before advancing; the cycle's last row already holds its start.
    // Mark visited rows in this scratch permutation; the replay inverse stays intact.
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
