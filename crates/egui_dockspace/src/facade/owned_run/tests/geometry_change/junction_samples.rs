//! Real junction hits must keep both axes coherent through clamping and recovery.

use super::*;

mod cancellation;

const FOURTH: ItemId = ItemId::new(4);
const ITEMS: [ItemId; 4] = [FIRST, SECOND, THIRD, FOURTH];

struct Sample {
    junction: Rect,
    panes: BTreeMap<ItemId, Rect>,
    feedback: Vec<DockspaceManipulationStatus>,
    passes: usize,
}

fn sample(
    harness: &mut Harness,
    time: f64,
    events: Vec<Event>,
    result: SurfacePresentationResult,
) -> Sample {
    let (output, feedback) = harness.run(time, events);
    let (mut output, receipt) = output.into_parts();
    output.textures_delta.clear();
    let nodes = &output
        .platform_output
        .accesskit_update
        .as_ref()
        .unwrap()
        .nodes;
    let junctions = nodes
        .iter()
        .filter(|(_, node)| node.role() == Role::Group && node.label() == Some("Resize pane grid"))
        .map(|(_, node)| bounds(node))
        .collect::<Vec<_>>();
    assert_eq!(
        junctions.len(),
        1,
        "the final frame must expose its junction: entry={:?}, time={time}, zoom={}, feedback={feedback:?}",
        harness.entry,
        harness.context.zoom_factor(),
    );
    assert_eq!(
        harness.panes.bounds.keys().copied().collect::<Vec<_>>(),
        ITEMS
    );
    let sample = Sample {
        junction: junctions[0],
        panes: harness.panes.bounds.clone(),
        feedback,
        passes: output.platform_output.num_completed_passes,
    };
    output.drop_without_applying_deltas();
    receipt.report(result);
    sample
}

fn presented(harness: &mut Harness, time: f64, events: Vec<Event>) -> Sample {
    sample(harness, time, events, SurfacePresentationResult::Presented)
}

fn fixture(entry: Entry) -> (Harness, Sample) {
    let mut harness = harness(entry, GeometryChange::Chrome, false, 0);
    harness
        .context
        .all_styles_mut(|style| style.animation_time = 0.2);
    let column = |items: [ItemId; 2]| {
        DockspaceNode::equal_split(
            DockspaceAxis::Vertical,
            items.map(|item| DockspaceNode::tabs([item])),
        )
        .unwrap()
    };
    harness
        .dockspace
        .reset_layout(
            DockspaceLayout::new([DockspaceSurfaceLayout::new(
                SURFACE,
                DockspaceRootLayout::new(
                    RootId::new(1),
                    DockspaceNode::equal_split(
                        DockspaceAxis::Horizontal,
                        [column([FIRST, SECOND]), column([THIRD, FOURTH])],
                    )
                    .unwrap(),
                ),
            )])
            .unwrap(),
        )
        .unwrap();
    let mut stable = presented(&mut harness, 0.0, Vec::new());
    for time in [0.1, 0.2, 0.3] {
        stable = presented(&mut harness, time, Vec::new());
    }
    (harness, stable)
}

fn assert_shifted(before: &Sample, after: &Sample, delta: egui::Vec2) {
    assert!((after.junction.center() - before.junction.center() - delta).length() < 0.1);
    for item in ITEMS {
        let old = before.panes[&item];
        let new = after.panes[&item];
        let horizontal = if matches!(item, FIRST | SECOND) {
            delta.x
        } else {
            -delta.x
        };
        let vertical = if matches!(item, FIRST | THIRD) {
            delta.y
        } else {
            -delta.y
        };
        let origin_delta = egui::vec2(
            if matches!(item, THIRD | FOURTH) {
                delta.x
            } else {
                0.0
            },
            if matches!(item, SECOND | FOURTH) {
                delta.y
            } else {
                0.0
            },
        );
        assert!(
            (new.min - old.min - origin_delta).length() < 0.1,
            "{item:?}: {old:?} -> {new:?}"
        );
        assert!(
            (new.width() - old.width() - horizontal).abs() < 0.1,
            "{item:?}: {old:?} -> {new:?}"
        );
        assert!(
            (new.height() - old.height() - vertical).abs() < 0.1,
            "{item:?}: {old:?} -> {new:?}"
        );
    }
}

fn assert_delivery(sample: &Sample) {
    assert_eq!(
        sample.feedback,
        [DockspaceManipulationStatus::Delivered(
            DockspaceManipulationDelivery::SplitterResize,
        )]
    );
}

#[test]
fn junction_pointer_samples_resize_all_four_panes_before_release() {
    for entry in [Entry::Owned, Entry::Callback] {
        for preview in [
            Some(SurfacePresentationResult::Presented),
            Some(SurfacePresentationResult::Dropped),
            None,
        ] {
            let (mut harness, before) = fixture(entry);
            let version = harness.dockspace.version();
            let start = before.junction.center();
            let delta = egui::vec2(32.0, 24.0);
            let end = start + delta * 1.5;
            let released = if let Some(result) = preview {
                let pressed = presented(&mut harness, 1.0, press(start));
                assert_eq!(pressed.feedback, []);
                assert!(harness.dockspace.has_active_gesture());
                let moved = sample(
                    &mut harness,
                    1.1,
                    vec![Event::PointerMoved(start + delta)],
                    result,
                );
                assert_eq!(moved.feedback, []);
                assert_eq!(harness.dockspace.version(), version);
                assert_shifted(&before, &moved, delta);
                // The release carries a new position without a preceding move.
                presented(&mut harness, 1.2, vec![button(end, false)])
            } else {
                let mut events = press(start);
                events.push(button(end, false));
                presented(&mut harness, 1.2, events)
            };
            assert_delivery(&released);
            assert_shifted(&before, &released, delta * 1.5);
            assert_ne!(harness.dockspace.version(), version);
            assert!(!harness.dockspace.has_active_gesture());
            let tail = presented(&mut harness, 1.3, vec![button(end, false)]);
            assert_eq!(tail.feedback, []);
            assert_eq!(tail.panes, released.panes);
            #[cfg(feature = "motion")]
            assert!(!harness.dockspace.session.surface_motion_is_active(SURFACE));
        }
    }
}

#[test]
fn a_junction_clamps_each_axis_and_returns_to_the_original_partition_without_drift() {
    for entry in [Entry::Owned, Entry::Callback] {
        let (mut harness, before) = fixture(entry);
        let version = harness.dockspace.version();
        let start = before.junction.center();
        presented(&mut harness, 1.0, press(start));
        // Exceed each corner, then return. A frozen gesture must solve each sample
        // from the press geometry instead of accumulating clamped deltas.
        for (time, delta) in [
            (1.1, egui::vec2(10_000.0, 10_000.0)),
            (1.2, egui::vec2(-10_000.0, 10_000.0)),
            (1.3, egui::vec2(-10_000.0, -10_000.0)),
            (1.4, egui::vec2(10_000.0, -10_000.0)),
        ] {
            let moved = presented(&mut harness, time, vec![Event::PointerMoved(start + delta)]);
            assert_eq!(moved.feedback, []);
            assert!(harness.dockspace.has_active_gesture());
            assert_eq!(harness.dockspace.version(), version);
            for pane in moved.panes.values() {
                assert!(pane.is_finite() && pane.width() > 0.0 && pane.height() > 0.0);
            }
            assert!((moved.panes[&FIRST].right() - moved.panes[&SECOND].right()).abs() < 0.1);
            assert!((moved.panes[&SECOND].top() - moved.panes[&FOURTH].top()).abs() < 0.1);
            let farther = presented(
                &mut harness,
                time + 0.01,
                vec![Event::PointerMoved(start + delta * 2.0)],
            );
            assert_eq!(farther.feedback, []);
            assert_eq!(
                farther.panes, moved.panes,
                "moving past either limit must keep the same partition"
            );
        }
        let returned = presented(&mut harness, 1.6, vec![Event::PointerMoved(start)]);
        assert_shifted(&before, &returned, egui::Vec2::ZERO);
        let released = presented(&mut harness, 1.7, vec![button(start, false)]);
        assert_delivery(&released);
        assert_eq!(harness.dockspace.version(), version);
        assert_shifted(&before, &released, egui::Vec2::ZERO);
        assert!(!harness.dockspace.has_active_gesture());

        let next = released.junction.center();
        let mut events = press(next);
        events.push(button(next + egui::vec2(-24.0, 16.0), false));
        let resized = presented(&mut harness, 1.8, events);
        assert_delivery(&resized);
        assert_shifted(&before, &resized, egui::vec2(-24.0, 16.0));
        assert_ne!(harness.dockspace.version(), version);
    }
}
