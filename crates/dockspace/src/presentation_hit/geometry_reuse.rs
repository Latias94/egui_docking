//! Exact replay of geometry, independent of the output capability that binds it.

use super::*;

#[cfg(test)]
#[path = "geometry_reuse_tests.rs"]
mod tests;

#[derive(Debug)]
pub(super) struct HitGeometry {
    pub(super) regions: Box<[PresentationHitRegion]>,
    // Fragment rows refer only to this immutable region arena.
    pub(super) touch_regions: Box<[(usize, LogicalRect)]>,
    // A private replay hint, unavailable after compaction removes any row.
    pub(super) emission_rows: Option<Box<[usize]>>,
    // A spatial leaf permutation, independent from all output authority.
    pub(super) touch_order: Box<[usize]>,
}

impl PartialEq for HitGeometry {
    fn eq(&self, other: &Self) -> bool {
        self.regions == other.regions && self.touch_regions == other.touch_regions
    }
}

pub(super) fn rect_bits(rect: LogicalRect) -> [u64; 4] {
    [rect.x(), rect.y(), rect.max().x(), rect.max().y()].map(f64::to_bits)
}

fn same_region(left: &PresentationHitRegion, right: &PresentationHitRegion) -> bool {
    left.id == right.id
        && rect_bits(left.hit.rect()) == rect_bits(right.hit.rect())
        && rect_bits(left.anchor) == rect_bits(right.anchor)
        && left.lanes == right.lanes
        && left.stack == right.stack
        && left.behavior == right.behavior
}

impl PresentationHitManifest {
    pub(crate) fn compile_reusing(
        output: SurfacePresentationOutputTicket,
        plan: &PresentationPlan,
        previous: Option<(&PresentationPlan, &Self)>,
    ) -> Result<Self, SceneBuildError> {
        debug_assert_eq!(output.surface(), plan.surface());
        if let Some((previous_plan, previous)) = previous
            && let Some(emission_rows) = &previous.geometry.emission_rows
            // Resize is common and must reject before traversing either inventory.
            && rect_bits(plan.bounds()) == rect_bits(previous_plan.bounds())
            && touch::same_context(plan, previous_plan)
        {
            let mut accepted_count = 0;
            let mut identical = true;
            emit_regions(plan, &mut |region| {
                identical &= emission_rows
                    .get(accepted_count)
                    .and_then(|&row| previous.geometry.regions.get(row))
                    .is_some_and(|previous| same_region(&region, previous));
                accepted_count += 1;
            })?;
            if identical && accepted_count == emission_rows.len() {
                return Ok(Self {
                    output,
                    geometry: Arc::clone(&previous.geometry),
                });
            }
        }
        Self::compile_with_order(
            output,
            plan,
            previous.map(|(_, manifest)| &*manifest.geometry),
        )
    }
}
