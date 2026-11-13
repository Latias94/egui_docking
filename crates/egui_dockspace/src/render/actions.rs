//! Translation from current-pass egui responses into opaque core actions.

use egui::accesskit::Action;
use egui::{Id, Key, PointerButton, Response, Ui};

use super::PointerActionAuthority;
pub(super) fn effective_enabled(ui: &Ui, enabled: bool) -> bool {
    enabled && ui.is_enabled() && ui.memory(|memory| memory.allows_interaction(ui.layer_id()))
}

pub(super) fn button_activation_origin(
    ui: &Ui,
    response: &Response,
    pointer_authority: PointerActionAuthority,
) -> Option<Id> {
    if !effective_enabled(ui, response.enabled()) {
        return None;
    }
    if response.has_focus()
        && let Some(key) = ui.input_mut(|input| {
            [Key::Enter, Key::Space]
                .into_iter()
                .find(|&key| input.consume_key(egui::Modifiers::NONE, key))
        })
    {
        return Some(response.id.with(("keyboard-activate", key)));
    }
    if ui.input(|input| input.has_accesskit_action_request(response.id, Action::Click)) {
        return Some(response.id.with("accesskit-click"));
    }
    (pointer_authority.accepts_local_pointer_actions()
        && response.clicked_by(PointerButton::Primary))
    .then(|| response.id.with("pointer-click"))
}

/// Classifies an activation already accepted by an egui button without consuming input.
pub(super) fn clicked_response_origin(ui: &Ui, response: &Response) -> Option<Id> {
    if !response.clicked() {
        return None;
    }
    if response.has_focus()
        && let Some(key) = ui.input(|input| {
            [Key::Enter, Key::Space]
                .into_iter()
                .find(|&key| input.key_pressed(key))
        })
    {
        return Some(response.id.with(("keyboard-activate", key)));
    }
    if ui.input(|input| input.has_accesskit_action_request(response.id, Action::Click)) {
        return Some(response.id.with("accesskit-click"));
    }
    Some(response.id.with("pointer-click"))
}
