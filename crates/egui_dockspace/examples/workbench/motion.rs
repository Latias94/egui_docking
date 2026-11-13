//! Live tuning uses the public policy without changing the docking theme.

use std::time::Duration;

use egui_dockspace::motion::{Curve, MotionMode, MotionPreference, MotionSettings, Transition};

pub(super) fn controls(
    ui: &mut egui::Ui,
    settings: &mut MotionSettings,
    preference: &mut MotionPreference,
) {
    egui::Panel::top("workbench-motion").show(ui, |ui| {
        ui.horizontal_wrapped(|ui| {
            let mut enabled = settings.mode != MotionMode::Off;
            ui.checkbox(&mut enabled, "Motion");
            settings.mode = if enabled {
                MotionMode::System
            } else {
                MotionMode::Off
            };
            let mut reduced = *preference == MotionPreference::Reduced;
            ui.checkbox(&mut reduced, "Reduce motion");
            *preference = if reduced {
                MotionPreference::Reduced
            } else {
                MotionPreference::Unknown
            };
            let spring = matches!(settings.layout, Transition::CriticalSpring { .. });
            let mut use_spring = spring;
            ui.selectable_value(&mut use_spring, true, "Spring");
            ui.selectable_value(&mut use_spring, false, "Tween");
            let mut millis = match settings.layout {
                Transition::CriticalSpring { response } => response.as_secs_f64() * 1000.0,
                Transition::Tween { duration, .. } => duration.as_secs_f64() * 1000.0,
                Transition::Instant => 200.0,
            };
            ui.add(
                egui::Slider::new(&mut millis, 0.0..=300.0)
                    .suffix(" ms")
                    .text(if use_spring { "response" } else { "duration" }),
            );
            let mut curve = match settings.layout {
                Transition::Tween { curve, .. } => curve,
                _ => Curve::EaseInOut,
            };
            if !use_spring {
                egui::ComboBox::from_id_salt("motion-curve")
                    .selected_text(format!("{curve:?}"))
                    .show_ui(ui, |ui| {
                        for (value, label) in [
                            (Curve::EaseInOut, "Ease in/out"),
                            (Curve::EaseOut, "Ease out"),
                            (Curve::Linear, "Linear"),
                        ] {
                            ui.selectable_value(&mut curve, value, label);
                        }
                    });
            }
            let time = Duration::from_secs_f64(millis / 1000.0);
            settings.layout = if use_spring {
                Transition::CriticalSpring { response: time }
            } else {
                Transition::Tween {
                    duration: time,
                    curve,
                }
            };
            ui.weak("Reorder or close panes; drag dividers directly.");
        });
    });
}
