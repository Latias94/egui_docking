//! Static executability of one immutable presentation plan and hit manifest.

use crate::geometry::{LogicalPoint, LogicalRect};
use crate::presentation_hit::{PresentationHitManifest, PresentationHitRegionKind};

use super::{PresentationPlan, ResolvedSurfaceProgramOperation, ResolvedSurfaceProgramStep};

/// Rows are private to the exact immutable hit manifest installed beside this link.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct SurfacePaintProgramLink {
    targets: Box<[(usize, LogicalPoint)]>,
}

impl SurfacePaintProgramLink {
    pub(crate) fn targets(&self) -> impl Iterator<Item = (usize, LogicalPoint)> + '_ {
        self.targets.iter().copied()
    }
}

#[cfg(test)]
pub(crate) fn validate_surface_paint_program(
    plan: &PresentationPlan,
    hit_manifest: &PresentationHitManifest,
) -> bool {
    link_surface_paint_program(plan, hit_manifest).is_some()
}

pub(crate) fn link_surface_paint_program(
    plan: &PresentationPlan,
    hit_manifest: &PresentationHitManifest,
) -> Option<SurfacePaintProgramLink> {
    let mut targets = Vec::new();
    plan.visit_surface_program(|step| {
        match step {
            ResolvedSurfaceProgramStep::Operation {
                operation: ResolvedSurfaceProgramOperation::PresentationMenuAnchor(anchor),
                ..
            } => {
                anchor.ready_bounds().ok_or(())?;
            }
            ResolvedSurfaceProgramStep::PopupMenu(menu) => {
                plan.tab_list_menu_backdrop_records()
                    .iter()
                    .any(|backdrop| backdrop.session() == menu.session())
                    .then_some(())
                    .ok_or(())?;
            }
            ResolvedSurfaceProgramStep::DropTarget(target) => {
                let row = hit_manifest
                    .region_index_for_kind(PresentationHitRegionKind::DropTarget(target.id()))
                    .ok_or(())?;
                let region = &hit_manifest.regions()[row];
                if region.is_passive() {
                    return Err(());
                }
                // DropTarget always has a receiver role. Match the remaining
                // descriptor-construction failure before any sink sees a step.
                let center = paint_receiver_center(region.hit().rect()).ok_or(())?;
                targets.push((row, center));
            }
            _ => {}
        }
        Ok::<(), ()>(())
    })
    .ok()?;
    Some(SurfacePaintProgramLink {
        targets: targets.into_boxed_slice(),
    })
}

pub(crate) fn paint_receiver_center(bounds: LogicalRect) -> Option<LogicalPoint> {
    LogicalPoint::new(
        bounds.x() + bounds.width() * 0.5,
        bounds.y() + bounds.height() * 0.5,
    )
    .ok()
}
