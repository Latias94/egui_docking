//! Exact half-open hit geometry at the egui boundary.

use egui::accesskit::Action;
use egui::{Key, PointerButton, Pos2, Rect, Response, Ui, pos2};

/// Tests one point using the same half-open maximum edges as `dockspace`.
pub(crate) fn contains_half_open(rect: Rect, point: Pos2) -> bool {
    rect.is_positive()
        && point.x >= rect.min.x
        && point.x < rect.max.x
        && point.y >= rect.min.y
        && point.y < rect.max.y
}

/// Converts a continuous half-open rectangle to egui's inclusive discrete hit domain.
///
/// Painting and accessibility retain the original rectangle. Only widget hit
/// registration uses the greatest representable coordinates below each maximum.
pub(crate) fn interact_rect(rect: Rect) -> Rect {
    if !rect.is_positive() {
        return Rect::NOTHING;
    }

    Rect::from_min_max(
        rect.min,
        pos2(rect.max.x.next_down(), rect.max.y.next_down()),
    )
}

/// Separates geometry-derived pointer activation from stable widget identity input.
pub(crate) fn semantic_activation(
    ui: &Ui,
    response: &Response,
    rect: Rect,
    interactions_current: bool,
) -> bool {
    interactions_current
        && response.clicked_by(PointerButton::Primary)
        && response
            .interact_pointer_pos()
            .is_some_and(|point| contains_half_open(rect, point))
        || response.has_focus()
            && ui.input(|input| input.key_pressed(Key::Enter) || input.key_pressed(Key::Space))
        || ui.input(|input| input.has_accesskit_action_request(response.id, Action::Click))
}

/// Accepts an exact accessibility focus request independently of stale geometry.
pub(crate) fn accesskit_focus_requested(ui: &Ui, response: &Response) -> bool {
    ui.input(|input| input.has_accesskit_action_request(response.id, Action::Focus))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn half_open_membership_includes_minimum_and_excludes_maximum() {
        let rect = Rect::from_min_max(pos2(-5.0, 3.0), pos2(8.0, 11.0));

        assert!(contains_half_open(rect, rect.min));
        assert!(contains_half_open(
            rect,
            pos2(rect.max.x.next_down(), rect.max.y.next_down())
        ));
        assert!(!contains_half_open(rect, pos2(rect.max.x, rect.min.y)));
        assert!(!contains_half_open(rect, pos2(rect.min.x, rect.max.y)));
        assert!(!contains_half_open(rect, rect.max));
    }

    #[test]
    fn egui_interaction_rect_has_the_exact_discrete_half_open_domain() {
        let rect = Rect::from_min_max(pos2(0.0, 0.0), pos2(120.0, 90.0));
        let hit = interact_rect(rect);

        assert_eq!(hit.min, rect.min);
        assert_eq!(hit.max.x.to_bits(), rect.max.x.next_down().to_bits());
        assert_eq!(hit.max.y.to_bits(), rect.max.y.next_down().to_bits());
        assert!(hit.contains(hit.max));
        assert!(!hit.contains(rect.max));
    }
}
