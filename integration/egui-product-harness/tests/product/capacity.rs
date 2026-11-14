use super::*;
use std::collections::BTreeMap;

const TITLES: [&str; 6] = [
    "正文一 — 编辑器🦀 / 非换行宽内容",
    "正文二 — 数据视图🧭 / Selected document",
    "正文三 — Пример документа / Διάγραμμα",
    "正文四 — 長い名前のドキュメント / 🚀",
    "正文五 — ملف المستند / Oversized content",
    "正文六 — 마지막 문서 / Stateful pane",
];

#[derive(Clone, Copy)]
struct ContentObservation {
    clip: Rect,
    tail: Rect,
    spill: Rect,
    offset: egui::Vec2,
    scroll: egui::Id,
}

struct ContentPanes {
    both_axes: bool,
    current: BTreeMap<ItemId, ContentObservation>,
    identities: BTreeMap<ItemId, egui::Id>,
    clicks: BTreeMap<ItemId, usize>,
}

impl ContentPanes {
    fn new(both_axes: bool) -> Self {
        Self {
            both_axes,
            current: BTreeMap::new(),
            identities: BTreeMap::new(),
            clicks: BTreeMap::new(),
        }
    }
}

impl PaneView for ContentPanes {
    fn title(&self, item: ItemId) -> Option<egui::WidgetText> {
        TITLES
            .get(item.get() as usize - 1)
            .map(|title| (*title).into())
    }

    fn ui(&mut self, item: ItemId, ui: &mut Ui) -> PaneResponse {
        let clip = ui.clip_rect();
        let mut responses = Vec::new();
        let mut tail = Rect::NOTHING;
        let mut spill = Rect::NOTHING;
        let scroll = if self.both_axes {
            egui::ScrollArea::both()
        } else {
            egui::ScrollArea::vertical()
        }
        .id_salt("oversized-document")
        .auto_shrink([false, false])
        .show(ui, |ui| {
            let origin = ui.next_widget_position();
            ui.set_min_size(vec2(1_800.0, 1_320.0));
            let document = Rect::from_min_size(origin, vec2(1_800.0, 1_320.0));
            ui.painter()
                .rect_filled(document, 0.0, document_color(item));
            let side = ui.put(
                Rect::from_min_size(origin + vec2(400.0, 28.0), vec2(120.0, 28.0)),
                egui::Button::new(format!("Clipped action {}", item.get())),
            );
            spill = side.rect;
            let end = ui.put(
                Rect::from_min_size(
                    origin + vec2(if self.both_axes { 1_660.0 } else { 20.0 }, 1_260.0),
                    vec2(120.0, 32.0),
                ),
                egui::Button::new(format!("Document end {}", item.get())),
            );
            tail = end.rect;
            for response in [side, end] {
                if response.clicked() {
                    *self.clicks.entry(item).or_default() += 1;
                }
                if response.interact_rect.is_positive() {
                    assert!(clip.expand(0.5).contains_rect(response.interact_rect));
                }
                responses.push(response);
            }
        });
        if let Some(previous) = self.identities.insert(item, scroll.id) {
            assert_eq!(scroll.id, previous, "content identity must follow {item}");
        }
        assert!(
            self.identities
                .iter()
                .all(|(&other, &id)| other == item || id != scroll.id),
            "different panes must have independent scroll state: {item}"
        );
        self.current.insert(
            item,
            ContentObservation {
                clip,
                tail,
                spill,
                offset: scroll.state.clamped_offset(),
                scroll: scroll.id,
            },
        );
        let mut content = PaneResponse::default();
        for response in &responses {
            content = content.with_content(response);
        }
        for id in [
            scroll.id.with("area"),
            scroll.id.with(0_usize),
            scroll.id.with(1_usize),
        ] {
            if let Some(response) = ui.ctx().read_response(id) {
                content = content.with_content(&response);
            }
        }
        content
    }
}

fn document_color(item: ItemId) -> egui::Color32 {
    egui::Color32::from_rgb(120 + item.get() as u8, 38, 173)
}

fn click(point: Pos2) -> Vec<Event> {
    vec![
        Event::PointerMoved(point),
        pointer_button(point, true),
        pointer_button(point, false),
    ]
}

fn capacity_layout(count: u64) -> DockspaceLayout {
    DockspaceLayout::new([DockspaceSurfaceLayout::new(
        SURFACE,
        DockspaceRootLayout::new(
            ROOT,
            DockspaceNode::equal_split(
                DockspaceAxis::Horizontal,
                [
                    DockspaceNode::tabs([FIRST]),
                    DockspaceNode::tabs((2..=count).map(ItemId::new)),
                ],
            )
            .unwrap(),
        ),
    )])
    .unwrap()
}

fn frame(
    ctx: &Context,
    dock: &mut Dockspace,
    panes: &mut ContentPanes,
    size: egui::Vec2,
    events: Vec<Event>,
) -> FrameOutput {
    panes.current.clear();
    let mut raw = input(events);
    raw.screen_rect = Some(Rect::from_min_size(Pos2::ZERO, size));
    raw.time = Some(ctx.input(|input| input.time) + 1.0 / 60.0);
    let (output, response) = dock.run_single_surface(ctx, raw, SURFACE, panes).unwrap();
    let close_requests = response.close_request_events().to_vec();
    assert!(close_requests.is_empty());
    let (mut output, receipt) = output.into_parts();
    receipt.report(SurfacePresentationResult::Presented);
    output.textures_delta.clear();
    for (&item, observation) in &panes.current {
        assert!(observation.clip.is_finite());
        for shape in &output.shapes {
            if let egui::Shape::Rect(rect) = &shape.shape
                && rect.fill == document_color(item)
            {
                let visible = rect.rect.intersect(shape.clip_rect);
                if visible.is_positive() {
                    assert!(observation.clip.expand(0.5).contains_rect(visible));
                    assert!(
                        Rect::from_min_size(Pos2::ZERO, size)
                            .expand(0.5)
                            .contains_rect(visible)
                    );
                }
            }
        }
    }
    FrameOutput {
        output,
        close_requests,
    }
}

fn conserve_items(dock: &Dockspace, count: u64) {
    fn collect(node: egui_dockspace::DockspaceNodeView<'_>, items: &mut Vec<ItemId>) {
        if let Some(tabs) = node.tabs() {
            assert!(!tabs.items().is_empty());
            items.extend_from_slice(tabs.items());
        } else {
            for child in node.split().unwrap().children() {
                collect(child, items);
            }
        }
    }
    let mut items = Vec::new();
    for surface in dock.view().surfaces() {
        if let Some(root) = surface.main_root() {
            collect(root.content().unwrap(), &mut items);
        }
        for floating in surface.contained() {
            collect(floating.root().unwrap().content().unwrap(), &mut items);
        }
    }
    items.sort_unstable();
    assert_eq!(items, (1..=count).map(ItemId::new).collect::<Vec<_>>());
}

fn merge_tab(
    ctx: &Context,
    dock: &mut Dockspace,
    panes: &mut ContentPanes,
    size: egui::Vec2,
    source: ItemId,
    target: ItemId,
) {
    let stable = frame(ctx, dock, panes, size, Vec::new());
    let label = TITLES[source.get() as usize - 1];
    let (source_node, _) = accesskit_node(&stable.output, Role::Tab, label);
    let (source_widget, source_bounds) = ctx
        .viewport(|viewport| {
            viewport
                .prev_pass
                .widgets
                .layers()
                .flat_map(|(_, widgets)| widgets)
                .find(|widget| {
                    widget.id.accesskit_id() == source_node && widget.enabled && widget.visible
                })
                .map(|widget| (widget.id, widget.interact_rect))
        })
        .expect("a real source tab must have an enabled and visible widget registration");
    let target_point = panes.current[&target].clip.center();
    let mut start = None;
    let mut observed_hits = Vec::new();
    // Scroll controls can overlay a visible tab; a contained title can sit
    // underneath it. Ask egui which widget wins at each candidate point.
    for fraction in [0.5, 0.9, 0.1, 0.75, 0.25] {
        let point = Pos2::new(
            source_bounds.min.x + source_bounds.width() * fraction,
            source_bounds.center().y,
        );
        frame(ctx, dock, panes, size, vec![Event::PointerMoved(point)]);
        let hit = ctx.viewport(|viewport| viewport.hits.drag);
        observed_hits.push((point, hit.map(|hit| (hit.id, hit.interact_rect))));
        if hit.is_some_and(|hit| hit.id == source_widget) {
            start = Some(point);
            break;
        }
    }
    let start = start.unwrap_or_else(|| panic!(
        "the source tab must have an exposed drag point: item={source}, bounds={source_bounds:?}, hits={observed_hits:?}",
    ));
    assert!(
        ctx.viewport(|viewport| viewport
            .hits
            .drag
            .is_some_and(|hit| hit.id == source_widget)),
        "the source tab must own the actual drag hit: item={source}, point={start:?}",
    );
    frame(
        ctx,
        dock,
        panes,
        size,
        vec![Event::PointerMoved(start), pointer_button(start, true)],
    );
    assert!(
        dock.has_active_gesture(),
        "the source tab must acquire a gesture: {source}"
    );
    frame(
        ctx,
        dock,
        panes,
        size,
        vec![Event::PointerMoved(start + vec2(16.0, 12.0))],
    );
    let preview = settle_until(8, || {
        let frame = frame(
            ctx,
            dock,
            panes,
            size,
            vec![Event::PointerMoved(target_point)],
        );
        let missing =
            if try_node_rect(&frame.output, Role::Label, "Dockspace center drop guide").is_some() {
                Vec::new()
            } else {
                vec!["content pane center guide"]
            };
        (frame, missing)
    })
    .unwrap();
    let guide = node_center(&preview.output, Role::Label, "Dockspace center drop guide");
    frame(ctx, dock, panes, size, vec![Event::PointerMoved(guide)]);
    frame(ctx, dock, panes, size, vec![pointer_button(guide, false)]);
    frame(ctx, dock, panes, size, Vec::new());
    assert!(dock.view().item(source).unwrap().is_selected());
    assert!(dock.view().item(source).unwrap().contained().is_none());
    assert!(
        dock.view()
            .item(target)
            .unwrap()
            .tabs()
            .items()
            .contains(&source),
        "the released tab must join the intended target group"
    );
    assert!(!dock.has_active_gesture());
}

fn reveal_first(ctx: &Context, dock: &mut Dockspace, panes: &mut ContentPanes, size: egui::Vec2) {
    let stable = frame(ctx, dock, panes, size, Vec::new());
    if let Some(tab) =
        try_node_rect(&stable.output, Role::Tab, TITLES[0]).filter(|rect| rect.is_positive())
    {
        let point = tab.center();
        frame(ctx, dock, panes, size, click(point));
    } else {
        let (menu, _) = accesskit_node(&stable.output, Role::Button, "Show hidden tabs");
        frame(
            ctx,
            dock,
            panes,
            size,
            vec![accesskit_action(menu, Action::Click)],
        );
        let menu = frame(ctx, dock, panes, size, Vec::new());
        let (first, _) = accesskit_node(&menu.output, Role::MenuItem, TITLES[0]);
        frame(
            ctx,
            dock,
            panes,
            size,
            vec![accesskit_action(first, Action::Click)],
        );
    }
    let revealed = frame(ctx, dock, panes, size, Vec::new());
    assert!(dock.view().item(FIRST).unwrap().is_selected());
    assert!(node_rect(&revealed.output, Role::Tab, TITLES[0]).is_positive());
}

fn scroll_to_document_end(
    ctx: &Context,
    dock: &mut Dockspace,
    panes: &mut ContentPanes,
    size: egui::Vec2,
) {
    let neighbor_offset = panes.current[&SECOND].offset;
    let point = panes.current[&FIRST].clip.center();
    frame(ctx, dock, panes, size, vec![Event::PointerMoved(point)]);
    for _ in 0..12 {
        let state = panes.current[&FIRST];
        if state.clip.contains_rect(state.tail) {
            break;
        }
        frame(
            ctx,
            dock,
            panes,
            size,
            vec![mouse_wheel(vec2(
                if panes.both_axes { -2_400.0 } else { 0.0 },
                -2_400.0,
            ))],
        );
    }
    let state = panes.current[&FIRST];
    assert!(ctx.read_response(state.scroll.with(1_usize)).is_some());
    assert!(state.offset.y > 0.0);
    assert_eq!(state.offset.x > 0.0, panes.both_axes);
    assert!(
        state.clip.contains_rect(state.tail),
        "scroll must reach the real last button: {:?} in {:?}",
        state.tail,
        state.clip
    );
    let point = state.tail.center();
    frame(ctx, dock, panes, size, click(point));
    assert_eq!(panes.clicks.get(&FIRST), Some(&1));
    assert_eq!(
        panes.current[&SECOND].offset, neighbor_offset,
        "scrolling one pane must not change its neighbor's offset"
    );
}

#[test]
fn oversized_content_remains_scrollable_and_clipped_through_continuous_docking() {
    for both_axes in [false, true] {
        let ctx = Context::default();
        ctx.enable_accesskit();
        let size = vec2(640.0, 300.0);
        let mut dock = Dockspace::builder(("content-capacity", both_axes), capacity_layout(6))
            .build()
            .unwrap();
        let mut panes = ContentPanes::new(both_axes);
        frame(&ctx, &mut dock, &mut panes, size, Vec::new());
        frame(&ctx, &mut dock, &mut panes, size, Vec::new());
        let clipped = panes.current[&FIRST];
        let neighbor = panes.current[&SECOND].clip;
        let overlap = clipped.spill.intersect(neighbor);
        assert!(
            overlap.is_positive(),
            "the clipped button must overlap the adjacent body: left={:?}, right={neighbor:?}, button={:?}",
            clipped.clip,
            clipped.spill,
        );
        let point = overlap.center();
        assert!(
            !clipped.clip.contains(point) && neighbor.contains(point),
            "the test click must be inside the adjacent body: point={point:?}, left={:?}, right={neighbor:?}",
            clipped.clip,
        );
        frame(&ctx, &mut dock, &mut panes, size, click(point));
        assert_eq!(
            panes.clicks.get(&FIRST),
            None,
            "a clipped button cannot steal the adjacent pane's click"
        );
        scroll_to_document_end(&ctx, &mut dock, &mut panes, size);
        let retained = panes.current[&FIRST];
        for _ in 0..5 {
            let target_tabs = dock.view().item(FIRST).unwrap().tabs();
            let target = target_tabs.selected().unwrap();
            let remaining = (2..=6)
                .map(ItemId::new)
                .find(|item| !target_tabs.items().contains(item))
                .unwrap();
            let source_tabs = dock.view().item(remaining).unwrap().tabs();
            let source = source_tabs.selected().unwrap();
            merge_tab(&ctx, &mut dock, &mut panes, size, source, target);
            let merged = dock.view().item(source).unwrap().tabs();
            assert!(merged.items().contains(&FIRST));
            conserve_items(&dock, 6);
        }
        reveal_first(&ctx, &mut dock, &mut panes, size);
        let after = panes.current[&FIRST];
        assert_eq!(after.scroll, retained.scroll);
        assert!((after.offset.y - retained.offset.y).abs() < 0.5);
        assert_eq!(panes.clicks.get(&FIRST), Some(&1));
        assert_eq!(root_tabs(&dock).unwrap().len(), 6);
    }
}

#[test]
fn compression_reveals_constrained_panes_and_preserves_real_content_interaction() {
    let ctx = Context::default();
    ctx.enable_accesskit();
    let size = vec2(640.0, 300.0);
    let mut style = DockStyle {
        minimum_pane_size: vec2(900.0, 180.0),
        ..Default::default()
    };
    let mut dock = Dockspace::builder("compressed-content", capacity_layout(2))
        .style(style.clone())
        .build()
        .unwrap();
    let mut panes = ContentPanes::new(true);
    frame(&ctx, &mut dock, &mut panes, size, Vec::new());
    frame(&ctx, &mut dock, &mut panes, size, Vec::new());
    assert!(!panes.current.contains_key(&SECOND));
    style.pane_overflow_policy = egui_dockspace::PaneOverflowPolicy::Compress;
    dock.set_style(style).unwrap();
    frame(&ctx, &mut dock, &mut panes, size, Vec::new());
    frame(&ctx, &mut dock, &mut panes, size, Vec::new());
    for item in [FIRST, SECOND] {
        let clip = panes.current[&item].clip;
        assert!(clip.width() > 0.0 && clip.width() < 900.0);
        assert!(clip.height() > 0.0);
    }
    scroll_to_document_end(&ctx, &mut dock, &mut panes, size);
    let retained = panes.current[&FIRST];
    merge_tab(&ctx, &mut dock, &mut panes, size, SECOND, FIRST);
    reveal_first(&ctx, &mut dock, &mut panes, size);
    conserve_items(&dock, 2);
    assert_eq!(panes.current[&FIRST].scroll, retained.scroll);
    assert_eq!(panes.clicks.get(&FIRST), Some(&1));
}

#[test]
fn tiny_and_zero_surfaces_recover_scrolled_content_and_new_float_redock_gestures() {
    let ctx = Context::default();
    ctx.enable_accesskit();
    let size = vec2(480.0, 300.0);
    let mut dock = Dockspace::builder("tiny-content-capacity", capacity_layout(2))
        .build()
        .unwrap();
    let mut panes = ContentPanes::new(true);
    frame(&ctx, &mut dock, &mut panes, size, Vec::new());
    let stable = frame(&ctx, &mut dock, &mut panes, size, Vec::new());
    scroll_to_document_end(&ctx, &mut dock, &mut panes, size);
    let retained = panes.current[&FIRST];
    let source = tab_center(&stable.output, TITLES[1]);
    frame(
        &ctx,
        &mut dock,
        &mut panes,
        size,
        vec![Event::PointerMoved(source), pointer_button(source, true)],
    );
    frame(
        &ctx,
        &mut dock,
        &mut panes,
        size,
        vec![Event::PointerMoved(source + vec2(-32.0, 48.0))],
    );
    assert!(dock.has_active_gesture());
    frame(
        &ctx,
        &mut dock,
        &mut panes,
        vec2(12.0, 8.0),
        key_press(Key::Escape),
    );
    assert!(!dock.has_active_gesture());
    frame(
        &ctx,
        &mut dock,
        &mut panes,
        egui::Vec2::ZERO,
        vec![Event::WindowFocused(false), Event::PointerGone],
    );
    frame(
        &ctx,
        &mut dock,
        &mut panes,
        size,
        vec![Event::WindowFocused(true), pointer_button(source, false)],
    );
    frame(&ctx, &mut dock, &mut panes, size, Vec::new());
    assert_eq!(panes.current[&FIRST].scroll, retained.scroll);
    assert!(panes.current[&FIRST].offset.y > 0.0);
    conserve_items(&dock, 2);
    merge_tab(&ctx, &mut dock, &mut panes, size, SECOND, FIRST);
    reveal_first(&ctx, &mut dock, &mut panes, size);
    let stable = frame(&ctx, &mut dock, &mut panes, size, Vec::new());
    let start = tab_center(&stable.output, TITLES[0]);
    let target = start + vec2(40.0, 100.0);
    let shift_button = |pos, pressed| Event::PointerButton {
        pos,
        button: PointerButton::Primary,
        pressed,
        modifiers: Modifiers::SHIFT,
    };
    frame(
        &ctx,
        &mut dock,
        &mut panes,
        size,
        vec![
            Event::ModifiersChanged(Modifiers::SHIFT),
            Event::PointerMoved(start),
            shift_button(start, true),
        ],
    );
    frame(
        &ctx,
        &mut dock,
        &mut panes,
        size,
        vec![Event::PointerMoved(target)],
    );
    frame(
        &ctx,
        &mut dock,
        &mut panes,
        size,
        vec![
            shift_button(target, false),
            Event::ModifiersChanged(Modifiers::NONE),
        ],
    );
    frame(&ctx, &mut dock, &mut panes, size, Vec::new());
    assert!(dock.view().item(FIRST).unwrap().contained().is_some());
    assert_eq!(panes.current[&FIRST].scroll, retained.scroll);
    assert!(panes.current[&FIRST].offset.y > 0.0);
    merge_tab(&ctx, &mut dock, &mut panes, size, FIRST, SECOND);
    conserve_items(&dock, 2);
    assert_eq!(panes.current[&FIRST].scroll, retained.scroll);
    assert_eq!(panes.clicks.get(&FIRST), Some(&1));
    let merged = dock.view().item(FIRST).unwrap().tabs();
    assert!(merged.items().contains(&SECOND));
}
