//! Public tab callbacks preserve core geometry, accessibility and input ownership.

use dockspace::policy::{DockPolicy, TabBarInteraction, TabBarPolicy, TabBarVisibility};
use egui::accesskit::Role;
use egui::{Color32, Context, Event, FullOutput, Pos2, RawInput, Rect, Ui, vec2};
use egui_dockspace::{
    DockStyle, Dockspace, DockspaceLayout, DockspaceNode, DockspaceRootLayout,
    DockspaceSurfaceLayout, ItemId, PaneResponse, PaneView, RootId, SurfaceId,
    SurfacePresentationResult, Tab, TabBar, TabBarId, TabBarLayout, TabBarSlot,
};

const SURFACE: SurfaceId = SurfaceId::new(1);
const FIRST: ItemId = ItemId::new(1);
const SECOND: ItemId = ItemId::new(2);
const CUSTOM_FILL: Color32 = Color32::from_rgb(22, 140, 120);

#[derive(Clone)]
struct TabSnapshot {
    item: ItemId,
    bar: TabBarId,
    rect: Rect,
    visible: Rect,
    content: Rect,
    enabled: bool,
}

#[derive(Clone)]
struct SlotSnapshot {
    bar: TabBarId,
    slot: TabBarSlot,
    rect: Rect,
    clip: Rect,
    enabled: bool,
    items: Vec<ItemId>,
}

struct Panes {
    extra_width: f32,
    layout: TabBarLayout,
    tabs: Vec<TabSnapshot>,
    slots: Vec<SlotSnapshot>,
    tab_button: Option<Rect>,
    slot_button: Option<Rect>,
    tab_clicks: usize,
    slot_clicks: usize,
    slot_menu: bool,
    missing: bool,
}

impl Default for Panes {
    fn default() -> Self {
        Self {
            extra_width: 40.0,
            layout: TabBarLayout {
                leading_width: 36.0,
                trailing_width: 64.0,
            },
            tabs: Vec::new(),
            slots: Vec::new(),
            tab_button: None,
            slot_button: None,
            tab_clicks: 0,
            slot_clicks: 0,
            slot_menu: false,
            missing: false,
        }
    }
}

impl PaneView for Panes {
    fn title(&self, item: ItemId) -> Option<egui::WidgetText> {
        (!self.missing).then(|| if item == FIRST { "First" } else { "Second" }.into())
    }

    fn ui(&mut self, _item: ItemId, _ui: &mut Ui) -> PaneResponse {
        PaneResponse::default()
    }

    fn tab_content_width(&self, _item: ItemId, _ui: &Ui, title: &egui::Galley) -> f32 {
        title.size().x + self.extra_width
    }

    fn tab_ui(&mut self, ui: &mut Ui, tab: &Tab<'_>) {
        self.tabs.push(TabSnapshot {
            item: tab.item,
            bar: tab.bar,
            rect: tab.rect,
            visible: tab.visible_rect,
            content: tab.content_rect,
            enabled: tab.enabled && ui.is_enabled(),
        });
        let mut custom = tab.clone();
        custom.visuals.fill = CUSTOM_FILL;
        custom.paint_background(ui.painter());
        let button = Rect::from_min_max(
            egui::pos2(tab.content_rect.right() - 24.0, tab.content_rect.top()),
            tab.content_rect.right_bottom(),
        );
        let title_rect = Rect::from_min_max(tab.content_rect.min, button.left_bottom());
        custom.paint_title(ui, title_rect);
        if tab.item == SECOND {
            let response = ui.put(button, egui::Button::new("!"));
            self.tab_button = Some(response.rect);
            self.tab_clicks += usize::from(response.clicked());
        }
    }

    fn tab_bar_layout(&self, _bar: TabBarId, _ui: &Ui) -> TabBarLayout {
        self.layout
    }

    fn tab_bar_ui(&mut self, ui: &mut Ui, bar: TabBar<'_>, slot: TabBarSlot) {
        self.slots.push(SlotSnapshot {
            bar: bar.id(),
            slot,
            rect: ui.max_rect(),
            clip: ui.clip_rect(),
            enabled: ui.is_enabled(),
            items: bar.items().collect(),
        });
        if slot == TabBarSlot::Trailing {
            let response = if self.slot_menu {
                ui.menu_button("Action", |ui| ui.label("Custom action"))
                    .response
            } else {
                ui.button("Action")
            };
            self.slot_button = Some(response.rect);
            self.slot_clicks += usize::from(response.clicked());
        } else {
            ui.label("2");
        }
    }
}

struct Harness {
    context: Context,
    dock: Dockspace,
    // Exercise forwarding of every new callback through Box<PaneView>.
    panes: Box<Panes>,
    close_requests: usize,
    width: f32,
}

impl Harness {
    fn new(policy: DockPolicy) -> Self {
        let context = Context::default();
        context.enable_accesskit();
        let layout = DockspaceLayout::new([DockspaceSurfaceLayout::new(
            SURFACE,
            DockspaceRootLayout::new(RootId::new(1), DockspaceNode::tabs([FIRST, SECOND])),
        )])
        .unwrap();
        let mut style = DockStyle {
            tab_corner_radius: egui::CornerRadius::same(6),
            ..Default::default()
        };
        style.visuals.tab_stroke = Some(egui::Stroke::new(1.0, Color32::RED));
        Self {
            context,
            dock: Dockspace::builder("custom-tab-test", layout)
                .policy(policy)
                .style(style)
                .build()
                .unwrap(),
            panes: Box::default(),
            close_requests: 0,
            width: 800.0,
        }
    }

    fn run(&mut self, events: Vec<Event>) -> FullOutput {
        self.panes.tabs.clear();
        self.panes.slots.clear();
        let input = RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(self.width, 400.0))),
            time: Some(self.context.input(|input| input.time) + 0.1),
            events,
            ..Default::default()
        };
        let (output, response) = self
            .dock
            .run_single_surface(&self.context, input, SURFACE, &mut self.panes)
            .unwrap();
        assert_eq!(
            response.presentation_settlement_capability(),
            egui_dockspace::DockspaceCapability::Supported
        );
        self.close_requests += response
            .close_request_events()
            .iter()
            .filter(|event| !event.reused())
            .count();
        let (mut output, receipt) = output.into_parts();
        receipt.report(SurfacePresentationResult::Presented);
        output.textures_delta.clear();
        output
    }

    fn settle(&mut self) {
        for _ in 0..3 {
            self.run(Vec::new()).drop_without_applying_deltas();
        }
    }

    fn click(&mut self, point: Pos2) {
        self.run(vec![Event::PointerMoved(point)])
            .drop_without_applying_deltas();
        for pressed in [true, false] {
            self.run(vec![Event::PointerButton {
                pos: point,
                button: egui::PointerButton::Primary,
                pressed,
                modifiers: egui::Modifiers::NONE,
            }])
            .drop_without_applying_deltas();
        }
        self.settle();
    }

    fn tab(&self, item: ItemId) -> &TabSnapshot {
        self.panes.tabs.iter().find(|tab| tab.item == item).unwrap()
    }
}

#[test]
fn custom_width_and_fixed_slots_share_the_core_allocation() {
    let mut harness = Harness::new(DockPolicy::default());
    harness.settle();
    let leading = harness
        .panes
        .slots
        .iter()
        .find(|slot| slot.slot == TabBarSlot::Leading)
        .unwrap();
    let trailing = harness
        .panes
        .slots
        .iter()
        .find(|slot| slot.slot == TabBarSlot::Trailing)
        .unwrap();
    assert_eq!(leading.rect.width(), 36.0);
    assert_eq!(trailing.rect.width(), 64.0);
    for slot in &harness.panes.slots {
        assert!(slot.rect.contains_rect(slot.clip));
        assert_eq!(slot.items, [FIRST, SECOND]);
        assert!(slot.enabled);
        for tab in &harness.panes.tabs {
            assert_eq!(tab.bar, slot.bar);
            assert!(!tab.visible.intersect(slot.rect).is_positive());
        }
    }
    let original = harness.tab(FIRST).rect.width();
    harness.panes.extra_width += 32.0;
    harness.settle();
    assert_eq!(harness.tab(FIRST).rect.width(), original + 32.0);
}

#[test]
fn custom_paint_keeps_default_selection_accessibility_and_close_controls() {
    let mut harness = Harness::new(DockPolicy::default());
    harness.settle();
    let output = harness.run(Vec::new());
    assert!(output.shapes.iter().any(|shape| matches!(
        &shape.shape, egui::Shape::Rect(rect)
            if rect.fill == CUSTOM_FILL && rect.corner_radius == egui::CornerRadius::same(6)
    )));
    let point = harness.tab(SECOND).content.left_center() + vec2(4.0, 0.0);
    harness.click(point);
    assert!(harness.dock.view().item(SECOND).unwrap().is_selected());
    let output = harness.run(Vec::new());
    let nodes = &output
        .platform_output
        .accesskit_update
        .as_ref()
        .unwrap()
        .nodes;
    assert!(
        nodes
            .iter()
            .any(|(_, node)| node.role() == Role::Tab && node.label() == Some("Second"))
    );
    let close = nodes
        .iter()
        .find_map(|(_, node)| {
            (node.role() == Role::Button && node.label() == Some("Close Second"))
                .then(|| node.bounds().unwrap())
        })
        .unwrap();
    #[allow(clippy::cast_possible_truncation)]
    let point = egui::pos2(
        ((close.x0 + close.x1) * 0.5) as f32,
        ((close.y0 + close.y1) * 0.5) as f32,
    );
    harness.click(point);
    assert_eq!(harness.close_requests, 1);
}

#[test]
fn application_buttons_own_clicks_without_selecting_the_underlying_tab() {
    let mut harness = Harness::new(DockPolicy::default());
    harness.settle();
    harness.click(harness.panes.tab_button.unwrap().center());
    assert_eq!(harness.panes.tab_clicks, 1);
    assert!(harness.dock.view().item(FIRST).unwrap().is_selected());
    harness.click(harness.panes.slot_button.unwrap().center());
    assert_eq!(harness.panes.slot_clicks, 1);
    assert!(harness.dock.view().item(FIRST).unwrap().is_selected());
    assert_eq!(harness.close_requests, 0);
}

#[test]
fn ordinary_egui_menus_in_slots_keep_complete_output_settlement() {
    let mut harness = Harness::new(DockPolicy::default());
    harness.panes.slot_menu = true;
    harness.settle();
    harness.click(harness.panes.slot_button.unwrap().center());
    let output = harness.run(Vec::new());
    let texts = output
        .shapes
        .iter()
        .filter_map(|shape| match &shape.shape {
            egui::Shape::Text(text) => Some(text.galley.job.text.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert!(texts.contains(&"Custom action"), "painted text: {texts:?}");
    assert!(harness.dock.view().item(FIRST).unwrap().is_selected());
}

#[test]
fn oversubscribed_slots_shrink_without_overlapping_operable_tabs() {
    let mut harness = Harness::new(DockPolicy::default());
    harness.width = 200.0;
    harness.panes.layout = TabBarLayout {
        leading_width: 5000.0,
        trailing_width: 5000.0,
    };
    harness.settle();
    assert!(!harness.panes.slots.is_empty());
    for slot in &harness.panes.slots {
        assert!(slot.rect.left() >= 0.0 && slot.rect.right() <= harness.width);
        for tab in &harness.panes.tabs {
            assert!(!tab.visible.intersect(slot.rect).is_positive());
        }
    }
    assert!(harness.tab(FIRST).visible.width() >= harness.dock.style().tab_min_width);
    harness.width = 800.0;
    harness.panes.layout = TabBarLayout::default();
    harness.settle();
    assert!(harness.panes.slots.is_empty());
    assert_eq!(harness.panes.tabs.len(), 2);
}

#[test]
fn disabled_and_hidden_bars_apply_to_application_chrome() {
    let mut policy = DockPolicy::default();
    policy.set_tab_bar(TabBarPolicy::new(
        TabBarVisibility::Visible,
        TabBarInteraction::Disabled,
    ));
    let mut harness = Harness::new(policy);
    harness.settle();
    assert_eq!(harness.panes.tabs.len(), 2);
    assert!(harness.panes.tabs.iter().all(|tab| !tab.enabled));
    assert!(harness.panes.slots.iter().all(|slot| !slot.enabled));
    harness.click(harness.panes.slot_button.unwrap().center());
    assert_eq!(harness.panes.slot_clicks, 0);
    let mut policy = DockPolicy::default();
    policy.set_tab_bar(TabBarPolicy::new(
        TabBarVisibility::Hidden,
        TabBarInteraction::Disabled,
    ));
    harness.dock.set_policy(policy).unwrap();
    harness.settle();
    assert!(harness.panes.tabs.is_empty());
    assert!(harness.panes.slots.is_empty());
}

#[test]
fn unloaded_panes_use_default_missing_tabs_without_application_callbacks() {
    let mut harness = Harness::new(DockPolicy::default());
    harness.panes.missing = true;
    harness.settle();
    assert!(harness.panes.tabs.is_empty());
    let output = harness.run(Vec::new());
    assert!(
        output
            .platform_output
            .accesskit_update
            .as_ref()
            .unwrap()
            .nodes
            .iter()
            .any(|(_, node)| {
                node.role() == Role::Tab && node.label() == Some("Missing pane 1")
            })
    );
}

#[test]
fn fixed_slots_keep_their_identity_and_position_while_tabs_scroll() {
    let mut harness = Harness::new(DockPolicy::default());
    harness.width = 380.0;
    let mut style = harness.dock.style().clone();
    style.tab_min_width = 160.0;
    style.tab_max_width = 160.0;
    harness.dock.set_style(style).unwrap();
    harness.settle();
    let before = harness.panes.slots.clone();
    let second_before = harness.tab(SECOND).rect.left();
    let output = harness.run(Vec::new());
    let target = output
        .platform_output
        .accesskit_update
        .as_ref()
        .unwrap()
        .nodes
        .iter()
        .find_map(|(id, node)| (node.label() == Some("Scroll tabs forward")).then_some(*id))
        .unwrap();
    harness
        .run(vec![Event::AccessKitActionRequest(
            egui::accesskit::ActionRequest {
                action: egui::accesskit::Action::Click,
                target_tree: egui::accesskit::TreeId::ROOT,
                target_node: target,
                data: None,
            },
        )])
        .drop_without_applying_deltas();
    harness.settle();
    assert!(harness.tab(SECOND).rect.left() < second_before);
    for (before, after) in before.iter().zip(&harness.panes.slots) {
        assert_eq!(before.bar, after.bar);
        assert_eq!(before.slot, after.slot);
        assert_eq!(before.rect, after.rect);
    }
}

#[test]
fn invalid_application_measurements_are_rejected_at_the_host_boundary() {
    for (extra_width, leading_width) in [(f32::NAN, 0.0), (0.0, -1.0), (0.0, f32::INFINITY)] {
        let mut harness = Harness::new(DockPolicy::default());
        harness.panes.extra_width = extra_width;
        harness.panes.layout.leading_width = leading_width;
        let input = RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(800.0, 400.0))),
            ..Default::default()
        };
        assert!(
            harness
                .dock
                .run_single_surface(&harness.context, input, SURFACE, &mut harness.panes)
                .is_err()
        );
    }
}
