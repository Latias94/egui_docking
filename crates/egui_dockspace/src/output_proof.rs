//! Conservative evidence that complete owned surface spans survived.
//!
//! The caller must bracket every surface paint, including ordinary geometry,
//! pane contents, source-gap handling, and transient visuals. Painting must be
//! synchronous and append-only. Pane implementations must
//! honor the supplied paint disposition and opacity, and must not defer drawing,
//! mutate an earlier shape, or write to an arbitrary other layer. The one
//! Tooltip layer derived from egui's actual dragged widget may append a data
//! drag preview while preserving its previous prefix and global transform.
//! Other-layer snapshots preserve layer identity, global transforms, and content.
//! Nonempty graphics on an unenumerated layer make the capture unavailable. Adapter-owned chrome may
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

/// Checks append-only paint on the supplied layer and the exact data-drag ghost.
pub(super) struct PaintAppendCheck {
    context: Context,
    viewport: ViewportId,
    layer: LayerId,
    drag_layer: Option<LayerId>,
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

type PaintAppendAuditor = Box<dyn FnMut(&Painter, &mut dyn FnMut(), &mut bool)>;

// A fork may provide an inherent getter with the same signature. Inherent
// method lookup then selects its fixed implementation without a public registry.
#[allow(dead_code)]
trait ContextPaintAuditFallback {
    fn paint_append_auditor(&self) -> Option<PaintAppendAuditor> {
        None
    }
}

impl ContextPaintAuditFallback for Context {}

/// Audits application callbacks during one synchronous surface paint.
pub(super) struct PaintAudit {
    // Preserve the former local-variable drop order: cached descriptors must
    // release their owners before the comparison releases its retained galleys.
    snapshots: PaintSnapshotCache,
    comparison: PaintComparison,
    backend: Option<PaintAppendAuditor>,
    drag_layer: Option<LayerId>,
    valid: bool,
}

impl PaintAudit {
    pub(super) fn new(context: &Context) -> Self {
        let drag_layer = drag_ghost_layer(context);
        Self {
            snapshots: PaintSnapshotCache::default(),
            comparison: PaintComparison::default(),
            // The optional backend checkpoint only permits same-layer paint.
            // Keep its fast path for ordinary runs; the exact drag layer needs
            // the complete fallback prefix audit, not a skipped checkpoint.
            backend: if drag_layer.is_some() {
                None
            } else {
                context.paint_append_auditor()
            },
            drag_layer,
            valid: true,
        }
    }

    /// Runs the callback once, retaining every preceding callback's failure.
    pub(super) fn check_append(&mut self, painter: &Painter, paint: impl FnOnce()) {
        let mut paint = Some(paint);
        let mut paint_once = || {
            paint
                .take()
                .expect("the paint append auditor must not repeat the callback")();
        };
        if let Some(backend) = self.backend.as_mut() {
            // The backend must update the aggregate before releasing its
            // checkpoint's opaque payloads, matching finish_reusing below.
            backend(painter, &mut paint_once, &mut self.valid);
            self.valid &= paint.is_none();
        } else {
            let check = PaintAppendCheck::begin_reusing_with_drag_layer(
                painter,
                &mut self.snapshots,
                self.drag_layer,
            );
            paint_once();
            check.finish_reusing(
                painter,
                &mut self.snapshots,
                &mut self.comparison,
                &mut self.valid,
            );
        }
    }

    pub(super) const fn is_valid(&self) -> bool {
        self.valid
    }
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
        let drag_layer = drag_ghost_layer(painter.ctx());
        Self {
            context: painter.ctx().clone(),
            viewport: painter.ctx().viewport_id(),
            layer: painter.layer_id(),
            drag_layer,
            before: snapshot_with_drag_layer(painter, drag_layer),
        }
    }

    /// Observes every live descriptor while reusing identical pure owned slots.
    #[cfg(test)]
    pub(super) fn begin_reusing(painter: &Painter, cache: &mut PaintSnapshotCache) -> Self {
        Self::begin_reusing_with_drag_layer(painter, cache, drag_ghost_layer(painter.ctx()))
    }

    fn begin_reusing_with_drag_layer(
        painter: &Painter,
        cache: &mut PaintSnapshotCache,
        drag_layer: Option<LayerId>,
    ) -> Self {
        Self {
            context: painter.ctx().clone(),
            viewport: painter.ctx().viewport_id(),
            layer: painter.layer_id(),
            drag_layer,
            before: cache.reusable.take().map_or_else(
                || snapshot_with_drag_layer(painter, drag_layer),
                |before| snapshot_reusing(painter, before, drag_layer),
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
            .is_some_and(|before| preserves_snapshot(painter, before, self.drag_layer))
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
            preserves_snapshot_with(painter, before, self.drag_layer, |left, right| {
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
        let after = snapshot_with_drag_layer(painter, self.drag_layer)?;
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
            let after = snapshot_with_drag_layer(painter, self.append_check.drag_layer)?;
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
    #[cfg(any(test, feature = "native-render-support"))]
    pub(super) fn context(&self) -> &Context {
        &self.context
    }

    /// Returns the enclosing half-open range for callers that own one surface.
    #[cfg(test)]
    pub(super) fn is_present_once(&self, output: &FullOutput) -> Option<(usize, usize)> {
        let order = final_paint_layer_order(&self.context);
        self.present_spans(output)?.enclosing_range(&order)
    }

    /// Binds a complete descriptor check to this immutable capture identity.
    /// The caller must keep the checked FullOutput exclusively owned until use.
    pub(super) fn check_owned_output(
        &self,
        output: &FullOutput,
        layer_order: &[LayerId],
    ) -> Option<OwnedOutputCheck> {
        let spans = self.present_spans(output)?;
        spans.enclosing_range(layer_order)?;
        Some(OwnedOutputCheck {
            begin: Arc::clone(&self.begin),
            end: Arc::clone(&self.end),
            spans,
        })
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

impl SurfaceOutputSpans {
    fn enclosing_range(&self, layer_order: &[LayerId]) -> Option<(usize, usize)> {
        let mut spans = self
            .spans
            .iter()
            .copied()
            .enumerate()
            .map(|(index, span)| (0, index, span))
            .collect::<Vec<_>>();
        if !spans_follow_paint_order(&mut spans, layer_order) {
            return None;
        }
        Some((spans.first()?.2.range.0, spans.last()?.2.range.1))
    }
}

/// Evidence consumed only with the exclusively held output which was checked.
pub(super) struct OwnedOutputCheck {
    begin: Arc<SurfaceMarker>,
    end: Arc<SurfaceMarker>,
    spans: SurfaceOutputSpans,
}

impl OwnedOutputCheck {
    pub(super) fn is_for(&self, proof: &SurfaceOutputProof) -> bool {
        Arc::ptr_eq(&self.begin, &proof.begin) && Arc::ptr_eq(&self.end, &proof.end)
    }

    pub(super) fn into_spans(self) -> SurfaceOutputSpans {
        self.spans
    }
}

#[derive(Clone, Copy)]
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

fn drag_ghost_layer(context: &Context) -> Option<LayerId> {
    // egui 0.37 Ui::dnd_drag_source uses exactly this layer when
    // is_being_dragged(id) is true. Derive identity before user paint, never
    // discover it from unenumerated graphics after the callback.
    context
        .dragged_id()
        .map(|id| LayerId::new(egui::Order::Tooltip, id))
}

fn paint_layers(painter: &Painter, drag_layer: Option<LayerId>) -> Vec<LayerId> {
    let mut layers = painter
        .ctx()
        .memory(|memory| memory.layer_ids().collect::<Vec<_>>());
    if let Some(layer) = drag_layer
        && !layers.contains(&layer)
    {
        layers.push(layer);
    }
    layers
}

#[cfg(test)]
fn snapshot(painter: &Painter) -> Option<PaintSnapshot> {
    snapshot_with_drag_layer(painter, None)
}

fn snapshot_with_drag_layer(
    painter: &Painter,
    drag_layer: Option<LayerId>,
) -> Option<PaintSnapshot> {
    let layers = paint_layers(painter, drag_layer);
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

fn snapshot_reusing(
    painter: &Painter,
    before: PaintSnapshot,
    drag_layer: Option<LayerId>,
) -> Option<PaintSnapshot> {
    let layers = paint_layers(painter, drag_layer);
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
        return snapshot_with_layers(painter, layers);
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
fn preserves_snapshot(
    painter: &Painter,
    before: &PaintSnapshot,
    drag_layer: Option<LayerId>,
) -> bool {
    preserves_snapshot_with(painter, before, drag_layer, |left, right| left == right)
}

fn preserves_snapshot_with(
    painter: &Painter,
    before: &PaintSnapshot,
    drag_layer: Option<LayerId>,
    shapes_equal: impl FnMut(&ClippedShape, &ClippedShape) -> bool,
) -> bool {
    let layers = paint_layers(painter, drag_layer);
    preserves_snapshot_with_layers(painter, before, layers, drag_layer, shapes_equal)
}

fn preserves_snapshot_with_layers(
    painter: &Painter,
    before: &PaintSnapshot,
    layers: Vec<LayerId>,
    drag_layer: Option<LayerId>,
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
                        Some(list) => {
                            let entries = list.all_entries();
                            let length = entries.len();
                            if Some(layer) == drag_layer {
                                length >= expected.shapes.len()
                                    && clipped_shapes_match(
                                        entries.take(expected.shapes.len()),
                                        expected.shapes.iter(),
                                        &mut shapes_equal,
                                    )
                            } else {
                                clipped_shapes_match(
                                    entries,
                                    expected.shapes.iter(),
                                    &mut shapes_equal,
                                )
                            }
                        }
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
mod drag_tests;

#[cfg(test)]
mod tests;
