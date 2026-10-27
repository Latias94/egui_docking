//! Translation from current-pass egui responses into opaque core actions.

use egui::accesskit::Action;
use egui::{Key, PointerButton, Response, Ui};

use super::PointerActionAuthority;
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
