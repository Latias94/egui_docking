use super::*;

#[test]
fn locked_overflow_menu_selects_hidden_tabs_without_reordering_or_closing() {
    #[derive(Clone, Copy, Debug)]
    enum Activation {
        Pointer,
        AccessKit,
        Keyboard,
    }

    for activation in [
        Activation::Pointer,
        Activation::AccessKit,
        Activation::Keyboard,
    ] {
        let context = Context::default();
        context.enable_accesskit();
        let style = DockStyle {
            tab_min_width: 220.0,
            tab_max_width: 220.0,
            ..DockStyle::default()
        };
        let mut dockspace = Dockspace::builder(
            format!("product-locked-overflow-{activation:?}"),
            overflow_layout(),
        )
        .style(style)
        .build()
        .expect("the overflow product fixture initializes");
        let mut panes = Panes;
        let _ = run_frame(&context, &mut dockspace, &mut panes, Vec::new());
        let unlocked = run_frame(&context, &mut dockspace, &mut panes, Vec::new());
        let (old_close, close_node) = accesskit_node(&unlocked.output, Role::Button, "Close First");
        assert!(close_node.supports_action(Action::Click));
        let old_close_point = accesskit_node_rect(close_node).center();

        let mut policy = dockspace.policy().clone();
        policy.set_arrangement_locked(true);
        dockspace
            .set_policy(policy)
            .expect("the arrangement lock commits");
        let _ = run_frame(&context, &mut dockspace, &mut panes, Vec::new());
        let locked = run_frame(&context, &mut dockspace, &mut panes, Vec::new());
        assert!(locked.close_requests.is_empty());
        assert!(
            locked
                .output
                .platform_output
                .accesskit_update
                .as_ref()
                .unwrap()
                .nodes
                .iter()
                .filter(|(_, node)| node.role() == Role::Button
                    && node
                        .label()
                        .is_some_and(|label| label.starts_with("Close ")))
                .all(|(_, node)| node.is_disabled() && !node.supports_action(Action::Click)),
            "locked close controls must be absent or disabled"
        );
        let rejected_close = run_frame(
            &context,
            &mut dockspace,
            &mut panes,
            vec![
                accesskit_action(old_close, Action::Click),
                Event::PointerMoved(old_close_point),
                pointer_button(old_close_point, true),
                pointer_button(old_close_point, false),
            ],
        );
        assert!(rejected_close.close_requests.is_empty(), "{activation:?}");
        assert_eq!(tab_items(&dockspace), [FIRST, SECOND, THIRD, FOURTH]);

        let stable = run_frame(&context, &mut dockspace, &mut panes, Vec::new());
        assert!(stable.close_requests.is_empty());
        let (control, control_node) =
            accesskit_node(&stable.output, Role::Button, "Show hidden tabs");
        assert!(control_node.supports_action(Action::Click));
        let open_events = match activation {
            Activation::Pointer => {
                let point = accesskit_node_rect(control_node).center();
                vec![
                    Event::PointerMoved(point),
                    pointer_button(point, true),
                    pointer_button(point, false),
                ]
            }
            Activation::AccessKit | Activation::Keyboard => {
                vec![accesskit_action(control, Action::Click)]
            }
        };
        let opened = run_frame(&context, &mut dockspace, &mut panes, open_events);
        assert!(opened.close_requests.is_empty());
        let menu = run_frame(&context, &mut dockspace, &mut panes, Vec::new());
        assert!(menu.close_requests.is_empty());
        let (row, row_node) = accesskit_node(&menu.output, Role::MenuItem, "Fourth");
        assert!(row_node.supports_action(Action::Click));
        assert!(row_node.supports_action(Action::ScrollIntoView));
        let select_events = match activation {
            Activation::Pointer => {
                let point = accesskit_node_rect(row_node).center();
                vec![
                    Event::PointerMoved(point),
                    pointer_button(point, true),
                    pointer_button(point, false),
                ]
            }
            Activation::AccessKit => vec![accesskit_action(row, Action::Click)],
            Activation::Keyboard => {
                let navigated =
                    run_frame(&context, &mut dockspace, &mut panes, key_press(Key::End));
                assert!(navigated.close_requests.is_empty());
                key_press(Key::Enter)
            }
        };
        let selected = run_frame(&context, &mut dockspace, &mut panes, select_events);
        assert!(selected.close_requests.is_empty(), "{activation:?}");
        let settled = run_frame(&context, &mut dockspace, &mut panes, Vec::new());
        assert!(settled.close_requests.is_empty());
        assert!(
            dockspace.view().item(FOURTH).unwrap().is_selected(),
            "{activation:?}"
        );
        assert_eq!(
            tab_items(&dockspace),
            [FIRST, SECOND, THIRD, FOURTH],
            "{activation:?}"
        );
        assert_eq!(surface_ids(&dockspace), [SURFACE]);
        assert!(
            dockspace
                .view()
                .surface(SURFACE)
                .unwrap()
                .contained()
                .next()
                .is_none()
        );
        assert!(dockspace.policy().arrangement_locked());
    }
}

#[test]
fn locked_rear_contained_tab_and_content_keep_focus_and_raise_without_geometry_changes() {
    for content_click in [false, true] {
        let context = Context::default();
        context.enable_accesskit();
        context.options_mut(|options| options.max_passes = 4.try_into().unwrap());
        let rear = LogicalRect::new(80.0, 80.0, 300.0, 220.0).unwrap();
        let front = LogicalRect::new(300.0, 240.0, 300.0, 220.0).unwrap();
        let items = if content_click {
            [SECOND, THIRD]
        } else {
            [THIRD, SECOND]
        };
        let layout = DockspaceLayout::new([DockspaceSurfaceLayout::new(
            SURFACE,
            DockspaceRootLayout::new(ROOT, DockspaceNode::central_tabs([FIRST])),
        )
        .with_contained(DockspaceContainedLayout::new(
            FLOATING,
            DockspaceRootLayout::new(FLOATING_ROOT, DockspaceNode::tabs(items)),
            rear,
        ))
        .with_contained(DockspaceContainedLayout::new(
            FRONT_FLOATING,
            DockspaceRootLayout::new(FRONT_FLOATING_ROOT, DockspaceNode::tabs([FOURTH])),
            front,
        ))])
        .unwrap();
        let mut dockspace = Dockspace::builder(
            ("product-locked-contained-navigation", content_click),
            layout,
        )
        .build()
        .expect("the stacked product fixture initializes");
        let mut panes = FocusPanes::new("product-locked-contained-navigation", true);
        let _ = run_focus_frame(&context, &mut dockspace, &mut panes, Vec::new());
        let (unlocked, _) = run_focus_frame(&context, &mut dockspace, &mut panes, Vec::new());
        let (old_resize, resize_node) = unlocked
            .output
            .platform_output
            .accesskit_update
            .as_ref()
            .unwrap()
            .nodes
            .iter()
            .find(|(_, node)| {
                node.role() == Role::Splitter
                    && node.label() == Some("Resize floating right edge")
                    && (accesskit_node_rect(node).center().x - front.max().x() as f32).abs()
                        <= dockspace.style().floating_resize_extent
            })
            .expect("the front window exposes its right resize control");
        assert!(resize_node.supports_action(Action::Increment));
        let old_resize = *old_resize;

        let mut policy = dockspace.policy().clone();
        policy.set_arrangement_locked(true);
        dockspace
            .set_policy(policy)
            .expect("the arrangement lock commits");
        let _ = run_focus_frame(&context, &mut dockspace, &mut panes, Vec::new());
        let (stable, _) = run_focus_frame(&context, &mut dockspace, &mut panes, Vec::new());
        assert_eq!(
            dockspace
                .view()
                .surface(SURFACE)
                .unwrap()
                .contained()
                .map(|contained| contained.id())
                .collect::<Vec<_>>(),
            [FLOATING, FRONT_FLOATING]
        );
        assert_eq!(
            dockspace.view().item(SECOND).unwrap().is_selected(),
            content_click
        );
        let source = if content_click {
            panes.target_rect().center()
        } else {
            tab_center(&stable.output, "Second")
        };
        let (clicked, _) = run_focus_frame(
            &context,
            &mut dockspace,
            &mut panes,
            vec![
                Event::PointerMoved(source),
                pointer_button(source, true),
                pointer_button(source, false),
            ],
        );
        assert!(clicked.close_requests.is_empty());
        let _ = run_focus_frame(&context, &mut dockspace, &mut panes, Vec::new());
        let (focused, capability) =
            run_focus_frame(&context, &mut dockspace, &mut panes, Vec::new());
        assert!(focused.close_requests.is_empty());
        assert_eq!(capability, DockspaceCapability::Supported);
        assert!(panes.target_requests() > 0, "content_click={content_click}");
        assert!(
            context.memory(|memory| memory.has_focus(panes.target)),
            "content_click={content_click}"
        );
        assert!(dockspace.view().item(SECOND).unwrap().is_selected());
        assert_eq!(
            dockspace
                .view()
                .surface(SURFACE)
                .unwrap()
                .contained()
                .map(|contained| contained.id())
                .collect::<Vec<_>>(),
            [FRONT_FLOATING, FLOATING]
        );

        let title = contained_title_point(&dockspace);
        let resize = Pos2::new(rear.max().x() as f32 - 1.0, rear.min().y() as f32 + 100.0);
        for (start, end) in [
            (title, title + vec2(40.0, 30.0)),
            (resize, resize + vec2(40.0, 0.0)),
        ] {
            let (pressed, _) = run_focus_frame(
                &context,
                &mut dockspace,
                &mut panes,
                vec![Event::PointerMoved(start), pointer_button(start, true)],
            );
            assert!(pressed.close_requests.is_empty());
            let (moved, _) = run_focus_frame(
                &context,
                &mut dockspace,
                &mut panes,
                vec![Event::PointerMoved(end)],
            );
            assert!(moved.close_requests.is_empty());
            assert_eq!(contained_rect(&dockspace), rear);
            let (released, _) = run_focus_frame(
                &context,
                &mut dockspace,
                &mut panes,
                vec![Event::PointerMoved(end), pointer_button(end, false)],
            );
            assert!(released.close_requests.is_empty());
            assert_eq!(contained_rect(&dockspace), rear);
        }
        let (inert_resize, _) = run_focus_frame(
            &context,
            &mut dockspace,
            &mut panes,
            vec![accesskit_action(old_resize, Action::Increment)],
        );
        assert!(inert_resize.close_requests.is_empty());
        assert_eq!(contained_rect(&dockspace), rear);
        assert_eq!(
            dockspace.view().contained(FRONT_FLOATING).unwrap().rect(),
            front
        );
        let root = dockspace.view().root(FLOATING_ROOT).unwrap();
        let tabs = root.content().unwrap().tabs().unwrap();
        assert_eq!(tabs.items(), &items);
        for item in items {
            let location = dockspace.view().item(item).unwrap();
            assert_eq!(location.root(), FLOATING_ROOT);
            assert_eq!(location.contained(), Some(FLOATING));
        }
        assert_eq!(surface_ids(&dockspace), [SURFACE]);
        assert!(dockspace.policy().arrangement_locked());
    }
}
