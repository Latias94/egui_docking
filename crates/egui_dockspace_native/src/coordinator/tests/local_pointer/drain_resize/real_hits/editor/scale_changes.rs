//! Independent DPI and UI scaling revoke resize input before fresh egui hits resume.

use super::*;

fn change_scales(
    editor: &mut Editor,
    native_scale: f32,
    presentation_scale: f32,
) -> HostFrameReport {
    let mut facts = NativeWindowFacts::live()
        .with_content_size(geometry().content_size())
        .with_native_scale_factor(ScaleFactor::new(f64::from(native_scale)).unwrap())
        .with_presentation_scale_factor(ScaleFactor::new(f64::from(presentation_scale)).unwrap())
        .with_input(NativeWindowInputState::ReceivesInput, None)
        .with_presentation(NativeWindowPresentationState::Visible);
    let mut capabilities = NativeHostCapabilities::none_supported()
        .with(NativeHostCapability::AuthoritativeInventory)
        .with(NativeHostCapability::AuthoritativeButtonState)
        .with(NativeHostCapability::PointerHitTestObservation);
    if editor.desktop {
        facts = facts.with_content_bounds(PhysicalRect::new(400.0, -300.0, 1200.0, 900.0).unwrap());
        capabilities = capabilities
            .with(NativeHostCapability::DesktopPointerPosition)
            .with(NativeHostCapability::HoveredWindow);
    }
    editor
        .native
        .report_snapshot(
            [(editor.binding, facts)],
            NativeWorkAreaRoster::Unknown,
            capabilities,
        )
        .unwrap();
    let report = editor.commit(true);
    editor.native_scale = native_scale;
    editor.presentation_scale = presentation_scale;
    editor
        .context
        .options_mut(|options| options.zoom_factor = presentation_scale / native_scale);
    report
}

fn divisor(scene: &RenderedScene, axis: DockspaceAxis) -> Divider {
    *scene
        .dividers
        .iter()
        .find(|divider| divider.axis == axis)
        .unwrap()
}

fn scale_change_during_resize(desktop: bool, native_dpi: bool, result: SurfacePresentationResult) {
    let mut editor = Editor::new(desktop);
    let original = editor.present();
    let original_surface = editor
        .native
        .session
        .presented_surface(FIRST_SURFACE)
        .unwrap();
    let divider = divisor(&original, DockspaceAxis::Horizontal);
    let start = original.divider_point(divider);
    original.assert_divider_hit(&editor.context, divider, start);
    let version = editor.native.session.version();
    editor.pointer(start, Some(ElementState::Pressed));
    editor.pointer(
        LogicalPoint::new(start.x() + 24.0, start.y()).unwrap(),
        None,
    );
    assert!(editor.native.session.has_active_gesture());
    let held = editor.hold_output();
    let old_coordinates = editor
        .native
        .session
        .native_local_coordinates(editor.binding)
        .unwrap();

    let changed = if native_dpi {
        change_scales(&mut editor, 1.25, 1.5)
    } else {
        change_scales(&mut editor, 2.0, 2.5)
    };
    assert!(
        matches!(changed.manipulation_feedback(), [feedback] if matches!(feedback.status(), Status::Cancelled(_)))
    );
    assert!(!changed.workspace_changed());
    assert_eq!(editor.native.session.version(), version);
    assert!(!editor.native.session.has_active_gesture());
    let changed_coordinates = editor
        .native
        .session
        .native_local_coordinates(editor.binding)
        .unwrap();
    if native_dpi {
        assert_ne!(changed_coordinates, old_coordinates);
    } else {
        // Presentation scaling changes surface coordinates, while the raw
        // client-pixel identity deliberately remains independent of UI zoom.
        assert_eq!(changed_coordinates, old_coordinates);
    }
    editor
        .native
        .session
        .report_surface_presentation(held, result)
        .unwrap();
    let late = editor.commit(true);
    assert_eq!(late.manipulation_feedback(), []);
    assert_eq!(editor.native.session.version(), version);
    assert!(!editor.native.session.has_active_gesture());
    let tail = editor.pointer(start, Some(ElementState::Released));
    assert_eq!(tail.manipulation_feedback(), []);
    assert_eq!(editor.native.session.version(), version);

    // Returning to the same numeric scale must issue new authority, never revive
    // the original preview or the cancelled pointer session.
    let returned = change_scales(&mut editor, 2.0, 1.5);
    assert_eq!(returned.manipulation_feedback(), []);
    if native_dpi {
        assert_ne!(
            editor
                .native
                .session
                .native_local_coordinates(editor.binding)
                .unwrap(),
            old_coordinates
        );
    }
    let fresh = editor.present();
    assert_ne!(
        editor
            .native
            .session
            .presented_surface(FIRST_SURFACE)
            .unwrap(),
        original_surface
    );
    let fresh_divider = divisor(&fresh, DockspaceAxis::Horizontal);
    assert_eq!(fresh_divider.draw, divider.draw);
    let point = fresh.divider_point(fresh_divider);
    fresh.assert_divider_hit(&editor.context, fresh_divider, point);
    editor.pointer(point, Some(ElementState::Pressed));
    assert!(editor.native.session.has_active_gesture());
    let end = LogicalPoint::new(point.x() + 18.0, point.y()).unwrap();
    let delivered = editor.pointer(end, Some(ElementState::Released));
    super::super::terminal(&delivered, Delivery::SplitterResize);
    let final_scene = editor.present();
    assert_ne!(
        divisor(&final_scene, DockspaceAxis::Horizontal).draw,
        divider.draw
    );
    assert_inventory(&editor.native, 1);
    editor.assert_quiescent();
}

#[test]
fn changing_native_dpi_or_ui_scale_revokes_a_resize_and_accepts_fresh_real_hits() {
    for desktop in [false, true] {
        for native_dpi in [false, true] {
            for result in [
                SurfacePresentationResult::Presented,
                SurfacePresentationResult::Dropped,
            ] {
                scale_change_during_resize(desktop, native_dpi, result);
            }
        }
    }
}
