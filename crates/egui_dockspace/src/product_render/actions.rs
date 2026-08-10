//! Translation from current-pass egui responses into opaque core actions.

use dockspace::runtime::SurfaceGesturePhase;
use egui::{PointerButton, Response};

use super::geometry::logical_point;

pub(crate) fn gesture_phase(response: &Response) -> Option<SurfaceGesturePhase> {
    let current = response.interact_pointer_pos().and_then(logical_point);
    if response.drag_started_by(PointerButton::Primary) {
        let current = current?;
        let initial = response
            .interact_pointer_pos()
            .map(|position| position - response.total_drag_delta().unwrap_or_default())
            .and_then(logical_point)?;
        return Some(SurfaceGesturePhase::Begin { initial, current });
    }
    if response.drag_stopped_by(PointerButton::Primary) {
        return Some(current.map_or(SurfaceGesturePhase::Cancel, |current| {
            SurfaceGesturePhase::Release { current }
        }));
    }
    response
        .dragged_by(PointerButton::Primary)
        .then_some(current)
        .flatten()
        .map(|current| SurfaceGesturePhase::Move { current })
}
