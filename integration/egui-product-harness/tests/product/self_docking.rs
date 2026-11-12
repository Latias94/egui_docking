//! Self-docking follows the destination topology left after removing the payload.

use super::*;

fn split_source_layout(source: DockspaceNode) -> DockspaceLayout {
    DockspaceLayout::new([DockspaceSurfaceLayout::new(
        SURFACE,
        DockspaceRootLayout::new(
            ROOT,
            DockspaceNode::equal_split(
                DockspaceAxis::Horizontal,
                [source, DockspaceNode::tabs([THIRD])],
            )
            .expect("the source and remaining leaf form a valid split"),
        ),
    )])
    .expect("the self-docking layout is valid")
}

fn drag_first_tab_over_its_source(
    layout: DockspaceLayout,
) -> (Context, Dockspace, ProjectionPanes, Pos2, FrameOutput) {
    let context = Context::default();
    context.enable_accesskit();
    let mut dockspace = Dockspace::builder("product-self-docking", layout)
        .style(instrumented_style())
        .build()
        .expect("the self-docking facade initializes");
    let mut panes = ProjectionPanes::default();
    let _ = run_pane_frame(&context, &mut dockspace, &mut panes, Vec::new());
    let stable = run_pane_frame(&context, &mut dockspace, &mut panes, Vec::new());
    let tab = tab_center(&stable.output, "First");
    let source = panes.latest(FIRST).bounds.center();
    let _ = run_pane_frame(
        &context,
        &mut dockspace,
        &mut panes,
        vec![Event::PointerMoved(tab), pointer_button(tab, true)],
    );
    let _ = run_pane_frame(
        &context,
        &mut dockspace,
        &mut panes,
        vec![Event::PointerMoved(tab + vec2(16.0, 8.0))],
    );
    let _ = run_pane_frame(
        &context,
        &mut dockspace,
        &mut panes,
        vec![Event::PointerMoved(source)],
    );
    let preview = run_pane_frame(&context, &mut dockspace, &mut panes, Vec::new());
    (context, dockspace, panes, source, preview)
}

#[test]
fn last_tab_of_an_ordinary_leaf_has_no_inner_self_docking_targets() {
    let (context, mut dockspace, mut panes, source, preview) =
        drag_first_tab_over_its_source(split_source_layout(DockspaceNode::tabs([FIRST])));
    let passive = dockspace.style().visuals.drop_guide_fill.unwrap();
    let active = dockspace.style().visuals.drop_guide_active_fill.unwrap();
    assert_eq!(
        guide_button_count(&preview.output, passive, active),
        4,
        "removing the last tab deletes this ordinary leaf, leaving only the root's outer guides",
    );
    assert!(
        try_node_rect(&preview.output, Role::Label, "Dockspace center drop guide").is_none(),
        "the source leaf must have neither a painted nor an interactive center guide",
    );
    let _ = run_pane_frame(
        &context,
        &mut dockspace,
        &mut panes,
        vec![pointer_button(source, false)],
    );
    assert!(dockspace.view().item(FIRST).unwrap().contained().is_some());
    assert_eq!(root_tabs(&dockspace), Some(vec![THIRD]));
}

#[test]
fn last_tab_of_an_ordinary_leaf_can_still_dock_at_the_root_outer_edge() {
    let (context, mut dockspace, mut panes, _, preview) =
        drag_first_tab_over_its_source(split_source_layout(DockspaceNode::tabs([FIRST])));
    let target = nearest_guide_center(
        &preview.output,
        dockspace.style().visuals.drop_guide_fill.unwrap(),
        dockspace.style().visuals.drop_guide_active_fill.unwrap(),
        Pos2::new(800.0, 300.0),
    );
    let _ = run_pane_frame(
        &context,
        &mut dockspace,
        &mut panes,
        vec![Event::PointerMoved(target)],
    );
    let _ = run_pane_frame(
        &context,
        &mut dockspace,
        &mut panes,
        vec![pointer_button(target, false)],
    );
    assert_eq!(
        root_split(&dockspace),
        Some((DockspaceAxis::Horizontal, vec![vec![THIRD], vec![FIRST]])),
        "the outer edge belongs to the surviving root and can reposition the source pane",
    );
}

#[test]
fn a_source_leaf_with_remaining_tabs_keeps_its_inner_split_targets() {
    let (context, mut dockspace, mut panes, _, preview) =
        drag_first_tab_over_its_source(split_source_layout(DockspaceNode::tabs([FIRST, SECOND])));
    let passive = dockspace.style().visuals.drop_guide_fill.unwrap();
    let active = dockspace.style().visuals.drop_guide_active_fill.unwrap();
    assert_eq!(guide_button_count(&preview.output, passive, active), 9);
    let center = node_rect(&preview.output, Role::Label, "Dockspace center drop guide").center();
    let spacing = dockspace.style().drop_guide_extent + dockspace.style().drop_guide_gap;
    let target = nearest_guide_center(
        &preview.output,
        passive,
        active,
        center + vec2(spacing, 0.0),
    );
    let _ = run_pane_frame(
        &context,
        &mut dockspace,
        &mut panes,
        vec![Event::PointerMoved(target)],
    );
    let _ = run_pane_frame(
        &context,
        &mut dockspace,
        &mut panes,
        vec![pointer_button(target, false)],
    );
    assert_eq!(
        root_split(&dockspace),
        Some((
            DockspaceAxis::Horizontal,
            vec![vec![SECOND], vec![FIRST], vec![THIRD]],
        )),
        "splitting the remaining source tab is a meaningful local docking action",
    );
}

#[test]
fn the_last_central_tab_keeps_center_and_outer_docking() {
    for outer in [false, true] {
        let layout = DockspaceLayout::new([DockspaceSurfaceLayout::new(
            SURFACE,
            DockspaceRootLayout::new(ROOT, DockspaceNode::central_tabs([FIRST])),
        )])
        .expect("the central placeholder layout is valid");
        let (context, mut dockspace, mut panes, _, preview) =
            drag_first_tab_over_its_source(layout);
        let passive = dockspace.style().visuals.drop_guide_fill.unwrap();
        let active = dockspace.style().visuals.drop_guide_active_fill.unwrap();
        assert_eq!(guide_button_count(&preview.output, passive, active), 5);
        let center =
            node_rect(&preview.output, Role::Label, "Dockspace center drop guide").center();
        let target = if outer {
            nearest_guide_center(&preview.output, passive, active, Pos2::new(800.0, 300.0))
        } else {
            center
        };
        let _ = run_pane_frame(
            &context,
            &mut dockspace,
            &mut panes,
            vec![Event::PointerMoved(target)],
        );
        let _ = run_pane_frame(
            &context,
            &mut dockspace,
            &mut panes,
            vec![pointer_button(target, false)],
        );
        assert!(dockspace.view().item(FIRST).unwrap().contained().is_none());
        if outer {
            assert_eq!(
                root_split(&dockspace),
                Some((DockspaceAxis::Horizontal, vec![vec![], vec![FIRST]])),
                "the central placeholder remains while its last tab docks alongside it",
            );
        } else {
            assert_eq!(root_tabs(&dockspace), Some(vec![FIRST]));
        }
    }
}
