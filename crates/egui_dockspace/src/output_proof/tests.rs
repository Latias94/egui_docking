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

#[test]
fn inherent_paint_auditor_takes_precedence_over_the_private_fallback() {
    struct OfficialContext;
    impl ContextPaintAuditFallback for OfficialContext {}

    struct ForkContext;
    impl ContextPaintAuditFallback for ForkContext {}
    impl ForkContext {
        #[allow(clippy::unnecessary_wraps, clippy::unused_self)]
        fn paint_append_auditor(&self) -> Option<PaintAppendAuditor> {
            Some(Box::new(|_, paint, valid| {
                paint();
                *valid = false;
            }))
        }
    }

    assert!(OfficialContext.paint_append_auditor().is_none());
    assert!(ContextPaintAuditFallback::paint_append_auditor(&ForkContext).is_none());
    with_painter(|painter| {
        let mut backend = ForkContext.paint_append_auditor().unwrap();
        let mut calls = 0;
        let mut valid = true;
        backend(painter, &mut || calls += 1, &mut valid);
        assert_eq!(calls, 1);
        assert!(!valid);
    });
}

#[test]
fn paint_audit_keeps_one_backend_session_and_consumes_each_callback_once() {
    with_painter(|painter| {
        let history = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
        let observed = std::rc::Rc::clone(&history);
        let mut checks = 0;
        let mut audit = PaintAudit::new(painter.ctx());
        audit.backend = Some(Box::new(move |_, paint, valid| {
            observed.borrow_mut().push((checks, *valid));
            checks += 1;
            paint();
            *valid &= checks != 2;
        }));
        let mut calls = 0;
        for _ in 0..3 {
            let owned = String::new();
            audit.check_append(painter, || {
                calls += 1;
                drop(owned);
            });
        }
        assert_eq!(calls, 3);
        assert_eq!(*history.borrow(), vec![(0, true), (1, true), (2, false)]);
        assert!(!audit.is_valid());
        assert!(audit.snapshots.reusable.is_none());
        assert!(audit.comparison.galleys.is_empty());
    });
}

#[test]
fn snapshot_paint_audit_keeps_failures_after_a_later_callback_restores_the_prefix() {
    with_painter(|painter| {
        let original = Shape::circle_filled(egui::pos2(20.0, 20.0), 5.0, egui::Color32::RED);
        let prefix = painter.add(original.clone());
        let mut audit = PaintAudit::new(painter.ctx());
        audit.backend = None;
        let mut calls = 0;
        audit.check_append(painter, || {
            calls += 1;
            painter.set(prefix, Shape::Noop);
        });
        assert!(!audit.is_valid());
        audit.check_append(painter, || {
            calls += 1;
            painter.set(prefix, original);
        });
        audit.check_append(painter, || {
            calls += 1;
            let suffix = painter.add(Shape::Noop);
            painter.set(
                suffix,
                Shape::circle_filled(egui::pos2(30.0, 30.0), 5.0, egui::Color32::GREEN),
            );
        });
        assert_eq!(calls, 3);
        assert!(!audit.is_valid());
        assert!(audit.snapshots.reusable.is_some());
    });
}

#[test]
fn snapshot_paint_audit_records_failure_before_an_opaque_drop_panics() {
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
        let mut audit = PaintAudit::new(painter.ctx());
        audit.backend = None;
        let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            audit.check_append(painter, || {
                let removed = painter.ctx().graphics_mut(|graphics| {
                    std::mem::take(graphics.get_mut(painter.layer_id()).unwrap())
                });
                drop(removed);
            });
        }));
        assert!(outcome.is_err());
        assert!(!audit.is_valid());
        assert!(audit.snapshots.reusable.is_none());
        assert_eq!(*events.lock().unwrap(), vec![("panics", false)]);
    });
}

#[test]
fn pane_panic_releases_the_snapshot_before_child_ui_finalization() {
    #[derive(Default)]
    struct Observation {
        child: Option<egui::Id>,
        rect_at_snapshot_drop: Option<egui::Rect>,
    }

    struct ObserveChildOnDrop {
        context: Context,
        observation: Arc<std::sync::Mutex<Observation>>,
    }

    impl Drop for ObserveChildOnDrop {
        fn drop(&mut self) {
            let child = self.observation.lock().unwrap().child.unwrap();
            let rect = self.context.read_response(child).unwrap().rect;
            self.observation.lock().unwrap().rect_at_snapshot_drop = Some(rect);
        }
    }

    struct PanickingPane(Arc<std::sync::Mutex<Observation>>);

    impl crate::PaneView for PanickingPane {
        fn title(&self, _item: crate::ItemId) -> Option<egui::WidgetText> {
            Some("Pane".into())
        }

        fn ui(&mut self, _item: crate::ItemId, ui: &mut egui::Ui) -> crate::PaneResponse {
            self.0.lock().unwrap().child = Some(ui.unique_id());
            ui.allocate_space(egui::vec2(20.0, 20.0));
            let removed = ui
                .ctx()
                .graphics_mut(|graphics| std::mem::take(graphics.get_mut(ui.layer_id()).unwrap()));
            drop(removed);
            panic!("the pane deliberately panics after removing its prefix");
        }
    }

    let context = Context::default();
    let surface = crate::SurfaceId::new(1);
    let layout = crate::DockspaceLayout::new([crate::DockspaceSurfaceLayout::new(
        surface,
        crate::DockspaceRootLayout::new(
            crate::RootId::new(1),
            crate::DockspaceNode::tabs([crate::ItemId::new(1)]),
        ),
    )])
    .unwrap();
    let mut docking = crate::Dockspace::builder("pane-panic-drop-order", layout)
        .build()
        .unwrap();
    let observation = Arc::new(std::sync::Mutex::new(Observation::default()));
    let mut panes = PanickingPane(Arc::clone(&observation));
    context
        .run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(640.0, 480.0),
                )),
                ..egui::RawInput::default()
            },
            |ui| {
                ui.painter().add(Shape::Callback(PaintCallback {
                    rect: ui.clip_rect(),
                    callback: Arc::new(ObserveChildOnDrop {
                        context: context.clone(),
                        observation: Arc::clone(&observation),
                    }),
                }));
                let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    docking.show_single_surface(surface, ui, &mut panes)
                }));
                assert!(outcome.is_err());
                let observation = observation.lock().unwrap();
                assert_eq!(observation.rect_at_snapshot_drop, Some(egui::Rect::NOTHING));
                let completed = context.read_response(observation.child.unwrap()).unwrap();
                assert!(completed.rect.is_finite() && completed.rect.is_positive());
            },
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
    let expected =
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| left == right)).map_err(|_| ());
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
        drag_layer: None,
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
                snapshot_reusing_with_layers(painter, snapshot(painter).unwrap(), layers.clone(),)
                    .is_none()
            );
            assert!(!preserves_snapshot_with_layers(
                painter,
                &before,
                layers,
                None,
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
            drag_layer: None,
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
            drag_layer: None,
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
                graphics.get_mut(painter.layer_id()).unwrap().mutate_shape(
                    slot,
                    |shape| match field {
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
                    },
                );
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
            let result =
                assert_reusing_outcome_matches_frozen(&actual, &expected, painter, &mut comparison);
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
            let result =
                assert_reusing_outcome_matches_frozen(&actual, &expected, painter, &mut comparison);
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
                assert_reusing_outcome_matches_frozen(&actual, &expected, painter, &mut comparison),
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
                assert!(
                    check.finish_reusing(painter, &mut cache, &mut comparison, &mut aggregate,)
                );
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
            let prefix = painter.circle_filled(egui::pos2(10.0, 10.0), 2.0, egui::Color32::BLUE);
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
                    graphics
                        .get_mut(painter.layer_id())
                        .unwrap()
                        .mutate_shape(shape, |shape| match field {
                            0 => shape.clip_rect.max.x = f32::NAN,
                            1 => shape.transform_after_tessellation.scaling = f32::NAN,
                            2 => {
                                let Shape::Circle(circle) = &mut shape.shape else {
                                    unreachable!();
                                };
                                circle.radius = f32::NAN;
                            }
                            _ => unreachable!(),
                        });
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
        let deferred = egui::emath::TSTransform::new(egui::vec2(-0.0, f32::from_bits(1)), 1.125);
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
            let unknown = LayerId::new(egui::Order::Foreground, egui::Id::unique("unknown-layer"));
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
    let swapped_popup_end = unique_marker_index(&output.shapes, &proof.spans[0].end).unwrap() + 1;
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
            let layer = LayerId::new(order, egui::Id::unique(("multilayer-ordered-extra", order)));
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
fn owned_output_check_is_bound_to_its_exact_capture_and_all_final_descriptors() {
    let (proof, output) = fixture();
    let (foreign, _) = fixture();
    let check = proof.check_owned_output(&output, &[]).unwrap();
    assert!(check.is_for(&proof));
    assert!(!check.is_for(&foreign));
    assert!(foreign.check_owned_output(&output, &[]).is_none());
    let mut changed = output.clone();
    changed.shapes.remove(1);
    assert!(proof.check_owned_output(&changed, &[]).is_none());
    let spans = check.into_spans();
    assert_eq!(spans.enclosing_range(&[]), Some((0, 4)));
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
