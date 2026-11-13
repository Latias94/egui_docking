//! Input-order tab focus, independently of renderer acknowledgement.

use egui::{Context, Id};

#[derive(Debug, Default)]
pub(crate) struct DeferredTabFocus {
    target: Option<Id>,
    handled: bool,
    applied: bool,
}

impl DeferredTabFocus {
    /// Records the latest primary press; only an eligible tab supplies a target.
    pub(crate) fn observe_primary_press(&mut self, target: Option<Id>) {
        self.target = target;
        self.handled |= target.is_some();
        self.applied = false;
    }

    /// Reports tab input handling even after later input supersedes its target.
    ///
    /// This is a framework input fact, not proof of core acceptance or presentation.
    pub(crate) const fn handled(&self) -> bool {
        self.handled
    }

    /// Returns the last tab destination, or none after a later application press.
    pub(crate) const fn target(&self) -> Option<Id> {
        self.target
    }

    /// Applies valid tab focus once, allowing a later pass to register its target.
    ///
    /// Returns `true` when this pass satisfies the intent, including
    /// when its target already owns focus. Invalid registrations remain pending.
    pub(crate) fn apply(&mut self, context: &Context) -> bool {
        if self.applied {
            return false;
        }
        let Some(id) = self.target else {
            return false;
        };
        let target = context.viewport(|viewport| viewport.this_pass.widgets.get(id).copied());
        let Some(target) =
            target.filter(|target| target.enabled && target.visible && target.sense.is_focusable())
        else {
            return false;
        };
        let applied = context.memory_mut(|memory| {
            if !memory.allows_interaction(target.layer_id) {
                return false;
            }
            if !memory.has_focus(id) {
                memory.request_focus(id);
            }
            true
        });
        if applied {
            self.applied = true;
        }
        applied
    }
}

#[cfg(test)]
mod tests {
    use egui::{Context, Id, Pos2, RawInput, Rect, TextEdit};

    use super::DeferredTabFocus;

    #[test]
    fn a_target_waits_for_an_enabled_registration_and_applies_only_once() {
        let context = Context::default();
        context.options_mut(|options| options.max_passes = 1.try_into().unwrap());
        let target = Id::unique("deferred-tab-focus-target");
        let external = Id::unique("deferred-tab-focus-external");
        let mut target_text = String::new();
        let mut external_text = String::new();
        let mut intent = DeferredTabFocus::default();
        intent.observe_primary_press(Some(target));
        for enabled in [false, true] {
            context
                .run_ui(
                    RawInput {
                        screen_rect: Some(Rect::from_min_size(
                            Pos2::ZERO,
                            egui::vec2(400.0, 200.0),
                        )),
                        focused: true,
                        ..RawInput::default()
                    },
                    |ui| {
                        ui.add_enabled(enabled, TextEdit::singleline(&mut target_text).id(target));
                        let external =
                            ui.add(TextEdit::singleline(&mut external_text).id(external));
                        assert_eq!(intent.apply(ui.ctx()), enabled);
                        if enabled {
                            assert_eq!(ui.memory(egui::Memory::focused), Some(target));
                            assert_eq!(intent.target(), Some(target));
                            external.request_focus();
                            assert!(!intent.apply(ui.ctx()));
                            assert_eq!(ui.memory(egui::Memory::focused), Some(external.id));
                        }
                    },
                )
                .drop_without_applying_deltas();
        }
        assert!(intent.handled());
    }
}
