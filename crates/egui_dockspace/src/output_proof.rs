//! Conservative, single-layer evidence that a complete surface span survived.
//!
//! The caller must bracket every surface paint, including ordinary geometry,
//! pane contents, source-gap handling, and transient visuals. Painting must be
//! synchronous and append-only on the supplied layer. Pane implementations must
//! honor the supplied paint disposition and opacity, and must not defer drawing,
//! mutate an earlier shape, or write to another layer. Other-layer snapshots
//! preserve layer identity, global transforms, and content. Nonempty graphics
//! on an unenumerated layer make the capture unavailable.
//!
//! This checks shape descriptors, not application callback internals or semantic
//! correctness. Persistent transforms are frozen at finish; subsequent shape or
//! transform changes may conservatively invalidate the proof. Validate all spans
//! and their relative order after output hooks, before scrubbing markers.

use std::sync::Arc;

use egui::epaint::{ClippedShape, PaintCallback};
use egui::{Context, FullOutput, LayerId, Painter, Shape, ViewportId};

/// An open capture bound to one context, viewport, and paint layer.
pub(super) struct SurfaceOutputCapture {
    append_check: PaintAppendCheck,
    begin: Arc<SurfaceMarker>,
}

/// Checks that a callback only appended shapes to its supplied paint layer.
pub(super) struct PaintAppendCheck {
    context: Context,
    viewport: ViewportId,
    layer: LayerId,
    before: Option<PaintSnapshot>,
}

impl PaintAppendCheck {
    /// Starts a conservative check; unknown layers leave it unavailable.
    pub(super) fn begin(painter: &Painter) -> Self {
        Self {
            context: painter.ctx().clone(),
            viewport: painter.ctx().viewport_id(),
            layer: painter.layer_id(),
            before: snapshot(painter),
        }
    }

    /// Returns whether the original prefix and every other layer are unchanged.
    pub(super) fn is_preserved(&self, painter: &Painter) -> bool {
        self.preserved_snapshot(painter).is_some()
    }

    fn preserved_snapshot(&self, painter: &Painter) -> Option<PaintSnapshot> {
        if painter.ctx() != &self.context
            || painter.ctx().viewport_id() != self.viewport
            || painter.layer_id() != self.layer
        {
            return None;
        }
        let before = self.before.as_ref()?;
        let after = snapshot(painter)?;
        if after.current.transform != before.current.transform
            || after.current.shapes.get(..before.current.shapes.len())?
                != before.current.shapes.as_slice()
            || after.others != before.others
        {
            return None;
        }
        Some(after)
    }
}

struct PaintSnapshot {
    current: LayerSnapshot,
    others: Vec<LayerSnapshot>,
}

#[derive(PartialEq)]
struct LayerSnapshot {
    layer: LayerId,
    transform: Option<egui::emath::TSTransform>,
    shapes: Vec<ClippedShape>,
}

impl SurfaceOutputCapture {
    /// Snapshots existing graphics before appending the private begin marker.
    pub(super) fn begin(painter: &Painter) -> Self {
        let append_check = PaintAppendCheck::begin(painter);
        let begin = Arc::new(SurfaceMarker);
        paint_marker(painter, &begin);
        Self {
            append_check,
            begin,
        }
    }

    /// Closes an append-only span, rejecting changes outside its current layer.
    ///
    /// Returns `None` for a foreign painter, changed prefix or other-layer
    /// snapshot, or missing/duplicated markers. All local UI paint finalization
    /// must have completed before calling this method.
    pub(super) fn finish(self, painter: &Painter) -> Option<SurfaceOutputProof> {
        if painter.ctx() != &self.append_check.context
            || painter.ctx().viewport_id() != self.append_check.viewport
            || painter.layer_id() != self.append_check.layer
        {
            return None;
        }
        let end = Arc::new(SurfaceMarker);
        paint_marker(painter, &end);
        let after = self.append_check.preserved_snapshot(painter)?;
        let prefix_len = self.append_check.before.as_ref()?.current.shapes.len();
        let layer = &after.current.shapes;
        let begin_index = unique_marker_index(layer, &self.begin)?;
        let end_index = unique_marker_index(layer, &end)?;
        if begin_index != prefix_len || end_index + 1 != layer.len() {
            return None;
        }
        let mut expected = layer.get(begin_index..=end_index)?.to_vec();
        if let Some(transform) = after.current.transform {
            for shape in &mut expected {
                shape.transform(transform);
            }
        }
        Some(SurfaceOutputProof {
            expected,
            pixels_per_point: painter.ctx().pixels_per_point(),
            begin: self.begin,
            end,
        })
    }
}

/// Exact shape/clip/transform evidence for one complete, bounded surface span.
pub(super) struct SurfaceOutputProof {
    expected: Vec<ClippedShape>,
    pixels_per_point: f32,
    begin: Arc<SurfaceMarker>,
    end: Arc<SurfaceMarker>,
}

impl SurfaceOutputProof {
    /// Returns the half-open final-output range `[begin, end)`, including markers.
    ///
    /// Both marker identities must occur exactly once in the entire output,
    /// including nested compounds, and the complete contiguous slice must match.
    /// The final pixel scale must exactly match the context scale at finish.
    /// The caller must additionally validate relative order between surface spans.
    pub(super) fn is_present_once(&self, output: &FullOutput) -> Option<(usize, usize)> {
        if output.pixels_per_point != self.pixels_per_point {
            return None;
        }
        let begin = unique_marker_index(&output.shapes, &self.begin)?;
        let end = unique_marker_index(&output.shapes, &self.end)?.checked_add(1)?;
        (output.shapes.get(begin..end)? == self.expected.as_slice()).then_some((begin, end))
    }
}

struct SurfaceMarker;

fn paint_marker(painter: &Painter, marker: &Arc<SurfaceMarker>) {
    painter.add(Shape::Callback(PaintCallback {
        rect: egui::Rect::from_min_size(egui::Pos2::ZERO, egui::Vec2::splat(1.0)),
        callback: marker.clone(),
    }));
}

fn snapshot(painter: &Painter) -> Option<PaintSnapshot> {
    let mut layers = painter
        .ctx()
        .memory(|memory| memory.layer_ids().collect::<Vec<_>>());
    if !layers.contains(&painter.layer_id()) {
        layers.push(painter.layer_id());
    }
    let layers = layers
        .into_iter()
        .map(|layer| (layer, painter.ctx().layer_transform_to_global(layer)))
        .collect::<Vec<_>>();
    painter.ctx().graphics(|graphics| {
        let mut remaining = graphics.clone();
        let mut current = None;
        let mut others = Vec::new();
        for (layer, transform) in layers {
            let shapes = graphics
                .get(layer)
                .map(|list| list.all_entries().cloned().collect())
                .unwrap_or_default();
            *remaining.entry(layer) = egui::layers::PaintList::default();
            let captured = LayerSnapshot {
                layer,
                transform,
                shapes,
            };
            if layer == painter.layer_id() {
                current = Some(captured);
            } else {
                others.push(captured);
            }
        }
        // GraphicLayers cannot enumerate IDs; never assign residual shapes to a guessed layer.
        if !remaining.drain(&[], &Default::default()).is_empty() {
            return None;
        }
        Some(PaintSnapshot {
            current: current?,
            others,
        })
    })
}

fn unique_marker_index(shapes: &[ClippedShape], marker: &Arc<SurfaceMarker>) -> Option<usize> {
    let mut found = None;
    for (index, clipped) in shapes.iter().enumerate() {
        match marker_count(&clipped.shape, marker) {
            0 => {}
            1 if found.is_none() => found = Some(index),
            _ => return None,
        }
    }
    found
}

fn marker_count(shape: &Shape, marker: &Arc<SurfaceMarker>) -> usize {
    match shape {
        Shape::Vec(shapes) => shapes.iter().map(|shape| marker_count(shape, marker)).sum(),
        Shape::Callback(callback) => usize::from(
            callback
                .callback
                .downcast_ref::<SurfaceMarker>()
                .is_some_and(|value| std::ptr::eq(value, marker.as_ref())),
        ),
        _ => 0,
    }
}

/// Removes only private markers, preserving user compounds (including empty ones).
///
/// Call only after every output proof and cross-span order check has completed.
pub(super) fn scrub_private_markers(output: &mut FullOutput) {
    output
        .shapes
        .retain_mut(|clipped| retain_user_shape(&mut clipped.shape));
}

fn retain_user_shape(shape: &mut Shape) -> bool {
    match shape {
        Shape::Vec(shapes) => {
            shapes.retain_mut(retain_user_shape);
            true
        }
        Shape::Callback(callback) => !callback.callback.is::<SurfaceMarker>(),
        _ => true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn with_painter(mut check: impl FnMut(&Painter)) {
        let context = Context::default();
        context
            .run_ui(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::Vec2::splat(100.0),
                    )),
                    ..egui::RawInput::default()
                },
                |ui| check(ui.painter()),
            )
            .drop_without_applying_deltas();
    }

    fn known_layer(context: &Context, name: &'static str) -> LayerId {
        let layer = LayerId::new(egui::Order::Middle, egui::Id::unique(name));
        context.memory_mut(|memory| {
            memory
                .areas_mut()
                .set_state(layer, egui::AreaState::default());
        });
        layer
    }

    #[test]
    fn append_check_accepts_new_shapes_on_the_supplied_layer() {
        with_painter(|painter| {
            let check = PaintAppendCheck::begin(painter);
            let capture = SurfaceOutputCapture::begin(painter);
            painter.circle_filled(egui::pos2(20.0, 20.0), 5.0, egui::Color32::RED);
            assert!(check.is_preserved(painter));
            assert!(capture.finish(painter).is_some());
        });
    }

    #[test]
    fn pane_append_check_rejects_changes_to_the_existing_surface_prefix() {
        with_painter(|painter| {
            let _capture = SurfaceOutputCapture::begin(painter);
            let chrome = painter.circle_filled(egui::pos2(20.0, 20.0), 5.0, egui::Color32::RED);
            let check = PaintAppendCheck::begin(painter);
            painter.circle_filled(egui::pos2(30.0, 30.0), 5.0, egui::Color32::GREEN);
            assert!(check.is_preserved(painter));

            painter.set(chrome, Shape::Noop);

            assert!(!check.is_preserved(painter));
        });
    }

    #[test]
    fn moving_an_identical_shape_between_known_layers_is_rejected() {
        with_painter(|painter| {
            let source = known_layer(painter.ctx(), "proof-source-layer");
            let destination = known_layer(painter.ctx(), "proof-destination-layer");
            let shape = Shape::circle_filled(egui::pos2(20.0, 20.0), 5.0, egui::Color32::RED);
            let other_painter = painter.ctx().layer_painter(source);
            other_painter.add(shape.clone());
            let check = PaintAppendCheck::begin(painter);
            let capture = SurfaceOutputCapture::begin(painter);
            assert!(check.is_preserved(painter));

            painter.ctx().graphics_mut(|graphics| {
                *graphics.entry(source) = egui::layers::PaintList::default();
                graphics
                    .entry(destination)
                    .add(other_painter.clip_rect(), shape.clone());
            });

            assert!(!check.is_preserved(painter));
            assert!(capture.finish(painter).is_none());
        });
    }

    #[test]
    fn changing_another_known_layers_global_transform_is_rejected() {
        with_painter(|painter| {
            let other = known_layer(painter.ctx(), "proof-transformed-layer");
            painter.ctx().layer_painter(other).circle_filled(
                egui::pos2(20.0, 20.0),
                5.0,
                egui::Color32::RED,
            );
            let check = PaintAppendCheck::begin(painter);
            let capture = SurfaceOutputCapture::begin(painter);
            assert!(check.is_preserved(painter));

            painter.ctx().set_transform_layer(
                other,
                egui::emath::TSTransform::from_translation(egui::vec2(10.0, 0.0)),
            );

            assert!(!check.is_preserved(painter));
            assert!(capture.finish(painter).is_none());
        });
    }

    #[test]
    fn unknown_nonempty_layers_make_both_initial_and_final_snapshots_unavailable() {
        for present_before_capture in [true, false] {
            with_painter(|painter| {
                let unknown =
                    LayerId::new(egui::Order::Foreground, egui::Id::unique("unknown-layer"));
                assert!(
                    !painter
                        .ctx()
                        .memory(|memory| memory.layer_ids().any(|layer| layer == unknown))
                );
                let paint_unknown = || {
                    painter.ctx().layer_painter(unknown).circle_filled(
                        egui::pos2(20.0, 20.0),
                        5.0,
                        egui::Color32::RED,
                    );
                };
                if present_before_capture {
                    paint_unknown();
                }
                let check = PaintAppendCheck::begin(painter);
                let capture = SurfaceOutputCapture::begin(painter);
                if !present_before_capture {
                    assert!(check.is_preserved(painter));
                    paint_unknown();
                }

                assert!(!check.is_preserved(painter));
                assert!(capture.finish(painter).is_none());
            });
        }
    }

    fn clipped(shape: Shape) -> ClippedShape {
        ClippedShape::new(
            egui::Rect::from_min_size(egui::Pos2::ZERO, egui::Vec2::splat(100.0)),
            shape,
        )
    }

    fn marker_shape(marker: &Arc<SurfaceMarker>) -> Shape {
        Shape::Callback(PaintCallback {
            rect: egui::Rect::from_min_size(egui::Pos2::ZERO, egui::Vec2::splat(1.0)),
            callback: marker.clone(),
        })
    }

    fn fixture() -> (SurfaceOutputProof, FullOutput) {
        let begin = Arc::new(SurfaceMarker);
        let end = Arc::new(SurfaceMarker);
        let expected = vec![
            clipped(marker_shape(&begin)),
            clipped(Shape::rect_filled(
                egui::Rect::from_min_size(egui::Pos2::ZERO, egui::Vec2::splat(20.0)),
                0.0,
                egui::Color32::RED,
            )),
            clipped(Shape::circle_filled(
                egui::pos2(30.0, 30.0),
                5.0,
                egui::Color32::GREEN,
            )),
            clipped(marker_shape(&end)),
        ];
        let output = FullOutput {
            shapes: expected.clone(),
            pixels_per_point: 2.0,
            ..FullOutput::default()
        };
        (
            SurfaceOutputProof {
                expected,
                pixels_per_point: 2.0,
                begin,
                end,
            },
            output,
        )
    }

    #[test]
    fn exact_span_reports_its_half_open_range() {
        let (proof, mut output) = fixture();
        output.shapes.insert(0, clipped(Shape::Noop));
        output.shapes.push(clipped(Shape::Noop));
        assert_eq!(proof.is_present_once(&output), Some((1, 5)));
    }

    #[test]
    fn deleting_ordinary_geometry_while_preserving_preview_is_rejected() {
        let (proof, mut output) = fixture();
        output.shapes.remove(1);
        assert_eq!(proof.is_present_once(&output), None);
    }

    #[test]
    fn a_different_or_unknown_final_pixel_scale_is_rejected() {
        let (proof, mut output) = fixture();
        for pixels_per_point in [1.0, 0.0, f32::NAN] {
            output.pixels_per_point = pixels_per_point;
            assert_eq!(proof.is_present_once(&output), None);
        }
    }

    #[test]
    fn clip_transform_and_order_changes_are_rejected() {
        let (proof, output) = fixture();
        let mut changed = output.clone();
        changed.shapes[1].clip_rect.max.x -= 1.0;
        assert_eq!(proof.is_present_once(&changed), None);
        let mut changed = output.clone();
        changed.shapes[1].transform_after_tessellation.translation.x += 1.0;
        assert_eq!(proof.is_present_once(&changed), None);
        let mut changed = output;
        changed.shapes.swap(1, 2);
        assert_eq!(proof.is_present_once(&changed), None);
    }

    #[test]
    fn missing_reversed_and_nested_duplicate_markers_are_rejected() {
        let (proof, output) = fixture();
        let mut changed = output.clone();
        changed.shapes.pop();
        assert_eq!(proof.is_present_once(&changed), None);
        let mut changed = output.clone();
        changed.shapes.swap(0, 3);
        assert_eq!(proof.is_present_once(&changed), None);
        let mut changed = output;
        changed
            .shapes
            .push(clipped(Shape::Vec(vec![Shape::Vec(vec![marker_shape(
                &proof.begin,
            )])])));
        assert_eq!(proof.is_present_once(&changed), None);
    }

    #[test]
    fn scrubbing_preserves_user_compounds_callbacks_and_noops() {
        let (proof, _) = fixture();
        let user_callback = Shape::Callback(PaintCallback {
            rect: egui::Rect::from_min_size(egui::Pos2::ZERO, egui::Vec2::splat(1.0)),
            callback: Arc::new(7_u8),
        });
        let mut output = FullOutput {
            shapes: vec![
                clipped(marker_shape(&proof.begin)),
                clipped(Shape::Vec(vec![
                    Shape::Noop,
                    user_callback.clone(),
                    Shape::Vec(vec![marker_shape(&proof.end)]),
                ])),
            ],
            ..FullOutput::default()
        };
        scrub_private_markers(&mut output);
        assert_eq!(
            output.shapes,
            vec![clipped(Shape::Vec(vec![
                Shape::Noop,
                user_callback,
                Shape::Vec(Vec::new()),
            ]))]
        );
    }
}
