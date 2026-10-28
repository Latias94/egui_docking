use super::*;

fn has_control(output: &FullOutput, role: Role, label: &str) -> bool {
    output
        .platform_output
        .accesskit_update
        .as_ref()
        .unwrap()
        .nodes
        .iter()
        .any(|(_, node)| node.role() == role && node.label() == Some(label))
}

#[test]
fn overflow_pointer_scroll_releases_to_menu_selection_and_clipped_close_stays_inert() {
    check_overflow_scroll_transition(true);
}

#[test]
fn overflow_accesskit_scroll_releases_to_menu_selection_and_clipped_close_stays_inert() {
    check_overflow_scroll_transition(false);
}

fn control_events(output: &FullOutput, label: &str, pointer: bool) -> Vec<Event> {
    let (id, node) = accesskit_node(output, Role::Button, label);
    assert!(node.supports_action(Action::Click));
    if pointer {
        let point = accesskit_node_rect(node).center();
        vec![
            Event::PointerMoved(point),
            pointer_button(point, true),
            pointer_button(point, false),
        ]
    } else {
        vec![accesskit_action(id, Action::Click)]
    }
}

fn check_overflow_scroll_transition(pointer: bool) {
    let context = Context::default();
    context.enable_accesskit();
    let style = DockStyle {
        tab_min_width: 320.0,
        tab_max_width: 320.0,
        ..DockStyle::default()
    };
    let mut dockspace = Dockspace::builder("product-overflow-owner-transition", overflow_layout())
        .style(style)
        .build()
        .unwrap();
    let mut panes = Panes;
    let initial = settle_until(8, || {
        let frame = run_frame(&context, &mut dockspace, &mut panes, Vec::new());
        let missing = if has_control(&frame.output, Role::Button, "Close First") {
            Vec::new()
        } else {
            vec!["initial tab close control"]
        };
        (frame, missing)
    })
    .unwrap();
    let (old_close, _) = accesskit_node(&initial.output, Role::Button, "Close First");

    let scrolled = settle_until(8, || {
        let frame = run_frame(&context, &mut dockspace, &mut panes, Vec::new());
        if !has_control(&frame.output, Role::Button, "Close First") {
            return (frame, Vec::new());
        }
        let changed = run_frame(
            &context,
            &mut dockspace,
            &mut panes,
            control_events(&frame.output, "Scroll tabs forward", pointer),
        );
        assert!(changed.close_requests.is_empty());
        let settled = run_frame(&context, &mut dockspace, &mut panes, Vec::new());
        let missing = if has_control(&settled.output, Role::Button, "Close First") {
            vec!["first close clipped out by explicit scroll"]
        } else {
            Vec::new()
        };
        (settled, missing)
    })
    .unwrap();
    assert!(dockspace.view().item(FIRST).unwrap().is_selected());
    assert!(!has_control(&scrolled.output, Role::Button, "Close First"));

    let stale = run_frame(
        &context,
        &mut dockspace,
        &mut panes,
        vec![accesskit_action(old_close, Action::Click)],
    );
    assert!(stale.close_requests.is_empty());
    assert!(dockspace.view().item(FIRST).unwrap().is_selected());
    let held = run_frame(&context, &mut dockspace, &mut panes, Vec::new());
    assert!(held.close_requests.is_empty());
    assert!(
        !has_control(&held.output, Role::Button, "Close First"),
        "an unchanged selection must not reclaim an explicit manual offset"
    );

    let (menu, _) = accesskit_node(&held.output, Role::Button, "Show hidden tabs");
    run_frame(
        &context,
        &mut dockspace,
        &mut panes,
        vec![accesskit_action(menu, Action::Click)],
    );
    let opened = settle_until(8, || {
        let frame = run_frame(&context, &mut dockspace, &mut panes, Vec::new());
        let missing = if has_control(&frame.output, Role::MenuItem, "Second") {
            Vec::new()
        } else {
            vec!["current overflow menu"]
        };
        (frame, missing)
    })
    .unwrap();
    let (second, _) = accesskit_node(&opened.output, Role::MenuItem, "Second");
    let selected = run_frame(
        &context,
        &mut dockspace,
        &mut panes,
        vec![accesskit_action(second, Action::Click)],
    );
    assert!(selected.close_requests.is_empty());
    let revealed = settle_until(8, || {
        let frame = run_frame(&context, &mut dockspace, &mut panes, Vec::new());
        let missing = if dockspace.view().item(SECOND).unwrap().is_selected()
            && has_control(&frame.output, Role::Button, "Close Second")
            && !has_control(&frame.output, Role::Menu, "Open tabs")
        {
            Vec::new()
        } else {
            vec!["menu selection revealed and popup closed"]
        };
        (frame, missing)
    })
    .unwrap();
    assert!(revealed.close_requests.is_empty());
    let backward = control_events(&revealed.output, "Scroll tabs backward", pointer);
    run_frame(&context, &mut dockspace, &mut panes, backward);
    let returned = settle_until(8, || {
        let frame = run_frame(&context, &mut dockspace, &mut panes, Vec::new());
        let missing = if has_control(&frame.output, Role::Button, "Close First") {
            Vec::new()
        } else {
            vec!["backward scroll reveals the preceding clipped tab"]
        };
        (frame, missing)
    })
    .unwrap();
    assert!(returned.close_requests.is_empty());
    assert!(dockspace.view().item(SECOND).unwrap().is_selected());
    assert_eq!(tab_items(&dockspace), [FIRST, SECOND, THIRD, FOURTH]);
    assert_eq!(surface_ids(&dockspace), [SURFACE]);
}
