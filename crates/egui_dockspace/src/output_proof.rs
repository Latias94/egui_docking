//! Conservative evidence that complete owned surface spans survived.
//!
//! The caller must bracket every surface paint, including ordinary geometry,
//! pane contents, source-gap handling, and transient visuals. Painting must be
//! synchronous and append-only. Pane implementations must
//! honor the supplied paint disposition and opacity, and must not defer drawing,
//! mutate an earlier shape, or write to another layer. Other-layer snapshots
//! preserve layer identity, global transforms, and content. Nonempty graphics
//! on an unenumerated layer make the capture unavailable. Adapter-owned chrome may
//! append to registered popup layers through the explicit multilayer capture;
//! every existing layer prefix and transform must remain unchanged.
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
    multilayer: bool,
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
        if !after.current.preserves_prefix(&before.current) || after.others != before.others {
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

impl LayerSnapshot {
    fn preserves_prefix(&self, before: &Self) -> bool {
        self.layer == before.layer
            && self.transform == before.transform
            && self.shapes.get(..before.shapes.len()) == Some(before.shapes.as_slice())
    }
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
            multilayer: false,
        }
    }

    /// Includes synchronous adapter-owned chrome appended to registered popup layers.
    pub(super) fn begin_multilayer(painter: &Painter) -> Self {
        Self {
            multilayer: true,
            ..Self::begin(painter)
        }
    }

    /// Closes the synchronous append-only spans selected by the capture mode.
    ///
    /// Returns `None` for a foreign painter, changed prefix or transform,
    /// unenumerated graphics, or missing/duplicated primary markers. Strict
    /// capture also rejects changes to any other layer. All local UI paint
    /// finalization must have completed before calling this method.
    pub(super) fn finish(self, painter: &Painter) -> Option<SurfaceOutputProof> {
        if painter.ctx() != &self.append_check.context
            || painter.ctx().viewport_id() != self.append_check.viewport
            || painter.layer_id() != self.append_check.layer
        {
            return None;
        }
        let end = Arc::new(SurfaceMarker);
        paint_marker(painter, &end);
        let before = self.append_check.before.as_ref()?;
        let after = if self.multilayer {
            let after = snapshot(painter)?;
            if !after.current.preserves_prefix(&before.current)
                || before.others.iter().any(|previous| {
                    !after
                        .others
                        .iter()
                        .any(|current| current.preserves_prefix(previous))
                })
            {
                return None;
            }
            after
        } else {
            self.append_check.preserved_snapshot(painter)?
        };
        let prefix_len = before.current.shapes.len();
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
        let mut spans = Vec::new();
        let mut primary_span_index = 0;
        if self.multilayer {
            let mut paint_layers = painter
                .ctx()
                .memory(|memory| memory.layer_ids().collect::<Vec<_>>());
            if !paint_layers.contains(&painter.layer_id()) {
                // GraphicLayers::drain paints unregistered layers last within
                // their Order. The snapshot excludes every unknown nonempty
                // layer except this explicitly captured primary painter.
                paint_layers.push(painter.layer_id());
            }
            // Match egui's Order-first drain with the memory order preserved
            // within each group; capture traversal itself is not paint order.
            paint_layers.sort_by_key(|layer| layer.order);
            let primary_order = paint_layers
                .iter()
                .position(|layer| *layer == painter.layer_id())?;
            for layer in after.others {
                let prefix_len = before
                    .others
                    .iter()
                    .find(|previous| previous.layer == layer.layer)
                    .map_or(0, |previous| previous.shapes.len());
                if layer.shapes.len() == prefix_len {
                    continue;
                }
                let order = paint_layers
                    .iter()
                    .position(|candidate| *candidate == layer.layer)?;
                let end = Arc::new(SurfaceMarker);
                let layer_painter = painter.ctx().layer_painter(layer.layer);
                paint_marker(&layer_painter, &end);
                let mut expected = layer.shapes.get(prefix_len..)?.to_vec();
                expected.push(painter.ctx().graphics(|graphics| {
                    graphics.get(layer.layer)?.all_entries().last().cloned()
                })?);
                if let Some(transform) = layer.transform {
                    for shape in &mut expected {
                        shape.transform(transform);
                    }
                }
                spans.push((order, SurfaceOutputSpan { expected, end }));
            }
            spans.sort_by_key(|(order, _)| *order);
            primary_span_index = spans.partition_point(|(order, _)| *order < primary_order);
        }
        Some(SurfaceOutputProof {
            expected,
            spans: spans.into_iter().map(|(_, span)| span).collect(),
            primary_span_index,
            pixels_per_point: painter.ctx().pixels_per_point(),
            begin: self.begin,
            end,
        })
    }
}

/// Exact shape/clip/transform evidence for the primary surface and popup suffixes.
pub(super) struct SurfaceOutputProof {
    expected: Vec<ClippedShape>,
    /// Popup suffixes in egui paint order, excluding the primary span.
    spans: Vec<SurfaceOutputSpan>,
    /// Insertion point of the primary span among the ordered popup suffixes.
    primary_span_index: usize,
    pixels_per_point: f32,
    begin: Arc<SurfaceMarker>,
    end: Arc<SurfaceMarker>,
}

/// An appended popup suffix, located by its unique private terminal marker.
struct SurfaceOutputSpan {
    expected: Vec<ClippedShape>,
    end: Arc<SurfaceMarker>,
}

impl std::fmt::Debug for SurfaceOutputProof {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("SurfaceOutputProof")
            .field("shapes", &self.expected.len())
            .field("additional_spans", &self.spans.len())
            .finish_non_exhaustive()
    }
}

impl SurfaceOutputProof {
    /// Returns the enclosing half-open output range, including every owned span.
    ///
    /// Every marker must occur exactly once in the entire output, including
    /// nested compounds. The primary begin/end slice and every popup suffix
    /// ending at its own marker must match their complete frozen contents.
    /// The spans must retain their captured egui layer order without overlap.
    /// The final pixel scale must exactly match the context scale at finish.
    /// Callers combining surfaces must also validate their relative order.
    pub(super) fn is_present_once(&self, output: &FullOutput) -> Option<(usize, usize)> {
        if output.pixels_per_point != self.pixels_per_point {
            return None;
        }
        let begin = unique_marker_index(&output.shapes, &self.begin)?;
        let end = unique_marker_index(&output.shapes, &self.end)?.checked_add(1)?;
        if output.shapes.get(begin..end)? != self.expected.as_slice() {
            return None;
        }
        let primary = (begin, end);
        let mut range: Option<(usize, usize)> = None;
        for index in 0..=self.spans.len() {
            let current = if index == self.primary_span_index {
                primary
            } else {
                let span = self.spans.get(if index < self.primary_span_index {
                    index
                } else {
                    index - 1
                })?;
                let end = unique_marker_index(&output.shapes, &span.end)?.checked_add(1)?;
                let begin = end.checked_sub(span.expected.len())?;
                if output.shapes.get(begin..end)? != span.expected.as_slice() {
                    return None;
                }
                (begin, end)
            };
            range = Some(if let Some((begin, end)) = range {
                if end > current.0 {
                    return None;
                }
                (begin, current.1)
            } else {
                current
            });
        }
        range
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
                let multilayer = SurfaceOutputCapture::begin_multilayer(painter);
                if !present_before_capture {
                    assert!(check.is_preserved(painter));
                    paint_unknown();
                }

                assert!(!check.is_preserved(painter));
                assert!(capture.finish(painter).is_none());
                assert!(multilayer.finish(painter).is_none());
            });
        }
    }

    fn show_foreground_popup(ui: &mut egui::Ui) {
        let owner = ui.button("Popup owner");
        let popup = egui::Id::unique("output-proof-real-popup");
        egui::Popup::open_id(ui.ctx(), popup);
        assert!(
            egui::Popup::menu(&owner)
                .id(popup)
                .open_memory(None)
                .show(|ui| {
                    ui.label("Native presentation commands");
                    let _ = ui.button("Move to new window");
                })
                .is_some()
        );
    }

    fn popup_fixture() -> (SurfaceOutputProof, FullOutput) {
        let context = Context::default();
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(640.0, 480.0),
            )),
            ..Default::default()
        };
        context
            .run_ui(input.clone(), show_foreground_popup)
            .drop_without_applying_deltas();
        let mut proof = None;
        let output = context.run_ui(input, |ui| {
            let capture = SurfaceOutputCapture::begin_multilayer(ui.painter());
            let strict = SurfaceOutputCapture::begin(ui.painter());
            let pane_check = PaintAppendCheck::begin(ui.painter());
            ui.painter()
                .circle_filled(egui::pos2(200.0, 200.0), 10.0, egui::Color32::BLUE);
            show_foreground_popup(ui);
            assert!(!pane_check.is_preserved(ui.painter()));
            assert!(
                strict.finish(ui.painter()).is_none(),
                "the official capture remains single-layer"
            );
            proof = capture.finish(ui.painter());
        });
        let proof = proof.expect("a real registered foreground popup preserves every layer prefix");
        assert!(
            !proof.spans.is_empty(),
            "the fixture must include the actual popup paint"
        );
        (proof, output)
    }

    #[test]
    fn multilayer_capture_accepts_a_real_popup_and_scrubs_all_private_markers() {
        let (proof, mut output) = popup_fixture();
        let begin = unique_marker_index(&output.shapes, &proof.begin).unwrap();
        let end = unique_marker_index(&output.shapes, &proof.spans[0].end).unwrap() + 1;
        assert_eq!(proof.is_present_once(&output), Some((begin, end)));
        scrub_private_markers(&mut output);
        assert!(output.shapes.iter().all(|shape| {
            marker_count(&shape.shape, &proof.begin) == 0
                && marker_count(&shape.shape, &proof.end) == 0
                && proof
                    .spans
                    .iter()
                    .all(|span| marker_count(&shape.shape, &span.end) == 0)
        }));
        assert!(
            !output.shapes.is_empty(),
            "popup and surface geometry survive marker cleanup"
        );
        output.textures_delta.clear();
    }

    #[test]
    fn multilayer_output_hooks_cannot_delete_or_change_popup_geometry() {
        let (proof, mut output) = popup_fixture();
        let span = &proof.spans[0];
        let end = unique_marker_index(&output.shapes, &span.end).unwrap();
        let start = end + 1 - span.expected.len();
        let geometry = start
            + span
                .expected
                .iter()
                .position(|shape| !matches!(shape.shape, Shape::Noop | Shape::Callback(_)))
                .expect("the popup contributes ordinary geometry");
        let mut removed = output.clone();
        removed.shapes.remove(geometry);
        assert_eq!(proof.is_present_once(&removed), None);
        removed.textures_delta.clear();
        let mut changed = output.clone();
        changed.shapes[geometry]
            .transform_after_tessellation
            .translation
            .x += 1.0;
        assert_eq!(proof.is_present_once(&changed), None);
        changed.textures_delta.clear();
        output.textures_delta.clear();
    }

    #[test]
    fn multilayer_output_hooks_cannot_duplicate_popup_markers() {
        let (proof, mut output) = popup_fixture();
        output
            .shapes
            .push(clipped(Shape::Vec(vec![Shape::Vec(vec![marker_shape(
                &proof.spans[0].end,
            )])])));
        assert_eq!(proof.is_present_once(&output), None);
        output.textures_delta.clear();
    }

    #[test]
    fn multilayer_output_hooks_cannot_swap_complete_primary_and_popup_spans() {
        let (proof, mut output) = popup_fixture();
        assert_eq!(proof.spans.len(), 1);
        assert!(proof.is_present_once(&output).is_some());
        let primary_begin = unique_marker_index(&output.shapes, &proof.begin).unwrap();
        let primary_end = unique_marker_index(&output.shapes, &proof.end).unwrap() + 1;
        let popup_end = unique_marker_index(&output.shapes, &proof.spans[0].end).unwrap() + 1;
        let popup_begin = popup_end - proof.spans[0].expected.len();
        assert!(primary_end <= popup_begin);
        let original = std::mem::take(&mut output.shapes);
        output.shapes.extend_from_slice(&original[..primary_begin]);
        output
            .shapes
            .extend_from_slice(&original[popup_begin..popup_end]);
        output
            .shapes
            .extend_from_slice(&original[primary_end..popup_begin]);
        output
            .shapes
            .extend_from_slice(&original[primary_begin..primary_end]);
        output.shapes.extend_from_slice(&original[popup_end..]);

        // Both complete payloads, including their unique markers, are intact.
        let swapped_primary_begin = unique_marker_index(&output.shapes, &proof.begin).unwrap();
        let swapped_primary_end = unique_marker_index(&output.shapes, &proof.end).unwrap() + 1;
        assert_eq!(
            output.shapes[swapped_primary_begin..swapped_primary_end],
            proof.expected
        );
        let swapped_popup_end =
            unique_marker_index(&output.shapes, &proof.spans[0].end).unwrap() + 1;
        assert_eq!(
            output.shapes[swapped_popup_end - proof.spans[0].expected.len()..swapped_popup_end],
            proof.spans[0].expected
        );
        assert_eq!(proof.is_present_once(&output), None);
        output.textures_delta.clear();
    }

    #[test]
    fn multilayer_capture_uses_egui_layer_order_instead_of_capture_traversal() {
        let context = Context::default();
        let mut proof = None;
        let mut output = context.run_ui(egui::RawInput::default(), |ui| {
            let primary = known_layer(ui.ctx(), "multilayer-ordered-primary");
            let painter = ui.ctx().layer_painter(primary);
            let capture = SurfaceOutputCapture::begin_multilayer(&painter);
            painter.circle_filled(egui::pos2(20.0, 20.0), 5.0, egui::Color32::BLUE);
            // Register Foreground before Background, so snapshot traversal is
            // deliberately different from egui's Order-first graphics drain.
            for order in [egui::Order::Foreground, egui::Order::Background] {
                let layer =
                    LayerId::new(order, egui::Id::unique(("multilayer-ordered-extra", order)));
                ui.memory_mut(|memory| {
                    memory
                        .areas_mut()
                        .set_state(layer, egui::AreaState::default())
                });
                ui.ctx().layer_painter(layer).circle_filled(
                    egui::pos2(30.0, 30.0),
                    5.0,
                    egui::Color32::GREEN,
                );
            }
            proof = capture.finish(&painter);
        });
        let proof = proof.expect("registered layers have explicit egui paint order");
        assert_eq!(proof.primary_span_index, 1);
        assert_eq!(proof.spans.len(), 2);
        assert!(proof.is_present_once(&output).is_some());
        output.textures_delta.clear();
    }

    #[test]
    fn multilayer_capture_rejects_changes_to_an_existing_other_layer_prefix_or_transform() {
        for change_transform in [false, true] {
            with_painter(|painter| {
                let other = known_layer(painter.ctx(), "multilayer-existing-prefix");
                let other_painter = painter.ctx().layer_painter(other);
                let original =
                    other_painter.circle_filled(egui::pos2(10.0, 10.0), 2.0, egui::Color32::RED);
                let capture = SurfaceOutputCapture::begin_multilayer(painter);
                other_painter.circle_filled(egui::pos2(20.0, 10.0), 2.0, egui::Color32::GREEN);
                if change_transform {
                    painter.ctx().set_transform_layer(
                        other,
                        egui::emath::TSTransform::from_translation(egui::vec2(1.0, 0.0)),
                    );
                } else {
                    other_painter.set(original, Shape::Noop);
                }
                assert!(capture.finish(painter).is_none());
            });
        }
    }

    #[test]
    fn multilayer_capture_accepts_new_registered_layers_and_freezes_their_transforms() {
        let context = Context::default();
        let mut proof = None;
        let mut output = context.run_ui(egui::RawInput::default(), |ui| {
            let capture = SurfaceOutputCapture::begin_multilayer(ui.painter());
            let layer = known_layer(ui.ctx(), "multilayer-new-layer");
            ui.ctx().set_transform_layer(
                layer,
                egui::emath::TSTransform::from_translation(egui::vec2(20.0, 10.0)),
            );
            ui.ctx().layer_painter(layer).circle_filled(
                egui::pos2(10.0, 10.0),
                2.0,
                egui::Color32::RED,
            );
            proof = capture.finish(ui.painter());
        });
        let proof = proof.expect("a newly registered layer has an empty original prefix");
        assert!(proof.is_present_once(&output).is_some());
        let end = unique_marker_index(&output.shapes, &proof.spans[0].end).unwrap();
        output.shapes[end]
            .transform_after_tessellation
            .translation
            .x += 1.0;
        assert_eq!(proof.is_present_once(&output), None);
        output.textures_delta.clear();
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
                spans: Vec::new(),
                primary_span_index: 0,
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
