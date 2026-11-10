//! Shared output tables and packet-scoped framework ownership joins.

use std::cell::Cell;
use std::sync::Arc;

use dockspace::geometry::{LogicalRect, LogicalSize};
use dockspace::model::{
    DockspaceLayout, DockspaceNode, DockspaceRootLayout, DockspaceSurfaceLayout, ItemId, RootId,
    SurfaceId,
};
use dockspace::policy::DockPolicy;
use dockspace::runtime::{
    DockspaceReceiverDescriptor, DockspaceSession, PaintedSurfaceOutput, SurfacePresentationResult,
    UniformSurfaceMetrics,
};
use egui::{Context, Event, Id, LayerId, Pos2, RawInput, Rect, Sense};

use super::{PointerScene, ProductPointer, ProductPointerInputSnapshot, rectangle_key};
use crate::product_render::ProductReceiverBinding;

const SURFACE: SurfaceId = SurfaceId::new(1);
const ITEM: ItemId = ItemId::new(1);

thread_local! {
    static JOIN_WORK: Cell<(usize, usize, usize)> = const { Cell::new((0, 0, 0)) };
}

pub(super) fn record_join(bindings: usize, rectangles: usize) {
    JOIN_WORK.with(|work| {
        let (joins, previous_bindings, previous_rectangles) = work.get();
        work.set((
            joins + 1,
            previous_bindings + bindings,
            previous_rectangles + rectangles,
        ));
    });
}

fn take_join_work() -> (usize, usize, usize) {
    JOIN_WORK.with(|work| work.replace((0, 0, 0)))
}

fn session() -> DockspaceSession {
    let layout = DockspaceLayout::new([DockspaceSurfaceLayout::new(
        SURFACE,
        DockspaceRootLayout::new(RootId::new(1), DockspaceNode::tabs([ITEM])),
    )])
    .unwrap();
    let mut session = DockspaceSession::from_layout(layout, DockPolicy::default()).unwrap();
    session.enable_host_pointer().unwrap();
    session
}

fn paint(session: &mut DockspaceSession) -> (PaintedSurfaceOutput, DockspaceReceiverDescriptor) {
    let mut frame = session.begin_host_frame().unwrap();
    frame
        .measure_surface(
            SURFACE,
            UniformSurfaceMetrics::new(
                LogicalRect::new(0.0, 0.0, 800.0, 600.0).unwrap(),
                LogicalSize::new(64.0, 48.0).unwrap(),
                80.0,
            )
            .unwrap(),
        )
        .unwrap();
    let plan = frame.paint_plan(SURFACE).unwrap().unwrap();
    let receiver = plan.tab_receiver(ITEM).unwrap();
    let receipt = plan.execute_surface_program(|_| Ok::<(), ()>(())).unwrap();
    frame.confirm_surface_painted(receipt).unwrap();
    let output = frame
        .commit()
        .unwrap()
        .take_painted_outputs()
        .pop()
        .unwrap();
    (output, receiver)
}

fn settle(session: &mut DockspaceSession) -> Vec<dockspace::runtime::SurfacePresentationCommit> {
    session
        .begin_host_frame()
        .unwrap()
        .commit()
        .unwrap()
        .surface_presentation_commits()
        .to_vec()
}

fn emit(
    pointer: &mut ProductPointer,
    output: &PaintedSurfaceOutput,
    receiver: DockspaceReceiverDescriptor,
    count: usize,
) {
    let layer = LayerId::new(egui::Order::Background, Id::unique("output-layer"));
    pointer.emitted(
        output.presentation_artifact(),
        layer,
        1,
        true,
        (0..count)
            .map(|index| ProductReceiverBinding {
                interact_rect: Rect::from_min_max(Pos2::ZERO, Pos2::new(100.0, 100.0)),
                interactive: true,
                widget_id: Id::unique(("owned-table", index)),
                layer_id: layer,
                receiver,
            })
            .collect(),
        vec![(ITEM, Id::unique("pane-focus"))],
    );
}

#[test]
fn pointer_drafts_share_output_tables_and_discard_only_private_changes() {
    for count in [1, 16, 128] {
        let mut session = session();
        let mut pointer = ProductPointer::default();
        let mut outputs = Vec::new();
        for _ in 0..4 {
            let (output, receiver) = paint(&mut session);
            emit(&mut pointer, &output, receiver, count);
            outputs.push(output);
        }
        pointer.position = Some(Pos2::new(12.0, 34.0));
        let mut draft = pointer.clone();
        for (original, copied) in pointer.pending.iter().zip(&draft.pending) {
            assert_eq!(original.receivers.len(), count);
            assert!(Arc::ptr_eq(&original.receivers, &copied.receivers));
            assert!(Arc::ptr_eq(
                &original.focus_bindings,
                &copied.focus_bindings
            ));
        }
        draft.position = Some(Pos2::new(56.0, 78.0));
        draft.pending.clear();
        drop(draft);
        assert_eq!(pointer.position, Some(Pos2::new(12.0, 34.0)));
        assert_eq!(pointer.pending.len(), outputs.len());
        assert!(
            pointer
                .pending
                .iter()
                .all(|scene| scene.receivers.len() == count)
        );
    }
}

#[test]
fn reversed_output_reports_keep_shared_tables_bound_to_the_exact_artifact() {
    let mut session = session();
    let mut pointer = ProductPointer::default();
    let mut outputs = Vec::new();
    for count in [1, 16, 128] {
        let (output, receiver) = paint(&mut session);
        emit(&mut pointer, &output, receiver, count);
        outputs.push(output);
    }
    let final_scene = pointer.pending.last().unwrap().clone();
    session
        .report_surface_presentation(outputs.pop().unwrap(), SurfacePresentationResult::Presented)
        .unwrap();
    pointer.observe(&settle(&mut session));
    assert!(pointer.presented.is_none());
    assert_eq!(pointer.pending.len(), 3);
    session
        .report_surface_presentation(outputs.pop().unwrap(), SurfacePresentationResult::Dropped)
        .unwrap();
    pointer.observe(&settle(&mut session));
    assert!(pointer.presented.is_none());
    assert_eq!(pointer.pending.len(), 3);
    session
        .report_surface_presentation(outputs.pop().unwrap(), SurfacePresentationResult::Presented)
        .unwrap();
    let commits = settle(&mut session);
    assert!(
        commits
            .iter()
            .any(|commit| commit.presents(final_scene.artifact))
    );

    // A draft can synchronize the committed presentation without consuming the
    // original's still-pending tables. Abandoning it preserves that original.
    let mut draft = pointer.clone();
    let frame = session.begin_host_frame().unwrap();
    draft.sync_presented(&frame, SURFACE);
    let presented = draft.presented.as_ref().unwrap();
    assert_eq!(presented.artifact, final_scene.artifact);
    assert!(Arc::ptr_eq(&presented.receivers, &final_scene.receivers));
    assert!(Arc::ptr_eq(
        &presented.focus_bindings,
        &final_scene.focus_bindings
    ));
    drop(frame);
    drop(draft);
    assert!(pointer.presented.is_none());
    assert_eq!(pointer.pending.len(), 3);

    pointer.observe(&commits);
    assert!(pointer.pending.is_empty());
    let presented = pointer.presented.as_ref().unwrap();
    assert_eq!(presented.artifact, final_scene.artifact);
    assert!(Arc::ptr_eq(&presented.receivers, &final_scene.receivers));
    assert!(Arc::ptr_eq(
        &presented.focus_bindings,
        &final_scene.focus_bindings
    ));
}

fn widgets(
    context: &Context,
    receiver: DockspaceReceiverDescriptor,
    count: usize,
) -> Vec<ProductReceiverBinding> {
    let mut bindings = Vec::new();
    for _ in 0..2 {
        bindings.clear();
        context
            .run_ui(
                RawInput {
                    screen_rect: Some(Rect::from_min_max(Pos2::ZERO, Pos2::new(800.0, 600.0))),
                    ..RawInput::default()
                },
                |ui| {
                    egui::Area::new(Id::unique("ownership-test-layer"))
                        .fixed_pos(Pos2::ZERO)
                        .movable(false)
                        // The area routes the layer; only content widgets are interactive.
                        .sense(Sense::hover())
                        .show(ui.ctx(), |ui| {
                            let bounds =
                                Rect::from_min_max(Pos2::new(10.0, 20.0), Pos2::new(100.0, 120.0));
                            ui.allocate_rect(bounds, Sense::hover());
                            for index in 0..count {
                                let response = ui.interact(
                                    bounds,
                                    Id::unique(("content", index)),
                                    Sense::click(),
                                );
                                bindings.push(ProductReceiverBinding {
                                    interact_rect: response.interact_rect,
                                    interactive: true,
                                    widget_id: response.id,
                                    layer_id: response.layer_id,
                                    receiver,
                                });
                            }
                        });
                },
            )
            .drop_without_applying_deltas();
    }
    bindings
}

#[test]
fn a_pointer_packet_joins_content_ownership_once_for_all_edges() {
    for count in [1, 16, 128] {
        for unknown in [0, 1] {
            let context = Context::default();
            context.options_mut(|options| options.max_passes = 1.try_into().unwrap());
            let mut session = session();
            let (output, receiver) = paint(&mut session);
            let mut bindings = widgets(&context, receiver, count + unknown);
            let point = bindings[0].interact_rect.center();
            bindings.truncate(count);
            bindings.extend_from_within(..);
            let scene = PointerScene {
                artifact: output.presentation_artifact(),
                layer: bindings[0].layer_id,
                completed_pass: context.cumulative_pass_nr(),
                coordinates_supported: true,
                receivers: bindings.into(),
                focus_bindings: Arc::from([]),
            };
            session
                .report_surface_presentation(output, SurfacePresentationResult::Presented)
                .unwrap();
            settle(&mut session);
            let mut pointer = ProductPointer {
                presented: Some(scene),
                ..ProductPointer::default()
            };
            take_join_work();
            let output = crate::run_ui(
                &context,
                RawInput {
                    screen_rect: Some(Rect::from_min_max(Pos2::ZERO, Pos2::new(800.0, 600.0))),
                    events: vec![Event::PointerMoved(point); 128],
                    ..RawInput::default()
                },
                |ui| {
                    let packet = super::pointer_packet(ui).unwrap();
                    let scene = pointer.presented.as_ref().unwrap();
                    let snapshot = packet.receivers();
                    assert_eq!(snapshot.completed_pass, scene.completed_pass);
                    assert_eq!((snapshot.layer_at)(point), Some(scene.layer));
                    assert_eq!(snapshot.interactive.len(), count + unknown);
                    egui::Area::new(Id::unique("ownership-test-layer"))
                        .fixed_pos(Pos2::ZERO)
                        .movable(false)
                        .sense(Sense::hover())
                        .show(ui.ctx(), |ui| {
                            let frame = session.begin_host_frame().unwrap();
                            let prepared = pointer
                                .prepare_packet(&frame, SURFACE, ui, false, &packet)
                                .unwrap();
                            assert_eq!(prepared.raw.len(), 128);
                            assert!(
                                prepared
                                    .raw
                                    .iter()
                                    .all(|edge| edge.blocked == (unknown == 1))
                            );
                        });
                },
            );
            let (output, receipt) = output.into_parts();
            output.drop_without_applying_deltas();
            drop(receipt);
            assert_eq!(take_join_work(), (1, count * 2, count + unknown));
        }
    }
}

#[test]
fn packet_join_preserves_widget_and_rectangle_multiplicity_without_repeating_per_point() {
    for count in [1, 16, 128] {
        for unknown in [0, 1] {
            let context = Context::default();
            context.options_mut(|options| options.max_passes = 1.try_into().unwrap());
            let (output, receiver) = paint(&mut session());
            let mut bindings = widgets(&context, receiver, count);
            let bounds = bindings[0].interact_rect;
            // Repeated declarations must not consume the undeclared widget's
            // identical rectangle; only distinct live widget IDs get credit.
            bindings.extend_from_within(..);
            let scene = PointerScene {
                artifact: output.presentation_artifact(),
                layer: bindings[0].layer_id,
                completed_pass: context.cumulative_pass_nr(),
                coordinates_supported: true,
                receivers: bindings.into(),
                focus_bindings: Arc::from([]),
            };
            let mut snapshot = ProductPointerInputSnapshot::capture(&context);
            snapshot.interactive = vec![bounds; count + unknown];
            take_join_work();
            let blockers = snapshot.unowned_interactive_rects(&scene);
            assert_eq!(blockers.len(), unknown);
            for _ in 0..128 {
                assert_eq!(
                    blockers.iter().any(|rect| rect.contains(bounds.center())),
                    unknown == 1
                );
                assert!(
                    !blockers
                        .iter()
                        .any(|rect| rect.contains(Pos2::new(200.0, 200.0)))
                );
            }
            assert_eq!(take_join_work(), (1, count * 2, count + unknown));
        }
    }
}

#[test]
fn ownership_join_keeps_unproven_or_noninteractive_widget_rectangles_blocked() {
    let context = Context::default();
    let (output, receiver) = paint(&mut session());
    let binding = widgets(&context, receiver, 1)[0];
    for changed in [
        ProductReceiverBinding {
            interactive: false,
            ..binding
        },
        ProductReceiverBinding {
            widget_id: Id::unique("not-registered"),
            ..binding
        },
        ProductReceiverBinding {
            layer_id: LayerId::new(egui::Order::Foreground, Id::unique("foreign")),
            ..binding
        },
        ProductReceiverBinding {
            interact_rect: binding.interact_rect.shrink(1.0),
            ..binding
        },
    ] {
        let scene = PointerScene {
            artifact: output.presentation_artifact(),
            layer: binding.layer_id,
            completed_pass: context.cumulative_pass_nr(),
            coordinates_supported: true,
            receivers: Arc::from([changed]),
            focus_bindings: Arc::from([]),
        };
        let mut snapshot = ProductPointerInputSnapshot::capture(&context);
        snapshot.interactive = vec![binding.interact_rect];
        assert_eq!(
            snapshot.unowned_interactive_rects(&scene),
            [binding.interact_rect]
        );
    }
}

#[test]
fn rectangle_keys_preserve_float_equality_including_signed_zero_and_nan() {
    let positive = Rect::from_min_max(Pos2::ZERO, Pos2::new(100.0, 100.0));
    let negative = Rect::from_min_max(Pos2::new(-0.0, -0.0), positive.max);
    assert_eq!(positive, negative);
    assert_eq!(rectangle_key(positive), rectangle_key(negative));
    for index in 0..4 {
        let mut values = [
            positive.min.x,
            positive.min.y,
            positive.max.x,
            positive.max.y,
        ];
        values[index] = f32::NAN;
        let nan = Rect::from_min_max(
            Pos2::new(values[0], values[1]),
            Pos2::new(values[2], values[3]),
        );
        assert!(rectangle_key(nan).is_none());
    }
    let infinite = Rect::EVERYTHING;
    assert_eq!(
        rectangle_key(infinite),
        Some([
            f32::NEG_INFINITY.to_bits(),
            f32::NEG_INFINITY.to_bits(),
            f32::INFINITY.to_bits(),
            f32::INFINITY.to_bits(),
        ])
    );
    assert_ne!(
        rectangle_key(positive),
        rectangle_key(positive.translate(egui::vec2(0.0, 1.0)))
    );
}
