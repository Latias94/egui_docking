//! The exact data-drag layer is an append-only exception, not an audit bypass.

use super::*;

#[test]
fn drag_ghost_is_frozen_before_callbacks_and_survives_snapshot_reuse_and_final_capture() {
    let context = Context::default();
    context.options_mut(|options| options.max_passes = 1.try_into().unwrap());
    let dragged = egui::Id::unique("proof-data-drag");
    let ghost = LayerId::new(egui::Order::Tooltip, dragged);
    let mut proof = None;
    let output = context.run_ui(egui::RawInput::default(), |ui| {
        ui.ctx().set_dragged_id(dragged);
        let capture = SurfaceOutputCapture::begin_multilayer(ui.painter());
        let mut audit = PaintAudit::new(ui.ctx());
        audit.check_append(ui.painter(), || {
            ui.ctx().layer_painter(ghost).circle_filled(
                egui::pos2(10.0, 10.0),
                3.0,
                egui::Color32::BLUE,
            );
            ui.ctx().transform_layer_shapes(
                ghost,
                egui::emath::TSTransform::from_translation(egui::vec2(20.0, 10.0)),
            );
        });
        assert!(
            audit.is_valid(),
            "an empty original ghost prefix is preserved"
        );
        assert!(audit.snapshots.reusable.is_some());
        // The current widget can stop dragging after painting its preview.
        // Neither the next checkpoint nor the final capture may lose the
        // already frozen layer or accept a replacement identity.
        ui.ctx().stop_dragging();
        audit.check_append(ui.painter(), || {
            ui.ctx().layer_painter(ghost).circle_filled(
                egui::pos2(50.0, 10.0),
                2.0,
                egui::Color32::GREEN,
            );
        });
        assert!(audit.is_valid());
        proof = capture.finish(ui.painter());
    });
    let proof = proof.expect("the exact ghost suffix must join the complete output");
    assert_eq!(proof.spans.len(), 1);
    assert_eq!(proof.spans[0].layer, ghost);
    assert!(proof.is_present_once(&output).is_some());
    output.drop_without_applying_deltas();
}

#[derive(Clone, Copy, Debug)]
enum Corruption {
    ExistingGhostPrefix,
    GhostGlobalTransform,
    OtherKnownTooltip,
    UnknownTooltip,
    ReplacementDrag,
}

#[test]
fn drag_exception_still_rejects_prefix_transform_and_other_layer_changes() {
    for corruption in [
        Corruption::ExistingGhostPrefix,
        Corruption::GhostGlobalTransform,
        Corruption::OtherKnownTooltip,
        Corruption::UnknownTooltip,
        Corruption::ReplacementDrag,
    ] {
        let context = Context::default();
        context.options_mut(|options| options.max_passes = 1.try_into().unwrap());
        context
            .run_ui(egui::RawInput::default(), |ui| {
                let dragged = egui::Id::unique("audited-data-drag");
                let ghost = LayerId::new(egui::Order::Tooltip, dragged);
                let other = LayerId::new(
                    egui::Order::Tooltip,
                    egui::Id::unique("unrelated-data-drag"),
                );
                ui.ctx().set_dragged_id(dragged);
                if matches!(corruption, Corruption::OtherKnownTooltip) {
                    ui.memory_mut(|memory| {
                        memory
                            .areas_mut()
                            .set_state(other, egui::AreaState::default());
                    });
                }
                ui.ctx().layer_painter(ghost).circle_filled(
                    egui::pos2(10.0, 10.0),
                    3.0,
                    egui::Color32::BLUE,
                );
                let mut audit = PaintAudit::new(ui.ctx());
                audit.check_append(ui.painter(), || match corruption {
                    Corruption::ExistingGhostPrefix => ui.ctx().transform_layer_shapes(
                        ghost,
                        egui::emath::TSTransform::from_translation(egui::vec2(5.0, 0.0)),
                    ),
                    Corruption::GhostGlobalTransform => ui.ctx().set_transform_layer(
                        ghost,
                        egui::emath::TSTransform::from_translation(egui::vec2(5.0, 0.0)),
                    ),
                    Corruption::OtherKnownTooltip | Corruption::UnknownTooltip => {
                        ui.ctx().layer_painter(other).circle_filled(
                            egui::pos2(20.0, 20.0),
                            2.0,
                            egui::Color32::RED,
                        );
                    }
                    Corruption::ReplacementDrag => {
                        ui.ctx().set_dragged_id(other.id);
                        ui.ctx().layer_painter(other).circle_filled(
                            egui::pos2(20.0, 20.0),
                            2.0,
                            egui::Color32::RED,
                        );
                    }
                });
                assert!(
                    !audit.is_valid(),
                    "{corruption:?} must not escape the checkpoint"
                );
            })
            .drop_without_applying_deltas();
    }
}
