//! Renderer texture deltas survive early returns and owned-output destruction.

use egui::{Context, FullOutput, Id, TexturesDelta};

fn retained_textures_id() -> Id {
    Id::unique(("egui_dockspace", "retained_renderer_textures"))
}

#[derive(Clone, Default)]
struct RetainedTexturesDelta(TexturesDelta);

impl Drop for RetainedTexturesDelta {
    fn drop(&mut self) {
        self.0.clear();
    }
}

fn retain_texture_delta(context: &Context, delta: TexturesDelta) {
    if delta.is_empty() {
        return;
    }
    context.data_mut(|data| {
        data.get_temp_mut_or_default::<RetainedTexturesDelta>(retained_textures_id())
            .0
            .append(delta);
    });
}

pub(crate) struct OutputTexturesGuard<'a> {
    context: &'a Context,
    delta: Option<TexturesDelta>,
}

impl<'a> OutputTexturesGuard<'a> {
    pub(crate) fn take(context: &'a Context, output: &mut FullOutput) -> Self {
        Self {
            context,
            delta: Some(std::mem::take(&mut output.textures_delta)),
        }
    }

    pub(crate) fn restore_into_output(mut self, output: &mut FullOutput) {
        output.textures_delta = self.delta.take().unwrap_or_default();
    }
}

impl Drop for OutputTexturesGuard<'_> {
    fn drop(&mut self) {
        if let Some(delta) = self.delta.take() {
            retain_texture_delta(self.context, delta);
        }
    }
}

pub(super) fn restore_retained_textures(context: &Context, output: &mut FullOutput) {
    let Some(mut retained) =
        context.data_mut(|data| data.remove_temp::<RetainedTexturesDelta>(retained_textures_id()))
    else {
        return;
    };
    retained
        .0
        .append(std::mem::take(&mut output.textures_delta));
    output.textures_delta = std::mem::take(&mut retained.0);
}

#[cfg(test)]
mod tests {
    use super::super::OwnedRunOutput;
    use super::*;
    use std::sync::{Arc, Mutex};

    #[test]
    fn owned_output_keeps_texture_retirement_before_ordered_user_payload_release() {
        struct Released {
            label: u8,
            context: Context,
            events: Arc<Mutex<Vec<(u8, bool)>>>,
        }
        impl Drop for Released {
            fn drop(&mut self) {
                let retained = self.context.data(|data| {
                    data.get_temp::<RetainedTexturesDelta>(retained_textures_id())
                        .is_some_and(|delta| !delta.0.is_empty())
                });
                self.events.lock().unwrap().push((self.label, retained));
            }
        }
        let context = Context::default();
        let events = Arc::new(Mutex::new(Vec::new()));
        let result: Result<(), ()> = (|| {
            let mut raw = FullOutput::default();
            for label in [1, 2] {
                raw.shapes.push(egui::epaint::ClippedShape::new(
                    egui::Rect::EVERYTHING,
                    egui::Shape::Callback(egui::epaint::PaintCallback {
                        rect: egui::Rect::EVERYTHING,
                        callback: Arc::new(Released {
                            label,
                            context: context.clone(),
                            events: Arc::clone(&events),
                        }),
                    }),
                ));
            }
            raw.textures_delta.free(egui::TextureId::User(99));
            let mut output = OwnedRunOutput::new(raw);
            let _guard = output.take_textures(&context);
            // A later local capture holds the first payload, as the terminal
            // pass proof does. It must release before the owned output itself.
            let _frozen = output.output.shapes[0].clone();
            Err(())
        })();
        assert!(result.is_err());
        assert_eq!(*events.lock().unwrap(), [(1, true), (2, true)]);
        let mut retired = FullOutput::default();
        restore_retained_textures(&context, &mut retired);
        retired.drop_without_applying_deltas();
    }

    #[test]
    fn output_delta_is_retained_when_a_later_result_path_returns_early() {
        let context = Context::default();
        let texture = egui::TextureId::User(42);
        let mut output = FullOutput::default();
        output.textures_delta.push(
            texture,
            egui::epaint::ImageDelta::full(
                egui::ColorImage::filled([1, 1], egui::Color32::RED),
                egui::TextureOptions::NEAREST,
            ),
        );

        let result: Result<(), ()> = (|| {
            let _guard = OutputTexturesGuard::take(&context, &mut output);
            Err(())
        })();

        assert!(result.is_err());
        restore_retained_textures(&context, &mut output);
        assert!(output.textures_delta.set.contains_key(&texture));
        output.drop_without_applying_deltas();
    }

    #[test]
    fn consecutive_failed_outputs_preserve_set_and_free_append_semantics() {
        let context = Context::default();
        let texture = egui::TextureId::User(43);
        let mut first = TexturesDelta::default();
        first.push(
            texture,
            egui::epaint::ImageDelta::full(
                egui::ColorImage::filled([1, 1], egui::Color32::RED),
                egui::TextureOptions::NEAREST,
            ),
        );
        retain_texture_delta(&context, first);

        let mut second = TexturesDelta::default();
        second.free(texture);
        retain_texture_delta(&context, second);

        let mut output = FullOutput::default();
        restore_retained_textures(&context, &mut output);
        assert!(!output.textures_delta.set.contains_key(&texture));
        assert!(output.textures_delta.free.contains(&texture));
        output.drop_without_applying_deltas();
    }
}
