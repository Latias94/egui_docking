//! Product journeys from explicit touch contacts, including synthesized pointer duplicates.

use super::*;
use dockspace::runtime::DockspaceTouchEvent;

fn frame(
    context: &Context,
    dockspace: &mut Dockspace,
    panes: &mut MagneticPanes,
    time: f64,
    events: Vec<Event>,
) -> (egui::FullOutput, Vec<DockspaceTouchEvent>) {
    let mut raw = input(events);
    raw.time = Some(time);
    let mut touch = Vec::new();
    let output = crate::run_ui(context, raw, |ui| {
        let response = dockspace.show_single_surface(SURFACE, ui, panes).unwrap();
        touch.extend_from_slice(response.touch_events());
    });
    let (mut output, receipt) = output.into_parts();
    receipt.report(crate::SurfacePresentationResult::Presented);
    output.textures_delta.clear();
    (output, touch)
}

fn contact(phase: egui::TouchPhase, pos: Pos2) -> Event {
    Event::Touch {
        device_id: egui::TouchDeviceId(1),
        id: egui::TouchId(7),
        phase,
        pos,
        force: None,
    }
}

fn setup() -> (Context, Dockspace, MagneticPanes, Pos2) {
    let context = Context::default();
    context.enable_accesskit();
    context.options_mut(|options| options.max_passes = 1.try_into().unwrap());
    let layout = DockspaceLayout::new([DockspaceSurfaceLayout::new(
        SURFACE,
        DockspaceRootLayout::new(ROOT, DockspaceNode::tabs([FIRST, SECOND])),
    )])
    .unwrap();
    let mut dockspace = Dockspace::builder("touch-product", layout).build().unwrap();
    let mut panes = MagneticPanes::new();
    let mut target = None;
    for index in 0..8 {
        let (output, _) = frame(
            &context,
            &mut dockspace,
            &mut panes,
            f64::from(index) * 0.05,
            Vec::new(),
        );
        if let Some(rect) = try_tab_rect(&output, "Second") {
            target = Some(rect.center());
        }
    }
    (context, dockspace, panes, target.unwrap())
}

#[test]
fn explicit_touch_tap_deduplicates_synthesized_pointer_and_selects_on_release() {
    let (context, mut dockspace, mut panes, point) = setup();
    let before = dockspace.version();
    frame(
        &context,
        &mut dockspace,
        &mut panes,
        1.0,
        vec![
            contact(egui::TouchPhase::Start, point),
            Event::PointerMoved(point),
            pointer_button(point, true),
        ],
    );
    assert_eq!(dockspace.version(), before);
    assert!(!dockspace.view().item(SECOND).unwrap().is_selected());
    let wake = dockspace.session.pointer_continuation().unwrap();
    assert_ne!(wake.pointer().get(), 1);
    assert_eq!(wake.deadline(), std::time::Duration::from_millis(1350));
    let (_, touch) = frame(
        &context,
        &mut dockspace,
        &mut panes,
        1.2,
        vec![
            contact(egui::TouchPhase::End, point),
            pointer_button(point, false),
            Event::PointerGone,
        ],
    );
    assert!(touch.is_empty());
    assert!(dockspace.view().item(SECOND).unwrap().is_selected());
    assert!(!dockspace.session.has_active_gesture());
    assert!(dockspace.session.pointer_continuation().is_none());
    let selected = dockspace.version();
    frame(
        &context,
        &mut dockspace,
        &mut panes,
        1.3,
        vec![contact(egui::TouchPhase::End, point)],
    );
    assert_eq!(dockspace.version(), selected);
}

#[test]
#[allow(
    clippy::cast_possible_truncation,
    reason = "the actual close-control bounds are inside the finite egui viewport"
)]
fn explicit_touch_close_control_checks_release_slop_and_keeps_ordinary_taps() {
    for with_move in [false, true] {
        let (context, mut dockspace, mut panes, _) = setup();
        let (output, _) = frame(&context, &mut dockspace, &mut panes, 0.5, Vec::new());
        let close = output
            .platform_output
            .accesskit_update
            .as_ref()
            .unwrap()
            .nodes
            .iter()
            .find_map(|(_, node)| {
                (node.role() == Role::Button && node.label() == Some("Close Second"))
                    .then(|| node.bounds().unwrap())
            })
            .expect("the actual tab close control is exposed");
        let start = Pos2::new(close.x0 as f32 + 1.0, close.y0 as f32 + 1.0);
        let end = Pos2::new(close.x1 as f32 - 1.0, close.y1 as f32 - 1.0);
        assert!(start.distance(end) > 8.0);
        let mut run = |time, events| {
            let mut raw = input(events);
            raw.time = Some(time);
            let mut closes = 0;
            let mut touch = Vec::new();
            let output = crate::run_ui(&context, raw, |ui| {
                let response = dockspace
                    .show_single_surface(SURFACE, ui, &mut panes)
                    .unwrap();
                closes += response.close_requests().count();
                touch.extend_from_slice(response.touch_events());
            });
            let (output, receipt) = output.into_parts();
            output.drop_without_applying_deltas();
            receipt.report(crate::SurfacePresentationResult::Presented);
            assert!(dockspace.session.pointer_continuation().is_none());
            (closes, touch)
        };
        assert_eq!(
            run(
                1.0,
                vec![
                    contact(egui::TouchPhase::Start, start),
                    Event::PointerMoved(start),
                    pointer_button(start, true),
                ]
            ),
            (0, Vec::new())
        );
        let mut yielded = 0;
        if with_move {
            let (closes, events) = run(
                1.1,
                vec![
                    contact(egui::TouchPhase::Move, end),
                    Event::PointerMoved(end),
                ],
            );
            assert_eq!(closes, 0);
            yielded += events
                .iter()
                .filter(|event| matches!(event, DockspaceTouchEvent::YieldedToContent { .. }))
                .count();
        }
        let (closes, events) = run(
            1.2,
            vec![
                contact(egui::TouchPhase::End, end),
                pointer_button(end, false),
                Event::PointerGone,
            ],
        );
        yielded += events
            .iter()
            .filter(|event| matches!(event, DockspaceTouchEvent::YieldedToContent { .. }))
            .count();
        assert_eq!(closes, 0, "a distant release cannot request a close");
        assert_eq!(yielded, 1);
        let tap = Pos2::new(
            ((close.x0 + close.x1) * 0.5) as f32,
            ((close.y0 + close.y1) * 0.5) as f32,
        );
        assert_eq!(
            run(
                1.3,
                vec![
                    contact(egui::TouchPhase::Start, tap),
                    Event::PointerMoved(tap),
                    pointer_button(tap, true),
                ]
            ),
            (0, Vec::new())
        );
        let (closes, events) = run(
            1.4,
            vec![
                contact(egui::TouchPhase::End, tap),
                pointer_button(tap, false),
                Event::PointerGone,
            ],
        );
        assert_eq!(
            closes, 1,
            "a new contact still uses ordinary close semantics"
        );
        assert!(events.is_empty());
        assert!(!dockspace.session.has_active_gesture());
    }
}

#[test]
#[allow(
    clippy::cast_possible_truncation,
    reason = "the actual menu bounds are inside the finite egui viewport"
)]
fn explicit_touch_long_press_opens_the_existing_semantic_menu_once() {
    let (context, mut dockspace, mut panes, point) = setup();
    let before = dockspace.version();
    frame(
        &context,
        &mut dockspace,
        &mut panes,
        1.0,
        vec![
            contact(egui::TouchPhase::Start, point),
            Event::PointerMoved(point),
            pointer_button(point, true),
        ],
    );
    let wake = dockspace.session.pointer_continuation().unwrap();
    let (_, events) = frame(&context, &mut dockspace, &mut panes, 1.349, Vec::new());
    assert!(events.is_empty());
    assert_eq!(dockspace.session.pointer_continuation(), Some(wake));
    let (_, events) = frame(&context, &mut dockspace, &mut panes, 1.35, Vec::new());
    assert!(
        matches!(events.as_slice(), [DockspaceTouchEvent::Armed { pointer, surface }]
        if *pointer == wake.pointer() && *surface == SURFACE)
    );
    assert_eq!(dockspace.version(), before);
    assert!(!dockspace.view().item(SECOND).unwrap().is_selected());
    let (mut output, events) = frame(
        &context,
        &mut dockspace,
        &mut panes,
        1.36,
        vec![
            contact(egui::TouchPhase::End, point),
            pointer_button(point, false),
            Event::PointerGone,
        ],
    );
    let [DockspaceTouchEvent::ContextMenuRequested(request)] = events.as_slice() else {
        panic!("stationary armed release must request one semantic menu: {events:?}");
    };
    assert_eq!(request.pointer(), wake.pointer());
    assert_eq!(request.root(), ROOT);
    assert_eq!(request.item(), Some(SECOND));
    assert_eq!(dockspace.version(), before);
    assert!(!dockspace.session.has_active_gesture());
    for index in 0..3 {
        if output
            .platform_output
            .accesskit_update
            .as_ref()
            .is_some_and(|tree| {
                tree.nodes.iter().any(|(_, node)| {
                    node.role() == Role::Menu && node.label() == Some("Presentation commands")
                })
            })
        {
            break;
        }
        let (next, events) = frame(
            &context,
            &mut dockspace,
            &mut panes,
            1.4 + f64::from(index) * 0.05,
            Vec::new(),
        );
        assert!(events.is_empty());
        output = next;
    }
    assert!(
        output
            .platform_output
            .accesskit_update
            .as_ref()
            .unwrap()
            .nodes
            .iter()
            .any(|(_, node)| {
                node.role() == Role::Menu && node.label() == Some("Presentation commands")
            })
    );
    let float = output
        .platform_output
        .accesskit_update
        .as_ref()
        .unwrap()
        .nodes
        .iter()
        .find_map(|(_, node)| {
            (node.role() == Role::MenuItem && node.label() == Some("Float"))
                .then(|| node.bounds())
                .flatten()
        })
        .expect("the existing presentation menu exposes its Float command");
    let point = Pos2::new(
        ((float.x0 + float.x1) * 0.5) as f32,
        ((float.y0 + float.y1) * 0.5) as f32,
    );
    frame(
        &context,
        &mut dockspace,
        &mut panes,
        1.6,
        vec![Event::PointerMoved(point), pointer_button(point, true)],
    );
    frame(
        &context,
        &mut dockspace,
        &mut panes,
        1.7,
        vec![pointer_button(point, false)],
    );
    for index in 0..8 {
        frame(
            &context,
            &mut dockspace,
            &mut panes,
            1.8 + f64::from(index) * 0.1,
            Vec::new(),
        );
        if dockspace.view().item(SECOND).unwrap().contained().is_some() {
            break;
        }
    }
    assert!(dockspace.view().item(SECOND).unwrap().contained().is_some());
    assert_eq!(
        dockspace.view().item(FIRST).unwrap().root(),
        dockspace.view().item(SECOND).unwrap().root()
    );
}

#[test]
fn explicit_touch_prearm_motion_yields_without_selecting_or_dragging() {
    let (context, mut dockspace, mut panes, point) = setup();
    let before = dockspace.version();
    frame(
        &context,
        &mut dockspace,
        &mut panes,
        1.0,
        vec![contact(egui::TouchPhase::Start, point)],
    );
    let moved = point + vec2(9.0, 0.0);
    let (_, events) = frame(
        &context,
        &mut dockspace,
        &mut panes,
        1.01,
        vec![contact(egui::TouchPhase::Move, moved)],
    );
    assert!(matches!(
        events.as_slice(),
        [DockspaceTouchEvent::YieldedToContent { .. }]
    ));
    assert_eq!(dockspace.version(), before);
    assert!(!dockspace.view().item(SECOND).unwrap().is_selected());
    assert!(!dockspace.session.has_active_gesture());
    assert!(dockspace.session.pointer_continuation().is_none());
    frame(
        &context,
        &mut dockspace,
        &mut panes,
        1.5,
        vec![contact(egui::TouchPhase::End, point)],
    );
    assert_eq!(dockspace.version(), before);
}

#[test]
fn explicit_touch_long_press_then_motion_uses_the_normal_preview_and_dock_release() {
    for same_batch in [true, false] {
        let context = Context::default();
        context.enable_accesskit();
        context.options_mut(|options| options.max_passes = 1.try_into().unwrap());
        let mut dockspace = Dockspace::builder("touch-dock", magnetic_layout())
            .build()
            .unwrap();
        let mut panes = MagneticPanes::new();
        let mut source = None;
        for index in 0..8 {
            let (output, _) = frame(
                &context,
                &mut dockspace,
                &mut panes,
                f64::from(index) * 0.05,
                Vec::new(),
            );
            source = try_tab_rect(&output, "Third")
                .map(|rect| rect.center())
                .or(source);
        }
        let source = source.unwrap();
        let target = panes.target_rect.get().unwrap().center();
        let version = dockspace.version();
        frame(
            &context,
            &mut dockspace,
            &mut panes,
            1.0,
            vec![contact(egui::TouchPhase::Start, source)],
        );
        assert!(!dockspace.view().item(THIRD).unwrap().is_selected());
        let (_, events) = frame(&context, &mut dockspace, &mut panes, 1.35, Vec::new());
        assert!(matches!(
            events.as_slice(),
            [DockspaceTouchEvent::Armed { .. }]
        ));
        assert_eq!(dockspace.version(), version);
        let events = if same_batch {
            frame(
                &context,
                &mut dockspace,
                &mut panes,
                1.36,
                vec![
                    contact(egui::TouchPhase::Move, target),
                    contact(egui::TouchPhase::End, target),
                ],
            )
            .1
        } else {
            frame(
                &context,
                &mut dockspace,
                &mut panes,
                1.36,
                vec![contact(egui::TouchPhase::Move, target)],
            );
            assert!(dockspace.session.has_active_gesture());
            assert_eq!(dockspace.version(), version);
            assert!(!dockspace.view().item(THIRD).unwrap().is_selected());
            frame(&context, &mut dockspace, &mut panes, 1.4, Vec::new());
            frame(
                &context,
                &mut dockspace,
                &mut panes,
                1.5,
                vec![contact(egui::TouchPhase::Move, target)],
            );
            frame(
                &context,
                &mut dockspace,
                &mut panes,
                1.6,
                vec![contact(egui::TouchPhase::End, target)],
            )
            .1
        };
        assert!(
            events
                .iter()
                .all(|event| !matches!(event, DockspaceTouchEvent::ContextMenuRequested(_)))
        );
        for index in 0..8 {
            frame(
                &context,
                &mut dockspace,
                &mut panes,
                1.7 + f64::from(index) * 0.1,
                Vec::new(),
            );
            if dockspace
                .view()
                .item(THIRD)
                .unwrap()
                .tabs()
                .items()
                .contains(&FIRST)
            {
                break;
            }
        }
        assert!(
            dockspace
                .view()
                .item(THIRD)
                .unwrap()
                .tabs()
                .items()
                .contains(&FIRST)
        );
        assert!(!dockspace.session.has_active_gesture());
        assert!(dockspace.session.pointer_continuation().is_none());
    }
}

#[test]
fn touch_content_scrolling_keeps_the_pane_scroll_owner_without_arming_docking() {
    struct ScrollPane {
        viewport: Option<Rect>,
        offset: f32,
    }
    impl PaneView for ScrollPane {
        fn title(&self, _: ItemId) -> Option<egui::WidgetText> {
            Some("Scrollable pane".into())
        }
        fn ui(&mut self, _: ItemId, ui: &mut Ui) -> crate::PaneResponse {
            let result = egui::ScrollArea::vertical()
                .id_salt("touch-content-scroll")
                // Register the content drag owner before the first touch.
                .scroll_source(egui::scroll_area::ScrollSource::ALL)
                .auto_shrink([false; 2])
                .show(ui, |ui| {
                    for index in 0..200 {
                        ui.label(format!("Scrollable row {index}"));
                    }
                });
            self.viewport = Some(result.inner_rect);
            self.offset = result.state.clamped_offset().y;
            crate::PaneResponse::default()
        }
    }
    let context = Context::default();
    context.options_mut(|options| options.max_passes = 1.try_into().unwrap());
    let layout = DockspaceLayout::new([DockspaceSurfaceLayout::new(
        SURFACE,
        DockspaceRootLayout::new(ROOT, DockspaceNode::central_tabs([FIRST])),
    )])
    .unwrap();
    let mut dockspace = Dockspace::builder("touch-scroll-pane", layout)
        .build()
        .unwrap();
    let mut pane = ScrollPane {
        viewport: None,
        offset: 0.0,
    };
    let mut run = |time: f64, events: Vec<Event>| {
        let mut raw = input(events);
        raw.time = Some(time);
        let output = crate::run_ui(&context, raw, |ui| {
            let response = dockspace
                .show_single_surface(SURFACE, ui, &mut pane)
                .unwrap();
            assert!(response.touch_events().is_empty());
        });
        let (output, receipt) = output.into_parts();
        output.drop_without_applying_deltas();
        receipt.report(crate::SurfacePresentationResult::Presented);
        assert!(!dockspace.session.has_active_gesture());
        assert!(dockspace.session.pointer_continuation().is_none());
        (pane.viewport.unwrap(), pane.offset, dockspace.version())
    };
    for index in 0..6 {
        run(f64::from(index) * 0.05, Vec::new());
    }
    let (viewport, initial_offset, version) = run(0.5, Vec::new());
    let start = viewport.center();
    run(
        1.0,
        vec![
            contact(egui::TouchPhase::Start, start),
            Event::PointerMoved(start),
            pointer_button(start, true),
        ],
    );
    let moved = start - vec2(0.0, 80.0);
    let (_, offset, after) = run(
        1.1,
        vec![
            contact(egui::TouchPhase::Move, moved),
            Event::PointerMoved(moved),
        ],
    );
    assert!(
        offset > initial_offset + 40.0,
        "the actual pane scroll owner must receive touch motion: {initial_offset} -> {offset}"
    );
    assert_eq!(after, version);
    run(
        1.2,
        vec![
            contact(egui::TouchPhase::End, moved),
            pointer_button(moved, false),
            Event::PointerGone,
        ],
    );
}
