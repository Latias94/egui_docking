//! Geometry changes must not strand a cancelled drag or the next divider drag.

use std::collections::BTreeMap;

use dockspace::model::DockspaceAxis;
use dockspace::runtime::{
    DockspaceManipulationDelivery, DockspaceManipulationFeedback, DockspaceManipulationStatus,
};
use egui::{Key, Modifiers};

use super::*;

#[cfg(feature = "motion")]
mod motion_recovery;
mod resize_samples;

const THIRD: ItemId = ItemId::new(3);

#[derive(Default)]
struct Panes {
    bounds: BTreeMap<ItemId, Rect>,
}

impl PaneView for Panes {
    fn title(&self, item: ItemId) -> Option<egui::WidgetText> {
        Some(format!("Pane {}", item.get()).into())
    }

    fn ui(&mut self, item: ItemId, ui: &mut Ui) -> crate::PaneResponse {
        self.bounds.insert(item, ui.available_rect_before_wrap());
        crate::PaneResponse::from(&ui.label(format!("Content {}", item.get())))
    }
}

struct Frame {
    tabs: BTreeMap<String, Rect>,
    splitter: Rect,
    passes: usize,
    feedback: Vec<DockspaceManipulationStatus>,
}

#[derive(Clone, Copy, Debug, Hash)]
enum Entry {
    Owned,
    Callback,
}

#[allow(
    clippy::cast_possible_truncation,
    reason = "actual AccessKit bounds are finite coordinates within the test viewport"
)]
fn bounds(node: &egui::accesskit::Node) -> Rect {
    let bounds = node.bounds().expect("painted widgets expose their bounds");
    Rect::from_min_max(
        egui::pos2(bounds.x0 as f32, bounds.y0 as f32),
        egui::pos2(bounds.x1 as f32, bounds.y1 as f32),
    )
}

struct Harness {
    entry: Entry,
    context: Context,
    dockspace: Dockspace,
    panes: Panes,
}

impl Harness {
    fn run(
        &mut self,
        time: f64,
        events: Vec<Event>,
    ) -> (crate::DockspaceOutput, Vec<DockspaceManipulationStatus>) {
        let Self {
            entry,
            context,
            dockspace,
            panes,
        } = self;
        panes.bounds.clear();
        let mut raw = input(time, events);
        raw.screen_rect = Some(Rect::from_min_size(
            Pos2::ZERO,
            egui::vec2(640.0, 480.0) / context.zoom_factor(),
        ));
        let mut feedback = Vec::new();
        let mut observe = |response: crate::DockspaceResponse| {
            feedback.extend(
                response
                    .manipulation_feedback()
                    .iter()
                    .map(DockspaceManipulationFeedback::status),
            );
        };
        let output = match entry {
            Entry::Owned => {
                let (output, response) = dockspace
                    .run_single_surface(context, raw, SURFACE, panes)
                    .expect("a geometry change and cancellation must remain a valid owned run");
                observe(response);
                output
            }
            Entry::Callback => crate::run_ui(context, raw, |ui| {
                let response = dockspace
                    .show_single_surface(SURFACE, ui, panes)
                    .expect("a geometry change and cancellation must remain a valid callback");
                observe(response);
            }),
        };
        (output, feedback)
    }

    fn frame(&mut self, time: f64, events: Vec<Event>) -> Frame {
        self.frame_with_result(time, events, SurfacePresentationResult::Presented)
    }

    fn frame_with_result(
        &mut self,
        time: f64,
        events: Vec<Event>,
        result: SurfacePresentationResult,
    ) -> Frame {
        let (output, feedback) = self.run(time, events);
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
            .collect::<BTreeMap<_, _>>();
        assert_eq!(
            tabs.len(),
            3,
            "all three pane identities must survive at {time}: {tabs:?}"
        );
        let splitters = nodes
            .iter()
            .filter(|(_, node)| {
                node.role() == Role::Splitter && node.label() == Some("Resize panes")
            })
            .map(|(_, node)| bounds(node))
            .collect::<Vec<_>>();
        assert_eq!(
            splitters.len(),
            1,
            "the layout retains one ordinary divider at {time}"
        );
        let frame = Frame {
            tabs,
            splitter: splitters[0],
            passes: output.platform_output.num_completed_passes,
            feedback,
        };
        output.drop_without_applying_deltas();
        receipt.report(result);
        frame
    }
}

fn button(pos: Pos2, pressed: bool) -> Event {
    Event::PointerButton {
        pos,
        button: PointerButton::Primary,
        pressed,
        modifiers: Modifiers::NONE,
    }
}

#[derive(Clone, Copy, Debug, Hash)]
enum GeometryChange {
    Zoom,
    Chrome,
    ZoomAndChrome,
}

fn harness(
    entry: Entry,
    change: GeometryChange,
    focus_loss: bool,
    discarded_passes: usize,
) -> Harness {
    let context = Context::default();
    context.enable_accesskit();
    // Isolate input recovery from the deliberate splitter suspension during
    // structural motion. Existing motion tests qualify that separate boundary.
    context.all_styles_mut(|style| style.animation_time = 0.0);
    context.options_mut(|options| options.max_passes = NonZeroUsize::new(1).unwrap());
    let layout = DockspaceLayout::new([DockspaceSurfaceLayout::new(
        SURFACE,
        DockspaceRootLayout::new(
            RootId::new(1),
            DockspaceNode::equal_split(
                DockspaceAxis::Horizontal,
                [
                    DockspaceNode::central_tabs([FIRST, SECOND]),
                    DockspaceNode::tabs([THIRD]),
                ],
            )
            .unwrap(),
        ),
    )])
    .unwrap();
    let mut dockspace = Dockspace::builder(
        ("geometry-cancel-resize", entry, change, focus_loss),
        layout,
    )
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
    Harness {
        entry,
        context,
        dockspace,
        panes: Panes::default(),
    }
}

fn apply_geometry_change(
    harness: &mut Harness,
    change: GeometryChange,
) -> Vec<DockspaceManipulationStatus> {
    let mut feedback = Vec::new();
    if matches!(change, GeometryChange::Zoom | GeometryChange::ZoomAndChrome) {
        harness.context.set_zoom_factor(1.5);
    }
    if matches!(
        change,
        GeometryChange::Chrome | GeometryChange::ZoomAndChrome
    ) {
        let mut style = harness.dockspace.style().clone();
        style.tab_bar_height += 12.0;
        style.splitter_thickness += 4.0;
        feedback.extend(
            harness
                .dockspace
                .set_style(style)
                .unwrap()
                .manipulation_feedback()
                .iter()
                .map(DockspaceManipulationFeedback::status),
        );
    }
    feedback
}

fn cancelled_drag_then_resize(
    entry: Entry,
    change: GeometryChange,
    focus_loss: bool,
    discarded_passes: usize,
) {
    let mut harness = harness(entry, change, focus_loss, discarded_passes);
    let mut rendered = harness.frame(0.0, Vec::new());
    for time in [0.1, 0.2, 0.3] {
        rendered = harness.frame(time, Vec::new());
    }
    let source = rendered.tabs["Pane 2"].center();
    harness.frame(1.0, press(source));
    harness.frame(
        1.05,
        vec![Event::PointerMoved(source + egui::vec2(12.0, 30.0))],
    );
    assert!(harness.dockspace.has_active_gesture());
    let before_cancel = harness.dockspace.version();
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
    let cancelled = harness.frame(1.1, vec![cancel]);
    assert_eq!(cancelled.passes, discarded_passes + 1);
    assert!(
        (cancelled.splitter.height() - 480.0 / harness.context.zoom_factor()).abs() < 0.1,
        "cancelled geometry must refresh to the actual viewport: divider={:?}, viewport={:?}",
        cancelled.splitter,
        harness.context.content_rect(),
    );
    feedback.extend(cancelled.feedback);
    assert!(
        matches!(
            feedback.as_slice(),
            [DockspaceManipulationStatus::Cancelled(_)]
        ),
        "cancellation publishes once across style updates and discarded passes: {feedback:?}"
    );
    assert!(!harness.dockspace.has_active_gesture());
    assert_eq!(harness.dockspace.version(), before_cancel);
    for item in [SECOND, THIRD] {
        assert!(
            (harness.panes.bounds[&item].top() - harness.dockspace.style().tab_bar_height).abs()
                < 0.1,
            "the cancellation output must already use the current tab-bar height",
        );
    }
    assert_eq!(
        harness.dockspace.view().item(FIRST).unwrap().tabs().items(),
        &[FIRST, SECOND]
    );
    assert_eq!(
        harness.dockspace.view().item(THIRD).unwrap().tabs().items(),
        &[THIRD]
    );

    resize_after_cancel(&mut harness, source, cancelled.splitter.center());
}

fn resize_after_cancel(harness: &mut Harness, source: Pos2, splitter: Pos2) {
    // The stale release cannot finish docking; the next press uses only the
    // final output's fresh divider geometry, without any stabilizing idle run.
    let before_resize = harness.dockspace.version();
    let recovered = harness.frame(
        1.2,
        vec![
            Event::WindowFocused(true),
            button(source, false),
            Event::PointerMoved(splitter),
            button(splitter, true),
        ],
    );
    assert_eq!(recovered.feedback, []);
    assert!(
        harness.dockspace.has_active_gesture(),
        "a fresh divider press must recover"
    );
    let left = harness.panes.bounds[&SECOND];
    let right = harness.panes.bounds[&THIRD];
    let end = splitter + egui::vec2(40.0, 0.0);
    let moved = harness.frame(1.3, vec![Event::PointerMoved(end)]);
    assert_eq!(moved.feedback, []);
    let released = harness.frame(1.4, vec![button(end, false)]);
    let settled = harness.frame(1.5, Vec::new());
    let feedback = released
        .feedback
        .into_iter()
        .chain(settled.feedback)
        .collect::<Vec<_>>();
    assert_eq!(
        feedback,
        [DockspaceManipulationStatus::Delivered(
            DockspaceManipulationDelivery::SplitterResize
        )],
    );
    assert!(!harness.dockspace.has_active_gesture());
    assert!(
        harness.panes.bounds[&SECOND].width() > left.width() + 20.0,
        "left before={left:?}, after={:?}",
        harness.panes.bounds[&SECOND]
    );
    assert!(
        harness.panes.bounds[&THIRD].width() < right.width() - 20.0,
        "right before={right:?}, after={:?}",
        harness.panes.bounds[&THIRD]
    );
    assert_ne!(harness.dockspace.version(), before_resize);
}

#[test]
fn zoom_during_drag_allows_cancel_and_immediate_divider_resize() {
    for focus_loss in [false, true] {
        for discarded_passes in [0, 2] {
            cancelled_drag_then_resize(
                Entry::Owned,
                GeometryChange::Zoom,
                focus_loss,
                discarded_passes,
            );
        }
    }
}

#[test]
fn chrome_change_during_drag_allows_cancel_and_immediate_divider_resize() {
    for focus_loss in [false, true] {
        for discarded_passes in [0, 2] {
            cancelled_drag_then_resize(
                Entry::Owned,
                GeometryChange::Chrome,
                focus_loss,
                discarded_passes,
            );
        }
    }
}

#[test]
fn zoom_and_chrome_change_during_drag_allow_cancel_and_immediate_divider_resize() {
    for focus_loss in [false, true] {
        for discarded_passes in [0, 2] {
            cancelled_drag_then_resize(
                Entry::Owned,
                GeometryChange::ZoomAndChrome,
                focus_loss,
                discarded_passes,
            );
        }
    }
}

#[test]
fn callback_geometry_changes_allow_cancel_and_immediate_divider_resize() {
    for change in [
        GeometryChange::Zoom,
        GeometryChange::Chrome,
        GeometryChange::ZoomAndChrome,
    ] {
        for focus_loss in [false, true] {
            for discarded_passes in [0, 2] {
                cancelled_drag_then_resize(Entry::Callback, change, focus_loss, discarded_passes);
            }
        }
    }
}
