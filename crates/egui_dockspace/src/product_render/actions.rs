//! Translation from current-pass egui responses into opaque core actions.

use dockspace::geometry::LogicalPoint;
use dockspace::runtime::SurfaceGesturePhase;
use egui::accesskit::Action;
use egui::{Event, Key, PointerButton, Pos2, Response, Ui};

use super::PointerActionAuthority;
use super::geometry::logical_point;

#[derive(Debug, Clone, Copy)]
pub(super) struct LocalPrimaryPress {
    ordinal: usize,
    position: Pos2,
    point: LogicalPoint,
}

impl LocalPrimaryPress {
    pub(super) const fn ordinal(self) -> usize {
        self.ordinal
    }

    pub(super) const fn position(self) -> Pos2 {
        self.position
    }

    pub(super) const fn point(self) -> LogicalPoint {
        self.point
    }
}

pub(super) fn effective_enabled(ui: &Ui, enabled: bool) -> bool {
    enabled && ui.is_enabled()
}

pub(super) fn button_activated(
    ui: &Ui,
    response: &Response,
    pointer_authority: PointerActionAuthority,
) -> bool {
    if !effective_enabled(ui, response.enabled()) {
        return false;
    }
    let keyboard = response.has_focus()
        && ui.input_mut(|input| {
            input.consume_key(egui::Modifiers::NONE, Key::Enter)
                || input.consume_key(egui::Modifiers::NONE, Key::Space)
        });
    let accesskit =
        ui.input(|input| input.has_accesskit_action_request(response.id, Action::Click));
    let pointer = pointer_authority.accepts_local_pointer_actions()
        && response.clicked_by(PointerButton::Primary);
    pointer || keyboard || accesskit
}

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

pub(super) fn local_primary_presses(
    ui: &Ui,
    pointer_authority: PointerActionAuthority,
) -> Vec<LocalPrimaryPress> {
    if !effective_enabled(ui, pointer_authority.accepts_local_pointer_actions()) {
        return Vec::new();
    }
    ui.input(|input| {
        input
            .events
            .iter()
            .enumerate()
            .filter_map(|(ordinal, event)| match event {
                Event::PointerButton {
                    pos,
                    button: PointerButton::Primary,
                    pressed: true,
                    ..
                } => Some(LocalPrimaryPress {
                    ordinal,
                    position: *pos,
                    point: logical_point(*pos)?,
                }),
                _ => None,
            })
            .collect()
    })
}
