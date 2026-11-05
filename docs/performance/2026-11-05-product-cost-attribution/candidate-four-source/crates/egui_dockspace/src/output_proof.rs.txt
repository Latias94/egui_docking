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

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use egui::epaint::{ClippedShape, Galley, PaintCallback, TextShape};
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

/// Reuses exact galley comparisons only during one synchronous surface paint.
#[derive(Default)]
pub(super) struct PaintComparison {
    // The strong owner fixes the value and prevents address reuse while memoized.
    galleys: HashMap<*const Galley, (Arc<Galley>, bool)>,
    different_owner_compared: bool,
}

/// Recycles only pure descriptors whose owners are already retained by the comparison.
#[derive(Default)]
pub(super) struct PaintSnapshotCache {
    reusable: Option<PaintSnapshot>,
}

impl PaintComparison {
    fn owns_galley(&self, galley: &Arc<Galley>) -> bool {
        self.galleys
            .get(&Arc::as_ptr(galley))
            .is_some_and(|(owner, _)| Arc::ptr_eq(owner, galley))
    }

    fn clipped_shapes_equal(&mut self, left: &ClippedShape, right: &ClippedShape) -> bool {
        let ClippedShape {
            clip_rect,
            shape,
            transform_after_tessellation,
        } = left;
        *clip_rect == right.clip_rect
            && self.shapes_equal(shape, &right.shape)
            && *transform_after_tessellation == right.transform_after_tessellation
    }

    fn shapes_equal(&mut self, left: &Shape, right: &Shape) -> bool {
        match (left, right) {
            (Shape::Vec(left), Shape::Vec(right)) => {
                left.len() == right.len()
                    && left
                        .iter()
                        .zip(right)
                        .all(|(left, right)| self.shapes_equal(left, right))
            }
            (Shape::Text(left), Shape::Text(right)) => self.text_shapes_equal(left, right),
            _ => left == right,
        }
    }

    fn text_shapes_equal(&mut self, left: &TextShape, right: &TextShape) -> bool {
        let TextShape {
            pos,
            galley,
            underline,
            fallback_color,
            override_text_color,
            opacity_factor,
            angle,
        } = left;
        *pos == right.pos
            && self.galleys_equal(galley, &right.galley)
            && *underline == right.underline
            && *fallback_color == right.fallback_color
            && *override_text_color == right.override_text_color
            && *opacity_factor == right.opacity_factor
            && *angle == right.angle
    }

    fn galleys_equal(&mut self, left: &Arc<Galley>, right: &Arc<Galley>) -> bool {
        if !Arc::ptr_eq(left, right) {
            self.different_owner_compared = true;
            return left == right;
        }
        self.galleys
            .entry(Arc::as_ptr(left))
            .or_insert_with(|| {
                // Preserve non-reflexive values and upstream debug assertions.
                let equal = left.as_ref() == right.as_ref();
                (Arc::clone(left), equal)
            })
            .1
    }
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

    /// Observes every live descriptor while reusing identical pure owned slots.
    pub(super) fn begin_reusing(painter: &Painter, cache: &mut PaintSnapshotCache) -> Self {
        Self {
            context: painter.ctx().clone(),
            viewport: painter.ctx().viewport_id(),
            layer: painter.layer_id(),
            before: cache.reusable.take().map_or_else(
                || snapshot(painter),
                |before| snapshot_reusing(painter, before),
            ),
        }
    }

    /// Checks and recycles one owned capture without exposing a reuse proof.
    pub(super) fn finish_reusing(
        self,
        painter: &Painter,
        cache: &mut PaintSnapshotCache,
        comparison: &mut PaintComparison,
        aggregate_valid: &mut bool,
    ) -> bool {
        comparison.different_owner_compared = false;
        let preserved = self.is_preserved_with_comparison(painter, comparison);
        *aggregate_valid &= preserved;
        // A successful full comparison visited and memoized every same-owner Text.
        let all_same_owner = preserved && !comparison.different_owner_compared;
        self.recycle_impl(cache, comparison, preserved, all_same_owner);
        preserved
    }

    /// Releases opaque payloads at the original boundary, outside the context lock.
    #[cfg(test)]
    pub(super) fn recycle(
        self,
        cache: &mut PaintSnapshotCache,
        comparison: &PaintComparison,
        preserved: bool,
    ) {
        self.recycle_impl(cache, comparison, preserved, false);
    }

    fn recycle_impl(
        mut self,
        cache: &mut PaintSnapshotCache,
        comparison: &PaintComparison,
        preserved: bool,
        all_same_owner: bool,
    ) {
        cache.reusable = None;
        if !preserved {
            return;
        }
        let Some(mut before) = self.before.take() else {
            return;
        };
        drop(self);
        for layer in std::iter::once(&mut before.current).chain(&mut before.others) {
            for clipped in &mut layer.shapes {
                let retain = match &clipped.shape {
                    Shape::Noop => true,
                    Shape::Text(text) => all_same_owner || comparison.owns_galley(&text.galley),
                    _ => false,
                };
                if !retain {
                    let released = std::mem::replace(&mut clipped.shape, Shape::Noop);
                    drop(released);
                }
            }
        }
        cache.reusable = Some(before);
    }

    /// Returns whether the original prefix and every other layer are unchanged.
    pub(super) fn is_preserved(&self, painter: &Painter) -> bool {
        if painter.ctx() != &self.context
            || painter.ctx().viewport_id() != self.viewport
            || painter.layer_id() != self.layer
        {
            return false;
        }
        self.before
            .as_ref()
            .is_some_and(|before| preserves_snapshot(painter, before))
    }

    /// Uses a caller-owned comparison scoped to this synchronous surface paint.
    pub(super) fn is_preserved_with_comparison(
        &self,
        painter: &Painter,
        comparison: &mut PaintComparison,
    ) -> bool {
        if painter.ctx() != &self.context
            || painter.ctx().viewport_id() != self.viewport
            || painter.layer_id() != self.layer
        {
            return false;
        }
        self.before.as_ref().is_some_and(|before| {
            preserves_snapshot_with(painter, before, |left, right| {
                comparison.clipped_shapes_equal(left, right)
            })
        })
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
        if self.multilayer {
            for layer in after.others {
                let prefix_len = before
                    .others
                    .iter()
                    .find(|previous| previous.layer == layer.layer)
                    .map_or(0, |previous| previous.shapes.len());
                if layer.shapes.len() == prefix_len {
                    continue;
                }
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
                spans.push(SurfaceOutputSpan {
                    layer: layer.layer,
                    expected,
                    end,
                });
            }
        }
        Some(SurfaceOutputProof {
            #[cfg(any(test, feature = "native-render-support"))]
            context: painter.ctx().clone(),
            expected,
            primary_layer: painter.layer_id(),
            spans,
            pixels_per_point: painter.ctx().pixels_per_point(),
            begin: self.begin,
            end,
        })
    }
}

/// Exact shape/clip/transform evidence for the primary surface and popup suffixes.
pub(super) struct SurfaceOutputProof {
    #[cfg(any(test, feature = "native-render-support"))]
    context: Context,
    expected: Vec<ClippedShape>,
    primary_layer: LayerId,
    /// Popup suffixes frozen by their layer identity, without an order claim.
    spans: Vec<SurfaceOutputSpan>,
    pixels_per_point: f32,
    begin: Arc<SurfaceMarker>,
    end: Arc<SurfaceMarker>,
}

/// An appended popup suffix, located by its unique private terminal marker.
struct SurfaceOutputSpan {
    layer: LayerId,
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
    /// Returns the enclosing half-open range for callers that own one surface.
    #[cfg(any(test, feature = "native-render-support"))]
    pub(super) fn is_present_once(&self, output: &FullOutput) -> Option<(usize, usize)> {
        let order = final_paint_layer_order(&self.context);
        self.is_present_once_in_paint_order(output, &order)
    }

    pub(super) fn is_present_once_in_paint_order(
        &self,
        output: &FullOutput,
        layer_order: &[LayerId],
    ) -> Option<(usize, usize)> {
        let evidence = self.present_spans(output)?;
        let mut spans = evidence
            .spans
            .into_iter()
            .enumerate()
            .map(|(index, span)| (0, index, span))
            .collect::<Vec<_>>();
        if !spans_follow_paint_order(&mut spans, layer_order) {
            return None;
        }
        Some((spans.first()?.2.range.0, spans.last()?.2.range.1))
    }

    /// Returns each exact owned interval with its egui layer identity.
    ///
    /// Every marker must occur exactly once in the entire output, including
    /// nested compounds. The primary slice and every popup suffix must match
    /// their complete frozen contents, and the final pixel scale must match.
    pub(super) fn present_spans(&self, output: &FullOutput) -> Option<SurfaceOutputSpans> {
        if output.pixels_per_point != self.pixels_per_point {
            return None;
        }
        let begin = unique_marker_index(&output.shapes, &self.begin)?;
        let end = unique_marker_index(&output.shapes, &self.end)?.checked_add(1)?;
        if output.shapes.get(begin..end)? != self.expected.as_slice() {
            return None;
        }
        let primary = (begin, end);
        let mut spans = Vec::with_capacity(self.spans.len() + 1);
        spans.push(SurfaceOutputSpanEvidence {
            layer: self.primary_layer,
            range: primary,
        });
        for span in &self.spans {
            let end = unique_marker_index(&output.shapes, &span.end)?.checked_add(1)?;
            let begin = end.checked_sub(span.expected.len())?;
            if output.shapes.get(begin..end)? != span.expected.as_slice() {
                return None;
            }
            spans.push(SurfaceOutputSpanEvidence {
                layer: span.layer,
                range: (begin, end),
            });
        }
        Some(SurfaceOutputSpans { spans })
    }
}

pub(super) struct SurfaceOutputSpans {
    pub(super) spans: Vec<SurfaceOutputSpanEvidence>,
}

pub(super) struct SurfaceOutputSpanEvidence {
    pub(super) layer: LayerId,
    pub(super) range: (usize, usize),
}

pub(super) fn final_paint_layer_order(context: &Context) -> Vec<LayerId> {
    context.memory(|memory| {
        let mut layers = memory.layer_ids().collect::<Vec<_>>();
        layers.sort_by_key(|layer| layer.order);
        layers
    })
}

pub(super) fn spans_follow_paint_order(
    spans: &mut [(usize, usize, SurfaceOutputSpanEvidence)],
    layer_order: &[LayerId],
) -> bool {
    for (_, _, span) in spans.iter() {
        if layer_order.contains(&span.layer) {
            continue;
        }
        // egui drains registered areas first, then unregistered graphics by
        // hash iteration within the same Order; that fallback is not provable.
        if spans.iter().any(|(_, _, other)| {
            other.layer != span.layer
                && other.layer.order == span.layer.order
                && !layer_order.contains(&other.layer)
        }) {
            return false;
        }
    }

    for (_, _, left) in spans.iter() {
        for (_, _, right) in spans.iter() {
            if left.layer.order != right.layer.order || left.layer == right.layer {
                continue;
            }
            if layer_order_rank(left.layer, right.layer, layer_order).is_none() {
                return false;
            }
        }
    }

    // Within one layer, synchronous participants append in enrollment order.
    spans.sort_by(|left, right| {
        let (left_participant, left_span, left) = left;
        let (right_participant, right_span, right) = right;
        left.layer.order.cmp(&right.layer.order).then_with(|| {
            if left.layer == right.layer {
                (left_participant, left_span).cmp(&(right_participant, right_span))
            } else {
                layer_order_rank(left.layer, right.layer, layer_order)
                    .unwrap_or(std::cmp::Ordering::Equal)
            }
        })
    });
    spans
        .windows(2)
        .all(|pair| pair[0].2.range.1 <= pair[1].2.range.0)
}

fn layer_order_rank(
    left: LayerId,
    right: LayerId,
    layer_order: &[LayerId],
) -> Option<std::cmp::Ordering> {
    match (
        layer_order.iter().position(|layer| *layer == left),
        layer_order.iter().position(|layer| *layer == right),
    ) {
        (Some(left), Some(right)) => Some(left.cmp(&right)),
        (Some(_), None) => Some(std::cmp::Ordering::Less),
        (None, Some(_)) => Some(std::cmp::Ordering::Greater),
        (None, None) => None,
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
    let layers = painter
        .ctx()
        .memory(|memory| memory.layer_ids().collect::<Vec<_>>());
    snapshot_with_layers(painter, layers)
}

fn checked_layer_ids(painter: &Painter, mut layers: Vec<LayerId>) -> Option<Vec<LayerId>> {
    if layers.len() > 1 {
        let mut unique = HashSet::with_capacity(layers.len());
        if !layers.iter().all(|layer| unique.insert(*layer)) {
            return None;
        }
    }
    if !layers.contains(&painter.layer_id()) {
        layers.push(painter.layer_id());
    }
    Some(layers)
}

fn snapshot_with_layers(painter: &Painter, layers: Vec<LayerId>) -> Option<PaintSnapshot> {
    let layers = checked_layer_ids(painter, layers)?;
    let layers = layers
        .into_iter()
        .map(|layer| (layer, painter.ctx().layer_transform_to_global(layer)))
        .collect::<Vec<_>>();
    painter.ctx().graphics(|graphics| {
        let mut remaining = graphics.clone();
        let mut extractor = egui::layers::GraphicLayers::default();
        let mut current = None;
        let mut others = Vec::new();
        for (layer, transform) in layers {
            let list = std::mem::take(remaining.entry(layer));
            let shapes = if list.is_empty() {
                Vec::new()
            } else {
                // PaintList has no owned iterator; drain this cloned layer without transforms.
                *extractor.entry(layer) = list;
                extractor.drain(&[], &Default::default())
            };
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

fn snapshot_reusing(painter: &Painter, before: PaintSnapshot) -> Option<PaintSnapshot> {
    let layers = painter
        .ctx()
        .memory(|memory| memory.layer_ids().collect::<Vec<_>>());
    snapshot_reusing_with_layers(painter, before, layers)
}

fn snapshot_reusing_with_layers(
    painter: &Painter,
    mut before: PaintSnapshot,
    layers: Vec<LayerId>,
) -> Option<PaintSnapshot> {
    let current_layer = painter.layer_id();
    let layers = checked_layer_ids(painter, layers)?;
    if before.current.layer != current_layer
        || !layers
            .iter()
            .copied()
            .filter(|layer| *layer != current_layer)
            .eq(before.others.iter().map(|layer| layer.layer))
    {
        drop(before);
        return snapshot(painter);
    }
    let layers = layers
        .into_iter()
        .map(|layer| (layer, painter.ctx().layer_transform_to_global(layer)))
        .collect::<Vec<_>>();
    painter.ctx().graphics_mut(|graphics| {
        if !no_unknown_shapes(graphics, layers.iter().map(|&(layer, _)| layer)) {
            return None;
        }
        let mut others = before.others.iter_mut();
        for (layer, transform) in layers {
            let captured = if layer == current_layer {
                &mut before.current
            } else {
                others
                    .next()
                    .expect("the complete layer roster was matched")
            };
            captured.transform = transform;
            refill_snapshot_shapes(&mut captured.shapes, graphics.get(layer));
        }
        Some(before)
    })
}

fn refill_snapshot_shapes(shapes: &mut Vec<ClippedShape>, live: Option<&egui::layers::PaintList>) {
    let Some(live) = live else {
        shapes.clear();
        return;
    };
    let entries = live.all_entries();
    let length = entries.len();
    for (index, live) in entries.enumerate() {
        if let Some(cached) = shapes.get_mut(index) {
            if !reusable_shape(cached, live) {
                *cached = live.clone();
            }
        } else {
            shapes.push(live.clone());
        }
    }
    shapes.truncate(length);
}

fn reusable_shape(cached: &ClippedShape, live: &ClippedShape) -> bool {
    let ClippedShape {
        clip_rect,
        shape,
        transform_after_tessellation,
    } = cached;
    raw_rect_bits(*clip_rect) == raw_rect_bits(live.clip_rect)
        && raw_transform_bits(*transform_after_tessellation)
            == raw_transform_bits(live.transform_after_tessellation)
        && match (shape, &live.shape) {
            (Shape::Noop, Shape::Noop) => true,
            (Shape::Text(cached), Shape::Text(live)) => {
                let TextShape {
                    pos,
                    galley,
                    underline,
                    fallback_color,
                    override_text_color,
                    opacity_factor,
                    angle,
                } = cached;
                let egui::Stroke { width, color } = *underline;
                raw_pos_bits(*pos) == raw_pos_bits(live.pos)
                    && Arc::ptr_eq(galley, &live.galley)
                    && width.to_bits() == live.underline.width.to_bits()
                    && color == live.underline.color
                    && *fallback_color == live.fallback_color
                    && *override_text_color == live.override_text_color
                    && opacity_factor.to_bits() == live.opacity_factor.to_bits()
                    && angle.to_bits() == live.angle.to_bits()
            }
            _ => false,
        }
}

fn raw_pos_bits(pos: egui::Pos2) -> [u32; 2] {
    let egui::Pos2 { x, y } = pos;
    [x.to_bits(), y.to_bits()]
}

fn raw_rect_bits(rect: egui::Rect) -> [[u32; 2]; 2] {
    let egui::Rect { min, max } = rect;
    [raw_pos_bits(min), raw_pos_bits(max)]
}

fn raw_transform_bits(transform: egui::emath::TSTransform) -> [u32; 3] {
    let egui::emath::TSTransform {
        scaling,
        translation: egui::Vec2 { x, y },
    } = transform;
    [scaling.to_bits(), x.to_bits(), y.to_bits()]
}

/// Borrows the live known lists and clones only the unenumerated remainder.
fn preserves_snapshot(painter: &Painter, before: &PaintSnapshot) -> bool {
    preserves_snapshot_with(painter, before, |left, right| left == right)
}

fn preserves_snapshot_with(
    painter: &Painter,
    before: &PaintSnapshot,
    shapes_equal: impl FnMut(&ClippedShape, &ClippedShape) -> bool,
) -> bool {
    let layers = painter
        .ctx()
        .memory(|memory| memory.layer_ids().collect::<Vec<_>>());
    preserves_snapshot_with_layers(painter, before, layers, shapes_equal)
}

fn preserves_snapshot_with_layers(
    painter: &Painter,
    before: &PaintSnapshot,
    layers: Vec<LayerId>,
    mut shapes_equal: impl FnMut(&ClippedShape, &ClippedShape) -> bool,
) -> bool {
    let Some(layers) = checked_layer_ids(painter, layers) else {
        return false;
    };
    let layers = layers
        .into_iter()
        .map(|layer| (layer, painter.ctx().layer_transform_to_global(layer)))
        .collect::<Vec<_>>();
    painter.ctx().graphics_mut(|graphics| {
        // A complete snapshot rejects unknown shapes before comparing any descriptor.
        if !no_unknown_shapes(graphics, layers.iter().map(|&(layer, _)| layer)) {
            return false;
        }

        let current_layer = painter.layer_id();
        let Some(&(layer, transform)) = layers.iter().rfind(|(layer, _)| *layer == current_layer)
        else {
            return false;
        };
        let expected = &before.current;
        if layer != expected.layer || transform != expected.transform {
            return false;
        }
        let current_matches = match graphics.get(layer) {
            Some(list) => clipped_shapes_match(
                list.all_entries().take(expected.shapes.len()),
                expected.shapes.iter(),
                &mut shapes_equal,
            ),
            None => expected.shapes.is_empty(),
        };
        if !current_matches {
            return false;
        }

        let others = layers.iter().filter(|(layer, _)| *layer != current_layer);
        if others.clone().count() != before.others.len() {
            return false;
        }
        others
            .zip(&before.others)
            .all(|(&(layer, transform), expected)| {
                layer == expected.layer
                    && transform == expected.transform
                    && match graphics.get(layer) {
                        Some(list) => clipped_shapes_match(
                            list.all_entries(),
                            expected.shapes.iter(),
                            &mut shapes_equal,
                        ),
                        None => expected.shapes.is_empty(),
                    }
            })
    })
}

fn no_unknown_shapes(
    graphics: &mut egui::layers::GraphicLayers,
    layers: impl ExactSizeIterator<Item = LayerId>,
) -> bool {
    let mut restore = PaintListRestore {
        graphics,
        lists: Vec::with_capacity(layers.len()),
    };
    for layer in layers {
        if let Some(list) = restore.graphics.get_mut(layer) {
            restore.lists.push((layer, std::mem::take(list)));
        }
    }
    let mut remaining = restore.graphics.clone();
    let empty = remaining.drain(&[], &Default::default()).is_empty();
    drop(restore);
    empty
}

fn clipped_shapes_match<'a, 'b>(
    left: impl ExactSizeIterator<Item = &'a ClippedShape>,
    right: impl ExactSizeIterator<Item = &'b ClippedShape>,
    shapes_equal: &mut impl FnMut(&ClippedShape, &ClippedShape) -> bool,
) -> bool {
    left.len() == right.len()
        && left
            .zip(right)
            .all(|(left, right)| shapes_equal(left, right))
}

/// Restores existing slots even if residual inspection unwinds.
struct PaintListRestore<'a> {
    graphics: &'a mut egui::layers::GraphicLayers,
    lists: Vec<(LayerId, egui::layers::PaintList)>,
}

impl Drop for PaintListRestore<'_> {
    fn drop(&mut self) {
        for (layer, list) in self.lists.drain(..).rev() {
            *self
                .graphics
                .get_mut(layer)
                .expect("residual inspection never removes an existing layer slot") = list;
        }
    }
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

    // Frozen before ownership extraction, including the separate shape clone
    // and residual-layer rejection, so the oracle stays independent.
    fn frozen_snapshot(painter: &Painter) -> Option<PaintSnapshot> {
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
            if !remaining.drain(&[], &Default::default()).is_empty() {
                return None;
            }
            Some(PaintSnapshot {
                current: current?,
                others,
            })
        })
    }

    fn transform_bits(transform: egui::emath::TSTransform) -> [u32; 3] {
        [
            transform.scaling.to_bits(),
            transform.translation.x.to_bits(),
            transform.translation.y.to_bits(),
        ]
    }

    fn assert_snapshot_matches_frozen(painter: &Painter) -> Option<PaintSnapshot> {
        let expected = frozen_snapshot(painter);
        let actual = snapshot(painter);
        assert_eq!(actual.is_some(), expected.is_some());
        if let (Some(actual), Some(expected)) = (&actual, &expected) {
            assert_eq!(actual.others.len(), expected.others.len());
            for (actual, expected) in std::iter::once(&actual.current)
                .chain(&actual.others)
                .zip(std::iter::once(&expected.current).chain(&expected.others))
            {
                assert_eq!(actual.layer, expected.layer);
                assert_eq!(
                    actual.transform.map(transform_bits),
                    expected.transform.map(transform_bits)
                );
                assert_eq!(actual.shapes, expected.shapes);
                for (actual, expected) in actual.shapes.iter().zip(&expected.shapes) {
                    let clip_bits = |rect: egui::Rect| {
                        [
                            rect.min.x.to_bits(),
                            rect.min.y.to_bits(),
                            rect.max.x.to_bits(),
                            rect.max.y.to_bits(),
                        ]
                    };
                    assert_eq!(clip_bits(actual.clip_rect), clip_bits(expected.clip_rect));
                    assert_eq!(
                        transform_bits(actual.transform_after_tessellation),
                        transform_bits(expected.transform_after_tessellation)
                    );
                }
            }
        }
        actual
    }

    fn frozen_append_is_preserved(check: &PaintAppendCheck, painter: &Painter) -> bool {
        painter.ctx() == &check.context
            && painter.ctx().viewport_id() == check.viewport
            && painter.layer_id() == check.layer
            && check.before.as_ref().is_some_and(|before| {
                frozen_snapshot(painter).is_some_and(|after| {
                    after.current.layer == before.current.layer
                        && after.current.transform == before.current.transform
                        && after.current.shapes.get(..before.current.shapes.len())
                            == Some(before.current.shapes.as_slice())
                        && after.others == before.others
                })
            })
    }

    fn assert_append_matches_frozen(check: &PaintAppendCheck, painter: &Painter) -> bool {
        let expected = frozen_append_is_preserved(check, painter);
        assert_eq!(check.is_preserved(painter), expected);
        let mut comparison = PaintComparison::default();
        for _ in 0..2 {
            assert_eq!(
                check.is_preserved_with_comparison(painter, &mut comparison),
                expected
            );
        }
        expected
    }

    fn assert_append_outcome_matches_frozen(
        check: &PaintAppendCheck,
        painter: &Painter,
        comparison: &mut PaintComparison,
    ) -> Result<bool, ()> {
        let expected = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            frozen_append_is_preserved(check, painter)
        }))
        .map_err(|_| ());
        let plain =
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| check.is_preserved(painter)))
                .map_err(|_| ());
        let memoized = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            check.is_preserved_with_comparison(painter, comparison)
        }))
        .map_err(|_| ());
        assert_eq!(plain, expected);
        assert_eq!(memoized, expected);
        expected
    }

    fn assert_comparison_matches_derived(
        comparison: &mut PaintComparison,
        left: &ClippedShape,
        right: &ClippedShape,
    ) -> Result<bool, ()> {
        let expected = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| left == right))
            .map_err(|_| ());
        let actual = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            comparison.clipped_shapes_equal(left, right)
        }))
        .map_err(|_| ());
        assert_eq!(actual, expected);
        expected
    }

    fn text_fixture(painter: &Painter) -> ClippedShape {
        let galley = painter.layout_no_wrap(
            "Memoized content".to_owned(),
            egui::FontId::proportional(14.0),
            egui::Color32::WHITE,
        );
        ClippedShape::new(
            painter.clip_rect(),
            TextShape::new(egui::pos2(10.0, 10.0), galley, egui::Color32::WHITE),
        )
    }

    fn fixture_text_mut(shape: &mut ClippedShape) -> &mut TextShape {
        let Shape::Text(text) = &mut shape.shape else {
            panic!("the fixture contains one text shape");
        };
        text
    }

    fn frozen_begin(painter: &Painter) -> PaintAppendCheck {
        PaintAppendCheck {
            context: painter.ctx().clone(),
            viewport: painter.ctx().viewport_id(),
            layer: painter.layer_id(),
            before: frozen_snapshot(painter),
        }
    }

    fn assert_reusing_outcome_matches_frozen(
        actual: &PaintAppendCheck,
        expected: &PaintAppendCheck,
        painter: &Painter,
        comparison: &mut PaintComparison,
    ) -> Result<bool, ()> {
        let expected = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            frozen_append_is_preserved(expected, painter)
        }))
        .map_err(|_| ());
        let actual = assert_append_outcome_matches_frozen(actual, painter, comparison);
        assert_eq!(actual, expected);
        expected
    }

    fn prime_snapshot_cache(
        painter: &Painter,
        cache: &mut PaintSnapshotCache,
        comparison: &mut PaintComparison,
    ) {
        let check = PaintAppendCheck::begin_reusing(painter, cache);
        assert!(check.is_preserved_with_comparison(painter, comparison));
        check.recycle(cache, comparison, true);
    }

    struct ReenterOnDrop {
        context: Context,
        layer: LayerId,
        clip: egui::Rect,
        observed: Option<std::sync::Weak<Galley>>,
        events: Arc<std::sync::Mutex<Vec<(&'static str, bool)>>>,
        name: &'static str,
        panic: bool,
    }

    impl Drop for ReenterOnDrop {
        fn drop(&mut self) {
            let alive = self
                .observed
                .as_ref()
                .is_some_and(|owner| owner.upgrade().is_some());
            self.events.lock().unwrap().push((self.name, alive));
            self.context.graphics_mut(|graphics| {
                graphics.entry(self.layer).add(self.clip, Shape::Noop);
            });
            assert!(!self.panic, "the callback destructor deliberately panics");
        }
    }

    fn reentrant_callback(
        painter: &Painter,
        events: &Arc<std::sync::Mutex<Vec<(&'static str, bool)>>>,
        name: &'static str,
        observed: Option<std::sync::Weak<Galley>>,
    ) -> Shape {
        Shape::Callback(PaintCallback {
            rect: painter.clip_rect(),
            callback: Arc::new(ReenterOnDrop {
                context: painter.ctx().clone(),
                layer: painter.layer_id(),
                clip: painter.clip_rect(),
                observed,
                events: Arc::clone(events),
                name,
                panic: false,
            }),
        })
    }

    #[test]
    fn finish_reusing_matches_frozen_and_resets_its_probe_for_each_check() {
        with_painter(|painter| {
            let original = text_fixture(painter);
            let slot = painter.add(original.shape.clone());
            let mut comparison = PaintComparison::default();
            let mut cache = PaintSnapshotCache::default();
            let mut aggregate = true;
            for stage in 0..4 {
                let expected = frozen_begin(painter);
                let actual = PaintAppendCheck::begin_reusing(painter, &mut cache);
                if stage == 0 {
                    let mut replacement = original.clone();
                    let text = fixture_text_mut(&mut replacement);
                    text.galley = Arc::new(text.galley.as_ref().clone());
                    painter.set(slot, replacement.shape);
                } else if stage == 2 {
                    painter.ctx().graphics_mut(|graphics| {
                        graphics
                            .get_mut(painter.layer_id())
                            .unwrap()
                            .mutate_shape(slot, |shape| {
                                fixture_text_mut(shape).underline.color = egui::Color32::RED
                            });
                    });
                }
                let frozen = frozen_append_is_preserved(&expected, painter);
                let actual =
                    actual.finish_reusing(painter, &mut cache, &mut comparison, &mut aggregate);
                assert_eq!(actual, frozen);
                assert_eq!(actual, stage != 2);
                assert_eq!(aggregate, stage < 2);
                assert_eq!(comparison.different_owner_compared, stage == 0);
                assert_eq!(cache.reusable.is_some(), actual);
            }
        });
    }

    #[test]
    fn duplicate_layer_ids_reject_all_capture_paths_before_shape_comparison() {
        with_painter(|painter| {
            let other = known_layer(painter.ctx(), "duplicate-roster-other");
            painter.add(Shape::Noop);
            let before = frozen_snapshot(painter).unwrap();
            for layers in [
                vec![painter.layer_id(), painter.layer_id()],
                vec![other, other, painter.layer_id()],
            ] {
                assert!(snapshot_with_layers(painter, layers.clone()).is_none());
                assert!(
                    snapshot_reusing_with_layers(
                        painter,
                        snapshot(painter).unwrap(),
                        layers.clone(),
                    )
                    .is_none()
                );
                assert!(!preserves_snapshot_with_layers(
                    painter,
                    &before,
                    layers,
                    |_, _| panic!("invalid rosters must not compare any shapes"),
                ));
            }
        });
    }

    #[test]
    fn duplicate_pane_roster_cannot_authorize_changes_inside_a_unique_outer_span() {
        with_painter(|painter| {
            let outer = SurfaceOutputCapture::begin_multilayer(painter);
            let prefix = painter.add(text_fixture(painter).shape);
            let frozen = frozen_begin(painter);
            let mut layers = painter
                .ctx()
                .memory(|memory| memory.layer_ids().collect::<Vec<_>>());
            if !layers.contains(&painter.layer_id()) {
                layers.push(painter.layer_id());
            }
            layers.push(painter.layer_id());
            let check = PaintAppendCheck {
                context: painter.ctx().clone(),
                viewport: painter.ctx().viewport_id(),
                layer: painter.layer_id(),
                before: snapshot_with_layers(painter, layers),
            };
            painter.set(prefix, Shape::Noop);
            assert!(!frozen_append_is_preserved(&frozen, painter));
            let mut comparison = PaintComparison::default();
            let mut cache = PaintSnapshotCache::default();
            let mut aggregate = true;
            assert!(!check.finish_reusing(painter, &mut cache, &mut comparison, &mut aggregate));
            assert!(!aggregate);
            assert!(cache.reusable.is_none());
            // The terminal span cannot repair a failed pane's execution evidence.
            assert!(outer.finish(painter).is_some());
        });
    }

    #[test]
    fn duplicate_outer_roster_remains_unavailable_when_the_final_roster_is_unique() {
        with_painter(|painter| {
            let append_check = PaintAppendCheck {
                context: painter.ctx().clone(),
                viewport: painter.ctx().viewport_id(),
                layer: painter.layer_id(),
                before: snapshot_with_layers(painter, vec![painter.layer_id(), painter.layer_id()]),
            };
            let begin = Arc::new(SurfaceMarker);
            paint_marker(painter, &begin);
            let capture = SurfaceOutputCapture {
                append_check,
                begin,
                multilayer: true,
            };
            painter.add(Shape::Noop);
            assert!(capture.finish(painter).is_none());
        });
    }

    #[test]
    fn snapshot_cache_observes_every_text_field_and_gap_mutation_before_reuse() {
        for field in 0..16 {
            with_painter(|painter| {
                let original = text_fixture(painter);
                let slot = painter.add(original.shape.clone());
                painter.add(Shape::Noop);
                let mut comparison = PaintComparison::default();
                let mut cache = PaintSnapshotCache::default();
                prime_snapshot_cache(painter, &mut cache, &mut comparison);
                let buffer = cache.reusable.as_ref().unwrap().current.shapes.as_ptr();
                painter.ctx().graphics_mut(|graphics| {
                    graphics
                        .get_mut(painter.layer_id())
                        .unwrap()
                        .mutate_shape(slot, |shape| match field {
                            0 => shape.clip_rect.min.x += 1.0,
                            1 => shape.clip_rect.min.y += 1.0,
                            2 => shape.clip_rect.max.x -= 1.0,
                            3 => shape.clip_rect.max.y -= 1.0,
                            4 => shape.transform_after_tessellation.scaling = 2.0,
                            5 => shape.transform_after_tessellation.translation.x = 1.0,
                            6 => shape.transform_after_tessellation.translation.y = 1.0,
                            _ => {
                                let text = fixture_text_mut(shape);
                                match field {
                                    7 => text.pos.x += 1.0,
                                    8 => text.pos.y += 1.0,
                                    9 => text.underline.width = 1.0,
                                    10 => text.underline.color = egui::Color32::RED,
                                    11 => text.fallback_color = egui::Color32::RED,
                                    12 => text.override_text_color = Some(egui::Color32::GREEN),
                                    13 => text.opacity_factor = 0.5,
                                    14 => text.angle = 0.5,
                                    15 => text.galley = Arc::new(text.galley.as_ref().clone()),
                                    _ => unreachable!(),
                                }
                            }
                        });
                });
                let expected = frozen_begin(painter);
                let actual = PaintAppendCheck::begin_reusing(painter, &mut cache);
                assert!(cache.reusable.is_none());
                let before = actual.before.as_ref().unwrap();
                assert_eq!(before.current.shapes.as_ptr(), buffer);
                assert_eq!(
                    before.current.shapes,
                    expected.before.as_ref().unwrap().current.shapes
                );
                // Restore A after the inter-callback change to B; B must be the baseline.
                painter.set(slot, original.shape);
                let result = assert_reusing_outcome_matches_frozen(
                    &actual,
                    &expected,
                    painter,
                    &mut comparison,
                );
                assert_eq!(result, Ok(field == 15));
                actual.recycle(&mut cache, &comparison, result.unwrap());
            });
        }
    }

    #[test]
    fn snapshot_cache_refreshes_global_transforms_and_layer_rosters() {
        for change in 0..4 {
            with_painter(|painter| {
                let first = known_layer(painter.ctx(), "cache-metadata-first");
                let _ = known_layer(painter.ctx(), "cache-metadata-second");
                painter.add(text_fixture(painter).shape);
                painter.ctx().layer_painter(first).add(Shape::Noop);
                let mut comparison = PaintComparison::default();
                let mut cache = PaintSnapshotCache::default();
                prime_snapshot_cache(painter, &mut cache, &mut comparison);
                match change {
                    0 => painter.ctx().set_transform_layer(
                        painter.layer_id(),
                        egui::emath::TSTransform::from_translation(egui::vec2(2.0, 3.0)),
                    ),
                    1 => {
                        let _ = known_layer(painter.ctx(), "cache-metadata-added");
                    }
                    2 => {
                        let layers = painter
                            .ctx()
                            .memory(|memory| memory.layer_ids().collect::<Vec<_>>());
                        let mut memory = egui::Memory::default();
                        for layer in layers.into_iter().rev() {
                            memory
                                .areas_mut()
                                .set_state(layer, egui::AreaState::default());
                        }
                        painter.ctx().memory_mut(|current| *current = memory);
                    }
                    3 => painter.ctx().set_transform_layer(
                        painter.layer_id(),
                        egui::emath::TSTransform::from_scaling(f32::NAN),
                    ),
                    _ => unreachable!(),
                }
                let expected = frozen_begin(painter);
                let actual = PaintAppendCheck::begin_reusing(painter, &mut cache);
                let before = actual.before.as_ref().unwrap();
                let frozen = expected.before.as_ref().unwrap();
                assert_eq!(
                    before.current.transform.map(transform_bits),
                    frozen.current.transform.map(transform_bits)
                );
                assert_eq!(
                    before
                        .others
                        .iter()
                        .map(|layer| layer.layer)
                        .collect::<Vec<_>>(),
                    frozen
                        .others
                        .iter()
                        .map(|layer| layer.layer)
                        .collect::<Vec<_>>()
                );
                let result = assert_reusing_outcome_matches_frozen(
                    &actual,
                    &expected,
                    painter,
                    &mut comparison,
                );
                assert_eq!(result, Ok(change != 3));
                actual.recycle(&mut cache, &comparison, result.unwrap());
                if change == 3 {
                    assert!(cache.reusable.is_none());
                }
            });
        }
    }

    #[test]
    fn snapshot_cache_keeps_unknown_before_unavailable_after_its_removal() {
        with_painter(|painter| {
            painter.add(text_fixture(painter).shape);
            let mut comparison = PaintComparison::default();
            let mut cache = PaintSnapshotCache::default();
            prime_snapshot_cache(painter, &mut cache, &mut comparison);
            let unknown = LayerId::new(egui::Order::Foreground, egui::Id::unique("cache-unknown"));
            painter.ctx().layer_painter(unknown).add(Shape::Noop);
            let expected = frozen_begin(painter);
            let actual = PaintAppendCheck::begin_reusing(painter, &mut cache);
            assert!(actual.before.is_none());
            painter.ctx().graphics_mut(|graphics| {
                *graphics.get_mut(unknown).unwrap() = egui::layers::PaintList::default();
            });
            assert_eq!(
                assert_reusing_outcome_matches_frozen(&actual, &expected, painter, &mut comparison),
                Ok(false)
            );
            actual.recycle(&mut cache, &comparison, false);
            assert!(cache.reusable.is_none());
            prime_snapshot_cache(painter, &mut cache, &mut comparison);
        });
    }

    #[test]
    fn snapshot_cache_captures_float_bits_without_running_begin_equality() {
        for nan in [0x7fc0_0001, 0x7fc0_1234] {
            with_painter(|painter| {
                let slot = painter.add(text_fixture(painter).shape);
                painter.add(Shape::Noop);
                let mut comparison = PaintComparison::default();
                let mut cache = PaintSnapshotCache::default();
                prime_snapshot_cache(painter, &mut cache, &mut comparison);
                painter.ctx().graphics_mut(|graphics| {
                    graphics
                        .get_mut(painter.layer_id())
                        .unwrap()
                        .mutate_shape(slot, |shape| {
                            shape.clip_rect.max.x = f32::from_bits(nan);
                            shape.transform_after_tessellation.translation.x = -0.0;
                            fixture_text_mut(shape).angle = -0.0;
                        });
                });
                let expected = frozen_begin(painter);
                let actual = PaintAppendCheck::begin_reusing(painter, &mut cache);
                let shape = &actual.before.as_ref().unwrap().current.shapes[0];
                assert_eq!(shape.clip_rect.max.x.to_bits(), nan);
                assert_eq!(
                    shape.transform_after_tessellation.translation.x.to_bits(),
                    (-0.0_f32).to_bits()
                );
                let Shape::Text(text) = &shape.shape else {
                    unreachable!()
                };
                assert_eq!(text.angle.to_bits(), (-0.0_f32).to_bits());
                let first = painter.ctx().graphics(|graphics| {
                    graphics
                        .get(painter.layer_id())
                        .unwrap()
                        .all_entries()
                        .next()
                        .unwrap()
                        .clone()
                });
                painter.ctx().graphics_mut(|graphics| {
                    let list = graphics.get_mut(painter.layer_id()).unwrap();
                    *list = egui::layers::PaintList::default();
                    let index = list.add(first.clip_rect, first.shape.clone());
                    list.mutate_shape(index, |shape| {
                        shape.transform_after_tessellation = first.transform_after_tessellation
                    });
                });
                assert_eq!(
                    assert_reusing_outcome_matches_frozen(
                        &actual,
                        &expected,
                        painter,
                        &mut comparison
                    ),
                    Ok(false)
                );
                actual.recycle(&mut cache, &comparison, false);
                assert!(cache.reusable.is_none());
            });
        }
    }

    #[test]
    fn snapshot_cache_releases_unsupported_callbacks_before_the_next_begin() {
        for reusing in [false, true] {
            for nested in [false, true] {
                with_painter(|painter| {
                    let events = Arc::new(std::sync::Mutex::new(Vec::new()));
                    let callback = reentrant_callback(painter, &events, "released", None);
                    painter.add(if nested {
                        Shape::Vec(vec![callback])
                    } else {
                        callback
                    });
                    let mut comparison = PaintComparison::default();
                    let mut cache = PaintSnapshotCache::default();
                    let check = if reusing {
                        PaintAppendCheck::begin_reusing(painter, &mut cache)
                    } else {
                        frozen_begin(painter)
                    };
                    assert!(check.is_preserved_with_comparison(painter, &mut comparison));
                    if reusing {
                        check.recycle(&mut cache, &comparison, true);
                    } else {
                        drop(check);
                    }
                    let removed = painter.ctx().graphics_mut(|graphics| {
                        std::mem::take(graphics.get_mut(painter.layer_id()).unwrap())
                    });
                    drop(removed);
                    assert_eq!(*events.lock().unwrap(), vec![("released", false)]);
                    let expected = frozen_begin(painter);
                    let actual = PaintAppendCheck::begin_reusing(painter, &mut cache);
                    assert_eq!(
                        assert_reusing_outcome_matches_frozen(
                            &actual,
                            &expected,
                            painter,
                            &mut comparison
                        ),
                        Ok(true)
                    );
                    actual.recycle(&mut cache, &comparison, true);
                });
            }
        }
    }

    #[test]
    fn failed_snapshot_cache_preserves_text_and_callback_drop_order() {
        for reusing in [false, true] {
            with_painter(|painter| {
                let events = Arc::new(std::sync::Mutex::new(Vec::new()));
                let mut weak = Vec::new();
                for name in ["first", "second"] {
                    let mut shape = text_fixture(painter);
                    let text = fixture_text_mut(&mut shape);
                    text.galley = Arc::new(text.galley.as_ref().clone());
                    let observed = Arc::downgrade(&text.galley);
                    weak.push(observed.clone());
                    painter.add(shape.shape);
                    painter.add(reentrant_callback(painter, &events, name, Some(observed)));
                }
                let mut comparison = PaintComparison::default();
                let mut cache = PaintSnapshotCache::default();
                let check = if reusing {
                    PaintAppendCheck::begin_reusing(painter, &mut cache)
                } else {
                    frozen_begin(painter)
                };
                let removed = painter.ctx().graphics_mut(|graphics| {
                    std::mem::take(graphics.get_mut(painter.layer_id()).unwrap())
                });
                drop(removed);
                assert!(!check.is_preserved_with_comparison(painter, &mut comparison));
                assert!(comparison.galleys.is_empty());
                if reusing {
                    check.recycle(&mut cache, &comparison, false);
                } else {
                    drop(check);
                }
                assert!(cache.reusable.is_none());
                assert!(weak.iter().all(|owner| owner.upgrade().is_none()));
                assert_eq!(
                    *events.lock().unwrap(),
                    vec![("first", false), ("second", false)]
                );
            });
        }
    }

    #[test]
    fn snapshot_cache_releases_equal_text_owners_not_retained_by_the_memo() {
        for mode in 0..3 {
            with_painter(|painter| {
                let events = Arc::new(std::sync::Mutex::new(Vec::new()));
                let mut original = text_fixture(painter);
                let text = fixture_text_mut(&mut original);
                text.galley = Arc::new(text.galley.as_ref().clone());
                let observed = Arc::downgrade(&text.galley);
                let mut replacement = original.clone();
                let text = fixture_text_mut(&mut replacement);
                text.galley = Arc::new(text.galley.as_ref().clone());
                let slot = painter.add(original.shape);
                painter.add(reentrant_callback(
                    painter,
                    &events,
                    "unmemoized",
                    Some(observed.clone()),
                ));
                let mut comparison = PaintComparison::default();
                let mut cache = PaintSnapshotCache::default();
                let check = if mode == 0 {
                    frozen_begin(painter)
                } else {
                    PaintAppendCheck::begin_reusing(painter, &mut cache)
                };
                painter.set(slot, replacement.shape);
                assert!(frozen_append_is_preserved(&check, painter));
                if mode == 2 {
                    let mut aggregate = true;
                    assert!(check.finish_reusing(
                        painter,
                        &mut cache,
                        &mut comparison,
                        &mut aggregate,
                    ));
                    assert!(aggregate);
                } else {
                    assert!(check.is_preserved_with_comparison(painter, &mut comparison));
                    if mode == 1 {
                        check.recycle(&mut cache, &comparison, true);
                    } else {
                        drop(check);
                    }
                }
                assert!(comparison.galleys.is_empty());
                assert!(observed.upgrade().is_none());
                let removed = painter.ctx().graphics_mut(|graphics| {
                    std::mem::take(graphics.get_mut(painter.layer_id()).unwrap())
                });
                drop(removed);
                assert_eq!(*events.lock().unwrap(), vec![("unmemoized", false)]);
            });
        }
    }

    #[test]
    fn snapshot_cache_stays_empty_when_an_opaque_drop_reenters_and_panics() {
        for mode in 0..3 {
            with_painter(|painter| {
                let events = Arc::new(std::sync::Mutex::new(Vec::new()));
                painter.add(Shape::Callback(PaintCallback {
                    rect: painter.clip_rect(),
                    callback: Arc::new(ReenterOnDrop {
                        context: painter.ctx().clone(),
                        layer: painter.layer_id(),
                        clip: painter.clip_rect(),
                        observed: None,
                        events: Arc::clone(&events),
                        name: "panics",
                        panic: true,
                    }),
                }));
                painter.add(reentrant_callback(painter, &events, "after-panic", None));
                let mut comparison = PaintComparison::default();
                let mut cache = PaintSnapshotCache::default();
                let check = if mode == 0 {
                    frozen_begin(painter)
                } else {
                    PaintAppendCheck::begin_reusing(painter, &mut cache)
                };
                if mode != 2 {
                    assert!(check.is_preserved_with_comparison(painter, &mut comparison));
                }
                let removed = painter.ctx().graphics_mut(|graphics| {
                    std::mem::take(graphics.get_mut(painter.layer_id()).unwrap())
                });
                drop(removed);
                let mut aggregate = true;
                let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    if mode == 2 {
                        check.finish_reusing(painter, &mut cache, &mut comparison, &mut aggregate);
                    } else if mode == 1 {
                        check.recycle(&mut cache, &comparison, true);
                    } else {
                        drop(check);
                    }
                }));
                assert!(outcome.is_err());
                assert_eq!(aggregate, mode != 2);
                assert!(cache.reusable.is_none());
                assert_eq!(
                    *events.lock().unwrap(),
                    vec![("panics", false), ("after-panic", false)]
                );
            });
        }
    }

    #[test]
    fn paint_comparison_matches_every_text_and_clipped_shape_field() {
        with_painter(|painter| {
            let original = text_fixture(painter);
            let mut comparison = PaintComparison::default();
            for _ in 0..2 {
                assert_eq!(
                    assert_comparison_matches_derived(&mut comparison, &original, &original),
                    Ok(true)
                );
            }
            let mut equal_owner = original.clone();
            let text = fixture_text_mut(&mut equal_owner);
            text.galley = Arc::new(text.galley.as_ref().clone());
            assert_eq!(
                assert_comparison_matches_derived(&mut comparison, &original, &equal_owner),
                Ok(true)
            );
            for field in 0..9 {
                let mut changed = original.clone();
                match field {
                    0 => changed.clip_rect.max.x -= 1.0,
                    1 => changed.transform_after_tessellation.scaling = 2.0,
                    _ => {
                        let text = fixture_text_mut(&mut changed);
                        match field {
                            2 => text.pos.x += 1.0,
                            3 => Arc::make_mut(&mut text.galley).num_vertices += 1,
                            4 => text.underline = egui::Stroke::new(1.0, egui::Color32::RED),
                            5 => text.fallback_color = egui::Color32::RED,
                            6 => text.override_text_color = Some(egui::Color32::GREEN),
                            7 => text.opacity_factor = 0.5,
                            8 => text.angle = 0.25,
                            _ => unreachable!(),
                        }
                    }
                }
                assert_eq!(
                    assert_comparison_matches_derived(&mut comparison, &original, &changed),
                    Ok(false),
                    "field {field}"
                );
            }
        });
    }

    #[test]
    fn paint_comparison_preserves_text_prefixes_across_successive_append_checks() {
        with_painter(|painter| {
            let original = text_fixture(painter);
            let prefix = painter.add(original.shape.clone());
            let mut comparison = PaintComparison::default();
            let first = PaintAppendCheck::begin(painter);
            painter.add(Shape::Noop);
            assert!(first.is_preserved_with_comparison(painter, &mut comparison));
            let second = PaintAppendCheck::begin(painter);
            let mut replacement = original.clone();
            let text = fixture_text_mut(&mut replacement);
            text.galley = Arc::new(text.galley.as_ref().clone());
            painter.set(prefix, replacement.shape.clone());
            for check in [&first, &second] {
                assert!(frozen_append_is_preserved(check, painter));
                assert!(check.is_preserved_with_comparison(painter, &mut comparison));
            }
            let third = PaintAppendCheck::begin(painter);
            assert!(third.is_preserved_with_comparison(painter, &mut comparison));
            fixture_text_mut(&mut replacement).opacity_factor = 0.5;
            painter.set(prefix, replacement.shape);
            for check in [&first, &second, &third] {
                assert!(!frozen_append_is_preserved(check, painter));
                assert!(!check.is_preserved_with_comparison(painter, &mut comparison));
            }
        });
    }

    #[test]
    fn paint_comparison_keeps_owners_alive_and_cow_changes_distinct() {
        with_painter(|painter| {
            let mut original = text_fixture(painter);
            let text = fixture_text_mut(&mut original);
            // Keep this owner independent of the font layout cache.
            text.galley = Arc::new(text.galley.as_ref().clone());
            let mut galley = Arc::clone(&text.galley);
            let old = Arc::downgrade(&galley);
            let mut comparison = PaintComparison::default();
            assert_eq!(
                assert_comparison_matches_derived(&mut comparison, &original, &original),
                Ok(true)
            );
            drop(original);
            assert!(Arc::get_mut(&mut galley).is_none());
            Arc::make_mut(&mut galley).num_vertices += 1;
            let previous = old
                .upgrade()
                .expect("the comparison retains its exact owner");
            assert!(!Arc::ptr_eq(&previous, &galley));
            let make_shape = |galley| {
                ClippedShape::new(
                    painter.clip_rect(),
                    TextShape::new(egui::pos2(10.0, 10.0), galley, egui::Color32::WHITE),
                )
            };
            let previous = make_shape(previous);
            let changed = make_shape(galley);
            assert_eq!(
                assert_comparison_matches_derived(&mut comparison, &previous, &changed),
                Ok(false)
            );
            assert_eq!(
                assert_comparison_matches_derived(&mut comparison, &previous, &previous),
                Ok(true)
            );
            drop(previous);
            drop(comparison);
            assert!(old.upgrade().is_none());
        });
    }

    #[test]
    fn paint_comparison_preserves_nan_results_and_upstream_debug_panics() {
        with_painter(|painter| {
            let original = text_fixture(painter);
            let mut comparison = PaintComparison::default();
            for field in 0..7 {
                let mut shape = original.clone();
                match field {
                    0 => shape.clip_rect.max.x = f32::NAN,
                    1 => shape.transform_after_tessellation.translation.x = f32::NAN,
                    _ => {
                        let text = fixture_text_mut(&mut shape);
                        let galley = Arc::make_mut(&mut text.galley);
                        match field {
                            2 => galley.pixels_per_point = f32::NAN,
                            3 => Arc::make_mut(&mut galley.job).first_row_min_height = f32::NAN,
                            4 => {
                                Arc::make_mut(&mut galley.rows[0].row).glyphs[0].advance_width =
                                    f32::NAN;
                            }
                            5 => galley.rect.max.x = f32::NAN,
                            6 => galley.rows[0].pos.x = f32::NAN,
                            _ => unreachable!(),
                        }
                    }
                }
                for _ in 0..2 {
                    assert_ne!(
                        assert_comparison_matches_derived(&mut comparison, &shape, &shape),
                        Ok(true),
                        "field {field}"
                    );
                }
            }
        });
    }

    #[test]
    fn paint_comparison_preserves_derived_short_circuit_order() {
        with_painter(|painter| {
            let original = text_fixture(painter);
            let mut invalid = original.clone();
            Arc::make_mut(&mut fixture_text_mut(&mut invalid).galley)
                .rect
                .max
                .x = f32::NAN;
            let mut cases = Vec::new();

            let mut left = original.clone();
            fixture_text_mut(&mut left).pos.x = f32::NAN;
            let mut right = left.clone();
            right.clip_rect.max.x -= 1.0;
            cases.push((left, right));

            let mut right = invalid.clone();
            fixture_text_mut(&mut right).pos.x += 1.0;
            cases.push((invalid.clone(), right));

            let mut left = original.clone();
            left.transform_after_tessellation.translation.x = f32::NAN;
            let mut right = left.clone();
            right.shape = Shape::Noop;
            cases.push((left, right));

            let mut left = original.clone();
            Arc::make_mut(&mut fixture_text_mut(&mut left).galley).pixels_per_point = f32::NAN;
            left.transform_after_tessellation.translation.x = f32::NAN;
            cases.push((left.clone(), left));

            let mut left = original.clone();
            left.shape = Shape::Vec(vec![invalid.shape.clone()]);
            let mut right = left.clone();
            let Shape::Vec(shapes) = &mut right.shape else {
                unreachable!();
            };
            shapes.push(Shape::Noop);
            cases.push((left, right));

            let mut left = original.clone();
            left.shape = Shape::Vec(vec![Shape::Noop, invalid.shape.clone()]);
            let mut right = left.clone();
            let Shape::Vec(shapes) = &mut right.shape else {
                unreachable!();
            };
            shapes[0] = Shape::Vec(Vec::new());
            cases.push((left, right));

            let mut comparison = PaintComparison::default();
            for (left, right) in cases {
                for _ in 0..2 {
                    assert_eq!(
                        assert_comparison_matches_derived(&mut comparison, &left, &right),
                        Ok(false)
                    );
                }
            }
        });
    }

    #[test]
    fn paint_comparison_preserves_nested_shapes_mesh_equality_and_callback_identity() {
        with_painter(|painter| {
            let text = text_fixture(painter);
            let mut mesh = egui::Mesh::default();
            mesh.vertices.push(egui::epaint::Vertex {
                pos: egui::pos2(f32::NAN, 0.0),
                ..Default::default()
            });
            let mesh = Shape::Mesh(Arc::new(mesh));
            let marker = Arc::new(SurfaceMarker);
            let nested = ClippedShape::new(
                painter.clip_rect(),
                Shape::Vec(vec![
                    text.shape,
                    Shape::Vec(vec![mesh, marker_shape(&marker), Shape::Vec(Vec::new())]),
                ]),
            );
            let mut comparison = PaintComparison::default();
            for _ in 0..2 {
                assert_eq!(
                    assert_comparison_matches_derived(&mut comparison, &nested, &nested),
                    Ok(true)
                );
            }
            let mut changed = nested.clone();
            let Shape::Vec(outer) = &mut changed.shape else {
                unreachable!();
            };
            let Shape::Vec(inner) = &mut outer[1] else {
                unreachable!();
            };
            inner[1] = marker_shape(&Arc::new(SurfaceMarker));
            assert_eq!(
                assert_comparison_matches_derived(&mut comparison, &nested, &changed),
                Ok(false)
            );
        });
    }

    #[test]
    fn borrowed_append_check_restores_existing_empty_and_populated_slots() {
        with_painter(|painter| {
            let empty = known_layer(painter.ctx(), "borrowed-existing-empty");
            let missing = known_layer(painter.ctx(), "borrowed-missing-slot");
            painter.ctx().graphics_mut(|graphics| {
                let _ = graphics.entry(empty);
            });
            let layers = [painter.layer_id(), empty, missing];
            let slots = painter
                .ctx()
                .graphics(|graphics| layers.map(|layer| graphics.get(layer).is_some()));
            assert!(slots[1]);
            assert!(!slots[2]);
            let check = PaintAppendCheck::begin(painter);
            for _ in 0..2 {
                assert!(assert_append_matches_frozen(&check, painter));
                assert_eq!(
                    painter
                        .ctx()
                        .graphics(|graphics| layers.map(|layer| graphics.get(layer).is_some())),
                    slots
                );
            }

            let other = known_layer(painter.ctx(), "borrowed-populated");
            painter.ctx().layer_painter(other).circle_filled(
                egui::pos2(20.0, 20.0),
                2.0,
                egui::Color32::RED,
            );
            painter.circle_filled(egui::pos2(10.0, 10.0), 2.0, egui::Color32::BLUE);
            let check = PaintAppendCheck::begin(painter);
            for _ in 0..3 {
                painter.circle_filled(egui::pos2(30.0, 30.0), 2.0, egui::Color32::GREEN);
                assert!(assert_append_matches_frozen(&check, painter));
                painter.ctx().graphics(|graphics| {
                    assert!(graphics.get(empty).unwrap().is_empty());
                    assert!(graphics.get(missing).is_none());
                    assert_eq!(graphics.get(other).unwrap().all_entries().len(), 1);
                });
            }
        });
    }

    #[test]
    fn borrowed_append_check_rejects_frozen_prefix_layer_and_transform_changes() {
        for change in 0..5 {
            with_painter(|painter| {
                let first = known_layer(painter.ctx(), "borrowed-first");
                let second = known_layer(painter.ctx(), "borrowed-second");
                let prefix =
                    painter.circle_filled(egui::pos2(10.0, 10.0), 2.0, egui::Color32::BLUE);
                let check = PaintAppendCheck::begin(painter);
                match change {
                    0 => painter.set(prefix, Shape::Noop),
                    1 => {
                        let _ = known_layer(painter.ctx(), "borrowed-new-empty-layer");
                    }
                    2 => painter.ctx().set_transform_layer(
                        first,
                        egui::emath::TSTransform::from_translation(egui::vec2(1.0, 0.0)),
                    ),
                    3 => {
                        painter.ctx().layer_painter(second).circle_filled(
                            egui::pos2(20.0, 20.0),
                            2.0,
                            egui::Color32::RED,
                        );
                    }
                    4 => {
                        let layers = painter
                            .ctx()
                            .memory(|memory| memory.layer_ids().collect::<Vec<_>>());
                        let mut reordered = egui::Memory::default();
                        for layer in layers.into_iter().rev() {
                            reordered
                                .areas_mut()
                                .set_state(layer, egui::AreaState::default());
                        }
                        painter.ctx().memory_mut(|memory| *memory = reordered);
                    }
                    _ => unreachable!(),
                }
                for _ in 0..2 {
                    assert!(!assert_append_matches_frozen(&check, painter));
                }
            });
        }
    }

    #[test]
    fn borrowed_append_check_matches_frozen_nan_prefix_and_suffix_outcomes() {
        for field in 0..4 {
            with_painter(|painter| {
                let shape = painter.circle_filled(egui::pos2(10.0, 10.0), 2.0, egui::Color32::BLUE);
                if field == 3 {
                    painter.ctx().set_transform_layer(
                        painter.layer_id(),
                        egui::emath::TSTransform::from_scaling(f32::NAN),
                    );
                } else {
                    painter.ctx().graphics_mut(|graphics| {
                        graphics.get_mut(painter.layer_id()).unwrap().mutate_shape(
                            shape,
                            |shape| match field {
                                0 => shape.clip_rect.max.x = f32::NAN,
                                1 => shape.transform_after_tessellation.scaling = f32::NAN,
                                2 => {
                                    let Shape::Circle(circle) = &mut shape.shape else {
                                        unreachable!();
                                    };
                                    circle.radius = f32::NAN;
                                }
                                _ => unreachable!(),
                            },
                        );
                    });
                }
                let check = PaintAppendCheck::begin(painter);
                let mut comparison = PaintComparison::default();
                for _ in 0..2 {
                    // emath may assert on NaN positions in debug builds.
                    let expected = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                        frozen_append_is_preserved(&check, painter)
                    }));
                    let actual = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                        check.is_preserved(painter)
                    }));
                    let memoized = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                        check.is_preserved_with_comparison(painter, &mut comparison)
                    }))
                    .map_err(|_| ());
                    assert_eq!(memoized, expected.as_ref().copied().map_err(|_| ()));
                    assert_eq!(actual.is_err(), expected.is_err());
                    if let (Ok(actual), Ok(expected)) = (actual, expected) {
                        assert_eq!(actual, expected);
                        assert!(!actual);
                    }
                }
            });
        }
        with_painter(|painter| {
            let check = PaintAppendCheck::begin(painter);
            let shape = painter.add(Shape::Noop);
            painter.ctx().graphics_mut(|graphics| {
                graphics
                    .get_mut(painter.layer_id())
                    .unwrap()
                    .mutate_shape(shape, |shape| shape.clip_rect.max.x = f32::NAN);
            });
            assert!(assert_append_matches_frozen(&check, painter));
        });
    }

    #[test]
    fn borrowed_append_check_compares_list_lengths_before_nan_descriptors() {
        for (current, before_count, after_count) in [(true, 2, 1), (false, 2, 1), (false, 1, 2)] {
            with_painter(|painter| {
                let other = known_layer(painter.ctx(), "length-before-nan");
                let target = if current { painter.layer_id() } else { other };
                let mut clip = painter.clip_rect();
                clip.max.x = f32::NAN;
                let reset = |graphics: &mut egui::layers::GraphicLayers, count| {
                    let list = graphics.entry(target);
                    *list = egui::layers::PaintList::default();
                    list.add(clip, Shape::Noop);
                    for _ in 1..count {
                        list.add(painter.clip_rect(), Shape::Noop);
                    }
                };
                painter
                    .ctx()
                    .graphics_mut(|graphics| reset(graphics, before_count));
                let check = PaintAppendCheck::begin(painter);
                painter
                    .ctx()
                    .graphics_mut(|graphics| reset(graphics, after_count));
                let mut comparison = PaintComparison::default();
                for _ in 0..2 {
                    assert_eq!(
                        assert_append_outcome_matches_frozen(&check, painter, &mut comparison),
                        Ok(false)
                    );
                }
            });
        }
    }

    #[test]
    fn borrowed_append_check_rejects_unknown_shapes_before_nan_descriptors() {
        for current in [true, false] {
            with_painter(|painter| {
                let other = known_layer(painter.ctx(), "unknown-before-nan");
                let target = if current { painter.layer_id() } else { other };
                let mut clip = painter.clip_rect();
                clip.max.x = f32::NAN;
                painter.ctx().graphics_mut(|graphics| {
                    graphics.entry(target).add(clip, Shape::Noop);
                });
                let check = PaintAppendCheck::begin(painter);
                let unknown = LayerId::new(
                    egui::Order::Foreground,
                    egui::Id::unique("unknown-precedes-nan-comparison"),
                );
                painter.ctx().layer_painter(unknown).add(Shape::Noop);
                let mut comparison = PaintComparison::default();
                for _ in 0..2 {
                    assert_eq!(
                        assert_append_outcome_matches_frozen(&check, painter, &mut comparison),
                        Ok(false)
                    );
                }
                assert!(painter.ctx().graphics(|graphics| {
                    !graphics.get(target).unwrap().is_empty()
                        && !graphics.get(unknown).unwrap().is_empty()
                }));
            });
        }
    }

    #[test]
    fn borrowed_append_check_compares_current_then_other_roster_then_other_contents() {
        for change in 0..3 {
            with_painter(|painter| {
                let first = known_layer(painter.ctx(), "ordered-nan-first");
                let second = known_layer(painter.ctx(), "ordered-nan-second");
                let ordered = painter.ctx().memory(|memory| {
                    memory
                        .layer_ids()
                        .filter(|layer| *layer == first || *layer == second)
                        .collect::<Vec<_>>()
                });
                let [other, current] = ordered.as_slice() else {
                    panic!("both registered layers occur in memory order");
                };
                let current = painter.ctx().layer_painter(*current);
                let other = painter.ctx().layer_painter(*other);
                let mut invalid_clip = painter.clip_rect();
                invalid_clip.max.x = f32::NAN;
                let (current_clip, other_clip) = if change == 1 {
                    (invalid_clip, painter.clip_rect())
                } else {
                    (painter.clip_rect(), invalid_clip)
                };
                let (current_shape, other_shape) = painter.ctx().graphics_mut(|graphics| {
                    (
                        graphics
                            .entry(current.layer_id())
                            .add(current_clip, Shape::Noop),
                        graphics
                            .entry(other.layer_id())
                            .add(other_clip, Shape::Noop),
                    )
                });
                let check = PaintAppendCheck::begin(&current);
                match change {
                    0 => current.set(current_shape, Shape::Vec(Vec::new())),
                    1 => other.set(other_shape, Shape::Vec(Vec::new())),
                    2 => {
                        let _ = known_layer(painter.ctx(), "other-roster-length-before-nan");
                    }
                    _ => unreachable!(),
                }
                let mut comparison = PaintComparison::default();
                for _ in 0..2 {
                    let outcome =
                        assert_append_outcome_matches_frozen(&check, &current, &mut comparison);
                    if change == 1 {
                        assert_ne!(outcome, Ok(true));
                    } else {
                        assert_eq!(outcome, Ok(false));
                    }
                }
            });
        }
    }

    #[test]
    fn borrowed_append_check_accepts_its_unregistered_layer_and_rejects_foreign_painters() {
        with_painter(|painter| {
            let layer = LayerId::new(
                egui::Order::Foreground,
                egui::Id::unique("borrowed-current"),
            );
            let current = painter.ctx().layer_painter(layer);
            assert!(
                !painter
                    .ctx()
                    .memory(|memory| memory.layer_ids().any(|known| known == layer))
            );
            let check = PaintAppendCheck::begin(&current);
            current.circle_filled(egui::pos2(10.0, 10.0), 2.0, egui::Color32::RED);
            assert!(assert_append_matches_frozen(&check, &current));
            assert!(assert_append_matches_frozen(&check, &current));
            assert!(!assert_append_matches_frozen(&check, painter));
        });
    }

    #[test]
    fn snapshot_matches_frozen_empty_and_multiple_registered_layers() {
        with_painter(|painter| {
            let empty = assert_snapshot_matches_frozen(painter).unwrap();
            assert!(empty.current.shapes.is_empty());
            let empty_layer = known_layer(painter.ctx(), "snapshot-empty-layer");
            let foreground = LayerId::new(
                egui::Order::Foreground,
                egui::Id::unique("snapshot-foreground"),
            );
            painter.ctx().memory_mut(|memory| {
                memory
                    .areas_mut()
                    .set_state(foreground, egui::AreaState::default());
            });
            let middle = known_layer(painter.ctx(), "snapshot-middle");
            painter.add(Shape::Noop);
            painter.circle_filled(egui::pos2(10.0, 10.0), 2.0, egui::Color32::RED);
            painter.ctx().layer_painter(foreground).extend([
                Shape::Vec(Vec::new()),
                Shape::circle_filled(egui::pos2(20.0, 20.0), 3.0, egui::Color32::GREEN),
            ]);
            painter.ctx().layer_painter(middle).circle_filled(
                egui::pos2(30.0, 30.0),
                4.0,
                egui::Color32::BLUE,
            );

            let captured = assert_snapshot_matches_frozen(painter).unwrap();
            assert_eq!(captured.current.shapes.len(), 2);
            for (layer, count) in [(empty_layer, 0), (foreground, 2), (middle, 1)] {
                assert_eq!(
                    captured
                        .others
                        .iter()
                        .find(|captured| captured.layer == layer)
                        .unwrap()
                        .shapes
                        .len(),
                    count
                );
            }
        });
    }

    #[test]
    fn snapshot_preserves_nested_markers_and_raw_transform_bits() {
        with_painter(|painter| {
            let other = known_layer(painter.ctx(), "snapshot-transformed-other");
            let global = egui::emath::TSTransform::new(egui::vec2(7.25, -3.5), 1.5);
            let deferred =
                egui::emath::TSTransform::new(egui::vec2(-0.0, f32::from_bits(1)), 1.125);
            painter
                .ctx()
                .set_transform_layer(painter.layer_id(), global);
            painter.ctx().set_transform_layer(
                other,
                egui::emath::TSTransform::from_translation(egui::vec2(-2.5, 4.75)),
            );
            let points = [egui::pos2(-0.0, f32::from_bits(1)), egui::pos2(12.5, -0.0)];
            let clip = egui::Rect::from_min_max(egui::pos2(-0.0, -0.0), egui::pos2(99.5, 99.75));
            let marker = Arc::new(SurfaceMarker);
            let compound = Shape::Vec(vec![
                Shape::Noop,
                Shape::Vec(vec![
                    Shape::line_segment(points, egui::Stroke::new(0.75, egui::Color32::RED)),
                    marker_shape(&marker),
                    Shape::Vec(Vec::new()),
                ]),
            ]);
            painter.ctx().graphics_mut(|graphics| {
                let list = graphics.entry(painter.layer_id());
                let index = list.add(clip, compound);
                list.mutate_shape(index, |shape| {
                    shape.transform_after_tessellation = deferred;
                });
                graphics.entry(other).add(
                    painter.clip_rect(),
                    Shape::circle_filled(egui::pos2(20.0, 20.0), 2.0, egui::Color32::BLUE),
                );
            });

            let captured = assert_snapshot_matches_frozen(painter).unwrap();
            assert_eq!(captured.current.transform, Some(global));
            let shape = &captured.current.shapes[0];
            assert_eq!(
                transform_bits(shape.transform_after_tessellation),
                transform_bits(deferred)
            );
            assert_eq!(shape.clip_rect.min.x.to_bits(), (-0.0_f32).to_bits());
            assert_eq!(shape.clip_rect.min.y.to_bits(), (-0.0_f32).to_bits());
            let Shape::Vec(outer) = &shape.shape else {
                panic!("the outer compound must not be flattened");
            };
            let Shape::Vec(inner) = &outer[1] else {
                panic!("the nested compound must not be flattened");
            };
            let Shape::LineSegment { points: actual, .. } = &inner[0] else {
                panic!("the nested line must remain in local coordinates");
            };
            let point_bits = |point: egui::Pos2| [point.x.to_bits(), point.y.to_bits()];
            assert_eq!(actual.map(point_bits), points.map(point_bits));
            assert_eq!(
                unique_marker_index(&captured.current.shapes, &marker),
                Some(0)
            );
            let repeated = assert_snapshot_matches_frozen(painter).unwrap();
            assert_eq!(captured.current.shapes, repeated.current.shapes);
            assert!(captured.others == repeated.others);
        });
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
                assert_eq!(
                    assert_snapshot_matches_frozen(painter).is_none(),
                    present_before_capture
                );
                let check = PaintAppendCheck::begin(painter);
                let capture = SurfaceOutputCapture::begin(painter);
                let multilayer = SurfaceOutputCapture::begin_multilayer(painter);
                if !present_before_capture {
                    assert!(check.is_preserved(painter));
                    paint_unknown();
                }

                assert!(assert_snapshot_matches_frozen(painter).is_none());
                let entries = painter.ctx().graphics(|graphics| {
                    graphics
                        .get(painter.layer_id())
                        .unwrap()
                        .all_entries()
                        .cloned()
                        .collect::<Vec<_>>()
                });
                assert!(!assert_append_matches_frozen(&check, painter));
                painter.ctx().graphics(|graphics| {
                    assert!(
                        graphics
                            .get(painter.layer_id())
                            .unwrap()
                            .all_entries()
                            .eq(entries.iter())
                    );
                    assert!(!graphics.get(unknown).unwrap().is_empty());
                });
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
    fn multilayer_capture_records_span_layer_identities_without_ordering_them() {
        let context = Context::default();
        let mut proof = None;
        let mut primary_layer = None;
        let mut output = context.run_ui(egui::RawInput::default(), |ui| {
            let primary = known_layer(ui.ctx(), "multilayer-ordered-primary");
            primary_layer = Some(primary);
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
        assert_eq!(proof.primary_layer, primary_layer.unwrap());
        assert_eq!(proof.spans.len(), 2);
        assert!(
            proof
                .spans
                .iter()
                .any(|span| span.layer.order == egui::Order::Background)
        );
        assert!(
            proof
                .spans
                .iter()
                .any(|span| span.layer.order == egui::Order::Foreground)
        );
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
                context: Context::default(),
                expected,
                primary_layer: LayerId::background(),
                spans: Vec::new(),
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
