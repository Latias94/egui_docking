//! Complete egui hit graphs join callback recovery, contained drain and divider input.
//!
//! The fixture controls raw callbacks and renderer terminals. Widget registration,
//! receiver translation, candidate retry and core publication use production code.

use super::*;

mod editor;

use editor::Editor;

fn terminal(report: &HostFrameReport, delivery: Delivery) {
    assert!(
        matches!(report.manipulation_feedback(), [feedback] if feedback.status() == Status::Delivered(delivery)),
        "{report:?}"
    );
    assert!(
        report.workspace_changed(),
        "the gesture must commit its actual geometry or topology"
    );
}

fn drain_and_resize_with_completed_hits(
    desktop: bool,
    late_output: bool,
    reentry: bool,
    items: [ItemId; 2],
) {
    eprintln!(
        "completed-hit editor journey: desktop={desktop}, late_output={late_output}, reentry={reentry}, items={items:?}"
    );
    let mut editor = Editor::new(desktop);
    let mut held = None;
    for (index, item) in items.into_iter().enumerate() {
        let mut scene = editor.present();
        let source = scene.tab_point(item);
        let mut before = editor.native.session.version();
        if reentry && index == 0 {
            editor.pointer(source, Some(ElementState::Pressed));
            editor.pointer(scene.outline, None);
            assert!(editor.native.session.has_active_gesture());
            scene = editor.present();
            before = editor.native.session.version();
            assert!(
                editor
                    .native
                    .session
                    .view()
                    .item(item)
                    .unwrap()
                    .contained()
                    .is_some()
            );
            for event in [
                WindowEvent::CursorLeft {
                    device_id: DeviceId::dummy(),
                },
                WindowEvent::CursorEntered {
                    device_id: DeviceId::dummy(),
                },
            ] {
                let report = editor.callback(event);
                assert!(report.manipulation_feedback().is_empty());
                assert!(editor.native.session.has_active_gesture());
                assert_eq!(editor.native.session.version(), before);
            }
        }
        if late_output && index == 1 {
            // A queued output still contains the last floating tab. Drop it only
            // after the root drains, without presenting a successor out of order.
            held = Some(editor.hold_output());
        }
        let press = editor.pointer(source, Some(ElementState::Pressed));
        if reentry && index == 0 {
            assert!(
                matches!(press.manipulation_feedback(), [feedback] if matches!(feedback.status(), Status::Cancelled(_))),
                "{press:?}"
            );
            assert_eq!(editor.native.session.version(), before);
        } else {
            assert!(press.manipulation_feedback().is_empty());
        }
        editor.pointer(scene.outline, None);
        if held.is_none() {
            editor.present();
        }
        let report = editor.pointer(scene.outline, Some(ElementState::Released));
        terminal(&report, Delivery::Dock);
        assert!(!editor.native.session.has_active_gesture());
        assert_inventory(&editor.native, 1 - index);
        assert!(
            editor
                .native
                .session
                .view()
                .item(item)
                .unwrap()
                .contained()
                .is_none()
        );
    }
    assert!(editor.native.session.view().root(INSPECTOR_ROOT).is_none());
    if let Some(output) = held {
        let before = editor.native.session.version();
        editor
            .native
            .session
            .report_surface_presentation(output, SurfacePresentationResult::Dropped)
            .unwrap();
        let report = editor.commit(true);
        assert!(report.manipulation_feedback().is_empty());
        assert_eq!(editor.native.session.version(), before);
    }
    for axis in [DockspaceAxis::Horizontal, DockspaceAxis::Vertical] {
        let scene = editor.present();
        let divider = *scene
            .dividers
            .iter()
            .find(|divider| divider.axis == axis)
            .unwrap();
        let start = scene.divider_point(divider);
        scene.assert_divider_hit(&editor.context, divider, start);
        let target = match axis {
            DockspaceAxis::Horizontal => LogicalPoint::new(start.x() + 18.0, start.y()).unwrap(),
            DockspaceAxis::Vertical => LogicalPoint::new(start.x(), start.y() + 18.0).unwrap(),
        };
        let before = editor.native.session.version();
        editor.pointer(start, Some(ElementState::Pressed));
        assert!(editor.native.session.has_active_gesture());
        editor.pointer(target, None);
        editor.present();
        let released = editor.pointer(target, Some(ElementState::Released));
        terminal(&released, Delivery::SplitterResize);
        assert_ne!(editor.native.session.version(), before);
        assert!(!editor.native.session.has_active_gesture());
        let after = editor.present();
        assert_ne!(
            after
                .dividers
                .iter()
                .find(|other| other.visual == divider.visual)
                .unwrap()
                .draw,
            divider.draw
        );
        assert_inventory(&editor.native, 0);
    }
    editor.assert_quiescent();
}

#[test]
fn real_egui_hits_drain_both_editor_tabs_and_resize_both_nested_dividers() {
    for desktop in [false, true] {
        for late_output in [false, true] {
            for items in [[THIRD, INSPECTOR], [INSPECTOR, THIRD]] {
                drain_and_resize_with_completed_hits(desktop, late_output, false, items);
            }
        }
    }
}

#[test]
fn real_egui_hits_recover_a_lost_release_before_draining_and_resizing() {
    for desktop in [false, true] {
        for items in [[THIRD, INSPECTOR], [INSPECTOR, THIRD]] {
            drain_and_resize_with_completed_hits(desktop, false, true, items);
        }
    }
}
