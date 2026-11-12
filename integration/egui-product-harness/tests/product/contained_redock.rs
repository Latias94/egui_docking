use super::*;

#[test]
fn first_contained_drag_projection_paints_the_same_guide_it_hits() {
    let context = Context::default();
    context.enable_accesskit();
    let mut dockspace = Dockspace::builder(
        "product-contained-first-guide",
        contained_layout_at(LogicalRect::new(300.0, 200.0, 240.0, 220.0).unwrap()),
    )
    .style(instrumented_style())
    .build()
    .unwrap();
    let mut panes = Panes;
    let _ = run_frame(&context, &mut dockspace, &mut panes, Vec::new());
    let _ = run_frame(&context, &mut dockspace, &mut panes, Vec::new());
    let source = contained_title_point(&dockspace);
    // Before source exclusion, the partial cover displaces the center guide left.
    let old_center = Pos2::new(270.0, 314.0);
    let first_drag = run_frame(
        &context,
        &mut dockspace,
        &mut panes,
        vec![
            Event::PointerMoved(source),
            pointer_button(source, true),
            Event::PointerMoved(old_center),
        ],
    );
    let style = dockspace.style();
    let passive = style.visuals.drop_guide_fill.unwrap();
    let active = style.visuals.drop_guide_active_fill.unwrap();
    if let Some(receiver) = try_node_rect(
        &first_drag.output,
        Role::Label,
        "Dockspace center drop guide",
    ) {
        assert_eq!(
            nearest_guide_center(&first_drag.output, passive, active, receiver.center()),
            receiver.center(),
            "the first source-excluded plan must not paint a guide from the idle layout",
        );
    } else {
        assert_eq!(guide_button_count(&first_drag.output, passive, active), 0);
    }
    let fresh = run_frame(
        &context,
        &mut dockspace,
        &mut panes,
        vec![Event::PointerMoved(old_center + vec2(1.0, 0.0))],
    );
    let receiver = node_rect(&fresh.output, Role::Label, "Dockspace center drop guide");
    assert_eq!(
        nearest_guide_center(&fresh.output, passive, active, receiver.center()),
        receiver.center(),
    );
    assert_eq!(
        rect_fill_count(&fresh.output, active),
        0,
        "motion over the old guide cannot retain its stale target"
    );
}

#[test]
fn contained_title_drag_keeps_the_main_center_guide_reachable() {
    let context = Context::default();
    context.enable_accesskit();
    let mut dockspace = Dockspace::builder("product-contained-stable-guide", contained_layout())
        .style(instrumented_style())
        .build()
        .unwrap();
    let mut panes = Panes;
    let _ = run_frame(&context, &mut dockspace, &mut panes, Vec::new());
    let _ = run_frame(&context, &mut dockspace, &mut panes, Vec::new());
    let source = contained_title_point(&dockspace);
    let _ = run_frame(
        &context,
        &mut dockspace,
        &mut panes,
        vec![Event::PointerMoved(source), pointer_button(source, true)],
    );
    let pointer = Pos2::new(400.0, 314.0);
    let _ = run_frame(
        &context,
        &mut dockspace,
        &mut panes,
        vec![Event::PointerMoved(source + vec2(-20.0, -10.0))],
    );
    let mut preview = run_frame(
        &context,
        &mut dockspace,
        &mut panes,
        vec![Event::PointerMoved(pointer)],
    );
    let target = node_rect(&preview.output, Role::Label, "Dockspace center drop guide");
    for _ in 0..3 {
        preview = run_frame(
            &context,
            &mut dockspace,
            &mut panes,
            vec![Event::PointerMoved(target.center())],
        );
        assert_eq!(
            node_rect(&preview.output, Role::Label, "Dockspace center drop guide",),
            target,
            "the dragged window must not push its own docking target away from the pointer",
        );
    }
    let _ = run_frame(
        &context,
        &mut dockspace,
        &mut panes,
        vec![
            Event::PointerMoved(target.center()),
            pointer_button(target.center(), false),
        ],
    );
    assert_eq!(root_tabs(&dockspace), Some(vec![FIRST, SECOND]));
    assert!(dockspace.view().contained(FLOATING).is_none());
}

#[test]
fn full_surface_contained_tab_redocks_after_held_preview_is_presented() {
    redock_full_surface_contained_tab(true);
}

#[test]
fn full_surface_contained_tab_redocks_on_direct_release() {
    redock_full_surface_contained_tab(false);
}

fn redock_full_surface_contained_tab(present_before_release: bool) {
    let context = Context::default();
    context.enable_accesskit();
    context.options_mut(|options| {
        options.max_passes = 4.try_into().expect("four is non-zero");
    });
    let mut dockspace = Dockspace::builder(
        (
            "product-full-surface-contained-redock",
            present_before_release,
        ),
        layout(),
    )
    .style(instrumented_style())
    .build()
    .expect("the full-window tab layout initializes");
    let mut panes = ProjectionPanes::default();
    let _ = run_pane_frame(&context, &mut dockspace, &mut panes, Vec::new());
    let stable = run_pane_frame(&context, &mut dockspace, &mut panes, Vec::new());
    let surface_bounds = node_rect(&stable.output, Role::Pane, "Dockspace workspace");
    assert_eq!(root_tabs(&dockspace), Some(vec![FIRST, SECOND]));

    let case = MAGNETIC_DOCKING_BASINS
        .cases()
        .find(|case| {
            case.basin() == MagneticDockBasin::Center && case.cadence() == CadenceProfile::Stepwise
        })
        .expect("the shared matrix has an ordinary center-basin sample");
    let float_point = normalized_pane_point(panes.latest(FIRST).bounds, case);
    let source = tab_center(&stable.output, "Second");
    let shift_button = |pos, pressed| Event::PointerButton {
        pos,
        button: PointerButton::Primary,
        pressed,
        modifiers: Modifiers::SHIFT,
    };
    let _ = run_pane_frame(
        &context,
        &mut dockspace,
        &mut panes,
        vec![Event::ModifiersChanged(Modifiers::SHIFT)],
    );
    let _ = run_pane_frame(
        &context,
        &mut dockspace,
        &mut panes,
        vec![Event::PointerMoved(source), shift_button(source, true)],
    );
    let _ = run_pane_frame(
        &context,
        &mut dockspace,
        &mut panes,
        vec![Event::PointerMoved(source + vec2(12.0, 8.0))],
    );
    let _ = run_pane_frame(
        &context,
        &mut dockspace,
        &mut panes,
        vec![Event::PointerMoved(float_point)],
    );
    let _ = run_pane_frame(
        &context,
        &mut dockspace,
        &mut panes,
        vec![shift_button(float_point, false)],
    );
    settle_until(8, || {
        let frame = run_pane_frame(&context, &mut dockspace, &mut panes, Vec::new());
        let missing = if dockspace
            .view()
            .item(SECOND)
            .is_some_and(|item| item.contained().is_some())
        {
            Vec::new()
        } else {
            vec!["Shift-dragged Second committed as contained"]
        };
        (frame, missing)
    })
    .unwrap_or_else(|timeout| panic!("the Shift tear-off must complete first: {timeout}"));

    let second = dockspace.view().item(SECOND).expect("Second remains open");
    let floating = second
        .contained()
        .expect("the real Shift drag created a contained presentation");
    let contained = dockspace
        .view()
        .contained(floating)
        .expect("Second's contained presentation exists");
    let expected_bounds = LogicalRect::new(
        f64::from(surface_bounds.min.x),
        f64::from(surface_bounds.min.y),
        f64::from(surface_bounds.width()),
        f64::from(surface_bounds.height()),
    )
    .expect("the measured surface has positive bounds");
    assert_eq!(
        contained.rect(),
        expected_bounds,
        "the actual Shift-created contained bounds must cover the entire surface before redocking",
    );
    assert_eq!(second.surface(), SURFACE);
    assert_ne!(second.root(), ROOT);
    assert_eq!(second.tabs().items(), &[SECOND]);
    assert_eq!(root_tabs(&dockspace), Some(vec![FIRST]));

    let unmodified = run_pane_frame(
        &context,
        &mut dockspace,
        &mut panes,
        vec![Event::ModifiersChanged(Modifiers::NONE)],
    );
    let second_tabs = unmodified
        .output
        .platform_output
        .accesskit_update
        .as_ref()
        .expect("AccessKit is enabled")
        .nodes
        .iter()
        .filter(|(_, node)| node.role() == Role::Tab && node.label() == Some("Second"))
        .count();
    assert_eq!(second_tabs, 1, "the contained source has one Second tab");
    let source = tab_center(&unmodified.output, "Second");
    let target_rect = panes.latest(FIRST).bounds;
    let target = normalized_pane_point(target_rect, case);
    assert!(surface_bounds.contains(source));
    assert!(surface_bounds.contains(target));
    assert!(target_rect.contains(target));
    let style = dockspace.style().clone();
    let passive = style.visuals.drop_guide_fill.unwrap();
    let active = style.visuals.drop_guide_active_fill.unwrap();
    let drop_fill = style.visuals.drop_fill.unwrap();
    let source_has_central = dockspace
        .view()
        .contained(floating)
        .unwrap()
        .root()
        .unwrap()
        .central()
        .is_some();
    let mut held_diagnostics = Vec::new();

    let _ = run_pane_frame(
        &context,
        &mut dockspace,
        &mut panes,
        vec![Event::PointerMoved(source), pointer_button(source, true)],
    );
    let _ = run_pane_frame(
        &context,
        &mut dockspace,
        &mut panes,
        vec![Event::PointerMoved(source + vec2(12.0, 8.0))],
    );

    if present_before_release {
        let mut moved = Some(run_pane_frame(
            &context,
            &mut dockspace,
            &mut panes,
            vec![Event::PointerMoved(target)],
        ));
        let held = settle_until(8, || {
            let frame = moved.take().unwrap_or_else(|| {
                run_pane_frame(&context, &mut dockspace, &mut panes, Vec::new())
            });
            // Contained placement shares the preview fill; it cannot activate a Dock guide.
            let mut missing = preview_readiness(
                &frame,
                target_rect,
                target,
                MagneticDockBasin::Center,
                drop_fill,
            );
            if rect_fill_count(&frame.output, active) == 0 {
                missing.push("active magnetic Dock guide, not a Contained preview");
            }
            held_diagnostics.push(format!(
                "pass={} preview={:?} guides={} source_bounds={:?} target_bounds={:?} interactive_at_target={:?}",
                context.cumulative_pass_nr(),
                magnetic_preview_rect(&frame.output, drop_fill, target_rect),
                guide_button_count(&frame.output, passive, active),
                panes.latest(SECOND).bounds,
                panes.latest(FIRST).bounds,
                context.interactive_rects_last_pass().into_iter().filter(|rect| rect.contains(target)).collect::<Vec<_>>(),
            ));
            (frame, missing)
        })
        .unwrap_or_else(|timeout| {
            panic!("the held sole tab must preview Dock inside its frozen window: {timeout}; source={source:?}, target={target:?}, source_has_central={source_has_central}, held={held_diagnostics:?}")
        });
        assert!(
            rects_with_fill(&held.output, &[passive, active])
                .iter()
                .all(|guide| !guide.contains(target)),
            "the main-pane point must use magnetic docking without aiming at a guide",
        );
        assert_eq!(root_tabs(&dockspace), Some(vec![FIRST]));
        assert_eq!(
            dockspace.view().item(SECOND).unwrap().contained(),
            Some(floating)
        );
    }

    let (output, release_response) = dockspace
        .run_single_surface(
            &context,
            input(vec![
                Event::PointerMoved(target),
                pointer_button(target, false),
            ]),
            SURFACE,
            &mut panes,
        )
        .expect("the direct release remains one real owned run");
    let (mut output, receipt) = output.into_parts();
    receipt.report(SurfacePresentationResult::Presented);
    output.textures_delta.clear();
    let released = FrameOutput {
        output,
        close_requests: release_response.close_request_events().to_vec(),
    };
    assert!(
        rects_with_fill(&released.output, &[passive, active])
            .iter()
            .all(|guide| !guide.contains(target)),
        "direct release must also use the ordinary main-pane point",
    );
    settle_until(8, || {
        let frame = run_pane_frame(&context, &mut dockspace, &mut panes, Vec::new());
        let mut missing = Vec::new();
        if root_tabs(&dockspace) != Some(vec![FIRST, SECOND]) {
            missing.push("exact main-root tabs [First, Second]");
        }
        if dockspace.view().contained(floating).is_some() {
            missing.push("the old contained presentation retired");
        }
        (frame, missing)
    })
    .unwrap_or_else(|timeout| {
        panic!(
            "sole-tab magnetic redock failed (present_before_release={present_before_release}): {timeout}; source={source:?}, target={target:?}, source_has_central={source_has_central}, release={:?}, feedback={:?}, source_item={:?}, main_tabs={:?}",
            release_response.mutation(),
            release_response.manipulation_feedback(),
            dockspace.view().item(SECOND),
            root_tabs(&dockspace),
        )
    });
    let second = dockspace
        .view()
        .item(SECOND)
        .expect("redocking keeps Second open");
    assert_eq!(second.surface(), SURFACE);
    assert_eq!(second.root(), ROOT);
    assert_eq!(second.contained(), None);
    assert!(second.is_selected());
    assert_eq!(surface_ids(&dockspace), [SURFACE]);
    assert_eq!(
        dockspace.view().surface(SURFACE).unwrap().contained_count(),
        0
    );
}
