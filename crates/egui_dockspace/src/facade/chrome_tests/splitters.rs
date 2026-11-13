//! Constrained separator values and endpoints through real keyboard focus.

use super::*;

fn splitter_values(output: &FullOutput) -> (f64, f64, f64) {
    let (_, node) = nodes(output)
        .iter()
        .find(|(_, node)| node.role() == Role::Splitter)
        .unwrap();
    (
        node.numeric_value().unwrap(),
        node.min_numeric_value().unwrap(),
        node.max_numeric_value().unwrap(),
    )
}

#[test]
fn home_end_move_focused_splitter_to_the_reported_accessible_limits() {
    for axis in [
        crate::DockspaceAxis::Horizontal,
        crate::DockspaceAxis::Vertical,
    ] {
        let style = DockStyle::default();
        let mut harness = Harness::new(
            vec!["First".to_owned(), "Second".to_owned()],
            ItemId::new(1),
            style.clone(),
        );
        let layout = DockspaceLayout::new([DockspaceSurfaceLayout::new(
            SURFACE,
            DockspaceRootLayout::new(
                RootId::new(1),
                DockspaceNode::equal_split(
                    axis,
                    [
                        DockspaceNode::tabs([ItemId::new(1)]),
                        DockspaceNode::tabs([ItemId::new(2)]),
                    ],
                )
                .unwrap(),
            ),
        )])
        .unwrap();
        harness.dockspace = Dockspace::builder(("separator-limits", axis), layout)
            .style(style)
            .build()
            .unwrap();
        harness.run(800.0, Vec::new());
        let initial = harness.run(800.0, Vec::new());
        let (value, minimum, maximum) = splitter_values(&initial);
        assert!(0.0 < minimum && minimum < value);
        assert!(value < maximum && maximum < 1.0);
        harness.focus(800.0, Role::Splitter, "Resize panes");
        for (key, expected) in [(Key::End, maximum), (Key::Home, minimum)] {
            harness.run(800.0, vec![super::key(key)]);
            let output = harness.run(800.0, Vec::new());
            let (actual, _, _) = splitter_values(&output);
            assert!(
                (actual - expected).abs() < 1e-5,
                "{axis:?} {key:?}: {actual} != {expected}"
            );
            assert!(nodes(&output).iter().any(|(id, node)| {
                node.role() == Role::Splitter
                    && harness
                        .context
                        .memory(egui::Memory::focused)
                        .is_some_and(|focused| focused.value() == id.0)
            }));
        }
    }
}
