use super::super::EditorProbe;
use super::*;

#[test]
fn editor_probe_excludes_real_junction_controls_from_ordinary_splitters() {
    use super::super::super::{ExamplePanes, example_layout};

    let context = egui::Context::default();
    context.enable_accesskit();
    let observations = Arc::new(Mutex::new(EditorObservations::default()));
    context.add_plugin(EditorProbe(Arc::clone(&observations)));
    let mut dockspace = egui_dockspace::Dockspace::builder("editor-probe", example_layout())
        .build()
        .unwrap();
    let mut panes = ExamplePanes::new();
    let mut output = context.run_ui(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1280.0, 820.0),
            )),
            ..Default::default()
        },
        |ui| {
            dockspace
                .show_single_surface(SURFACE, ui, &mut panes)
                .unwrap();
        },
    );
    output.textures_delta.clear();
    let update = output.platform_output.accesskit_update.unwrap();
    assert!(update.nodes.iter().any(|(_, node)| {
        node.role() == egui::accesskit::Role::Splitter
            && node
                .label()
                .is_some_and(|label| label.starts_with("Resize pane grid"))
    }));
    let observed = observations.lock().unwrap();
    assert_eq!(observed.splitters.len(), 2);
    for orientation in [
        egui::accesskit::Orientation::Horizontal,
        egui::accesskit::Orientation::Vertical,
    ] {
        assert_eq!(
            observed
                .splitters
                .iter()
                .filter(|(axis, _)| *axis == orientation)
                .count(),
            1,
        );
    }
}

#[test]
fn physical_resize_starts_in_hit_padding_at_fractional_scales() {
    for scale in [0.75, 1.0, 1.5, 2.0] {
        for axis in [DockspaceAxis::Horizontal, DockspaceAxis::Vertical] {
            let style = DockStyle::default();
            let hit = match axis {
                DockspaceAxis::Horizontal => egui::Rect::from_min_size(
                    egui::pos2(100.25, 30.5),
                    egui::vec2(style.splitter_hit_extent, 600.0),
                ),
                DockspaceAxis::Vertical => egui::Rect::from_min_size(
                    egui::pos2(100.25, 300.5),
                    egui::vec2(600.0, style.splitter_hit_extent),
                ),
            };
            let (source, target) = resize_points(hit, axis, scale);
            assert_eq!(source, source.round());
            assert_eq!(target, target.round());
            assert!(hit.contains(source / scale));
            let delta = (target - source) / scale;
            match axis {
                DockspaceAxis::Horizontal => {
                    assert!((source.x / scale - hit.center().x).abs() > 0.5);
                    assert_eq!(delta, egui::vec2(48.0, 0.0));
                }
                DockspaceAxis::Vertical => {
                    assert!((source.y / scale - hit.center().y).abs() > 0.5);
                    assert_eq!(delta, egui::vec2(0.0, 48.0));
                }
            }
        }
    }
}

#[test]
fn resize_completion_requires_both_neighboring_panes_to_change() {
    let rect = |x, y, width, height| {
        egui::Rect::from_min_size(egui::pos2(x, y), egui::vec2(width, height))
    };
    let cases = [
        (
            DockspaceAxis::Horizontal,
            [rect(0.0, 0.0, 200.0, 600.0), rect(206.0, 0.0, 800.0, 400.0)],
            [rect(0.0, 0.0, 248.0, 600.0), rect(254.0, 0.0, 752.0, 400.0)],
        ),
        (
            DockspaceAxis::Vertical,
            [rect(0.0, 0.0, 800.0, 400.0), rect(0.0, 406.0, 800.0, 200.0)],
            [rect(0.0, 0.0, 800.0, 448.0), rect(0.0, 454.0, 800.0, 152.0)],
        ),
    ];
    for (axis, before, after) in cases {
        assert!(resize_completed(axis, before, after));
        assert!(!resize_completed(axis, before, before));
        assert!(!resize_completed(axis, before, [after[0], before[1]]));
        assert!(!resize_completed(axis, before, [before[0], after[1]]));
    }
}

#[test]
fn reentry_observer_requires_the_same_held_press_on_both_sides() {
    use egui::plugin::Plugin as _;

    let point = egui::pos2(10.0, 20.0);
    let button = |pressed| egui::Event::PointerButton {
        pos: point,
        button: egui::PointerButton::Primary,
        pressed,
        modifiers: egui::Modifiers::NONE,
    };
    let observed = Arc::new(Mutex::new(EditorObservations::default()));
    let mut probe = EditorProbe(Arc::clone(&observed));
    let mut input = |events| {
        probe.input_hook(
            &egui::Context::default(),
            &mut egui::RawInput {
                events,
                screen_rect: Some(egui::Rect::from_min_max(
                    egui::Pos2::ZERO,
                    egui::pos2(800.0, 600.0),
                )),
                ..Default::default()
            },
        );
    };
    input(vec![
        egui::Event::PointerGone,
        egui::Event::PointerMoved(point),
    ]);
    assert_eq!(observed.lock().unwrap().pressed_reentries, 0);
    input(vec![button(true), egui::Event::PointerGone, button(false)]);
    input(vec![egui::Event::PointerMoved(point)]);
    assert_eq!(observed.lock().unwrap().pressed_reentries, 0);
    input(vec![button(true), egui::Event::PointerGone]);
    input(vec![egui::Event::PointerMoved(egui::pos2(-20.0, 20.0))]);
    assert_eq!(observed.lock().unwrap().pressed_reentries, 0);
    input(vec![egui::Event::PointerMoved(point)]);
    assert_eq!(observed.lock().unwrap().pressed_reentries, 1);
    input(vec![egui::Event::PointerMoved(point), button(false)]);
    assert_eq!(observed.lock().unwrap().pressed_reentries, 1);
}
