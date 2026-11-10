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
        self.run_input(raw)
    }

    fn run_input(&mut self, raw: RawInput) -> FullOutput {
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

fn pane_focus_cues(output: &FullOutput, color: egui::Color32) -> Vec<Rect> {
    fn visit(shape: &egui::Shape, color: egui::Color32, cues: &mut Vec<Rect>) {
        match shape {
            egui::Shape::Rect(rect) if rect.fill == color && rect.rect.height() == 2.0 => {
                cues.push(rect.rect);
            }
            egui::Shape::Vec(shapes) => {
                for shape in shapes {
                    visit(shape, color, cues);
                }
            }
            _ => {}
        }
    }
    let mut cues = Vec::new();
    for clipped in &output.shapes {
        visit(&clipped.shape, color, &mut cues);
    }
    cues
}

#[test]
fn pane_focus_cue_tracks_content_across_split_and_floating_groups_without_changing_selection() {
    use crate::{DockspaceAxis, DockspaceContainedLayout, FloatingPresentationId, LogicalRect};

    let color = egui::Color32::from_rgb(17, 192, 123);
    let mut style = DockStyle::default();
    style.visuals.pane_focus_color = Some(color);
    let mut harness = Harness::new(
        ["Editor", "Hidden", "Outline", "Inspector"]
            .map(str::to_owned)
            .to_vec(),
        ItemId::new(1),
        style.clone(),
    );
    let layout = DockspaceLayout::new([DockspaceSurfaceLayout::new(
        SURFACE,
        DockspaceRootLayout::new(
            RootId::new(1),
            DockspaceNode::split(
                DockspaceAxis::Horizontal,
                [
                    (DockspaceNode::tabs([ItemId::new(1), ItemId::new(2)]), 0.5),
                    (DockspaceNode::tabs([ItemId::new(3)]), 0.5),
                ],
            )
            .unwrap(),
        ),
    )
    .with_contained(DockspaceContainedLayout::new(
        FloatingPresentationId::new(1),
        DockspaceRootLayout::new(RootId::new(2), DockspaceNode::tabs([ItemId::new(4)])),
        LogicalRect::new(160.0, 200.0, 320.0, 250.0).unwrap(),
    ))])
    .unwrap();
    harness.dockspace = Dockspace::builder("split-pane-focus-regression", layout)
        .style(style)
        .build()
        .unwrap();
    for _ in 0..3 {
        let output = harness.run(900.0, Vec::new());
        assert!(pane_focus_cues(&output, color).is_empty());
    }
    let selected = |harness: &Harness| {
        (1..=4)
            .filter(|item| {
                harness
                    .dockspace
                    .view()
                    .item(ItemId::new(*item))
                    .unwrap()
                    .is_selected()
            })
            .collect::<Vec<_>>()
    };
    assert_eq!(selected(&harness), [1, 3, 4]);
    for (item, title) in [(1, "Editor"), (3, "Outline"), (4, "Inspector")] {
        let target = harness.panes.focus_target(ItemId::new(item)).unwrap();
        harness
            .context
            .memory_mut(|memory| memory.request_focus(target));
        for _ in 0..2 {
            let output = harness.run(900.0, Vec::new());
            let cues = pane_focus_cues(&output, color);
            assert_eq!(cues.len(), 1, "{title} must be the only focused pane");
            let tab = nodes(&output)
                .iter()
                .find(|(_, node)| node.role() == Role::Tab && node.label() == Some(title))
                .unwrap()
                .1
                .bounds()
                .unwrap();
            assert_eq!(f64::from(cues[0].bottom()), tab.y1);
            assert!(f64::from(cues[0].left()) <= tab.x0);
            assert!(f64::from(cues[0].right()) >= tab.x1);
            assert_eq!(selected(&harness), [1, 3, 4]);
            assert_eq!(harness.context.memory(egui::Memory::focused), Some(target));
        }
    }
    harness.focus_tab(900.0, "Editor");
    let output = harness.run(900.0, Vec::new());
    assert!(
        pane_focus_cues(&output, color).is_empty(),
        "tab focus is separate from content focus"
    );
    assert_eq!(selected(&harness), [1, 3, 4]);
}

#[test]
fn pane_focus_cue_follows_local_theme_and_clears_on_blur_or_unknown_focus() {
    let color = egui::Color32::from_rgb(29, 157, 211);
    let mut harness = floating_harness(DockStyle::default());
    let mut theme = (*harness.context.global_style()).clone();
    theme.visuals.selection.stroke.color = color;
    harness.local_style = Some(theme);
    for _ in 0..3 {
        harness.run(640.0, Vec::new());
    }
    let target = harness.panes.focus_target(ItemId::new(1)).unwrap();
    harness
        .context
        .memory_mut(|memory| memory.request_focus(target));
    let output = harness.run(640.0, Vec::new());
    assert_eq!(pane_focus_cues(&output, color).len(), 1);

    let mut blurred = RawInput {
        screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(640.0, 600.0))),
        time: Some(f64::from(harness.frame) * 0.1 + 0.05),
        focused: false,
        ..Default::default()
    };
    blurred
        .viewports
        .entry(egui::ViewportId::ROOT)
        .or_default()
        .focused = Some(false);
    let output = harness.run_input(blurred);
    assert!(pane_focus_cues(&output, color).is_empty());

    harness
        .context
        .memory_mut(|memory| memory.request_focus(egui::Id::unique("unknown-focused-widget")));
    let output = harness.run(640.0, Vec::new());
    assert!(
        pane_focus_cues(&output, color).is_empty(),
        "unregistered focus cannot revive pane history"
    );

    harness
        .context
        .memory_mut(|memory| memory.request_focus(target));
    let output = harness.run(640.0, Vec::new());
    assert_eq!(pane_focus_cues(&output, color).len(), 1);
    let mut style = DockStyle::default();
    style.visuals.pane_focus_color = Some(egui::Color32::TRANSPARENT);
    harness.dockspace.set_style(style).unwrap();
    let output = harness.run(640.0, Vec::new());
    assert!(pane_focus_cues(&output, color).is_empty());
    assert_eq!(harness.context.memory(egui::Memory::focused), Some(target));
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

struct OverflowNavigationLateDiscard {
    remaining: usize,
}

impl egui::plugin::Plugin for OverflowNavigationLateDiscard {
    fn debug_name(&self) -> &'static str {
        "product_chrome_tests::overflow_navigation_late_discard"
    }

    fn output_hook(&mut self, _context: &Context, output: &mut FullOutput) {
        if self.remaining > 0 {
            self.remaining -= 1;
            output
                .platform_output
                .request_discard_reasons
                .push(egui::RepaintCause::new_reason(
                    "retry overflow keyboard navigation after its output hook",
                ));
        }
    }
}

fn overflow_menu_with_first_row_focused() -> (Harness, f32, NodeId) {
    let mut harness = Harness::new(
        vec!["A".to_owned(), "B".to_owned()],
        ItemId::new(2),
        DockStyle {
            tab_min_width: 400.0,
            tab_max_width: 400.0,
            ..Default::default()
        },
    );
    // Keep room for one minimum-width tab and the optional overflow controls.
    let width = 640.0;
    let mut output = harness.run(width, Vec::new());
    for _ in 0..3 {
        output = harness.run(width, Vec::new());
    }
    let trigger = nodes(&output)
        .iter()
        .find(|(_, node)| node.label() == Some("Show hidden tabs"))
        .expect("the 640-point fixture keeps the overflow trigger beside a minimum-width tab")
        .0;
    harness.run(
        width,
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
        harness.run(width, Vec::new());
    }
    harness.run(width, vec![key(Key::Home)]);
    for _ in 0..3 {
        output = harness.run(width, Vec::new());
    }
    let first_row = nodes(&output)
        .iter()
        .find(|(_, node)| node.role() == Role::MenuItem && node.label() == Some("A"))
        .unwrap()
        .0;
    assert_eq!(
        harness
            .context
            .memory(egui::Memory::focused)
            .map(|id| id.value()),
        Some(first_row.0),
        "Home must establish the first menu row before testing keyboard traversal",
    );
    assert!(
        harness
            .dockspace
            .view()
            .item(ItemId::new(2))
            .unwrap()
            .is_selected()
    );
    (harness, width, first_row)
}

fn assert_overflow_arrow_up_then_enter(discarded_passes: usize) {
    let (mut harness, width, first_row) = overflow_menu_with_first_row_focused();
    harness.context.options_mut(|options| {
        options.max_passes = std::num::NonZeroUsize::new(discarded_passes + 1).unwrap();
    });
    harness.context.add_plugin(OverflowNavigationLateDiscard {
        remaining: discarded_passes,
    });
    let arrow = harness.run(width, vec![key(Key::ArrowUp)]);
    assert_eq!(
        arrow.platform_output.num_completed_passes,
        discarded_passes + 1
    );
    let focus_after_arrow = harness
        .context
        .memory(egui::Memory::focused)
        .map(|id| id.value());

    harness
        .context
        .with_plugin(|plugin: &mut OverflowNavigationLateDiscard| {
            plugin.remaining = discarded_passes;
        })
        .unwrap();
    let enter = harness.run(width, vec![key(Key::Enter)]);
    assert_eq!(
        enter.platform_output.num_completed_passes,
        discarded_passes + 1
    );
    let mut output = harness.run(width, Vec::new());
    for _ in 0..2 {
        output = harness.run(width, Vec::new());
    }
    assert_eq!(
        (
            focus_after_arrow,
            harness
                .dockspace
                .view()
                .item(ItemId::new(1))
                .unwrap()
                .is_selected(),
            harness.context.memory(egui::Memory::focused),
        ),
        (
            Some(first_row.0),
            true,
            harness.panes.focus_target(ItemId::new(1)),
        ),
        "ArrowUp must stay on the first row and the immediately following Enter must select its content, discarded_passes={discarded_passes}",
    );
    assert!(
        nodes(&output)
            .iter()
            .all(|(_, node)| node.role() != Role::Menu)
    );
}

#[test]
fn overflow_arrow_up_then_enter_keeps_the_first_menu_row() {
    assert_overflow_arrow_up_then_enter(0);
}

#[test]
fn overflow_arrow_up_then_enter_keeps_the_first_menu_row_across_late_discards() {
    assert_overflow_arrow_up_then_enter(2);
}

fn assert_overflow_tab_then_enter(discarded_passes: usize, reverse: bool) {
    let (mut harness, width, _) = overflow_menu_with_first_row_focused();
    if reverse {
        let output = harness.run(width, vec![key(Key::Tab)]);
        let second_row = nodes(&output)
            .iter()
            .find(|(_, node)| node.role() == Role::MenuItem && node.label() == Some("B"))
            .unwrap()
            .0;
        assert_eq!(
            harness
                .context
                .memory(egui::Memory::focused)
                .map(|id| id.value()),
            Some(second_row.0),
            "Shift+Tab must start from the second row reached by real Tab traversal"
        );
    }
    harness.context.options_mut(|options| {
        options.max_passes = std::num::NonZeroUsize::new(discarded_passes + 1).unwrap();
    });
    harness.context.add_plugin(OverflowNavigationLateDiscard {
        remaining: discarded_passes,
    });
    let traversal = harness.run(
        width,
        vec![Event::Key {
            key: Key::Tab,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers {
                shift: reverse,
                ..Default::default()
            },
        }],
    );
    assert_eq!(
        traversal.platform_output.num_completed_passes,
        discarded_passes + 1
    );
    if !reverse {
        let second_row = nodes(&traversal)
            .iter()
            .find(|(_, node)| node.role() == Role::MenuItem && node.label() == Some("B"))
            .unwrap()
            .0;
        assert_eq!(
            harness
                .context
                .memory(egui::Memory::focused)
                .map(|id| id.value()),
            Some(second_row.0),
            "Tab must reach B without a discarded pass restoring A, discarded_passes={discarded_passes}"
        );
    }
    assert!(
        harness
            .dockspace
            .view()
            .item(ItemId::new(2))
            .unwrap()
            .is_selected(),
        "Tab traversal must not activate a row, reverse={reverse}, discarded_passes={discarded_passes}"
    );

    harness
        .context
        .with_plugin(|plugin: &mut OverflowNavigationLateDiscard| {
            plugin.remaining = discarded_passes;
        })
        .unwrap();
    // Enter follows immediately. egui applies Shift+Tab's deferred destination
    // at its start; no idle pass may silently restore the former row.
    let enter = harness.run(width, vec![key(Key::Enter)]);
    assert_eq!(
        enter.platform_output.num_completed_passes,
        discarded_passes + 1
    );
    for _ in 0..3 {
        harness.run(width, Vec::new());
    }
    let expected = ItemId::new(if reverse { 1 } else { 2 });
    assert!(
        harness
            .dockspace
            .view()
            .item(expected)
            .unwrap()
            .is_selected(),
        "Enter must activate the row reached by Tab traversal, reverse={reverse}, discarded_passes={discarded_passes}"
    );
    assert_eq!(
        harness.context.memory(egui::Memory::focused),
        harness.panes.focus_target(expected),
        "the activated row must focus its content"
    );
}

#[test]
fn overflow_tab_then_enter_activates_the_terminal_framework_target() {
    assert_overflow_tab_then_enter(0, false);
}

#[test]
fn overflow_tab_then_enter_activates_the_terminal_framework_target_across_late_discards() {
    assert_overflow_tab_then_enter(2, false);
}

#[test]
fn overflow_shift_tab_then_enter_activates_the_terminal_framework_target() {
    assert_overflow_tab_then_enter(0, true);
}

#[test]
fn overflow_shift_tab_then_enter_activates_the_terminal_framework_target_across_late_discards() {
    assert_overflow_tab_then_enter(2, true);
}

fn overflow_with_external_buttons(
    harness: &mut Harness,
    width: f32,
    events: Vec<Event>,
    clicks: &mut [usize; 2],
) -> FullOutput {
    overflow_with_external_buttons_and_focus(harness, width, events, clicks, None)
}

fn overflow_with_external_buttons_and_focus(
    harness: &mut Harness,
    width: f32,
    events: Vec<Event>,
    clicks: &mut [usize; 2],
    request_focus: Option<usize>,
) -> FullOutput {
    let Harness {
        context,
        dockspace,
        panes,
        frame,
        ..
    } = harness;
    *frame += 1;
    let raw = RawInput {
        screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(width, 600.0))),
        time: Some(f64::from(*frame) * 0.1),
        events,
        ..Default::default()
    };
    let output = crate::run_ui(context, raw, |ui| {
        dockspace.show_single_surface(SURFACE, ui, panes).unwrap();
        for (index, (label, top)) in [("External first", 500.0), ("External second", 540.0)]
            .into_iter()
            .enumerate()
        {
            let response = ui.put(
                Rect::from_min_size(egui::pos2(width - 160.0, top), vec2(140.0, 28.0)),
                egui::Button::new(label),
            );
            if response.clicked() {
                clicks[index] += 1;
            }
            if request_focus == Some(index) {
                response.request_focus();
            }
        }
    });
    let (mut output, receipt) = output.into_parts();
    output.textures_delta.clear();
    receipt.report(SurfacePresentationResult::Presented);
    output
}

#[test]
fn overflow_consecutive_tabs_leave_for_real_buttons_without_activating_the_old_row() {
    for discarded_passes in [0, 2] {
        let (mut harness, width, _) = overflow_menu_with_first_row_focused();
        let mut clicks = [0, 0];
        overflow_with_external_buttons(&mut harness, width, Vec::new(), &mut clicks);
        harness.context.options_mut(|options| {
            options.max_passes = std::num::NonZeroUsize::new(discarded_passes + 1).unwrap();
        });
        harness
            .context
            .add_plugin(OverflowNavigationLateDiscard { remaining: 0 });
        for (role, label) in [
            (Role::MenuItem, "B"),
            (Role::Button, "External first"),
            (Role::Button, "External second"),
        ] {
            harness
                .context
                .with_plugin(|plugin: &mut OverflowNavigationLateDiscard| {
                    plugin.remaining = discarded_passes;
                })
                .unwrap();
            let output = overflow_with_external_buttons(
                &mut harness,
                width,
                vec![key(Key::Tab)],
                &mut clicks,
            );
            assert_eq!(
                output.platform_output.num_completed_passes,
                discarded_passes + 1
            );
            let target = nodes(&output)
                .iter()
                .find(|(_, node)| node.role() == role && node.label() == Some(label))
                .unwrap()
                .0;
            assert_eq!(
                harness
                    .context
                    .memory(egui::Memory::focused)
                    .map(|id| id.value()),
                Some(target.0),
                "consecutive Tab must reach {label}, discarded_passes={discarded_passes}"
            );
        }
        // A new core focus edge for B must not pull focus back from the buttons.
        overflow_with_external_buttons(&mut harness, width, vec![key(Key::Enter)], &mut clicks);
        assert_eq!(clicks, [0, 1]);
        for _ in 0..3 {
            overflow_with_external_buttons(&mut harness, width, Vec::new(), &mut clicks);
        }
        assert!(
            harness
                .dockspace
                .view()
                .item(ItemId::new(2))
                .unwrap()
                .is_selected()
        );
        assert!(
            !harness
                .dockspace
                .view()
                .item(ItemId::new(1))
                .unwrap()
                .is_selected()
        );
    }
}

#[test]
fn overflow_focus_confirmation_preserves_a_new_accesskit_focus_request() {
    for discarded_passes in [0, 2] {
        let (mut harness, width, _) = overflow_menu_with_first_row_focused();
        let mut clicks = [0, 0];
        let output = overflow_with_external_buttons(&mut harness, width, Vec::new(), &mut clicks);
        let external = nodes(&output)
            .iter()
            .find(|(_, node)| node.role() == Role::Button && node.label() == Some("External first"))
            .unwrap()
            .0;
        overflow_with_external_buttons(&mut harness, width, vec![key(Key::Tab)], &mut clicks);
        harness.context.options_mut(|options| {
            options.max_passes = std::num::NonZeroUsize::new(discarded_passes + 1).unwrap();
        });
        harness.context.add_plugin(OverflowNavigationLateDiscard {
            remaining: discarded_passes,
        });
        let output = overflow_with_external_buttons(
            &mut harness,
            width,
            vec![Event::AccessKitActionRequest(
                egui::accesskit::ActionRequest {
                    action: egui::accesskit::Action::Focus,
                    target_tree: egui::accesskit::TreeId::ROOT,
                    target_node: external,
                    data: None,
                },
            )],
            &mut clicks,
        );
        assert_eq!(
            output.platform_output.num_completed_passes,
            discarded_passes + 1
        );
        assert_eq!(
            harness
                .context
                .memory(egui::Memory::focused)
                .map(|id| id.value()),
            Some(external.0),
            "the old core confirmation must preserve the new AccessKit focus, discarded_passes={discarded_passes}"
        );
        overflow_with_external_buttons(&mut harness, width, vec![key(Key::Enter)], &mut clicks);
        assert_eq!(clicks, [1, 0]);
        assert!(
            harness
                .dockspace
                .view()
                .item(ItemId::new(2))
                .unwrap()
                .is_selected()
        );
    }
}

#[test]
fn overflow_focus_confirmation_preserves_before_docking_application_focus() {
    for discarded_passes in [0, 2] {
        let (mut harness, width, _) = overflow_menu_with_first_row_focused();
        harness.run(width, vec![key(Key::Tab)]);
        harness.context.options_mut(|options| {
            options.max_passes = std::num::NonZeroUsize::new(discarded_passes + 1).unwrap();
        });
        harness.context.add_plugin(OverflowNavigationLateDiscard {
            remaining: discarded_passes,
        });
        harness.frame += 1;
        let mut request_focus = true;
        let mut external = None;
        let (output, _) = harness
            .dockspace
            .run_single_surface_with_ui(
                &harness.context,
                RawInput {
                    screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(width, 600.0))),
                    time: Some(f64::from(harness.frame) * 0.1),
                    ..Default::default()
                },
                SURFACE,
                &mut harness.panes,
                |ui| {
                    let response = ui.put(
                        Rect::from_min_size(egui::pos2(width - 160.0, 540.0), vec2(140.0, 28.0)),
                        egui::Button::new("Application focus"),
                    );
                    external = Some(response.id);
                    if request_focus {
                        response.request_focus();
                        request_focus = false;
                    }
                },
            )
            .unwrap();
        let (mut output, receipt) = output.into_parts();
        receipt.report(SurfacePresentationResult::Presented);
        output.textures_delta.clear();
        assert_eq!(
            output.platform_output.num_completed_passes,
            discarded_passes + 1
        );
        assert_eq!(
            harness.context.memory(egui::Memory::focused),
            external,
            "core acknowledgement must preserve a new before_docking focus request, discarded_passes={discarded_passes}"
        );
    }
}

#[test]
fn overflow_focus_confirmation_preserves_application_focus_from_the_previous_run() {
    for discarded_passes in [0, 2] {
        let (mut harness, width, _) = overflow_menu_with_first_row_focused();
        let mut clicks = [0, 0];
        overflow_with_external_buttons(&mut harness, width, Vec::new(), &mut clicks);
        let output = overflow_with_external_buttons_and_focus(
            &mut harness,
            width,
            vec![key(Key::Tab)],
            &mut clicks,
            Some(0),
        );
        let external = nodes(&output)
            .iter()
            .find(|(_, node)| node.role() == Role::Button && node.label() == Some("External first"))
            .unwrap()
            .0;
        harness.context.options_mut(|options| {
            options.max_passes = std::num::NonZeroUsize::new(discarded_passes + 1).unwrap();
        });
        harness.context.add_plugin(OverflowNavigationLateDiscard {
            remaining: discarded_passes,
        });
        let output = overflow_with_external_buttons(&mut harness, width, Vec::new(), &mut clicks);
        assert_eq!(
            output.platform_output.num_completed_passes,
            discarded_passes + 1
        );
        assert_eq!(
            harness
                .context
                .memory(egui::Memory::focused)
                .map(|id| id.value()),
            Some(external.0),
            "core confirmation must not reclaim focus moved after docking, discarded_passes={discarded_passes}"
        );
        overflow_with_external_buttons(&mut harness, width, vec![key(Key::Enter)], &mut clicks);
        assert_eq!(clicks, [1, 0]);
    }
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
