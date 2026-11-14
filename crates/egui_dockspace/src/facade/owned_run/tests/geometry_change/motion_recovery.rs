//! Real egui hits must recover after a tab press interrupts moving content.

use super::*;
use crate::DockPlacement;

struct Sample {
    tabs: BTreeMap<String, Rect>,
    splitters: Vec<Rect>,
    feedback: Vec<DockspaceManipulationStatus>,
    passes: usize,
}

fn sample(harness: &mut Harness, time: f64, events: Vec<Event>) -> Sample {
    let (output, feedback) = harness.run(time, events);
    let (mut output, receipt) = output.into_parts();
    output.textures_delta.clear();
    let nodes = &output
        .platform_output
        .accesskit_update
        .as_ref()
        .unwrap()
        .nodes;
    let tabs = nodes
        .iter()
        .filter(|(_, node)| node.role() == Role::Tab)
        .map(|(_, node)| (node.label().unwrap().to_owned(), bounds(node)))
        .collect();
    let splitters = nodes
        .iter()
        .filter(|(_, node)| node.role() == Role::Splitter && node.label() == Some("Resize panes"))
        .map(|(_, node)| bounds(node))
        .collect();
    let sample = Sample {
        tabs,
        splitters,
        feedback,
        passes: output.platform_output.num_completed_passes,
    };
    output.drop_without_applying_deltas();
    receipt.report(SurfacePresentationResult::Presented);
    sample
}

fn moving(entry: Entry, discarded_passes: usize) -> (Harness, Sample) {
    let context = Context::default();
    context.enable_accesskit();
    context.options_mut(|options| options.max_passes = NonZeroUsize::new(1).unwrap());
    let layout = DockspaceLayout::new([DockspaceSurfaceLayout::new(
        SURFACE,
        DockspaceRootLayout::new(
            RootId::new(1),
            DockspaceNode::equal_split(
                DockspaceAxis::Horizontal,
                [
                    DockspaceNode::central_tabs([FIRST]),
                    DockspaceNode::tabs([SECOND]),
                    DockspaceNode::tabs([THIRD]),
                ],
            )
            .unwrap(),
        ),
    )])
    .unwrap();
    let mut dockspace = Dockspace::builder(("moving-tab-input-recovery", entry), layout)
        .style(crate::DockStyle {
            tab_min_width: 48.0,
            tab_max_width: 80.0,
            ..Default::default()
        })
        .build()
        .unwrap();
    dockspace
        .session
        .configure_output_capacity(
            NonZeroUsize::new(match entry {
                Entry::Owned => 1,
                Entry::Callback => discarded_passes + 1,
            })
            .unwrap(),
        )
        .unwrap();
    let mut harness = Harness {
        entry,
        context,
        dockspace,
        panes: Panes::default(),
    };
    for time in [0.0, 0.1, 0.2, 0.3] {
        let stable = sample(&mut harness, time, Vec::new());
        assert_eq!(stable.splitters.len(), 2);
    }
    let before = harness.panes.bounds[&SECOND];
    harness
        .dockspace
        .dock_item_current(THIRD, DockPlacement::After(FIRST))
        .unwrap();
    sample(&mut harness, 1.0, Vec::new());
    let moving = sample(&mut harness, 1.05, Vec::new());
    assert!(harness.dockspace.session.surface_motion_is_active(SURFACE));
    assert_eq!(moving.splitters, []);
    assert_ne!(harness.panes.bounds[&SECOND], before);
    assert!(!harness.dockspace.view().item(FIRST).unwrap().is_selected());
    (harness, moving)
}

fn press_during_motion_then_cancel(entry: Entry, focus_loss: bool, discarded_passes: usize) {
    let (mut harness, moving) = moving(entry, discarded_passes);
    let source = moving.tabs["Pane 1"].center();
    let pressed = sample(&mut harness, 1.06, press(source));
    assert_eq!(pressed.feedback, []);
    assert!(harness.dockspace.has_active_gesture());
    assert!(harness.dockspace.view().item(FIRST).unwrap().is_selected());
    let version = harness.dockspace.version();
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
    let cancelled = sample(&mut harness, 1.07, vec![cancel]);
    assert_eq!(cancelled.passes, discarded_passes + 1);
    assert!(matches!(
        cancelled.feedback.as_slice(),
        [DockspaceManipulationStatus::Cancelled(_)]
    ));
    assert!(!harness.dockspace.has_active_gesture());
    assert_eq!(harness.dockspace.version(), version);

    // A structural transition may finish after cancellation, but its final
    // presented geometry must expose a complete, operable split partition.
    let settled = sample(&mut harness, 1.5, Vec::new());
    assert_eq!(settled.feedback, []);
    assert_eq!(settled.tabs.len(), 3);
    assert_eq!(settled.splitters.len(), 1);
    assert!(!harness.dockspace.session.surface_motion_is_active(SURFACE));
    assert_eq!(harness.dockspace.version(), version);
    let splitter = settled.splitters[0].center();
    let left = harness.panes.bounds[&FIRST];
    let right = harness.panes.bounds[&SECOND];
    let pressed = sample(
        &mut harness,
        1.6,
        vec![
            Event::WindowFocused(true),
            button(source, false),
            Event::PointerMoved(splitter),
            button(splitter, true),
        ],
    );
    assert_eq!(pressed.feedback, []);
    assert!(harness.dockspace.has_active_gesture());
    let end = splitter + egui::vec2(40.0, 0.0);
    let moved = sample(&mut harness, 1.7, vec![Event::PointerMoved(end)]);
    assert_eq!(moved.feedback, []);
    assert!(harness.panes.bounds[&FIRST].width() > left.width() + 20.0);
    assert!(harness.panes.bounds[&SECOND].width() < right.width() - 20.0);
    let released = sample(&mut harness, 1.8, vec![button(end, false)]);
    assert_eq!(
        released.feedback,
        [DockspaceManipulationStatus::Delivered(
            DockspaceManipulationDelivery::SplitterResize
        )]
    );
    let settled = sample(&mut harness, 1.9, Vec::new());
    assert_eq!(settled.feedback, []);
    assert!(!harness.dockspace.has_active_gesture());
    assert!(!harness.dockspace.session.surface_motion_is_active(SURFACE));
    assert_eq!(settled.splitters.len(), 1);
    assert!(harness.panes.bounds[&FIRST].width() > left.width() + 20.0);
    assert!(harness.panes.bounds[&SECOND].width() < right.width() - 20.0);
    assert_eq!(
        harness.dockspace.view().item(FIRST).unwrap().tabs().items(),
        &[FIRST, THIRD]
    );
    assert_eq!(
        harness
            .dockspace
            .view()
            .item(SECOND)
            .unwrap()
            .tabs()
            .items(),
        &[SECOND]
    );
}

#[test]
fn a_tab_press_during_default_motion_can_cancel_then_resize_through_real_hits() {
    for entry in [Entry::Owned, Entry::Callback] {
        for focus_loss in [false, true] {
            for discarded_passes in [0, 2] {
                press_during_motion_then_cancel(entry, focus_loss, discarded_passes);
            }
        }
    }
}
