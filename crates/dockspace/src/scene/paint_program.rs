//! Static executability of one immutable presentation plan and hit manifest.

use crate::geometry::{LogicalPoint, LogicalRect};
use crate::presentation_hit::{PresentationHitManifest, PresentationHitRegionKind};

use super::{PresentationPlan, ResolvedSurfaceProgramOperation, ResolvedSurfaceProgramStep};

pub(crate) fn validate_surface_paint_program(
    plan: &PresentationPlan,
    hit_manifest: &PresentationHitManifest,
) -> bool {
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
                let region = hit_manifest
                    .region_for_kind(PresentationHitRegionKind::DropTarget(target.id()))
                    .ok_or(())?;
                if region.is_passive() {
                    return Err(());
                }
                // DropTarget always has a receiver role. Match the remaining
                // descriptor-construction failure before any sink sees a step.
                paint_receiver_center(region.hit().rect()).ok_or(())?;
            }
            _ => {}
        }
        Ok::<(), ()>(())
    })
    .is_ok()
}

pub(crate) fn paint_receiver_center(bounds: LogicalRect) -> Option<LogicalPoint> {
    LogicalPoint::new(
        bounds.x() + bounds.width() * 0.5,
        bounds.y() + bounds.height() * 0.5,
    )
    .ok()
}
