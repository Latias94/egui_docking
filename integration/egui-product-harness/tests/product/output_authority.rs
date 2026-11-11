use super::*;

#[test]
fn raw_pointer_close_request_is_not_replayed_after_an_output_hook_discard() {
    let context = Context::default();
    context.enable_accesskit();
    context.options_mut(|options| {
        options.max_passes = 4.try_into().expect("four is non-zero");
    });
    let mut dockspace = Dockspace::builder("product-raw-pointer-rerun", layout())
        .build()
        .expect("the raw-pointer rerun facade initializes");
    let mut panes = Panes;

    let _ = run_frame(&context, &mut dockspace, &mut panes, Vec::new());
    let stable = run_frame(&context, &mut dockspace, &mut panes, Vec::new());
    let close = node_center(&stable.output, Role::Button, "Close First");
    let before = root_tabs(&dockspace);
    context
        .plugin_or_default::<LateDiscardPlugin>()
        .lock()
        .remaining = 1;

    let mut requests_per_pass = Vec::new();
    let mut requests = Vec::new();
    let output = run_headless(
        &context,
        input(vec![
            Event::PointerMoved(close),
            pointer_button(close, true),
            pointer_button(close, false),
        ]),
        |ui| {
            let response = dockspace
                .show_single_surface(SURFACE, ui, &mut panes)
                .expect("every rerun preserves the committed raw input");
            requests_per_pass.push(response.close_request_events().len());
            requests.extend(response.close_request_events().iter().cloned());
        },
    );
    output.drop_without_applying_deltas();

    assert!(
        requests_per_pass.len() > 1,
        "the late output hook must force an actual UI rerun"
    );
    assert_eq!(
        requests_per_pass[0], 1,
        "the original press/release batch opens its close request before paint"
    );
    assert!(
        requests_per_pass[1..].iter().all(|count| *count == 0),
        "reruns must not replay the raw click or emit a reused close request"
    );
    assert_eq!(requests.len(), 1);
    assert!(!requests[0].reused());
    assert_eq!(root_tabs(&dockspace), before);

    let idle = run_frame(&context, &mut dockspace, &mut panes, Vec::new());
    assert!(
        idle.close_requests.is_empty(),
        "the next run must not retain an already consumed raw click"
    );
    assert_eq!(root_tabs(&dockspace), before);
}

#[test]
fn late_presentation_cannot_borrow_receivers_from_a_newer_unpresented_egui_pass() {
    let context = Context::default();
    context.enable_accesskit();
    let mut dockspace = Dockspace::builder("product-exact-receiver-pass", layout())
        .build()
        .expect("the late presentation fixture initializes");
    let mut panes = Panes;
    let _ = run_frame(&context, &mut dockspace, &mut panes, Vec::new());
    let stable = run_frame(&context, &mut dockspace, &mut panes, Vec::new());
    let close = node_center(&stable.output, Role::Button, "Close First");
    let (_, earlier) = run_ui(&context, input(Vec::new()), |ui| {
        dockspace
            .show_single_surface(SURFACE, ui, &mut panes)
            .expect("the earlier output compiles");
    })
    .into_parts();
    let (_, later) = run_ui(&context, input(Vec::new()), |ui| {
        dockspace
            .show_single_surface(SURFACE, ui, &mut panes)
            .expect("the later output compiles without presentation");
    })
    .into_parts();
    earlier.report(SurfacePresentationResult::Presented);
    let stale = run_frame(
        &context,
        &mut dockspace,
        &mut panes,
        vec![
            Event::PointerMoved(close),
            pointer_button(close, true),
            pointer_button(close, false),
        ],
    );
    assert!(
        stale.close_requests.is_empty(),
        "identical geometry in a newer unpresented pass cannot authorize the older output"
    );
    later.report(SurfacePresentationResult::Dropped);
    let _ = run_frame(&context, &mut dockspace, &mut panes, Vec::new());
    let _ = run_frame(&context, &mut dockspace, &mut panes, Vec::new());
    let fresh = run_frame(
        &context,
        &mut dockspace,
        &mut panes,
        vec![
            Event::PointerMoved(close),
            pointer_button(close, true),
            pointer_button(close, false),
        ],
    );
    assert_eq!(
        fresh.close_requests.len(),
        1,
        "a fresh qualified presentation restores ordinary controls"
    );
}

#[test]
fn rebinding_context_retires_the_old_pointer_owner_without_a_pending_output_report() {
    let context = Context::default();
    context.enable_accesskit();
    let mut dockspace = Dockspace::builder("product-pointer-context-rebind", layout())
        .style(instrumented_style())
        .build()
        .expect("the context rebind fixture initializes");
    let mut panes = Panes;
    let _ = run_frame(&context, &mut dockspace, &mut panes, Vec::new());
    let stable = run_frame(&context, &mut dockspace, &mut panes, Vec::new());
    let source = tab_center(&stable.output, "Second");
    let target = tab_center(&stable.output, "First");
    let _ = run_frame(
        &context,
        &mut dockspace,
        &mut panes,
        vec![Event::PointerMoved(source), pointer_button(source, true)],
    );
    let dragging = run_frame(
        &context,
        &mut dockspace,
        &mut panes,
        vec![Event::PointerMoved(target)],
    );
    let ghost = dockspace
        .style()
        .visuals
        .ghost_fill
        .expect("the ghost fill is fixed");
    assert!(rect_fill_count(&dragging.output, ghost) > 0);
    let (_, old_receipt) = run_ui(&context, input(Vec::new()), |ui| {
        dockspace
            .show_single_surface(SURFACE, ui, &mut panes)
            .expect("the last old-context output drains the prior report");
    })
    .into_parts();
    let before = dockspace.version();
    let replacement = Context::default();
    replacement.enable_accesskit();
    let rebound = run_frame(&replacement, &mut dockspace, &mut panes, Vec::new());
    assert_eq!(dockspace.version(), before);
    assert_eq!(root_tabs(&dockspace), Some(vec![FIRST, SECOND]));
    assert_eq!(rect_fill_count(&rebound.output, ghost), 0);
    assert_ne!(
        rebound.output.platform_output.cursor_icon,
        egui::CursorIcon::Grabbing
    );
    old_receipt.report(SurfacePresentationResult::Presented);
    let stable = run_frame(&replacement, &mut dockspace, &mut panes, Vec::new());
    let source = tab_center(&stable.output, "Second");
    let target = tab_center(&stable.output, "First");
    let _ = run_frame(
        &replacement,
        &mut dockspace,
        &mut panes,
        vec![
            Event::PointerMoved(source),
            pointer_button(source, true),
            Event::PointerMoved(target),
            pointer_button(target, false),
        ],
    );
    let _ = run_frame(&replacement, &mut dockspace, &mut panes, Vec::new());
    assert_eq!(root_tabs(&dockspace), Some(vec![SECOND, FIRST]));
}

#[test]
fn transformed_output_cannot_gain_pointer_authority_when_the_transform_is_removed() {
    let context = Context::default();
    context.enable_accesskit();
    let mut dockspace = Dockspace::builder("product-pointer-emitted-transform", layout())
        .build()
        .expect("the transformed output fixture initializes");
    let mut panes = Panes;
    let _ = run_frame(&context, &mut dockspace, &mut panes, Vec::new());
    let stable = run_frame(&context, &mut dockspace, &mut panes, Vec::new());
    let close = node_center(&stable.output, Role::Button, "Close First");
    let _ = run_headless(&context, input(Vec::new()), |ui| {
        ui.ctx().set_transform_layer(
            ui.layer_id(),
            egui::emath::TSTransform::from_translation(vec2(150.0, 80.0)),
        );
        dockspace
            .show_single_surface(SURFACE, ui, &mut panes)
            .expect("complete transformed output can still be presented");
    });
    let mut requests = Vec::new();
    let _ = run_headless(
        &context,
        input(vec![
            Event::PointerMoved(close),
            pointer_button(close, true),
            pointer_button(close, false),
        ]),
        |ui| {
            ui.ctx()
                .set_transform_layer(ui.layer_id(), egui::emath::TSTransform::IDENTITY);
            let response = dockspace
                .show_single_surface(SURFACE, ui, &mut panes)
                .expect("unsupported source coordinates remain capability-negative");
            requests.extend(response.close_request_events().iter().cloned());
        },
    );
    assert!(
        requests.is_empty(),
        "current identity transform cannot reinterpret a transformed presented output"
    );
    let _ = run_frame(&context, &mut dockspace, &mut panes, Vec::new());
    let fresh = run_frame(
        &context,
        &mut dockspace,
        &mut panes,
        vec![
            Event::PointerMoved(close),
            pointer_button(close, true),
            pointer_button(close, false),
        ],
    );
    assert_eq!(fresh.close_requests.len(), 1);
}

#[test]
fn changing_layer_coordinates_cancels_resize_before_interpreting_a_release() {
    let context = Context::default();
    context.enable_accesskit();
    let mut dockspace = Dockspace::builder(
        "product-pointer-transform-during-resize",
        contained_layout(),
    )
    .style(instrumented_style())
    .build()
    .expect("the resize coordinate fixture initializes");
    let mut panes = Panes;
    let _ = run_frame(&context, &mut dockspace, &mut panes, Vec::new());
    let _ = run_frame(&context, &mut dockspace, &mut panes, Vec::new());
    let initial = dockspace
        .view()
        .contained(FLOATING)
        .expect("the inspector exists")
        .rect();
    let source = Pos2::new(
        initial.max().x() as f32 - 1.0,
        initial.min().y() as f32 + 100.0,
    );
    let moved = source + vec2(30.0, 0.0);
    let _ = run_frame(
        &context,
        &mut dockspace,
        &mut panes,
        vec![Event::PointerMoved(source), pointer_button(source, true)],
    );
    let _ = run_frame(
        &context,
        &mut dockspace,
        &mut panes,
        vec![Event::PointerMoved(moved)],
    );
    let before = dockspace.version();
    let cancelled = run_headless(
        &context,
        input(vec![
            Event::PointerMoved(moved),
            pointer_button(moved, false),
        ]),
        |ui| {
            ui.ctx().set_transform_layer(
                ui.layer_id(),
                egui::emath::TSTransform::from_translation(vec2(150.0, 80.0)),
            );
            dockspace
                .show_single_surface(SURFACE, ui, &mut panes)
                .expect("coordinate invalidation cancels the prior owner");
        },
    );
    assert_eq!(dockspace.version(), before);
    assert_eq!(
        dockspace
            .view()
            .contained(FLOATING)
            .expect("the inspector survives")
            .rect(),
        initial
    );
    let ghost = dockspace
        .style()
        .visuals
        .ghost_fill
        .expect("the ghost fill is fixed");
    assert_eq!(rect_fill_count(&cancelled, ghost), 0);
}

#[test]
fn default_features_tab_drag_reorders_across_multipass_discard() {
    let context = Context::default();
    context.enable_accesskit();
    context.options_mut(|options| {
        options.max_passes = 4.try_into().expect("four is non-zero");
    });
    let mut dockspace = Dockspace::builder("product-tab-reorder", layout())
        .build()
        .expect("the product facade initializes");
    let mut panes = Panes;

    let _ = run_frame(&context, &mut dockspace, &mut panes, Vec::new());
    let stable = run_frame(&context, &mut dockspace, &mut panes, Vec::new());
    let first_rect = node_rect(&stable.output, Role::Tab, "First");
    let first = Pos2::new(
        first_rect.min.x + first_rect.width() * 0.25,
        first_rect.center().y,
    );
    let second = tab_center(&stable.output, "Second");

    let _ = run_frame(
        &context,
        &mut dockspace,
        &mut panes,
        vec![Event::PointerMoved(second), pointer_button(second, true)],
    );
    let (_, passes) = run_frame_with_discard_after_dockspace(
        &context,
        &mut dockspace,
        &mut panes,
        vec![Event::PointerMoved(first)],
    );
    assert!(
        passes > 1,
        "the fixture must execute an actual egui multipass"
    );
    let released = run_frame(
        &context,
        &mut dockspace,
        &mut panes,
        vec![Event::PointerMoved(first), pointer_button(first, false)],
    );
    let _ = accesskit_node(&released.output, Role::Tab, "First");
    let _ = accesskit_node(&released.output, Role::Tab, "Second");
    assert_eq!(
        tab_items(&dockspace),
        vec![SECOND, FIRST],
        "release commits the preview painted by the preceding terminal pass",
    );
    let settled = run_frame(&context, &mut dockspace, &mut panes, Vec::new());
    let _ = accesskit_node(&settled.output, Role::Tab, "First");
    let _ = accesskit_node(&settled.output, Role::Tab, "Second");
    assert_eq!(
        tab_items(&dockspace),
        vec![SECOND, FIRST],
        "the follow-up preserves the already committed exact preview result",
    );
}

#[test]
fn default_features_ordinary_release_commits_before_its_output_is_presented() {
    let context = Context::default();
    context.enable_accesskit();
    let mut dockspace = Dockspace::builder("product-release-resample", split_layout())
        .build()
        .expect("the product release facade initializes");
    let mut panes = Panes;

    let _ = run_frame(&context, &mut dockspace, &mut panes, Vec::new());
    let stable = run_frame(&context, &mut dockspace, &mut panes, Vec::new());
    let source = tab_center(&stable.output, "Second");
    let painted_top = Pos2::new(400.0, 72.0);
    let release_bottom = Pos2::new(400.0, 528.0);

    let _ = run_frame(
        &context,
        &mut dockspace,
        &mut panes,
        vec![Event::PointerMoved(source), pointer_button(source, true)],
    );
    for _ in 0..3 {
        let _ = run_frame(
            &context,
            &mut dockspace,
            &mut panes,
            vec![Event::PointerMoved(painted_top)],
        );
    }
    let (release_output, response) = dockspace
        .run_single_surface(
            &context,
            input(vec![
                Event::PointerMoved(release_bottom),
                pointer_button(release_bottom, false),
            ]),
            SURFACE,
            &mut panes,
        )
        .expect("the ordinary release publishes before renderer submission");
    assert!(response.mutation().workspace_changed());
    let [terminal] = response.manipulation_feedback() else {
        panic!(
            "one terminal delivery: {:?}",
            response.manipulation_feedback()
        );
    };
    assert_eq!(
        terminal.status(),
        egui_dockspace::DockspaceManipulationStatus::Delivered(
            egui_dockspace::DockspaceManipulationDelivery::Dock
        )
    );
    let (release_output, release_receipt) = release_output.into_parts();
    release_output.drop_without_applying_deltas();
    assert_eq!(
        root_split(&dockspace),
        Some((DockspaceAxis::Vertical, vec![vec![FIRST], vec![SECOND]])),
        "the release uses presented target geometry without waiting for a terminal preview",
    );
    let version_before_visual_update = dockspace.version();
    let mut visual_only_style = dockspace.style().clone();
    visual_only_style.visuals.tab_active_fill = Some(egui::Color32::from_rgb(41, 83, 109));
    let visual_mutation = dockspace
        .set_style(visual_only_style)
        .expect("visual-only styling preserves the delivered layout");
    assert_eq!(dockspace.version(), version_before_visual_update);
    assert!(visual_mutation.published_state_changed());
    assert_eq!(visual_mutation.affected_surfaces(), &[SURFACE]);
    assert_eq!(
        root_split(&dockspace),
        Some((DockspaceAxis::Vertical, vec![vec![FIRST], vec![SECOND]])),
        "adapter-only styling preserves the exact delivery",
    );

    release_receipt.report(SurfacePresentationResult::Presented);
    let (_, changed, _) =
        run_pane_frame_with_mutation(&context, &mut dockspace, &mut panes, Vec::new());
    assert!(
        !changed,
        "late output acknowledgement cannot repeat the release mutation"
    );
    assert_eq!(dockspace.version(), version_before_visual_update);
    assert_eq!(
        root_split(&dockspace),
        Some((DockspaceAxis::Vertical, vec![vec![FIRST], vec![SECOND]],)),
        "the late output acknowledgement preserves the delivered layout",
    );
}

#[test]
fn default_features_discarded_preview_cannot_release_a_drag() {
    let context = Context::default();
    context.enable_accesskit();
    context.options_mut(|options| {
        options.max_passes = 4.try_into().expect("four is non-zero");
    });
    let mut dockspace = Dockspace::builder("product-discarded-preview", split_layout())
        .style(instrumented_style())
        .build()
        .expect("the product discarded-preview facade initializes");
    let mut panes = Panes;

    let _ = run_frame(&context, &mut dockspace, &mut panes, Vec::new());
    let stable = run_frame(&context, &mut dockspace, &mut panes, Vec::new());
    let source = tab_center(&stable.output, "Second");
    let top = Pos2::new(400.0, 72.0);
    let bottom = Pos2::new(400.0, 528.0);

    let _ = run_frame(
        &context,
        &mut dockspace,
        &mut panes,
        vec![Event::PointerMoved(source), pointer_button(source, true)],
    );
    let _ = run_frame(
        &context,
        &mut dockspace,
        &mut panes,
        vec![Event::PointerMoved(top)],
    );
    let _ = run_frame(&context, &mut dockspace, &mut panes, Vec::new());

    context
        .plugin_or_default::<LateDiscardPlugin>()
        .lock()
        .remaining = 2;

    let passes = run_two_discarded_dockspace_passes(
        &context,
        &mut dockspace,
        &mut panes,
        vec![Event::PointerMoved(bottom)],
    );
    assert_eq!(
        passes, 3,
        "the fixture must omit dockspace from the final pass"
    );

    let _ = run_frame(
        &context,
        &mut dockspace,
        &mut panes,
        vec![Event::PointerMoved(bottom), pointer_button(bottom, false)],
    );
    assert_eq!(
        root_split(&dockspace).map(|(axis, _)| axis),
        Some(DockspaceAxis::Horizontal),
        "a preview painted only by discarded passes cannot release the drag",
    );

    let cancelled_version = dockspace.version();
    let _ = run_frame(&context, &mut dockspace, &mut panes, Vec::new());
    let cancelled = run_frame(&context, &mut dockspace, &mut panes, Vec::new());
    assert_eq!(
        root_split(&dockspace),
        Some((DockspaceAxis::Horizontal, vec![vec![FIRST], vec![SECOND]],)),
        "a later presentation cannot authorize a release whose exact target authority was lost",
    );
    assert_eq!(dockspace.version(), cancelled_version);
    let ghost_fill = instrumented_style().visuals.ghost_fill.unwrap();
    let drop_fill = instrumented_style().visuals.drop_fill.unwrap();
    assert_eq!(rect_fill_count(&cancelled.output, ghost_fill), 0);
    assert_eq!(rect_fill_count(&cancelled.output, drop_fill), 0);
    assert_ne!(
        cancelled.output.platform_output.cursor_icon,
        egui::CursorIcon::Grabbing,
        "idle must not retain a pending drag after the unknown-authority release"
    );
    assert!(context.dragged_id().is_none());

    let source = tab_center(&cancelled.output, "Second");
    let _ = run_frame(
        &context,
        &mut dockspace,
        &mut panes,
        vec![Event::PointerMoved(source), pointer_button(source, true)],
    );
    let fresh_drag = run_frame(
        &context,
        &mut dockspace,
        &mut panes,
        vec![Event::PointerMoved(bottom)],
    );
    assert!(rect_fill_count(&fresh_drag.output, drop_fill) > 0);
    let _ = run_frame(&context, &mut dockspace, &mut panes, Vec::new());
    let _ = run_frame(
        &context,
        &mut dockspace,
        &mut panes,
        vec![pointer_button(bottom, false)],
    );
    let _ = run_frame(&context, &mut dockspace, &mut panes, Vec::new());
    assert_eq!(
        root_split(&dockspace),
        Some((DockspaceAxis::Vertical, vec![vec![FIRST], vec![SECOND]],)),
        "a fresh press after presentation recovery can complete a new exact-preview gesture",
    );
}

#[test]
fn discarded_contained_move_preview_cannot_commit_release() {
    let context = Context::default();
    context.enable_accesskit();
    context.options_mut(|options| {
        options.max_passes = 4.try_into().expect("four is non-zero");
    });
    let mut dockspace = Dockspace::builder("product-discarded-contained-move", contained_layout())
        .style(instrumented_style())
        .build()
        .expect("the discarded contained-move facade initializes");
    let mut panes = Panes;

    let _ = run_frame(&context, &mut dockspace, &mut panes, Vec::new());
    let _ = run_frame(&context, &mut dockspace, &mut panes, Vec::new());
    let initial = contained_rect(&dockspace);
    let source = contained_title_point(&dockspace);
    let destination = source + vec2(-120.0, 80.0);

    let _ = run_frame(
        &context,
        &mut dockspace,
        &mut panes,
        vec![Event::PointerMoved(source), pointer_button(source, true)],
    );

    context
        .plugin_or_default::<LateDiscardPlugin>()
        .lock()
        .remaining = 2;
    let passes = run_two_discarded_dockspace_passes(
        &context,
        &mut dockspace,
        &mut panes,
        vec![Event::PointerMoved(destination)],
    );
    assert_eq!(
        passes, 3,
        "the fixture must omit dockspace from the terminal pass"
    );
    assert_eq!(
        contained_rect(&dockspace),
        initial,
        "a contained transform observed only in discarded passes remains transient"
    );

    let _ = run_frame(
        &context,
        &mut dockspace,
        &mut panes,
        vec![
            Event::PointerMoved(destination),
            pointer_button(destination, false),
        ],
    );
    assert_eq!(
        contained_rect(&dockspace),
        initial,
        "a discarded contained move preview cannot release or mutate durable geometry"
    );

    let cancelled_version = dockspace.version();
    let _ = run_frame(&context, &mut dockspace, &mut panes, Vec::new());
    let cancelled = run_frame(&context, &mut dockspace, &mut panes, Vec::new());
    assert_eq!(
        contained_rect(&dockspace),
        initial,
        "a later presentation must not revive a contained release with unknown target authority"
    );
    assert_eq!(dockspace.version(), cancelled_version);
    let ghost_fill = instrumented_style().visuals.ghost_fill.unwrap();
    let drop_fill = instrumented_style().visuals.drop_fill.unwrap();
    assert_eq!(rect_fill_count(&cancelled.output, ghost_fill), 0);
    assert_eq!(rect_fill_count(&cancelled.output, drop_fill), 0);
    assert_ne!(
        cancelled.output.platform_output.cursor_icon,
        egui::CursorIcon::Grabbing,
        "idle must not retain a pending contained transform after cancellation"
    );
    assert!(context.dragged_id().is_none());

    let source = contained_title_point(&dockspace);
    let destination = source + vec2(-120.0, 80.0);
    let _ = run_frame(
        &context,
        &mut dockspace,
        &mut panes,
        vec![Event::PointerMoved(source), pointer_button(source, true)],
    );
    let fresh_drag = run_frame(
        &context,
        &mut dockspace,
        &mut panes,
        vec![Event::PointerMoved(destination)],
    );
    assert_eq!(rect_fill_count(&fresh_drag.output, ghost_fill), 0);
    assert_eq!(
        fresh_drag.output.platform_output.cursor_icon,
        egui::CursorIcon::Grabbing
    );
    assert_eq!(
        node_rect(&fresh_drag.output, Role::TitleBar, "Second"),
        node_rect(&cancelled.output, Role::TitleBar, "Second").translate(vec2(-120.0, 80.0))
    );
    assert_eq!(contained_rect(&dockspace), initial);
    let _ = run_frame(&context, &mut dockspace, &mut panes, Vec::new());
    let _ = run_frame(
        &context,
        &mut dockspace,
        &mut panes,
        vec![pointer_button(destination, false)],
    );
    let _ = run_frame(&context, &mut dockspace, &mut panes, Vec::new());
    let committed = contained_rect(&dockspace);
    assert_eq!(committed.width(), initial.width());
    assert_eq!(committed.height(), initial.height());
    assert_eq!(
        committed.min().x(),
        initial.min().x() - 120.0,
        "a fresh gesture after presentation recovery commits the exact contained move on x"
    );
    assert_eq!(
        committed.min().y(),
        initial.min().y() + 80.0,
        "a fresh gesture after presentation recovery commits the exact contained move on y"
    );
}

#[test]
fn default_features_unfinished_egui_run_cannot_settle_preview() {
    let context = Context::default();
    context.enable_accesskit();
    let mut dockspace = Dockspace::builder("product-interrupted-preview", split_layout())
        .style(instrumented_style())
        .build()
        .expect("the product interrupted-preview facade initializes");
    let mut panes = Panes;

    let _ = run_frame(&context, &mut dockspace, &mut panes, Vec::new());
    let stable = run_frame(&context, &mut dockspace, &mut panes, Vec::new());
    let source = tab_center(&stable.output, "Second");
    let top = Pos2::new(400.0, 72.0);
    let bottom = Pos2::new(400.0, 528.0);

    let _ = run_frame(
        &context,
        &mut dockspace,
        &mut panes,
        vec![Event::PointerMoved(source), pointer_button(source, true)],
    );
    let _ = run_frame(
        &context,
        &mut dockspace,
        &mut panes,
        vec![Event::PointerMoved(top)],
    );
    let _ = run_frame(&context, &mut dockspace, &mut panes, Vec::new());

    context
        .plugin_or_default::<LateDiscardPlugin>()
        .lock()
        .panic_once = true;
    let interrupted = catch_unwind(AssertUnwindSafe(|| {
        let input = input(vec![Event::PointerMoved(bottom)]);
        let output = run_ui(&context, input, |ui| {
            dockspace
                .show_single_surface(SURFACE, ui, &mut panes)
                .expect("the interrupted product pass advances");
        });
        // Keep the direct owned run: the late hook must unwind before an output can be reported.
        let (mut output, _receipt) = output.into_parts();
        output.textures_delta.clear();
    }));
    assert!(
        interrupted.is_err(),
        "the late output hook must interrupt the run"
    );

    let recovered = run_frame(
        &context,
        &mut dockspace,
        &mut panes,
        vec![Event::PointerMoved(bottom), pointer_button(bottom, false)],
    );
    assert_eq!(
        rect_fill_count(
            &recovered.output,
            instrumented_style().visuals.ghost_fill.unwrap()
        ),
        0,
        "abandonment suppresses the stale pointer ghost in the recovery frame",
    );
    assert_eq!(
        rect_fill_count(
            &recovered.output,
            instrumented_style().visuals.drop_fill.unwrap()
        ),
        0,
        "abandonment suppresses the stale dock preview in the recovery frame",
    );
    assert_eq!(
        root_split(&dockspace).map(|(axis, _)| axis),
        Some(DockspaceAxis::Horizontal),
        "a preview from an unfinished egui run cannot release the drag",
    );
}

#[test]
fn owned_to_unowned_downgrade_cancels_without_a_stale_ghost_frame() {
    let context = Context::default();
    context.enable_accesskit();
    let mut style = instrumented_style();
    let active_fill = egui::Color32::from_rgb(47, 103, 157);
    style.visuals.tab_active_fill = Some(active_fill);
    let ghost_fill = style
        .visuals
        .ghost_fill
        .expect("the fixture sets ghost fill");
    let drop_fill = style.visuals.drop_fill.expect("the fixture sets drop fill");
    let passive_guide = style
        .visuals
        .drop_guide_fill
        .expect("the fixture sets passive guide fill");
    let active_guide = style
        .visuals
        .drop_guide_active_fill
        .expect("the fixture sets active guide fill");
    let mut dockspace = Dockspace::builder("product-owned-unowned-downgrade", layout())
        .style(style)
        .build()
        .expect("the downgrade fixture initializes");
    let mut panes = Panes;
    let before = root_tabs(&dockspace).expect("the fixture remains a tab group");

    let _ = run_frame(&context, &mut dockspace, &mut panes, Vec::new());
    let stable = run_frame(&context, &mut dockspace, &mut panes, Vec::new());
    let source_rect = node_rect(&stable.output, Role::Tab, "Second");
    let source = source_rect.center();
    let target = tab_center(&stable.output, "First");
    let _ = run_frame(
        &context,
        &mut dockspace,
        &mut panes,
        vec![Event::PointerMoved(source), pointer_button(source, true)],
    );
    let dragging = run_frame(
        &context,
        &mut dockspace,
        &mut panes,
        vec![Event::PointerMoved(target)],
    );
    assert!(
        rect_fill_count(&dragging.output, ghost_fill) > 0
            || rect_fill_count(&dragging.output, drop_fill) > 0,
        "the owned path must first establish a visible drag",
    );
    assert_eq!(
        rect_fill_count_in(&dragging.output, active_fill, source_rect),
        0,
        "the owned drag establishes a source gap before authority is downgraded",
    );

    context.options_mut(|options| {
        options.max_passes = 1.try_into().expect("one is non-zero");
    });

    let mut capability = None;
    let mut cancellation = Vec::new();
    let mut downgraded = context.run_ui(input(Vec::new()), |ui| {
        let response = dockspace
            .show_single_surface(SURFACE, ui, &mut panes)
            .expect("the capability downgrade cancels the active gesture");
        capability = Some(response.presentation_settlement_capability());
        cancellation.extend(
            response
                .manipulation_feedback()
                .iter()
                .map(|feedback| feedback.status()),
        );
    });
    downgraded.textures_delta.clear();
    assert_eq!(
        capability,
        Some(DockspaceCapability::Unavailable(
            DockspaceUnavailableReason::PresentationSettlementRequired,
        )),
    );
    assert_eq!(
        downgraded.platform_output.num_completed_passes, 1,
        "the capability downgrade settles in its first rendering pass",
    );
    assert_eq!(
        cancellation,
        [egui_dockspace::DockspaceManipulationStatus::Cancelled(
            egui_dockspace::DockspaceManipulationCancelReason::PointerProviderRetired
        )],
    );
    assert_eq!(rect_fill_count(&downgraded, ghost_fill), 0);
    assert_eq!(rect_fill_count(&downgraded, drop_fill), 0);
    assert_eq!(
        guide_button_count(&downgraded, passive_guide, active_guide),
        0,
        "authority cancellation suppresses the stale guide cluster in the terminal pass",
    );
    assert!(
        rect_fill_count_in(&downgraded, active_fill, source_rect) > 0,
        "authority cancellation restores the source tab instead of retaining its gap",
    );
    assert!(context.dragged_id().is_none());

    let (released, response) = dockspace
        .run_single_surface(
            &context,
            input(vec![
                Event::PointerMoved(target),
                pointer_button(target, false),
            ]),
            SURFACE,
            &mut panes,
        )
        .expect("the owned run resumes after pointer retirement");
    assert!(response.manipulation_feedback().is_empty());
    let (released, receipt) = released.into_parts();
    receipt.report(SurfacePresentationResult::Presented);
    released.drop_without_applying_deltas();
    assert_eq!(
        root_tabs(&dockspace).expect("the downgrade restores the source tabs"),
        before,
    );
}

#[test]
fn output_marker_mismatch_rolls_back_the_update_and_preserves_the_previous_drag() {
    // An owned output-hook failure rejects this unpublished update. The preceding
    // gesture and its exact preview remain available for a later independent run.
    let context = Context::default();
    context.enable_accesskit();
    context.options_mut(|options| {
        options.max_passes = 4.try_into().expect("four is non-zero");
    });
    let style = instrumented_style();
    let ghost_fill = style
        .visuals
        .ghost_fill
        .expect("the fixture sets ghost fill");
    let drop_fill = style.visuals.drop_fill.expect("the fixture sets drop fill");
    let passive_guide = style
        .visuals
        .drop_guide_fill
        .expect("the fixture sets passive guide fill");
    let active_guide = style
        .visuals
        .drop_guide_active_fill
        .expect("the fixture sets active guide fill");
    let mut dockspace = Dockspace::builder("product-output-marker-mismatch", layout())
        .style(style)
        .build()
        .expect("the output-mismatch fixture initializes");
    let mut panes = Panes;
    let before = root_tabs(&dockspace).expect("the fixture remains a tab group");

    let _ = run_frame(&context, &mut dockspace, &mut panes, Vec::new());
    let stable = run_frame(&context, &mut dockspace, &mut panes, Vec::new());
    let source = tab_center(&stable.output, "Second");
    let first = node_rect(&stable.output, Role::Tab, "First");
    let target = Pos2::new(first.min.x + first.width() * 0.25, first.center().y);
    let _ = run_frame(
        &context,
        &mut dockspace,
        &mut panes,
        vec![Event::PointerMoved(source), pointer_button(source, true)],
    );
    let dragging = run_frame(
        &context,
        &mut dockspace,
        &mut panes,
        vec![Event::PointerMoved(target)],
    );
    assert!(
        rect_fill_count(&dragging.output, ghost_fill) > 0
            || rect_fill_count(&dragging.output, drop_fill) > 0,
        "the owned path establishes presentation-qualified drag output",
    );
    let version = dockspace.version();
    let previous_preview = rect_with_fill(&dragging.output, drop_fill);
    let previous_guides = guide_button_count(&dragging.output, passive_guide, active_guide);

    context
        .plugin_or_default::<LateDiscardPlugin>()
        .lock()
        .strip_callback_compounds_once = true;
    let rejected = dockspace.run_single_surface(
        &context,
        input(vec![Event::PointerMoved(source + vec2(30.0, 120.0))]),
        SURFACE,
        &mut panes,
    );
    assert!(
        rejected.is_err(),
        "a stripped terminal output must never publish an owned update"
    );
    assert_eq!(dockspace.version(), version);
    assert_eq!(
        root_tabs(&dockspace).expect("the source tab group remains present"),
        before,
        "a forged or missing final-output proof cannot mutate topology",
    );
    let recovered = run_frame(&context, &mut dockspace, &mut panes, Vec::new());
    assert_eq!(dockspace.version(), version);
    assert_eq!(root_tabs(&dockspace), Some(before));
    assert_eq!(
        rect_with_fill(&recovered.output, drop_fill),
        previous_preview,
        "the rejected move cannot replace or cancel the preceding preview"
    );
    assert_eq!(
        guide_button_count(&recovered.output, passive_guide, active_guide),
        previous_guides
    );
    assert!(
        rect_fill_count(&recovered.output, ghost_fill) > 0 || previous_preview.is_some(),
        "the previous gesture remains active after rollback"
    );

    let (released, response) = dockspace
        .run_single_surface(
            &context,
            input(vec![
                Event::PointerMoved(target),
                pointer_button(target, false),
            ]),
            SURFACE,
            &mut panes,
        )
        .expect("a fresh run releases the retained gesture");
    assert!(response.mutation().workspace_changed());
    let [terminal] = response.manipulation_feedback() else {
        panic!(
            "one delivery after rollback: {:?}",
            response.manipulation_feedback()
        );
    };
    assert!(matches!(
        terminal.status(),
        egui_dockspace::DockspaceManipulationStatus::Delivered(_)
    ));
    let (mut released, receipt) = released.into_parts();
    receipt.report(SurfacePresentationResult::Presented);
    released.textures_delta.clear();
    assert_eq!(root_tabs(&dockspace), Some(vec![SECOND, FIRST]));
    assert_eq!(rect_fill_count(&released, ghost_fill), 0);
    assert_eq!(rect_fill_count(&released, drop_fill), 0);
    assert_eq!(
        guide_button_count(&released, passive_guide, active_guide),
        0
    );
    assert!(context.dragged_id().is_none());
    let committed = dockspace.version();
    let (_, changed, _) =
        run_pane_frame_with_mutation(&context, &mut dockspace, &mut panes, Vec::new());
    assert!(!changed);
    assert_eq!(dockspace.version(), committed);
}
