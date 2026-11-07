//! UI-consumed mouse motion facts for the physical smoke's held-motion barrier.

use std::collections::BTreeMap;

use eframe::NativeOutputToken;
use eframe::egui::{self, Event, PointerButton, ViewportId};

use super::{
    LogicalPoint, PointerObservation, SemanticLookupError, SemanticSelector, SemanticText,
    pointer_observation_matches, semantic_bounds, semantic_node,
};

const LABEL: &str = "Physical smoke consumed motion";

#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct MotionReceipt {
    viewport: ViewportId,
    pub(crate) lifetime: Option<NativeOutputToken>,
    sequence: u64,
    move_sequence: u64,
    cancelled: u64,
    position: Option<egui::Pos2>,
    moved_held: bool,
    pub(crate) primary_down: bool,
    latest: Option<egui::Pos2>,
    gone: u64,
    frame: Option<u64>,
    pass: u64,
}

impl MotionReceipt {
    pub(crate) fn matches(
        self,
        baseline: Self,
        viewport: ViewportId,
        sample: MotionSample,
        expected: LogicalPoint,
        pixels_per_point: f32,
    ) -> bool {
        self.viewport == viewport
            && baseline.viewport == viewport
            && self.lifetime == baseline.lifetime
            && self.sequence == sample.sequence
            && self.move_sequence > baseline.sequence
            && self.cancelled == baseline.cancelled
            && self.primary_down
            && self.position.is_some_and(|position| {
                sample.position
                    == LogicalPoint {
                        x: f64::from(position.x),
                        y: f64::from(position.y),
                    }
            })
            && pointer_observation_matches(
                PointerObservation {
                    position: sample.position,
                    primary_down: self.moved_held,
                },
                expected,
                true,
                pixels_per_point,
            )
    }
}

#[derive(Debug, Default)]
pub(crate) struct MotionProbe {
    viewports: BTreeMap<ViewportId, MotionReceipt>,
}

impl MotionProbe {
    pub(crate) fn latest(&self, viewport: ViewportId) -> Option<MotionReceipt> {
        self.viewports.get(&viewport).copied()
    }

    pub(crate) fn publish(&mut self, ui: &egui::Ui) {
        let context = ui.ctx();
        let viewport = context.viewport_id();
        let token =
            eframe::current_native_output_token().filter(|token| token.viewport_id() == viewport);
        let frame = context.cumulative_frame_nr();
        let pass = context.cumulative_pass_nr();
        let mut receipt = self.latest(viewport).unwrap_or_default();
        let same_window = match (receipt.lifetime, token) {
            (Some(previous), Some(current)) => {
                previous.same_context(current)
                    && previous.window_id() == current.window_id()
                    && previous.create_attempt() == current.create_attempt()
            }
            (None, None) => true,
            _ => false,
        };
        if !same_window
            || receipt.frame.is_some_and(|previous| frame < previous)
            || pass < receipt.pass
        {
            receipt = MotionReceipt::default();
        }
        receipt.viewport = viewport;
        receipt.lifetime = receipt.lifetime.or(token);
        receipt.pass = pass;
        ui.input(|input| {
            self.viewports
                .retain(|viewport, _| input.raw.viewports.contains_key(viewport));
            // One integration RawInput belongs to one outer frame. Repeated
            // passes must not replay its physical events into a new receipt.
            if receipt.frame != Some(frame) {
                receipt.frame = Some(frame);
                for event in &input.raw.events {
                    match event {
                        Event::PointerMoved(position) => {
                            receipt.sequence = receipt.sequence.saturating_add(1);
                            receipt.move_sequence = receipt.sequence;
                            receipt.position = Some(*position);
                            receipt.moved_held = receipt.primary_down;
                        }
                        Event::PointerButton {
                            button: PointerButton::Primary,
                            pressed,
                            ..
                        } => {
                            receipt.sequence = receipt.sequence.saturating_add(1);
                            receipt.primary_down = *pressed;
                            if !*pressed {
                                receipt.cancelled = receipt.sequence;
                            }
                        }
                        Event::WindowFocused(false)
                        | Event::Key {
                            key: egui::Key::Escape,
                            pressed: true,
                            ..
                        } => {
                            receipt.sequence = receipt.sequence.saturating_add(1);
                            receipt.primary_down = false;
                            receipt.cancelled = receipt.sequence;
                        }
                        Event::PointerGone => {
                            receipt.sequence = receipt.sequence.saturating_add(1);
                            receipt.gone = receipt.sequence;
                        }
                        _ => {}
                    }
                }
            }
            receipt.latest = input.pointer.latest_pos();
            receipt.primary_down &= input.pointer.primary_down();
        });
        self.viewports.insert(viewport, receipt);
        let id = ui.make_persistent_id(("physical-smoke-consumed-motion", viewport));
        context.accesskit_node_builder(id, |node| {
            node.set_role(egui::accesskit::Role::Label);
            node.set_label(LABEL);
            node.set_value(receipt.sequence.to_string());
            if let Some(position) = receipt.position {
                node.set_bounds(egui::accesskit::Rect::new(
                    f64::from(position.x) - 0.5,
                    f64::from(position.y) - 0.5,
                    f64::from(position.x) + 0.5,
                    f64::from(position.y) + 0.5,
                ));
            }
        });
    }
}

#[derive(Clone, Copy)]
pub(crate) struct MotionSample {
    sequence: u64,
    position: LogicalPoint,
}

pub(crate) fn sample(tree: &egui::accesskit::TreeUpdate) -> Result<Option<MotionSample>, String> {
    let selector = SemanticSelector {
        role: egui::accesskit::Role::Label,
        text: SemanticText::Label(LABEL),
    };
    let node = match semantic_node(tree, selector) {
        Ok(node) => node,
        Err(SemanticLookupError::Missing) => return Ok(None),
        Err(error) => return Err(format!("motion receipt selector is invalid: {error:?}")),
    };
    let sequence = node
        .value()
        .ok_or("motion receipt has no sequence")?
        .parse::<u64>()
        .map_err(|error| format!("motion receipt sequence is invalid: {error}"))?;
    if node.bounds().is_none() {
        return Ok(None);
    }
    let bounds = semantic_bounds(tree, selector)
        .map_err(|error| format!("motion receipt bounds are invalid: {error:?}"))?;
    Ok(Some(MotionSample {
        sequence,
        position: bounds.center(),
    }))
}

#[cfg(test)]
mod tests {
    use std::num::NonZeroUsize;
    use std::sync::{Arc, Mutex};

    use super::*;

    struct Probe(Arc<Mutex<MotionProbe>>);

    impl egui::Plugin for Probe {
        fn debug_name(&self) -> &'static str {
            "physical_smoke_motion_test"
        }

        fn on_begin_pass(&mut self, ui: &mut egui::Ui) {
            self.0.lock().unwrap().publish(ui);
        }
    }

    fn setup() -> (egui::Context, Arc<Mutex<MotionProbe>>) {
        let context = egui::Context::default();
        context.enable_accesskit();
        context.options_mut(|options| options.max_passes = NonZeroUsize::new(2).unwrap());
        let facts = Arc::new(Mutex::new(MotionProbe::default()));
        context.add_plugin(Probe(Arc::clone(&facts)));
        (context, facts)
    }

    fn button(position: egui::Pos2, pressed: bool) -> Event {
        Event::PointerButton {
            pos: position,
            button: PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::default(),
        }
    }

    fn frame(
        context: &egui::Context,
        facts: &Mutex<MotionProbe>,
        viewport: ViewportId,
        events: Vec<Event>,
        repeat: bool,
    ) -> (MotionReceipt, MotionSample) {
        let mut input = egui::RawInput {
            viewport_id: viewport,
            events,
            ..Default::default()
        };
        if viewport != ViewportId::ROOT {
            input
                .viewports
                .insert(viewport, egui::ViewportInfo::default());
        }
        let mut output = context.run_ui(input, |ui| {
            ui.label("Real UI pass");
            if repeat && ui.ctx().current_pass_index() == 0 {
                ui.ctx()
                    .request_discard("repeat the consumed physical input");
            }
        });
        // This semantic-only test has no renderer consuming texture updates.
        output.textures_delta.clear();
        let sample = sample(&output.platform_output.accesskit_update.unwrap())
            .unwrap()
            .unwrap();
        let receipt = facts.lock().unwrap().latest(viewport).unwrap();
        (receipt, sample)
    }

    fn press(context: &egui::Context, facts: &Mutex<MotionProbe>) -> MotionReceipt {
        let source = egui::pos2(40.0, 14.0);
        frame(
            context,
            facts,
            ViewportId::ROOT,
            vec![Event::PointerMoved(source), button(source, true)],
            false,
        )
        .0
    }

    fn logical(position: egui::Pos2) -> LogicalPoint {
        LogicalPoint {
            x: f64::from(position.x),
            y: f64::from(position.y),
        }
    }

    #[test]
    fn real_ui_motion_survives_same_batch_gone_and_empty_and_repeated_passes() {
        let (context, facts) = setup();
        let baseline = press(&context, &facts);
        let target = egui::pos2(12.0, 15.0);
        let (moved, sample) = frame(
            &context,
            &facts,
            ViewportId::ROOT,
            vec![Event::PointerMoved(target), Event::PointerGone],
            false,
        );
        assert_eq!(moved.latest, None);
        assert!(moved.gone > moved.move_sequence);
        assert!(moved.matches(baseline, ViewportId::ROOT, sample, logical(target), 1.0));
        let (empty, sample) = frame(&context, &facts, ViewportId::ROOT, vec![], false);
        assert_eq!(empty.sequence, moved.sequence);
        assert!(empty.matches(baseline, ViewportId::ROOT, sample, logical(target), 1.0));
        assert!(!empty.matches(empty, ViewportId::ROOT, sample, logical(target), 1.0));
        let pass = context.cumulative_pass_nr();
        let (repeated, sample) = frame(
            &context,
            &facts,
            ViewportId::ROOT,
            vec![Event::PointerMoved(egui::pos2(11.0, 16.0))],
            true,
        );
        assert_eq!(context.cumulative_pass_nr(), pass + 2);
        assert_eq!(repeated.sequence, empty.sequence + 1);
        assert!(repeated.matches(
            empty,
            ViewportId::ROOT,
            sample,
            logical(egui::pos2(11.0, 16.0)),
            1.0,
        ));
    }

    #[test]
    fn real_ui_later_gone_keeps_motion_but_not_old_or_foreign_samples() {
        let (context, facts) = setup();
        let baseline = press(&context, &facts);
        let target = egui::pos2(12.0, 15.0);
        let (moved, old_sample) = frame(
            &context,
            &facts,
            ViewportId::ROOT,
            vec![Event::PointerMoved(target)],
            false,
        );
        let (gone, sample) = frame(
            &context,
            &facts,
            ViewportId::ROOT,
            vec![Event::PointerGone],
            false,
        );
        assert!(gone.matches(baseline, ViewportId::ROOT, sample, logical(target), 1.0));
        assert!(!gone.matches(baseline, ViewportId::ROOT, old_sample, logical(target), 1.0));
        assert!(!gone.matches(moved, ViewportId::ROOT, sample, logical(target), 1.0));
        assert!(!gone.matches(
            baseline,
            ViewportId::ROOT,
            sample,
            logical(egui::pos2(40.0, 14.0)),
            1.0,
        ));
        let child = ViewportId::from_hash_of("real-motion-child");
        let (child_receipt, child_sample) = frame(
            &context,
            &facts,
            child,
            vec![button(target, true), Event::PointerMoved(target)],
            false,
        );
        assert!(!child_receipt.matches(
            baseline,
            ViewportId::ROOT,
            child_sample,
            logical(target),
            1.0,
        ));
    }

    #[test]
    fn real_ui_release_and_focus_cancellation_cannot_borrow_an_earlier_move() {
        let (unheld_context, unheld_facts) = setup();
        let target = egui::pos2(12.0, 15.0);
        let (unheld, _) = frame(
            &unheld_context,
            &unheld_facts,
            ViewportId::ROOT,
            vec![Event::PointerMoved(egui::pos2(40.0, 14.0))],
            false,
        );
        let (pressed_after_move, sample) = frame(
            &unheld_context,
            &unheld_facts,
            ViewportId::ROOT,
            vec![Event::PointerMoved(target), button(target, true)],
            false,
        );
        assert!(!pressed_after_move.matches(
            unheld,
            ViewportId::ROOT,
            sample,
            logical(target),
            1.0,
        ));
        let (context, facts) = setup();
        let baseline = press(&context, &facts);
        let (released, sample) = frame(
            &context,
            &facts,
            ViewportId::ROOT,
            vec![
                Event::PointerMoved(target),
                button(target, false),
                Event::PointerGone,
            ],
            false,
        );
        assert!(!released.matches(baseline, ViewportId::ROOT, sample, logical(target), 1.0));
        let (repressed, sample) = frame(
            &context,
            &facts,
            ViewportId::ROOT,
            vec![button(target, true), Event::PointerMoved(target)],
            false,
        );
        assert!(!repressed.matches(baseline, ViewportId::ROOT, sample, logical(target), 1.0));
        let (cancelled, sample) = frame(
            &context,
            &facts,
            ViewportId::ROOT,
            vec![Event::WindowFocused(false), Event::PointerMoved(target)],
            false,
        );
        assert!(!cancelled.matches(repressed, ViewportId::ROOT, sample, logical(target), 1.0));
    }
}
