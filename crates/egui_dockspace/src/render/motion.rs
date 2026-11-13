//! Passive decorations use the same clock and model without receiver authority.

use dockspace::motion::{RectMotion, Transition};

use super::{RenderContext, geometry};

#[derive(Clone, Copy)]
pub(super) enum Effect {
    Drop = 0,
    PointerFocus = 1,
}

fn key(context: &RenderContext<'_, '_, '_>, effect: Effect) -> egui::Id {
    context.instance_id.with((
        "decoration-motion",
        context.plan.surface(),
        context.ui.ctx().viewport_id(),
        effect as u8,
    ))
}

pub(super) fn begin(context: &RenderContext<'_, '_, '_>) {
    let navigation = context.ui.input(|input| {
        input.events.iter().rev().find_map(|event| match event {
            egui::Event::Key { pressed: true, .. } => Some(true),
            egui::Event::PointerButton { pressed: true, .. } => Some(false),
            _ => None,
        })
    });
    if let Some(keyboard) = navigation {
        let id = key(context, Effect::PointerFocus).with("keyboard-focus");
        context
            .ui
            .ctx()
            .data_mut(|data| data.insert_temp(id, keyboard));
    }
}

pub(super) fn sample(
    context: &mut RenderContext<'_, '_, '_>,
    effect: Effect,
    target: egui::Rect,
) -> egui::Rect {
    context.decoration_seen[effect as usize] = true;
    let Some(frame) = context.plan.motion_frame() else {
        return target;
    };
    let Ok(rect) = geometry::logical_rect(target) else {
        return target;
    };
    let id = key(context, effect);
    let transition = match effect {
        Effect::Drop => frame.settings.drop_preview_transition(frame.preference),
        Effect::PointerFocus => {
            // Focus observations can commit a frame after their initiating key.
            // Keep keyboard navigation immediate until a new pointer press.
            let keyboard = context.ui.ctx().data(|data| {
                data.get_temp::<bool>(id.with("keyboard-focus"))
                    .unwrap_or(true)
            });
            if keyboard {
                Transition::Instant
            } else {
                frame.settings.pointer_focus_transition(frame.preference)
            }
        }
    };
    let sample = context.ui.ctx().data_mut(|data| {
        let motion = data.get_temp_mut_or_insert_with(id, || RectMotion::new(rect));
        motion.retarget(rect, transition, frame.now);
        motion.sample(frame.now)
    });
    if sample.is_active() {
        context.ui.ctx().request_repaint();
    }
    geometry::egui_rect(sample.rect()).unwrap_or(target)
}

pub(super) fn finish(context: &mut RenderContext<'_, '_, '_>) {
    for effect in [Effect::Drop, Effect::PointerFocus] {
        if !context.decoration_seen[effect as usize] {
            let id = key(context, effect);
            context
                .ui
                .ctx()
                .data_mut(|data| data.remove::<RectMotion>(id));
        }
    }
}
