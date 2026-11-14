//! Cancellation restores the current partition across geometry and output changes.

use super::*;

#[test]
fn unpresented_junction_previews_and_dropped_cancellation_outputs_restore_immediately() {
    for entry in [Entry::Owned, Entry::Callback] {
        for preview in [
            SurfacePresentationResult::Presented,
            SurfacePresentationResult::Dropped,
        ] {
            for recovery in [
                SurfacePresentationResult::Presented,
                SurfacePresentationResult::Dropped,
            ] {
                cancel_across_output_results(entry, preview, recovery);
            }
        }
    }
}

fn cancel_across_output_results(
    entry: Entry,
    preview: SurfacePresentationResult,
    recovery: SurfacePresentationResult,
) {
    let (mut harness, before) = fixture(entry);
    let version = harness.dockspace.version();
    let start = before.junction.center();
    presented(&mut harness, 1.0, press(start));
    let moved = sample(
        &mut harness,
        1.1,
        vec![Event::PointerMoved(start + egui::vec2(32.0, -24.0))],
        preview,
    );
    assert_shifted(&before, &moved, egui::vec2(32.0, -24.0));
    let cancelled = sample(
        &mut harness,
        1.2,
        vec![Event::Key {
            key: Key::Escape,
            physical_key: Some(Key::Escape),
            pressed: true,
            repeat: false,
            modifiers: Modifiers::NONE,
        }],
        recovery,
    );
    assert!(matches!(
        cancelled.feedback.as_slice(),
        [DockspaceManipulationStatus::Cancelled(_)]
    ));
    assert_eq!(harness.dockspace.version(), version);
    assert!(!harness.dockspace.has_active_gesture());
    assert_eq!(cancelled.panes, before.panes);
    assert_eq!(cancelled.junction, before.junction);
    for time in [1.21, 1.22, 1.5] {
        let retried = presented(&mut harness, time, Vec::new());
        assert_eq!(retried.feedback, []);
        assert_eq!(retried.panes, before.panes);
        assert_eq!(retried.junction, before.junction);
        assert_eq!(harness.dockspace.version(), version);
        #[cfg(feature = "motion")]
        assert!(!harness.dockspace.session.surface_motion_is_active(SURFACE));
    }

    #[cfg(feature = "motion")]
    {
        harness
            .dockspace
            .dock_item_current(FOURTH, crate::DockPlacement::After(FIRST))
            .unwrap();
        let (output, feedback) = harness.run(2.0, Vec::new());
        assert_eq!(feedback, []);
        let (output, receipt) = output.into_parts();
        output.drop_without_applying_deltas();
        receipt.report(SurfacePresentationResult::Presented);
        assert!(
            harness.dockspace.session.surface_motion_is_active(SURFACE),
            "cancelling a resize must preserve later structural animation"
        );
    }
}

#[test]
fn cancelling_a_junction_after_geometry_changes_recovers_both_axes_and_the_next_press() {
    for entry in [Entry::Owned, Entry::Callback] {
        for change in [
            GeometryChange::Zoom,
            GeometryChange::Chrome,
            GeometryChange::ZoomAndChrome,
        ] {
            for focus_loss in [false, true] {
                for discarded_passes in [0, 2] {
                    cancel_after_geometry_change(
                        entry,
                        change,
                        focus_loss,
                        discarded_passes,
                        SurfacePresentationResult::Presented,
                    );
                }
            }
        }
    }
}

#[test]
fn a_new_chrome_ticket_keeps_resize_rollback_immediate_after_a_dropped_recovery() {
    for entry in [Entry::Owned, Entry::Callback] {
        cancel_after_geometry_change(
            entry,
            GeometryChange::Chrome,
            false,
            0,
            SurfacePresentationResult::Dropped,
        );
    }
}

fn cancel_after_geometry_change(
    entry: Entry,
    change: GeometryChange,
    focus_loss: bool,
    discarded_passes: usize,
    recovery: SurfacePresentationResult,
) {
    let (mut harness, before) = fixture(entry);
    harness
        .dockspace
        .session
        .configure_output_capacity(NonZeroUsize::new(discarded_passes + 1).unwrap())
        .unwrap();
    let start = before.junction.center();
    let delta = egui::vec2(32.0, 24.0);
    presented(&mut harness, 1.0, press(start));
    let moved = presented(&mut harness, 1.1, vec![Event::PointerMoved(start + delta)]);
    assert_shifted(&before, &moved, delta);
    let version = harness.dockspace.version();
    let mut feedback = apply_geometry_change(&mut harness, change);
    harness.context.options_mut(|options| {
        options.max_passes = NonZeroUsize::new(discarded_passes + 1).unwrap();
    });
    harness.context.add_plugin(LateDiscard {
        remaining: discarded_passes,
    });
    let cancel = if focus_loss {
        Event::WindowFocused(false)
    } else {
        Event::Key {
            key: Key::Escape,
            physical_key: Some(Key::Escape),
            pressed: true,
            repeat: false,
            modifiers: Modifiers::NONE,
        }
    };
    let cancelled = sample(&mut harness, 1.2, vec![cancel], recovery);
    assert_eq!(cancelled.passes, discarded_passes + 1);
    feedback.extend(cancelled.feedback);
    assert!(matches!(
        feedback.as_slice(),
        [DockspaceManipulationStatus::Cancelled(_)]
    ));
    assert!(!harness.dockspace.has_active_gesture());
    assert_eq!(harness.dockspace.version(), version);

    // Compare cancellation with the same committed layout under the new chrome
    // and viewport, rather than accepting the old preview's frozen dimensions.
    let (mut reference, _) = fixture(entry);
    reference
        .context
        .all_styles_mut(|style| style.animation_time = 0.0);
    apply_geometry_change(&mut reference, change);
    let expected = presented(&mut reference, 1.2, Vec::new());
    assert_eq!(
        cancelled.panes, expected.panes,
        "{entry:?}/{change:?}/focus_loss={focus_loss}/discard={discarded_passes}"
    );
    assert_eq!(cancelled.junction, expected.junction);
    if recovery == SurfacePresentationResult::Dropped {
        let retried = presented(&mut harness, 1.21, Vec::new());
        assert_eq!(retried.feedback, []);
        assert_eq!(retried.panes, expected.panes);
        assert_eq!(retried.junction, expected.junction);
        assert_eq!(harness.dockspace.version(), version);
    }
    let next = cancelled.junction.center();
    let pressed = presented(
        &mut harness,
        1.3,
        vec![
            Event::WindowFocused(true),
            button(start + delta, false),
            Event::PointerMoved(next),
            button(next, true),
        ],
    );
    assert_eq!(pressed.feedback, []);
    assert!(harness.dockspace.has_active_gesture());
    let end = next + egui::vec2(-24.0, 16.0);
    let released = presented(&mut harness, 1.4, vec![button(end, false)]);
    assert_delivery(&released);
    assert_shifted(&expected, &released, egui::vec2(-24.0, 16.0));
    assert!(!harness.dockspace.has_active_gesture());
    assert_ne!(harness.dockspace.version(), version);
}
