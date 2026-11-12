//! Immediate software painting of the core's ordered surface program.

use std::convert::Infallible;

use dockspace::geometry::LogicalRect;
use dockspace::model::ItemId;
use dockspace::runtime::{
    ContainedPaintRecord, DockspaceDragPreview, DockspaceDropDirection, DockspaceDropEligibility,
    DockspacePreviewVisual, PanePaintRecord, SurfacePaintPlan,
    SurfacePaintProgramOperation as Operation, SurfacePaintProgramRoot,
    SurfacePaintProgramStep as Step, SurfacePaintProgramVisitError, SurfaceProgramExecutionReceipt,
    TabListMenuPaintRecord, TabStripControlKind,
};
use font8x8::{BASIC_FONTS, UnicodeFonts};

const BACKGROUND: u32 = 0x0017_202d;
const PANEL: u32 = 0x0020_2c3c;
const BAR: u32 = 0x0029_374a;
const LINE: u32 = 0x0049_5b73;
const TEXT: u32 = 0x00e4_edf7;
const MUTED: u32 = 0x0094_a8be;
const BLUE: u32 = 0x0078_b6ff;

// Logical half-open rectangles; the rasterizer intersects them with the surface clip.
type Rect = [f64; 4];

pub(super) fn title(item: ItemId) -> &'static str {
    match item.get() {
        1 => "Document",
        2 => "Outline",
        3 => "Preview",
        4 => "Console",
        5 => "Inspector",
        _ => "Pane",
    }
}

pub(super) fn render(
    plan: SurfacePaintPlan<'_>,
    pixels: &mut [u32],
    size: [u32; 2],
    pixels_per_point: f64,
) -> Result<SurfaceProgramExecutionReceipt, SurfacePaintProgramVisitError<Infallible>> {
    pixels.fill(BACKGROUND);
    let mut paint = Pixels {
        pixels,
        size,
        scale: pixels_per_point,
        clip: rect(plan.bounds()),
    };
    plan.execute_surface_program(|step| {
        match step {
            Step::RootBegin {
                root,
                paint: disposition,
            } => {
                if !disposition.is_source_gap()
                    && let Some(frame) = root.contained()
                {
                    paint.fill(rect(frame.outer_bounds()), PANEL);
                    paint.border(rect(frame.outer_bounds()), BLUE);
                }
            }
            Step::Operation {
                root,
                operation,
                paint: disposition,
            } => {
                // Source gaps retain their program identity without emitting pixels.
                if !disposition.is_source_gap() {
                    paint_operation(&mut paint, root, operation);
                }
            }
            Step::PopupBackdrop(backdrop) => paint.tint(rect(backdrop.bounds()), 0, 36),
            Step::PopupMenu {
                menu,
                paint: disposition,
            } => {
                if !disposition.is_source_gap() {
                    paint_menu(&mut paint, menu);
                }
            }
            Step::DragPreview(preview) => paint_preview(&mut paint, plan, preview),
            Step::DropGuide { affordance, .. } => {
                // Structural guides are an inventory; only payload affordances are visible.
                if let Some(cluster) = affordance {
                    for target in cluster.targets() {
                        paint_guide(
                            &mut paint,
                            rect(target.draw_bounds()),
                            target.direction(),
                            target.is_active(),
                            target.eligibility() == DockspaceDropEligibility::Eligible,
                        );
                    }
                }
            }
            Step::ContainedTransformPreview(preview) => {
                if preview.surface() == plan.surface() && !preview.is_layout_projected() {
                    paint.tint(rect(preview.rect()), BLUE, 36);
                    paint.border(rect(preview.rect()), BLUE);
                }
            }
            Step::DragDecoration(source) => {
                if !source.source_is_projected() {
                    let bounds = rect(
                        source
                            .projected_root_bounds()
                            .unwrap_or(source.source_bounds()),
                    );
                    paint.tint(bounds, BLUE, 44);
                    paint.border(bounds, BLUE);
                    if let Some(item) = source.ghost_item() {
                        paint.label(inset(bounds, 6.0), bounds, title(item), TEXT);
                    }
                }
            }
            // These steps delimit the output or publish non-visual input/feedback facts.
            Step::RootEnd(_)
            | Step::DropTarget(_)
            | Step::InteractionFeedback
            | Step::SurfaceEnd { .. } => {}
        }
        Ok::<(), Infallible>(())
    })
}

fn paint_operation(
    paint: &mut Pixels<'_>,
    root: SurfacePaintProgramRoot<'_>,
    operation: Operation<'_>,
) {
    match operation {
        Operation::Pane(pane) => paint_pane(paint, pane),
        Operation::TabBar(bar) => {
            let bounds = rect(bar.bounds());
            paint.fill(bounds, BAR);
            for region in bar.group_drag_regions() {
                let region = rect(region.bounds());
                paint.label(inset(region, 3.0), intersect(region, bounds), "::", MUTED);
            }
            for reserved in [
                bar.leading_reserved_bounds(),
                bar.trailing_reserved_bounds(),
            ]
            .into_iter()
            .flatten()
            {
                paint.border(rect(reserved), LINE);
            }
            if let Some(after_tabs) = bar.after_tabs_bounds() {
                paint.border_clipped(rect(after_tabs), rect(bar.viewport()), LINE);
            }
        }
        Operation::Tab(tab) => {
            let visible = rect(tab.visible_bounds());
            paint_tab(
                paint,
                rect(tab.full_bounds()),
                visible,
                tab.item(),
                tab.selected(),
            );
            paint.label(
                rect(tab.text_layout_bounds()),
                intersect(visible, rect(tab.text_bounds())),
                title(tab.item()),
                TEXT,
            );
            if let Some(close) = tab.close_visual_bounds() {
                paint.icon(
                    rect(close),
                    visible,
                    "x",
                    if tab.operable() { TEXT } else { MUTED },
                );
            }
        }
        Operation::Splitter(splitter) => paint.fill(rect(splitter.draw_bounds()), LINE),
        Operation::SplitterJunction(junction) => paint.fill(rect(junction.hit_bounds()), LINE),
        Operation::TabStripControl(control) => {
            let bounds = rect(control.bounds());
            paint.fill(bounds, if control.menu_open() { LINE } else { BAR });
            let icon = match control.kind() {
                TabStripControlKind::ScrollBackward => "<",
                TabStripControlKind::ScrollForward => ">",
                TabStripControlKind::TabListMenu => "v",
            };
            paint.icon(
                bounds,
                bounds,
                icon,
                if control.enabled() { TEXT } else { MUTED },
            );
        }
        Operation::ContainedControls(frame) => paint_contained(paint, frame, root.title_item()),
        Operation::PresentationMenuAnchor(anchor) => {
            let bounds = rect(anchor.bounds());
            paint.fill(bounds, BAR);
            paint.icon(
                bounds,
                bounds,
                "+",
                if anchor.operable() { TEXT } else { MUTED },
            );
        }
    }
}

fn paint_pane(paint: &mut Pixels<'_>, pane: PanePaintRecord<'_>) {
    paint.fill(rect(pane.bounds()), PANEL);
    let bounds = intersect(rect(pane.content_bounds()), rect(pane.bounds()));
    let color = pane.selected().map_or(MUTED, accent);
    paint.fill_clipped(
        [bounds[0], bounds[1], bounds[0] + 3.0, bounds[3]],
        bounds,
        color,
    );
    let content = inset(bounds, 16.0);
    paint.text_at(
        content[0],
        content[1],
        bounds,
        pane.selected().map_or("Empty docking area", title),
        color,
    );
    if let Some(item) = pane.selected() {
        paint.text_at(
            content[0],
            content[1] + 28.0,
            content,
            pane_content(item),
            TEXT,
        );
    }
}

fn paint_contained(paint: &mut Pixels<'_>, frame: ContainedPaintRecord<'_>, item: Option<ItemId>) {
    let outer = rect(frame.outer_bounds());
    let title_bounds = rect(frame.title_bounds());
    paint.fill(
        title_bounds,
        if frame.is_frontmost() {
            0x0034_506f
        } else {
            BAR
        },
    );
    let drag_bounds = intersect(title_bounds, rect(frame.title_drag_bounds()));
    paint.label(
        inset(drag_bounds, 7.0),
        drag_bounds,
        item.map_or("Floating panes", title),
        TEXT,
    );
    if let Some(close) = frame.close_bounds() {
        paint.icon(
            rect(close),
            title_bounds,
            "x",
            if frame.close_operable() { TEXT } else { MUTED },
        );
    }
    for resize in frame.resize().filter(|resize| resize.operable()) {
        paint.border_clipped(rect(resize.hit_bounds()), outer, LINE);
    }
    paint.border(outer, if frame.is_frontmost() { BLUE } else { LINE });
}

fn paint_menu(paint: &mut Pixels<'_>, menu: TabListMenuPaintRecord<'_>) {
    let bounds = rect(menu.bounds());
    let viewport = intersect(bounds, rect(menu.viewport()));
    paint.fill(bounds, BAR);
    paint.border(bounds, LINE);
    for row in menu.rows() {
        let row_bounds = rect(row.bounds());
        paint.fill_clipped(
            row_bounds,
            viewport,
            if row.selected() { 0x0034_506f } else { BAR },
        );
        if row.focused() {
            paint.border_clipped(row_bounds, viewport, BLUE);
        }
        paint.label(
            inset(row_bounds, 6.0),
            intersect(row_bounds, viewport),
            title(row.item()),
            TEXT,
        );
    }
    if menu.maximum_scroll_offset() > 0.0 {
        let height = (viewport[3] - viewport[1]).max(0.0);
        let thumb = (height * height / (height + menu.maximum_scroll_offset()))
            .max(8.0)
            .min(height);
        let y =
            viewport[1] + (height - thumb) * menu.scroll_offset() / menu.maximum_scroll_offset();
        paint.fill_clipped(
            [viewport[2] - 3.0, y, viewport[2], y + thumb],
            viewport,
            MUTED,
        );
    }
}

fn paint_preview(
    paint: &mut Pixels<'_>,
    plan: SurfacePaintPlan<'_>,
    preview: DockspaceDragPreview<'_>,
) {
    let bounds = match preview.visual() {
        DockspacePreviewVisual::Dock {
            surface,
            rect: bounds,
        }
        | DockspacePreviewVisual::Contained {
            surface,
            rect: bounds,
            ..
        } if surface == plan.surface() => rect(bounds),
        // Native placement is desktop physical geometry, not a client rectangle.
        // Paint a source-host cue without inventing a desktop-to-client transform.
        DockspacePreviewVisual::Native { host_surface, .. } if host_surface == plan.surface() => {
            inset(rect(plan.bounds()), 4.0)
        }
        _ => return,
    };
    if let Some(bar) = preview.tab_bar() {
        paint.fill(rect(bar.bounds()), BAR);
        for tab in preview.tabs() {
            if let Some(visible) = tab.visible_bounds() {
                let clip = intersect(rect(visible), rect(tab.clip_bounds()));
                paint_tab(
                    paint,
                    rect(tab.full_bounds()),
                    clip,
                    tab.item(),
                    tab.selected(),
                );
                if let (Some(layout), Some(text_clip)) =
                    (tab.text_layout_bounds(), tab.text_bounds())
                {
                    paint.label(
                        rect(layout),
                        intersect(clip, rect(text_clip)),
                        title(tab.item()),
                        TEXT,
                    );
                }
                if let Some(close) = tab.close_visual_bounds() {
                    paint.icon(rect(close), clip, "x", MUTED);
                }
                if tab.is_payload() {
                    paint.border_clipped(rect(tab.full_bounds()), clip, BLUE);
                }
            }
        }
    }
    if !preview.is_layout_projected() {
        paint.tint(bounds, BLUE, 48);
        paint.border(bounds, BLUE);
    }
}

fn paint_tab(paint: &mut Pixels<'_>, bounds: Rect, clip: Rect, item: ItemId, selected: bool) {
    paint.fill_clipped(bounds, clip, if selected { 0x0034_506f } else { BAR });
    paint.border_clipped(bounds, clip, LINE);
    if selected {
        paint.fill_clipped(
            [bounds[0], bounds[3] - 2.0, bounds[2], bounds[3]],
            clip,
            accent(item),
        );
    }
}

fn paint_guide(
    paint: &mut Pixels<'_>,
    bounds: Rect,
    direction: DockspaceDropDirection,
    active: bool,
    eligible: bool,
) {
    let color = if eligible { BLUE } else { MUTED };
    paint.fill(bounds, if active { 0x0044_729c } else { BAR });
    paint.border(bounds, color);
    let cue = inset(bounds, 5.0);
    paint.border(cue, color);
    let [x0, y0, x1, y1] = cue;
    let emphasis = match direction {
        DockspaceDropDirection::Center => inset(cue, (x1 - x0).min(y1 - y0) * 0.25),
        DockspaceDropDirection::Left => [x0, y0, (x0 + 4.0).min(x1), y1],
        DockspaceDropDirection::Right => [(x1 - 4.0).max(x0), y0, x1, y1],
        DockspaceDropDirection::Top => [x0, y0, x1, (y0 + 4.0).min(y1)],
        DockspaceDropDirection::Bottom => [x0, (y1 - 4.0).max(y0), x1, y1],
    };
    paint.fill(emphasis, color);
}

fn accent(item: ItemId) -> u32 {
    match item.get() {
        1 => 0x008b_b9ff,
        2 => 0x008a_d5b3,
        3 => 0x00d9_a7f2,
        4 => 0x00ef_c77d,
        _ => 0x00f0_a3a4,
    }
}

fn pane_content(item: ItemId) -> &'static str {
    match item.get() {
        1 => {
            "// Application-owned content\n\nfn main() {\n    open_workspace();\n    edit_document();\n}\n\nDrag a tab to rearrange panes."
        }
        2 => "Workspace\n  src/\n    main.rs\n    editor.rs\n  assets/\n    scene.json",
        3 => {
            "Scene preview\n\n      +----------+\n      |  CAMERA  |\n      +----------+\n\nYour renderer owns this area."
        }
        4 => {
            "[info] Workspace loaded\n[info] Software renderer ready\n[info] No egui or GPU backend\n\nDrag dividers to resize."
        }
        5 => {
            "Selection: Camera\n\nPosition\n  X     0.0\n  Y     2.0\n  Z     8.0\n\nVisible: true"
        }
        _ => "Application pane content",
    }
}

fn rect(bounds: LogicalRect) -> Rect {
    [bounds.x(), bounds.y(), bounds.max().x(), bounds.max().y()]
}

fn intersect(a: Rect, b: Rect) -> Rect {
    [
        a[0].max(b[0]),
        a[1].max(b[1]),
        a[2].min(b[2]),
        a[3].min(b[3]),
    ]
}

fn inset(bounds: Rect, padding: f64) -> Rect {
    let x = padding.min(((bounds[2] - bounds[0]) * 0.5).max(0.0));
    let y = padding.min(((bounds[3] - bounds[1]) * 0.5).max(0.0));
    [bounds[0] + x, bounds[1] + y, bounds[2] - x, bounds[3] - y]
}

struct Pixels<'buffer> {
    pixels: &'buffer mut [u32],
    size: [u32; 2],
    scale: f64,
    clip: Rect,
}

impl Pixels<'_> {
    fn fill(&mut self, bounds: Rect, color: u32) {
        self.tint(bounds, color, 255);
    }

    fn fill_clipped(&mut self, bounds: Rect, clip: Rect, color: u32) {
        self.fill(intersect(bounds, clip), color);
    }

    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    fn tint(&mut self, bounds: Rect, color: u32, alpha: u32) {
        let bounds = intersect(bounds, self.clip);
        if bounds[2] <= bounds[0] || bounds[3] <= bounds[1] || self.size[0] == 0 {
            return;
        }
        // Pixel centers select the covered half-open logical rectangle.
        let x0 = (bounds[0] * self.scale - 0.5)
            .ceil()
            .clamp(0.0, f64::from(self.size[0])) as usize;
        let y0 = (bounds[1] * self.scale - 0.5)
            .ceil()
            .clamp(0.0, f64::from(self.size[1])) as usize;
        let x1 = (bounds[2] * self.scale - 0.5)
            .ceil()
            .clamp(0.0, f64::from(self.size[0])) as usize;
        let y1 = (bounds[3] * self.scale - 0.5)
            .ceil()
            .clamp(0.0, f64::from(self.size[1])) as usize;
        for row in self
            .pixels
            .chunks_exact_mut(self.size[0] as usize)
            .skip(y0)
            .take(y1 - y0)
        {
            for pixel in &mut row[x0..x1] {
                *pixel = if alpha == 255 {
                    color
                } else {
                    blend(*pixel, color, alpha)
                };
            }
        }
    }

    fn border(&mut self, bounds: Rect, color: u32) {
        self.border_clipped(bounds, bounds, color);
    }

    fn border_clipped(&mut self, bounds: Rect, clip: Rect, color: u32) {
        let [x0, y0, x1, y1] = bounds;
        for edge in [
            [x0, y0, x1, (y0 + 1.0).min(y1)],
            [x0, (y1 - 1.0).max(y0), x1, y1],
            [x0, y0, (x0 + 1.0).min(x1), y1],
            [(x1 - 1.0).max(x0), y0, x1, y1],
        ] {
            self.fill_clipped(edge, clip, color);
        }
    }

    fn label(&mut self, layout: Rect, clip: Rect, text: &str, color: u32) {
        let y = layout[1] + ((layout[3] - layout[1] - 8.0) * 0.5).max(0.0);
        self.text_at(layout[0], y, clip, text, color);
    }

    fn icon(&mut self, bounds: Rect, clip: Rect, text: &str, color: u32) {
        let width = text.bytes().fold(0.0, |width, _| width + 8.0);
        let left = bounds[0] + ((bounds[2] - bounds[0] - width) * 0.5).max(0.0);
        self.label(
            [left, bounds[1], bounds[2], bounds[3]],
            intersect(bounds, clip),
            text,
            color,
        );
    }

    fn text_at(&mut self, start_x: f64, mut y: f64, clip: Rect, text: &str, color: u32) {
        let mut x = start_x;
        for character in text.chars() {
            if character == '\n' {
                x = start_x;
                y += 14.0;
                continue;
            }
            if y >= clip[3] {
                break;
            }
            if let Some(glyph) = BASIC_FONTS.get(character).or_else(|| BASIC_FONTS.get('?')) {
                for (row, bits) in (0_u32..8).zip(glyph) {
                    for column in 0_u32..8 {
                        if bits & (1 << column) != 0 {
                            let x = x + f64::from(column);
                            let y = y + f64::from(row);
                            self.fill_clipped([x, y, x + 1.0, y + 1.0], clip, color);
                        }
                    }
                }
            }
            x += 8.0;
        }
    }
}

fn blend(background: u32, foreground: u32, alpha: u32) -> u32 {
    [0, 8, 16].into_iter().fold(0, |color, shift| {
        let source = (foreground >> shift) & 255;
        let destination = (background >> shift) & 255;
        color | (((source * alpha + destination * (255 - alpha)) / 255) << shift)
    })
}
