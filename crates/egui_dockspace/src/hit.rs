//! Exact half-open hit geometry at the egui boundary.

use egui::accesskit::Action;
use egui::{Key, PointerButton, Pos2, Rect, Response, Ui, pos2};

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum SemanticActivation {
    Pointer(Pos2),
    Semantic,
}

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
pub(crate) fn semantic_activation_fact(
    ui: &Ui,
    response: &Response,
    rect: Rect,
    pointer_interactions_current: bool,
    semantic_interactions_current: bool,
) -> Option<SemanticActivation> {
    if !semantic_interactions_current {
        return None;
    }

    if pointer_interactions_current
        && response.clicked_by(PointerButton::Primary)
        && let Some(point) = response
            .interact_pointer_pos()
            .filter(|point| contains_half_open(rect, *point))
    {
        return Some(SemanticActivation::Pointer(point));
    }
    // Keyboard and accessibility requests target an egui-stable widget identity,
    // rather than a pointer hit rectangle. They still require current core scene
    // authority: a retained widget identity must never authorize a graph mutation.
    if response.has_focus()
        && ui.input(|input| input.key_pressed(Key::Enter) || input.key_pressed(Key::Space))
        || ui.input(|input| input.has_accesskit_action_request(response.id, Action::Click))
    {
        return Some(SemanticActivation::Semantic);
    }
    None
}

/// Accepts an exact accessibility focus request independently of stale geometry.
pub(crate) fn accesskit_focus_requested(ui: &Ui, response: &Response) -> bool {
    ui.input(|input| input.has_accesskit_action_request(response.id, Action::Focus))
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui::{Event, Id, RawInput, vec2};

    fn accesskit_click_input(id: Id) -> RawInput {
        RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(120.0, 90.0))),
            events: vec![
                Event::AccessKitActionRequest(egui::accesskit::ActionRequest {
                    action: Action::Click,
                    target_tree: egui::accesskit::TreeId::ROOT,
                    target_node: id.accesskit_id(),
                    data: None,
                })
                .into(),
            ],
            ..RawInput::default()
        }
    }

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

    #[test]
    fn accesskit_click_requires_current_interaction_authority() {
        let context = egui::Context::default();
        let rect = Rect::from_min_size(Pos2::ZERO, vec2(80.0, 24.0));
        let id = Id::new("stale-semantic-activation");
        let mut current = None;
        let _ = crate::test_support::run_ui(&context, accesskit_click_input(id), |ui| {
            let response = ui.interact(interact_rect(rect), id, egui::Sense::click());
            current = semantic_activation_fact(ui, &response, rect, true, true);
        });
        assert_eq!(current, Some(SemanticActivation::Semantic));

        let mut stale = None;
        let _ = crate::test_support::run_ui(&context, accesskit_click_input(id), |ui| {
            let response = ui.interact(interact_rect(rect), id, egui::Sense::click());
            stale = semantic_activation_fact(ui, &response, rect, false, false);
        });
        assert_eq!(stale, None);
    }
}
