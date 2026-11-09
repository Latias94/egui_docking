//! Regression coverage for real tab layout, keyboard navigation, and chrome.

use dockspace::model::{
    DockspaceLayout, DockspaceNode, DockspaceRootLayout, DockspaceSurfaceLayout, ItemId, RootId,
    SurfaceId,
};
use dockspace::runtime::SurfacePresentationResult;
use egui::accesskit::{Node, NodeId, Role};
use egui::{Context, Event, FullOutput, Key, Pos2, RawInput, Rect, Ui, vec2};

use super::Dockspace;
use crate::{DockStyle, PaneResponse, PaneView};

const SURFACE: SurfaceId = SurfaceId::new(1);

struct Panes {
    titles: Vec<String>,
    text: String,
}

impl PaneView for Panes {
    fn title(&self, item: ItemId) -> Option<egui::WidgetText> {
        self.titles
            .get(item.get() as usize - 1)
            .map(|title| title.clone().into())
    }

    fn ui(&mut self, item: ItemId, ui: &mut Ui) -> PaneResponse {
        let target = self.focus_target(item).unwrap();
        let response = ui.add(egui::TextEdit::singleline(&mut self.text).id(target));
        PaneResponse::from(&response)
    }

    fn focus_target(&self, item: ItemId) -> Option<egui::Id> {
        Some(egui::Id::unique(("chrome-test-editor", item)))
    }
}

struct Harness {
    context: Context,
    dockspace: Dockspace,
    panes: Panes,
    frame: u32,
    local_style: Option<egui::Style>,
}

impl Harness {
    fn new(titles: Vec<String>, selected: ItemId, style: DockStyle) -> Self {
        let context = Context::default();
        context.enable_accesskit();
        let items = (1..=titles.len()).map(|index| ItemId::new(index as u64));
        let layout = DockspaceLayout::new([DockspaceSurfaceLayout::new(
            SURFACE,
            DockspaceRootLayout::new(
                RootId::new(1),
                DockspaceNode::central_tabs_with_selection(items, Some(selected)).unwrap(),
            ),
        )])
        .unwrap();
        Self {
            context,
            dockspace: Dockspace::builder("chrome-regression", layout)
                .style(style)
                .build()
                .unwrap(),
            panes: Panes {
                titles,
                text: String::new(),
            },
            frame: 0,
            local_style: None,
        }
    }

    fn tabs(selected: u64) -> Self {
        Self::new(
            (1..=6).map(|index| format!("Document {index}")).collect(),
            ItemId::new(selected),
            DockStyle {
                tab_min_width: 120.0,
                ..Default::default()
            },
        )
    }

    fn run(&mut self, width: f32, events: Vec<Event>) -> FullOutput {
        self.frame += 1;
        let raw = RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(width, 600.0))),
            time: Some(f64::from(self.frame) * 0.1),
            events,
            ..Default::default()
        };
        let output = if let Some(style) = &self.local_style {
            crate::run_ui(&self.context, raw, |ui| {
                ui.set_style(style.clone());
                self.dockspace
                    .show_single_surface(SURFACE, ui, &mut self.panes)
                    .unwrap();
            })
        } else {
            self.dockspace
                .run_single_surface(&self.context, raw, SURFACE, &mut self.panes)
                .unwrap()
                .0
        };
        let (mut output, receipt) = output.into_parts();
        receipt.report(SurfacePresentationResult::Presented);
        output.textures_delta.clear();
        output
    }

    fn focus_tab(&mut self, width: f32, label: &str) {
        self.focus(width, Role::Tab, label);
    }

    fn focus(&mut self, width: f32, role: Role, label: &str) {
        for _ in 0..100 {
            let output = self.run(width, vec![key(Key::Tab)]);
            let focused = self
                .context
                .memory(egui::Memory::focused)
                .map(|id| id.value());
            if nodes(&output).iter().any(|(id, node)| {
                Some(id.0) == focused && node.role() == role && node.label() == Some(label)
            }) {
                return;
            }
        }
        panic!("control {label:?} never received keyboard focus");
    }
}

fn nodes(output: &FullOutput) -> &[(NodeId, Node)] {
    &output
        .platform_output
        .accesskit_update
        .as_ref()
        .unwrap()
        .nodes
}

fn key(key: Key) -> Event {
    Event::Key {
        key,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: egui::Modifiers::NONE,
    }
}

fn texts(output: &FullOutput) -> Vec<(Rect, &egui::epaint::TextShape)> {
    fn visit<'a>(
        clip: Rect,
        shape: &'a egui::Shape,
        result: &mut Vec<(Rect, &'a egui::epaint::TextShape)>,
    ) {
        match shape {
            egui::Shape::Text(text) => result.push((clip, text)),
            egui::Shape::Vec(shapes) => {
                for shape in shapes {
                    visit(clip, shape, result);
                }
            }
            _ => {}
        }
    }
    let mut result = Vec::new();
    for clipped in &output.shapes {
        visit(clipped.clip_rect, &clipped.shape, &mut result);
    }
    result
}

fn floating_harness(style: DockStyle) -> Harness {
    use crate::{DockspaceContainedLayout, FloatingPresentationId, LogicalRect};
    let mut harness = Harness::new(
        vec!["Editor".to_owned(), "Inspector".to_owned()],
        ItemId::new(1),
        style.clone(),
    );
    let layout = DockspaceLayout::new([DockspaceSurfaceLayout::new(
        SURFACE,
        DockspaceRootLayout::new(
            RootId::new(1),
            DockspaceNode::central_tabs([ItemId::new(1)]),
        ),
    )
    .with_contained(DockspaceContainedLayout::new(
        FloatingPresentationId::new(1),
        DockspaceRootLayout::new(RootId::new(2), DockspaceNode::tabs([ItemId::new(2)])),
        LogicalRect::new(160.0, 100.0, 320.0, 300.0).unwrap(),
    ))])
    .unwrap();
    harness.dockspace = Dockspace::builder("floating-chrome-regression", layout)
        .style(style)
        .build()
        .unwrap();
    harness
}

#[test]
fn floating_resize_edge_paints_keyboard_focus_and_adjusts_the_same_edge() {
    let focus_color = egui::Color32::from_rgb(29, 213, 117);
    let mut style = DockStyle::default();
    style.visuals.splitter_hover_color = Some(focus_color);
    let mut harness = floating_harness(style);
    for _ in 0..3 {
        harness.run(640.0, Vec::new());
    }
    harness.focus(640.0, Role::Splitter, "Resize floating right edge");
    let before = harness.run(640.0, Vec::new());
    let edge = nodes(&before)
        .iter()
        .find(|(_, node)| node.label() == Some("Resize floating right edge"))
        .unwrap();
    let right_before = edge.1.bounds().unwrap().x1;
    assert_eq!(
        harness
            .context
            .memory(egui::Memory::focused)
            .map(|id| id.value()),
        Some(edge.0.0)
    );
    assert!(before.shapes.iter().any(|shape| matches!(&shape.shape, egui::Shape::Rect(rect) if rect.fill == focus_color && rect.rect.is_positive())));
    harness.run(640.0, vec![key(Key::ArrowRight)]);
    let after = harness.run(640.0, Vec::new());
    let edge_after = nodes(&after)
        .iter()
        .find(|(_, node)| node.label() == Some("Resize floating right edge"))
        .unwrap();
    assert_eq!(edge_after.1.bounds().unwrap().x1 - right_before, 16.0);
    assert_eq!(
        harness
            .context
            .memory(egui::Memory::focused)
            .map(|id| id.value()),
        Some(edge_after.0.0)
    );
}

#[test]
fn egui_derived_geometry_keeps_large_tab_and_floating_title_glyphs_inside_their_clips() {
    for override_font in [false, true] {
        let mut harness = floating_harness(DockStyle::default());
        harness.context.global_style_mut(|style| {
            if override_font {
                style.override_font_id = Some(egui::FontId::proportional(36.0));
            } else {
                style
                    .text_styles
                    .get_mut(&egui::TextStyle::Button)
                    .unwrap()
                    .size = 36.0;
            }
            style.spacing.interact_size.y = 40.0;
        });
        let mut derived = None;
        harness
            .context
            .run_ui(RawInput::default(), |ui| {
                derived = Some(DockStyle::from_egui(ui))
            })
            .drop_without_applying_deltas();
        let derived = derived.unwrap();
        derived.validate().unwrap();
        assert!(derived.tab_bar_height >= 40.0);
        harness.dockspace.set_style(derived).unwrap();
        for _ in 0..3 {
            harness.run(640.0, Vec::new());
        }
        let output = harness.run(640.0, Vec::new());
        let titles = texts(&output)
            .into_iter()
            .filter(|(_, text)| text.galley.job.text == "Inspector")
            .collect::<Vec<_>>();
        assert_eq!(
            titles.len(),
            2,
            "the floating title and its tab must both paint"
        );
        for (clip, text) in titles {
            assert!(
                clip.expand(1.0).contains_rect(text.visual_bounding_rect()),
                "override={override_font}: {:?} outside {clip:?}",
                text.visual_bounding_rect()
            );
        }
    }
}

#[test]
fn long_tab_ellipsis_fits_the_final_allocation_at_each_ui_zoom() {
    let title = "M".repeat(48);
    for zoom in [1.0, 1.5] {
        let mut harness = Harness::new(
            vec![title.clone(), "Other".to_owned()],
            ItemId::new(1),
            DockStyle::default(),
        );
        harness.context.set_zoom_factor(zoom);
        for _ in 0..3 {
            harness.run(1000.0, Vec::new());
        }
        let output = harness.run(1000.0, Vec::new());
        let (clip, text) = texts(&output)
            .into_iter()
            .find(|(_, text)| text.galley.job.text == title)
            .unwrap();
        assert!(text.galley.elided);
        let glyph = text.galley.rows[0].glyphs.last().unwrap();
        assert_eq!(glyph.chr, '…');
        assert!(text.pos.x + glyph.max_x() <= clip.right() + 0.5);
        assert!(clip.expand(1.0).contains_rect(text.visual_bounding_rect()));
    }
}

#[test]
fn overflow_rows_use_full_titles_and_native_popup_theme() {
    let title = "Document with a descriptive title and a unique suffix.rs".to_owned();
    let mut harness = Harness::new(
        vec![title.clone(), "Other".to_owned()],
        ItemId::new(1),
        DockStyle {
            tab_min_width: 400.0,
            tab_max_width: 400.0,
            ..Default::default()
        },
    );
    let theme_color = egui::Color32::from_rgb(180, 30, 130);
    let border_color = egui::Color32::from_rgb(50, 140, 190);
    let mut theme = (*harness.context.global_style()).clone();
    theme.visuals.override_text_color = Some(theme_color);
    theme.visuals.menu_corner_radius = egui::CornerRadius::same(12);
    theme.visuals.window_stroke = egui::Stroke::new(3.0, border_color);
    theme.spacing.button_padding.y = 6.0;
    harness.local_style = Some(theme.clone());
    let mut output = harness.run(640.0, Vec::new());
    for _ in 0..3 {
        output = harness.run(640.0, Vec::new());
    }
    let trigger = nodes(&output)
        .iter()
        .find(|(_, node)| node.label() == Some("Show hidden tabs"))
        .unwrap()
        .0;
    harness.run(
        640.0,
        vec![Event::AccessKitActionRequest(
            egui::accesskit::ActionRequest {
                action: egui::accesskit::Action::Click,
                target_tree: egui::accesskit::TreeId::ROOT,
                target_node: trigger,
                data: None,
            },
        )],
    );
    for _ in 0..3 {
        output = harness.run(640.0, Vec::new());
    }
    let menu = nodes(&output)
        .iter()
        .find(|(_, node)| node.role() == Role::Menu)
        .unwrap()
        .1
        .bounds()
        .unwrap();
    let row = nodes(&output)
        .iter()
        .find(|(_, node)| node.role() == Role::MenuItem && node.label() == Some(title.as_str()))
        .unwrap()
        .1
        .bounds()
        .unwrap();
    assert_eq!(
        row.x0 - menu.x0,
        f64::from(theme.spacing.menu_margin.left) + 3.0
    );
    assert_eq!(
        row.y0 - menu.y0,
        f64::from(theme.spacing.menu_margin.top) + 3.0
    );
    let (_, text) = texts(&output)
        .into_iter()
        .find(|(_, text)| text.galley.job.text == title && text.pos.y > 28.0)
        .unwrap();
    assert!(
        !text.galley.elided,
        "the menu has room for the complete title"
    );
    assert!(text.galley.size().x > 220.0);
    assert_eq!(text.override_text_color, Some(theme_color));
    fn has_frame(shape: &egui::Shape, border: egui::Color32) -> bool {
        match shape {
            egui::Shape::Rect(rect) => {
                rect.stroke.color == border
                    && rect.stroke.width == 3.0
                    && rect.corner_radius == egui::CornerRadius::same(12)
            }
            egui::Shape::Vec(shapes) => shapes.iter().any(|shape| has_frame(shape, border)),
            _ => false,
        }
    }
    assert!(
        output
            .shapes
            .iter()
            .any(|shape| has_frame(&shape.shape, border_color))
    );
}

#[test]
fn keyboard_navigation_reveals_hidden_targets_and_preserves_tab_focus() {
    let mut harness = Harness::tabs(1);
    for _ in 0..3 {
        harness.run(360.0, Vec::new());
    }
    harness.focus_tab(360.0, "Document 1");
    for (navigation, destination) in [
        (Key::End, 6),
        (Key::Home, 1),
        (Key::ArrowLeft, 6),
        (Key::ArrowRight, 1),
    ] {
        let title = format!("Document {destination}");
        harness.run(360.0, vec![key(navigation)]);
        for _ in 0..3 {
            let output = harness.run(360.0, Vec::new());
            let (id, node) = nodes(&output)
                .iter()
                .find(|(_, node)| node.role() == Role::Tab && node.label() == Some(title.as_str()))
                .expect("navigation must reveal its target");
            assert_eq!(node.is_selected(), Some(true), "{navigation:?}");
            assert_eq!(
                harness
                    .context
                    .memory(egui::Memory::focused)
                    .map(|id| id.value()),
                Some(id.0),
                "{navigation:?}"
            );
            assert!(
                harness
                    .dockspace
                    .view()
                    .item(ItemId::new(destination))
                    .unwrap()
                    .is_selected()
            );
        }
    }
    harness.run(360.0, vec![key(Key::Enter)]);
    for _ in 0..3 {
        harness.run(360.0, Vec::new());
    }
    assert_eq!(
        harness.context.memory(egui::Memory::focused),
        harness.panes.focus_target(ItemId::new(1))
    );
    harness.run(
        360.0,
        vec![Event::Text("typed after navigation".to_owned())],
    );
    assert_eq!(harness.panes.text, "typed after navigation");
}

#[test]
fn shrinking_a_strip_reveals_selection_without_overriding_later_manual_scroll() {
    let mut harness = Harness::tabs(6);
    for _ in 0..3 {
        harness.run(1000.0, Vec::new());
    }
    let output = harness.run(360.0, Vec::new());
    assert!(
        nodes(&output)
            .iter()
            .any(|(_, node)| node.role() == Role::Tab
                && node.label() == Some("Document 6")
                && node.is_selected() == Some(true))
    );

    let backward = nodes(&output)
        .iter()
        .find(|(_, node)| node.label() == Some("Scroll tabs backward"))
        .unwrap()
        .0;
    for _ in 0..4 {
        harness.run(
            360.0,
            vec![Event::AccessKitActionRequest(
                egui::accesskit::ActionRequest {
                    action: egui::accesskit::Action::Click,
                    target_tree: egui::accesskit::TreeId::ROOT,
                    target_node: backward,
                    data: None,
                },
            )],
        );
        harness.run(360.0, Vec::new());
    }
    for _ in 0..3 {
        let output = harness.run(360.0, Vec::new());
        assert!(
            !nodes(&output)
                .iter()
                .any(|(_, node)| node.role() == Role::Tab && node.label() == Some("Document 6"))
        );
        assert!(
            harness
                .dockspace
                .view()
                .item(ItemId::new(6))
                .unwrap()
                .is_selected()
        );
    }
    harness.focus_tab(360.0, "Document 1");
    harness.run(360.0, vec![key(Key::End)]);
    for _ in 0..3 {
        let output = harness.run(360.0, Vec::new());
        let (id, _) = nodes(&output)
            .iter()
            .find(|(_, node)| node.role() == Role::Tab && node.label() == Some("Document 6"))
            .expect("navigating back to the unchanged selection must reveal it");
        assert_eq!(
            harness
                .context
                .memory(egui::Memory::focused)
                .map(|id| id.value()),
            Some(id.0)
        );
    }
}

#[test]
fn explicit_tab_text_color_wins_over_global_color_during_keyboard_focus() {
    let mut style = DockStyle::default();
    let color = egui::Color32::from_rgb(20, 230, 50);
    style.visuals.tab_text_color = Some(color);
    let mut harness = Harness::new(
        vec!["First".to_owned(), "Second".to_owned()],
        ItemId::new(1),
        style,
    );
    harness.context.global_style_mut(|style| {
        style.visuals.override_text_color = Some(egui::Color32::from_rgb(180, 30, 130))
    });
    for _ in 0..3 {
        harness.run(640.0, Vec::new());
    }
    harness.focus_tab(640.0, "Second");
    let output = harness.run(640.0, Vec::new());
    let (_, text) = texts(&output)
        .into_iter()
        .find(|(_, text)| text.galley.job.text == "Second")
        .unwrap();
    assert_eq!(text.override_text_color, Some(color));
}
