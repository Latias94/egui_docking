//! Pointer resize samples are direct manipulation, including an unseen release.

use super::*;

#[derive(Clone, Copy, Debug)]
enum Sequence {
    PresentedPreview,
    DroppedPreview,
    OnePacket,
}

fn assert_no_layout_motion(harness: &Harness) {
    #[cfg(feature = "motion")]
    assert!(
        !harness.dockspace.session.surface_motion_is_active(SURFACE),
        "pointer resizing must not enqueue a structural transition",
    );
    #[cfg(not(feature = "motion"))]
    let _ = harness;
}

fn fixture(entry: Entry, axis: DockspaceAxis) -> (Harness, Frame) {
    let mut harness = harness(entry, GeometryChange::Chrome, false, 0);
    // Keep normal egui motion enabled; the no-motion build runs this same journey.
    harness
        .context
        .all_styles_mut(|style| style.animation_time = 0.2);
    if axis == DockspaceAxis::Vertical {
        harness
            .dockspace
            .reset_layout(
                DockspaceLayout::new([DockspaceSurfaceLayout::new(
                    SURFACE,
                    DockspaceRootLayout::new(
                        RootId::new(1),
                        DockspaceNode::equal_split(
                            axis,
                            [
                                DockspaceNode::central_tabs([FIRST, SECOND]),
                                DockspaceNode::tabs([THIRD]),
                            ],
                        )
                        .unwrap(),
                    ),
                )])
                .unwrap(),
            )
            .unwrap();
    }
    let mut stable = harness.frame(0.0, Vec::new());
    for time in [0.1, 0.2, 0.3] {
        stable = harness.frame(time, Vec::new());
    }
    (harness, stable)
}

fn sample_resize(entry: Entry, axis: DockspaceAxis, sequence: Sequence) {
    let (mut harness, stable) = fixture(entry, axis);
    let start = stable.splitter.center();
    let offset = match axis {
        DockspaceAxis::Horizontal => egui::vec2(40.0, 0.0),
        DockspaceAxis::Vertical => egui::vec2(0.0, 40.0),
    };
    let extent = |rect: Rect| match axis {
        DockspaceAxis::Horizontal => rect.width(),
        DockspaceAxis::Vertical => rect.height(),
    };
    let before = harness.dockspace.version();
    let leading = extent(harness.panes.bounds[&FIRST]);
    let trailing = extent(harness.panes.bounds[&THIRD]);
    let end = start + offset * 1.5;
    let released = if matches!(sequence, Sequence::OnePacket) {
        let mut events = press(start);
        events.extend([Event::PointerMoved(end), button(end, false)]);
        harness.frame(1.2, events)
    } else {
        let pressed = harness.frame(1.0, press(start));
        assert_eq!(pressed.feedback, []);
        let result = match sequence {
            Sequence::PresentedPreview => SurfacePresentationResult::Presented,
            Sequence::DroppedPreview => SurfacePresentationResult::Dropped,
            Sequence::OnePacket => unreachable!(),
        };
        let moved =
            harness.frame_with_result(1.1, vec![Event::PointerMoved(start + offset)], result);
        assert_eq!(moved.feedback, []);
        assert!(harness.dockspace.has_active_gesture());
        assert_eq!(harness.dockspace.version(), before);
        assert!(
            (extent(harness.panes.bounds[&FIRST]) - leading - 40.0).abs() < 0.1,
            "the current move must resize leading content immediately: {entry:?}/{axis:?}/{sequence:?}",
        );
        assert!((extent(harness.panes.bounds[&THIRD]) - trailing + 40.0).abs() < 0.1);
        assert!((moved.splitter.center() - start - offset).length() < 0.1);
        assert_no_layout_motion(&harness);
        harness.frame(1.2, vec![button(end, false)])
    };
    assert_eq!(
        released.feedback,
        [DockspaceManipulationStatus::Delivered(
            DockspaceManipulationDelivery::SplitterResize
        )],
    );
    assert!(!harness.dockspace.has_active_gesture());
    assert_ne!(harness.dockspace.version(), before);
    assert!((extent(harness.panes.bounds[&FIRST]) - leading - 60.0).abs() < 0.1);
    assert!((extent(harness.panes.bounds[&THIRD]) - trailing + 60.0).abs() < 0.1);
    assert!((released.splitter.center() - end).length() < 0.1);
    assert_no_layout_motion(&harness);

    let final_bounds = harness.panes.bounds.clone();
    let version = harness.dockspace.version();
    for time in [1.3, 1.4, 10.0] {
        let settled = harness.frame(time, Vec::new());
        assert_eq!(settled.feedback, []);
        assert_eq!(harness.panes.bounds, final_bounds);
        assert_eq!(harness.dockspace.version(), version);
        assert_no_layout_motion(&harness);
    }

    #[cfg(feature = "motion")]
    {
        harness
            .dockspace
            .dock_item_current(THIRD, crate::DockPlacement::After(FIRST))
            .unwrap();
        let (output, feedback) = harness.run(11.0, Vec::new());
        assert_eq!(feedback, []);
        let (output, receipt) = output.into_parts();
        output.drop_without_applying_deltas();
        receipt.report(SurfacePresentationResult::Presented);
        assert!(
            harness.dockspace.session.surface_motion_is_active(SURFACE),
            "finishing a resize must preserve later structural motion",
        );
    }
}

fn cancel_resize(entry: Entry, axis: DockspaceAxis, focus_loss: bool) {
    let (mut harness, stable) = fixture(entry, axis);
    let original = harness.panes.bounds.clone();
    let before = harness.dockspace.version();
    let start = stable.splitter.center();
    let offset = match axis {
        DockspaceAxis::Horizontal => egui::vec2(40.0, 0.0),
        DockspaceAxis::Vertical => egui::vec2(0.0, 40.0),
    };
    harness.frame(1.0, press(start));
    let moved = harness.frame(1.1, vec![Event::PointerMoved(start + offset)]);
    assert_eq!(moved.feedback, []);
    assert_ne!(harness.panes.bounds, original);
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
    let cancelled = harness.frame(1.2, vec![cancel]);
    assert!(matches!(
        cancelled.feedback.as_slice(),
        [DockspaceManipulationStatus::Cancelled(_)]
    ));
    assert!(!harness.dockspace.has_active_gesture());
    assert_eq!(harness.dockspace.version(), before);
    assert_eq!(harness.panes.bounds, original);
    assert_eq!(cancelled.splitter, stable.splitter);
    let next = cancelled.splitter.center();
    let pressed = harness.frame(
        1.3,
        vec![
            Event::WindowFocused(true),
            button(start + offset, false),
            Event::PointerMoved(next),
            button(next, true),
        ],
    );
    assert_eq!(pressed.feedback, []);
    assert!(harness.dockspace.has_active_gesture());
    let released = harness.frame(1.4, vec![button(next + offset * 0.5, false)]);
    assert_eq!(
        released.feedback,
        [DockspaceManipulationStatus::Delivered(
            DockspaceManipulationDelivery::SplitterResize
        )]
    );
    assert!(!harness.dockspace.has_active_gesture());
    assert_ne!(harness.dockspace.version(), before);
    assert!((released.splitter.center() - next - offset * 0.5).length() < 0.1);
}

#[test]
fn pointer_resizes_follow_each_sample_and_release_without_a_second_transition() {
    for entry in [Entry::Owned, Entry::Callback] {
        for axis in [DockspaceAxis::Horizontal, DockspaceAxis::Vertical] {
            for sequence in [
                Sequence::PresentedPreview,
                Sequence::DroppedPreview,
                Sequence::OnePacket,
            ] {
                sample_resize(entry, axis, sequence);
            }
        }
    }
}

#[test]
fn cancelling_a_resize_restores_the_current_partition_and_accepts_a_fresh_press() {
    for entry in [Entry::Owned, Entry::Callback] {
        for axis in [DockspaceAxis::Horizontal, DockspaceAxis::Vertical] {
            for focus_loss in [false, true] {
                cancel_resize(entry, axis, focus_loss);
            }
        }
    }
}
