use super::*;
use egui_dockspace::{
    DeferredCloseDecision, DockspaceAppliedClose, DockspaceCloseInertReason, DockspaceCloseOutcome,
    DockspaceClosePhase, DockspaceCloseResolution,
};

#[test]
fn contained_group_close_waits_for_application_approval_for_every_activation() {
    #[derive(Clone, Copy)]
    enum Activation {
        Pointer,
        AccessKit,
        Keyboard(Key),
    }

    for (name, activation) in [
        ("pointer", Activation::Pointer),
        ("accesskit", Activation::AccessKit),
        ("enter", Activation::Keyboard(Key::Enter)),
        ("space", Activation::Keyboard(Key::Space)),
    ] {
        let context = Context::default();
        context.enable_accesskit();
        let layout = DockspaceLayout::new([DockspaceSurfaceLayout::new(
            SURFACE,
            DockspaceRootLayout::new(ROOT, DockspaceNode::central_tabs([FIRST])),
        )
        .with_contained(DockspaceContainedLayout::new(
            FLOATING,
            DockspaceRootLayout::new(FLOATING_ROOT, DockspaceNode::tabs([SECOND, THIRD])),
            LogicalRect::new(500.0, 260.0, 240.0, 220.0)
                .expect("the group-close rectangle is valid"),
        ))])
        .expect("the group-close layout is valid");
        let mut dockspace = Dockspace::builder(format!("product-contained-close-{name}"), layout)
            .build()
            .expect("the product contained facade initializes");
        let mut panes = Panes;

        let _ = run_frame(&context, &mut dockspace, &mut panes, Vec::new());
        let stable = run_frame(&context, &mut dockspace, &mut panes, Vec::new());
        let before_version = dockspace.version();
        let before_rect = contained_rect(&dockspace);
        let assert_group_unchanged = |dockspace: &Dockspace| {
            assert_eq!(dockspace.version(), before_version, "{name}");
            assert_eq!(root_tabs(dockspace), Some(vec![FIRST]), "{name}");
            let contained = dockspace
                .view()
                .contained(FLOATING)
                .expect("the entire group remains contained until approval");
            assert_eq!(contained.rect(), before_rect, "{name}");
            let root = contained.root().expect("the group retains its root");
            assert_eq!(root.id(), FLOATING_ROOT, "{name}");
            let tabs = root
                .content()
                .and_then(|content| content.tabs())
                .expect("the complete group retains its tabs");
            assert_eq!(tabs.items(), &[SECOND, THIRD], "{name}");
            assert_eq!(tabs.selected(), Some(SECOND), "{name}");
            for item in [SECOND, THIRD] {
                let location = dockspace.view().item(item).expect("the item remains open");
                assert_eq!(location.root(), FLOATING_ROOT, "{name}");
                assert_eq!(location.surface(), SURFACE, "{name}");
                assert_eq!(location.contained(), Some(FLOATING), "{name}");
            }
        };
        let (close, _) = accesskit_node(&stable.output, Role::Button, "Close floating Second");
        let point = node_center(&stable.output, Role::Button, "Close floating Second");
        let activated = match activation {
            Activation::Pointer => {
                let _ = run_frame(
                    &context,
                    &mut dockspace,
                    &mut panes,
                    vec![Event::PointerMoved(point), pointer_button(point, true)],
                );
                run_frame(
                    &context,
                    &mut dockspace,
                    &mut panes,
                    vec![Event::PointerMoved(point), pointer_button(point, false)],
                )
            }
            Activation::AccessKit => run_frame(
                &context,
                &mut dockspace,
                &mut panes,
                vec![accesskit_action(close, Action::Click)],
            ),
            Activation::Keyboard(key) => {
                let _ = run_frame(
                    &context,
                    &mut dockspace,
                    &mut panes,
                    vec![accesskit_action(close, Action::Focus)],
                );
                run_frame(&context, &mut dockspace, &mut panes, key_press(key))
            }
        };

        assert_eq!(activated.close_requests.len(), 1, "{name}");
        assert_group_unchanged(&dockspace);
        let request = &activated.close_requests[0];
        assert!(!request.reused(), "{name}");
        assert_eq!(
            request.plan().target(),
            ClosePlanTarget::Root {
                root: FLOATING_ROOT
            },
            "{name}",
        );
        assert_eq!(
            request
                .plan()
                .items()
                .iter()
                .map(|item| item.item())
                .collect::<Vec<_>>(),
            [SECOND, THIRD],
            "{name}",
        );
        let first = dockspace
            .resolve_close(
                request.plan().request(),
                request.plan().items()[0].token(),
                CloseDecision::Allow,
            )
            .expect("the first group vote is recorded without closing any item");
        assert!(!first.mutation().workspace_changed(), "{name}");
        assert!(
            matches!(
                first.outcome(),
                DockspaceCloseOutcome::Processed {
                    resolution: DockspaceCloseResolution::Recorded { item: SECOND, .. },
                    application: None,
                    ..
                }
            ),
            "{name}: {first:?}"
        );
        assert_group_unchanged(&dockspace);

        let last = dockspace
            .resolve_close(
                request.plan().request(),
                request.plan().items()[1].token(),
                CloseDecision::Allow,
            )
            .expect("the last group vote atomically closes the entire root");
        assert!(last.mutation().workspace_changed(), "{name}");
        assert!(
            matches!(
                last.outcome(),
                DockspaceCloseOutcome::Processed {
                    resolution: DockspaceCloseResolution::Approved { .. },
                    plan: Some(plan),
                    application: Some(Ok(DockspaceAppliedClose::RootClosed { root: FLOATING_ROOT, items })),
                } if plan.phase() == DockspaceClosePhase::Applied && items == &[SECOND, THIRD]
            ),
            "{name}: {last:?}"
        );
        assert!(dockspace.view().contained(FLOATING).is_none(), "{name}");
        assert!(dockspace.view().root(FLOATING_ROOT).is_none(), "{name}");
        assert!(dockspace.view().item(SECOND).is_none(), "{name}");
        assert!(dockspace.view().item(THIRD).is_none(), "{name}");
        assert_eq!(root_tabs(&dockspace), Some(vec![FIRST]), "{name}");
        let settled = run_frame(&context, &mut dockspace, &mut panes, Vec::new());
        assert!(settled.close_requests.is_empty(), "{name}");
    }
}

#[test]
fn a_protected_group_disables_close_but_keeps_dock_back_available() {
    let context = Context::default();
    context.enable_accesskit();
    let mut policy = DockPolicy::default();
    policy.set_close_capability(dockspace::policy::CloseCapability::Disabled);
    let mut dockspace = Dockspace::builder("protected-group-close", contained_layout())
        .policy(policy)
        .build()
        .unwrap();
    let mut panes = Panes;
    run_frame(&context, &mut dockspace, &mut panes, Vec::new());
    let stable = run_frame(&context, &mut dockspace, &mut panes, Vec::new());
    let (close, node) = accesskit_node(&stable.output, Role::Button, "Close floating Second");
    assert!(node.is_disabled());
    let before = dockspace.version();
    let frame = run_frame(
        &context,
        &mut dockspace,
        &mut panes,
        vec![accesskit_action(close, Action::Click)],
    );
    assert!(frame.close_requests.is_empty());
    assert_eq!(dockspace.version(), before);
    assert!(dockspace.view().contained(FLOATING).is_some());
    let result = dockspace.dock_root_back_current(FLOATING_ROOT).unwrap();
    assert!(result.mutation().workspace_changed());
    assert!(dockspace.view().contained(FLOATING).is_none());
    assert_eq!(root_tabs(&dockspace), Some(vec![FIRST, SECOND]));
}

#[test]
fn contained_group_close_veto_preserves_every_item_through_the_public_facade() {
    let layout = DockspaceLayout::new([DockspaceSurfaceLayout::new(
        SURFACE,
        DockspaceRootLayout::new(ROOT, DockspaceNode::central_tabs([FIRST])),
    )
    .with_contained(DockspaceContainedLayout::new(
        FLOATING,
        DockspaceRootLayout::new(FLOATING_ROOT, DockspaceNode::tabs([SECOND, THIRD])),
        LogicalRect::new(500.0, 260.0, 240.0, 220.0).expect("the vetoed group rectangle is valid"),
    ))])
    .expect("the vetoed group layout is valid");
    let mut dockspace = Dockspace::builder("product-contained-close-veto", layout)
        .build()
        .expect("the vetoed group facade initializes");
    let before_version = dockspace.version();
    let before_rect = contained_rect(&dockspace);
    let requested = dockspace
        .request_close_root_current(FLOATING_ROOT)
        .expect("the public facade requests the entire group");
    assert!(!requested.mutation().workspace_changed());
    let DockspaceCloseRequestStatus::Requested(request) = requested.status() else {
        panic!("the contained root must open a close plan: {requested:?}")
    };
    let plan = request.plan();
    assert_eq!(
        plan.target(),
        ClosePlanTarget::Root {
            root: FLOATING_ROOT
        }
    );
    assert_eq!(
        plan.items()
            .iter()
            .map(|item| item.item())
            .collect::<Vec<_>>(),
        [SECOND, THIRD]
    );
    let allowed = dockspace
        .resolve_close(
            plan.request(),
            plan.items()[0].token(),
            CloseDecision::Allow,
        )
        .expect("one item allows closure without removing content");
    assert!(!allowed.mutation().workspace_changed());
    assert!(matches!(
        allowed.outcome(),
        DockspaceCloseOutcome::Processed {
            resolution: DockspaceCloseResolution::Recorded { item: SECOND, .. },
            application: None,
            ..
        }
    ));
    let vetoed = dockspace
        .resolve_close(plan.request(), plan.items()[1].token(), CloseDecision::Veto)
        .expect("one item vetoes the entire group");
    assert!(!vetoed.mutation().workspace_changed());
    assert!(matches!(
        vetoed.outcome(),
        DockspaceCloseOutcome::Processed {
            resolution: DockspaceCloseResolution::Vetoed { item: THIRD, .. },
            plan: Some(current),
            application: None,
        } if current.phase() == DockspaceClosePhase::NotApplied
    ));
    for item in plan.items() {
        let replay = dockspace
            .resolve_close(plan.request(), item.token(), CloseDecision::Allow)
            .expect("a vetoed request consumes late votes inertly");
        assert!(!replay.mutation().workspace_changed());
        assert!(matches!(
            replay.outcome(),
            DockspaceCloseOutcome::Processed {
                resolution: DockspaceCloseResolution::Inert(
                    DockspaceCloseInertReason::RetiredRequest
                ),
                application: None,
                ..
            }
        ));
    }
    assert_eq!(dockspace.version(), before_version);
    assert_eq!(root_tabs(&dockspace), Some(vec![FIRST]));
    assert_eq!(contained_rect(&dockspace), before_rect);
    for item in [SECOND, THIRD] {
        let location = dockspace
            .view()
            .item(item)
            .expect("veto preserves every item");
        assert_eq!(location.root(), FLOATING_ROOT);
        assert_eq!(location.contained(), Some(FLOATING));
        assert_eq!(location.tabs().items(), &[SECOND, THIRD]);
        assert_eq!(location.tabs().selected(), Some(SECOND));
    }
}

#[test]
fn contained_group_close_deferred_allow_commits_once_through_the_public_facade() {
    let layout = DockspaceLayout::new([DockspaceSurfaceLayout::new(
        SURFACE,
        DockspaceRootLayout::new(ROOT, DockspaceNode::central_tabs([FIRST])),
    )
    .with_contained(DockspaceContainedLayout::new(
        FLOATING,
        DockspaceRootLayout::new(FLOATING_ROOT, DockspaceNode::tabs([SECOND, THIRD])),
        LogicalRect::new(500.0, 260.0, 240.0, 220.0)
            .expect("the deferred group rectangle is valid"),
    ))])
    .expect("the deferred group layout is valid");
    let mut policy = DockPolicy::default();
    policy.set_close_capability(dockspace::policy::CloseCapability::DeferredAllowed);
    let mut dockspace = Dockspace::builder("product-contained-close-deferred", layout)
        .policy(policy)
        .build()
        .expect("the deferred group facade initializes");
    let before_version = dockspace.version();
    let before_rect = contained_rect(&dockspace);
    let requested = dockspace
        .request_close_root_current(FLOATING_ROOT)
        .expect("the public facade requests the entire group");
    assert!(!requested.mutation().workspace_changed());
    let DockspaceCloseRequestStatus::Requested(request) = requested.status() else {
        panic!("the contained root must open a close plan: {requested:?}")
    };
    let plan = request.plan();
    assert_eq!(
        plan.target(),
        ClosePlanTarget::Root {
            root: FLOATING_ROOT
        }
    );
    assert_eq!(
        plan.items()
            .iter()
            .map(|item| item.item())
            .collect::<Vec<_>>(),
        [SECOND, THIRD]
    );
    let deferred = dockspace
        .resolve_close(
            plan.request(),
            plan.items()[0].token(),
            CloseDecision::Deferred,
        )
        .expect("the initial vote issues an exact deferred continuation");
    assert!(!deferred.mutation().workspace_changed());
    let DockspaceCloseOutcome::Processed {
        resolution:
            DockspaceCloseResolution::Deferred {
                request,
                item: SECOND,
                continuation,
            },
        plan: Some(current),
        application: None,
    } = deferred.outcome()
    else {
        panic!("the public facade must return the continuation: {deferred:?}")
    };
    assert_eq!(*request, plan.request());
    assert_eq!(current.phase(), DockspaceClosePhase::AwaitingDecision);
    assert_eq!(current.items()[0].deferred_token(), Some(*continuation));
    let allowed = dockspace
        .resolve_close(
            plan.request(),
            plan.items()[1].token(),
            CloseDecision::Allow,
        )
        .expect("the other item can allow while the continuation remains pending");
    assert!(!allowed.mutation().workspace_changed());
    assert!(matches!(
        allowed.outcome(),
        DockspaceCloseOutcome::Processed {
            resolution: DockspaceCloseResolution::Recorded { item: THIRD, .. },
            application: None,
            ..
        }
    ));
    for item in plan.items() {
        let replay = dockspace
            .resolve_close(plan.request(), item.token(), CloseDecision::Allow)
            .expect("consumed initial tokens cannot replace the deferred continuation");
        assert!(!replay.mutation().workspace_changed());
        assert!(matches!(
            replay.outcome(),
            DockspaceCloseOutcome::Processed {
                resolution: DockspaceCloseResolution::Inert(
                    DockspaceCloseInertReason::AlreadyResolved
                ),
                application: None,
                ..
            }
        ));
    }
    assert_eq!(dockspace.version(), before_version);
    assert_eq!(root_tabs(&dockspace), Some(vec![FIRST]));
    assert_eq!(contained_rect(&dockspace), before_rect);
    for item in [SECOND, THIRD] {
        let location = dockspace
            .view()
            .item(item)
            .expect("the deferred group remains intact");
        assert_eq!(location.root(), FLOATING_ROOT);
        assert_eq!(location.contained(), Some(FLOATING));
        assert_eq!(location.tabs().items(), &[SECOND, THIRD]);
        assert_eq!(location.tabs().selected(), Some(SECOND));
    }

    let closed = dockspace
        .resolve_deferred_close(plan.request(), *continuation, DeferredCloseDecision::Allow)
        .expect("the final continuation atomically closes the complete root");
    assert!(closed.mutation().workspace_changed());
    assert!(matches!(
        closed.outcome(),
        DockspaceCloseOutcome::Processed {
            resolution: DockspaceCloseResolution::Approved { .. },
            plan: Some(current),
            application: Some(Ok(DockspaceAppliedClose::RootClosed { root: FLOATING_ROOT, items })),
        } if current.phase() == DockspaceClosePhase::Applied && items == &[SECOND, THIRD]
    ));
    assert!(dockspace.view().contained(FLOATING).is_none());
    assert!(dockspace.view().root(FLOATING_ROOT).is_none());
    assert!(dockspace.view().item(SECOND).is_none());
    assert!(dockspace.view().item(THIRD).is_none());
    assert_eq!(root_tabs(&dockspace), Some(vec![FIRST]));
    let committed_version = dockspace.version();
    for item in plan.items() {
        let replay = dockspace
            .resolve_close(plan.request(), item.token(), CloseDecision::Allow)
            .expect("initial tokens cannot commit the root twice");
        assert!(!replay.mutation().workspace_changed());
        assert!(matches!(
            replay.outcome(),
            DockspaceCloseOutcome::Processed {
                resolution: DockspaceCloseResolution::Inert(
                    DockspaceCloseInertReason::RetiredRequest
                ),
                application: None,
                ..
            }
        ));
    }
    let replay = dockspace
        .resolve_deferred_close(plan.request(), *continuation, DeferredCloseDecision::Allow)
        .expect("the deferred token cannot commit the root twice");
    assert!(!replay.mutation().workspace_changed());
    assert!(matches!(
        replay.outcome(),
        DockspaceCloseOutcome::Processed {
            resolution: DockspaceCloseResolution::Inert(DockspaceCloseInertReason::RetiredRequest),
            application: None,
            ..
        }
    ));
    assert_eq!(dockspace.version(), committed_version);
    assert_eq!(root_tabs(&dockspace), Some(vec![FIRST]));
}

#[test]
fn closing_the_focused_group_or_last_tab_focuses_the_persistent_workspace() {
    for contained in [false, true] {
        let context = Context::default();
        context.enable_accesskit();
        let layout = if contained {
            contained_layout()
        } else {
            DockspaceLayout::new([DockspaceSurfaceLayout::new(
                SURFACE,
                DockspaceRootLayout::new(ROOT, DockspaceNode::central_tabs([SECOND])),
            )])
            .unwrap()
        };
        let mut dockspace = Dockspace::builder(("close-to-workspace-focus", contained), layout)
            .build()
            .unwrap();
        let mut panes = FocusPanes::new("close-to-workspace-focus", true);
        run_focus_frame(&context, &mut dockspace, &mut panes, Vec::new());
        let (stable, _) = run_focus_frame(&context, &mut dockspace, &mut panes, Vec::new());
        let (tab, _) = accesskit_node(&stable.output, Role::Tab, "Second");
        run_focus_frame(
            &context,
            &mut dockspace,
            &mut panes,
            vec![accesskit_action(tab, Action::Click)],
        );
        run_focus_frame(&context, &mut dockspace, &mut panes, Vec::new());
        assert!(context.memory(|memory| memory.has_focus(panes.target)));
        let request = if contained {
            dockspace.request_close_root_current(FLOATING_ROOT)
        } else {
            dockspace.request_close_item_current(SECOND)
        }
        .unwrap();
        let DockspaceCloseRequestStatus::Requested(request) = request.status() else {
            panic!("group close plan")
        };
        for item in request.plan().items() {
            dockspace
                .resolve_close(request.plan().request(), item.token(), CloseDecision::Allow)
                .unwrap();
        }
        assert!(dockspace.view().item(SECOND).is_none());
        let (closed, capability) =
            run_focus_frame(&context, &mut dockspace, &mut panes, Vec::new());
        assert_eq!(capability, DockspaceCapability::Supported);
        let (workspace, _) = accesskit_node(&closed.output, Role::Pane, "Dockspace workspace");
        assert_eq!(
            closed
                .output
                .platform_output
                .accesskit_update
                .as_ref()
                .unwrap()
                .focus,
            workspace
        );
        let version = dockspace.version();
        for _ in 0..3 {
            let (idle, _) = run_focus_frame(&context, &mut dockspace, &mut panes, Vec::new());
            assert_eq!(
                idle.output
                    .platform_output
                    .accesskit_update
                    .as_ref()
                    .unwrap()
                    .focus,
                workspace
            );
            assert_eq!(dockspace.version(), version);
        }
        assert_eq!(
            panes.target_requests(),
            1,
            "workspace focus never requests a substitute pane"
        );
    }
}

#[test]
fn contained_presentation_menu_close_group_requests_a_root_close_plan() {
    let context = Context::default();
    context.enable_accesskit();
    let layout = DockspaceLayout::new([DockspaceSurfaceLayout::new(
        SURFACE,
        DockspaceRootLayout::new(ROOT, DockspaceNode::central_tabs([FIRST])),
    )
    .with_contained(DockspaceContainedLayout::new(
        FLOATING,
        DockspaceRootLayout::new(FLOATING_ROOT, DockspaceNode::tabs([SECOND, THIRD])),
        LogicalRect::new(500.0, 260.0, 240.0, 220.0)
            .expect("the menu group-close rectangle is valid"),
    ))])
    .expect("the menu group-close layout is valid");
    let mut dockspace = Dockspace::builder("product-presentation-menu-close-group", layout)
        .build()
        .expect("the menu group-close facade initializes");
    let mut panes = Panes;

    let _ = run_frame(&context, &mut dockspace, &mut panes, Vec::new());
    let stable = run_frame(&context, &mut dockspace, &mut panes, Vec::new());
    let before_version = dockspace.version();
    let before_rect = contained_rect(&dockspace);
    let contained_bounds = Rect::from_min_max(
        Pos2::new(before_rect.min().x() as f32, before_rect.min().y() as f32),
        Pos2::new(before_rect.max().x() as f32, before_rect.max().y() as f32),
    );
    let (menu, menu_node) = stable
        .output
        .platform_output
        .accesskit_update
        .as_ref()
        .expect("AccessKit is enabled")
        .nodes
        .iter()
        .find(|(_, node)| {
            node.role() == Role::Button
                && node.label() == Some("Presentation commands")
                && contained_bounds.contains(accesskit_node_rect(node).center())
        })
        .expect("the contained group exposes its own presentation menu");
    assert!(!menu_node.is_disabled());
    let sizing = run_frame(
        &context,
        &mut dockspace,
        &mut panes,
        vec![accesskit_action(*menu, Action::Click)],
    );
    assert!(sizing.close_requests.is_empty());
    let opened = run_frame(&context, &mut dockspace, &mut panes, Vec::new());
    assert!(opened.close_requests.is_empty());
    let (close, close_node) = accesskit_node(&opened.output, Role::MenuItem, "Close Group");
    let (_, dock_back_node) = accesskit_node(&opened.output, Role::MenuItem, "Dock Back");
    assert!(!close_node.is_disabled());
    assert!(close_node.supports_action(Action::Click));
    assert!(!dock_back_node.is_disabled());

    let activated = run_frame(
        &context,
        &mut dockspace,
        &mut panes,
        vec![accesskit_action(close, Action::Click)],
    );
    assert_eq!(activated.close_requests.len(), 1);
    let request = &activated.close_requests[0];
    assert!(!request.reused());
    assert_eq!(
        request.plan().target(),
        ClosePlanTarget::Root {
            root: FLOATING_ROOT
        }
    );
    assert_eq!(
        request.plan().phase(),
        DockspaceClosePhase::AwaitingDecision
    );
    assert_eq!(
        request
            .plan()
            .items()
            .iter()
            .map(|item| item.item())
            .collect::<Vec<_>>(),
        [SECOND, THIRD]
    );
    assert_eq!(dockspace.version(), before_version);
    assert_eq!(root_tabs(&dockspace), Some(vec![FIRST]));
    assert_eq!(contained_rect(&dockspace), before_rect);
    for item in [SECOND, THIRD] {
        let location = dockspace
            .view()
            .item(item)
            .expect("Close Group waits for the application");
        assert_eq!(location.root(), FLOATING_ROOT);
        assert_eq!(location.contained(), Some(FLOATING));
        assert_eq!(location.tabs().items(), &[SECOND, THIRD]);
        assert_eq!(location.tabs().selected(), Some(SECOND));
    }
    for item in request.plan().items() {
        dockspace
            .resolve_close(request.plan().request(), item.token(), CloseDecision::Allow)
            .expect("application approval closes the group requested by its menu");
    }
    assert!(dockspace.view().contained(FLOATING).is_none());
    assert!(dockspace.view().root(FLOATING_ROOT).is_none());
    assert!(dockspace.view().item(SECOND).is_none());
    assert!(dockspace.view().item(THIRD).is_none());
    assert_eq!(root_tabs(&dockspace), Some(vec![FIRST]));
    let settled = run_frame(&context, &mut dockspace, &mut panes, Vec::new());
    assert!(settled.close_requests.is_empty());
}
