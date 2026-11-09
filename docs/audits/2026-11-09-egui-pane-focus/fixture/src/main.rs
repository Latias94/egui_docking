use dockspace::geometry::LogicalRect;
use dockspace::model::*;
use dockspace::policy::DockPolicy;
use dockspace::runtime::DockspaceSession;
use eframe::egui::{self, Color32, Pos2, Rect, Vec2};
use egui_dockspace::{DockStyle, PaneResponse, PaneView};
use egui_dockspace_native::NativeDockspaceApp;
use serde_json::{Value, json};
use std::time::Duration;

const SURFACE: SurfaceId = SurfaceId::new(1);
const TITLES: [&str; 10] = [
    "Scene",
    "Document.rs",
    "History",
    "Profiler",
    "Animation",
    "Material settings and texture references",
    "Assets",
    "Console",
    "Inspector",
    "Preview",
];
struct Panes {
    text: String,
    number: f32,
    enabled: bool,
    long_title: bool,
}
impl PaneView for Panes {
    fn title(&self, item: ItemId) -> Option<egui::WidgetText> {
        if self.long_title && item == ItemId::new(2) {
            return Some("MMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMM".into());
        }
        TITLES.get(item.get() as usize - 1).map(|s| (*s).into())
    }
    fn ui(&mut self, item: ItemId, ui: &mut egui::Ui) -> PaneResponse {
        ui.label(TITLES[item.get() as usize - 1]);
        ui.separator();
        ui.label("Application-owned content");
        ui.horizontal(|ui| {
            let _ = ui.button("Apply");
            let _ = ui.button("Reset");
        });
        ui.checkbox(&mut self.enabled, "Enabled");
        ui.add(egui::Slider::new(&mut self.number, 0.0..=1.0).text("Value"));
        let target = self.focus_target(item).unwrap();
        let r = ui.add(
            egui::TextEdit::multiline(&mut self.text)
                .id(target)
                .desired_width(f32::INFINITY)
                .desired_rows(5),
        );
        PaneResponse::from(&r)
    }

    fn focus_target(&self, item: ItemId) -> Option<egui::Id> {
        Some(egui::Id::unique(("ui-audit-editor", item)))
    }
}
struct Recorder {
    path: String,
    pass: usize,
}
impl egui::plugin::Plugin for Recorder {
    fn debug_name(&self) -> &'static str {
        "ui-style-audit"
    }
    fn output_hook(&mut self, ctx: &egui::Context, out: &mut egui::FullOutput) {
        self.pass += 1;
        if self.pass < 8 {
            return;
        }
        let nodes: Vec<Value> = out.platform_output.accesskit_update.as_ref().map(|u| u.nodes.iter().filter_map(|(id,n)| {
            n.label().map(|label| json!({"id":id.0,"role":format!("{:?}",n.role()),"label":label,"bounds":n.bounds().map(|r| [r.x0,r.y0,r.x1,r.y1]),"disabled":n.is_disabled(),"selected":n.is_selected()}))
        }).collect()).unwrap_or_default();
        let mut texts = Vec::new();
        fn visit(shape: &egui::Shape, clip: Rect, texts: &mut Vec<Value>) {
            match shape {
                egui::Shape::Text(t) => texts.push(json!({"text":t.galley.job.text,"pos":[t.pos.x,t.pos.y],"size":[t.galley.size().x,t.galley.size().y],"ink_bounds":[t.pos.x+t.galley.mesh_bounds.min.x,t.pos.y+t.galley.mesh_bounds.min.y,t.pos.x+t.galley.mesh_bounds.max.x,t.pos.y+t.galley.mesh_bounds.max.y],"elided":t.galley.elided,"last_glyph":t.galley.rows.last().and_then(|r|r.glyphs.last().map(|g|json!({"char":g.chr,"x":r.pos.x+g.pos.x,"end_x":r.pos.x+g.pos.x+g.advance_width}))),"clip":[clip.min.x,clip.min.y,clip.max.x,clip.max.y],"fallback":t.fallback_color.to_srgba_unmultiplied(),"override":t.override_text_color.map(|c| c.to_srgba_unmultiplied())})),
                egui::Shape::Vec(v) => for s in v { visit(s,clip,texts); },
                _ => ()
            }
        }
        for s in &out.shapes {
            visit(&s.shape, s.clip_rect, &mut texts);
        }
        let rects: Vec<_> = out.shapes.iter().filter_map(|shape| {
            if let egui::Shape::Rect(rect) = &shape.shape {
                Some(json!({"bounds":[rect.rect.min.x,rect.rect.min.y,rect.rect.max.x,rect.rect.max.y],"fill":rect.fill.to_srgba_unmultiplied()}))
            } else { None }
        }).collect();
        std::fs::write(&self.path, json!({"pass":self.pass,"focus":ctx.memory(|m|m.focused()).map(|id|id.value()),"pixels_per_point":ctx.pixels_per_point(),"nodes":nodes,"texts":texts,"rects":rects,"selection_color":ctx.global_style().visuals.selection.stroke.color.to_srgba_unmultiplied()}).to_string()).unwrap();
    }
}
struct App {
    dock: NativeDockspaceApp<Panes>,
    window_open: bool,
    selected: bool,
    text: String,
    number: f32,
}
impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        egui::Panel::top("audit-heading").show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.strong("Dockspace / egui style audit");
                ui.separator();
                ui.label("Left: docking adapter    Right: native egui controls");
            });
        });
        egui::Panel::right("audit-egui-reference")
            .exact_size(280.0)
            .show(ui, |ui| {
                ui.heading("Native egui");
                ui.label("Same Visuals, Spacing and fonts");
                ui.horizontal(|ui| {
                    let _ = ui.button("Button");
                    let _ = ui.selectable_label(true, "Selected");
                });
                ui.horizontal(|ui| {
                    ui.menu_button("Native menu", |ui| {
                        let _ = ui.button("Scene");
                        let _ = ui.button("Document.rs");
                        ui.separator();
                        let _ = ui.add_enabled(false, egui::Button::new("Unavailable"));
                    });
                });
                ui.add_enabled_ui(false, |ui| {
                    let _ = ui.button("Disabled control");
                });
            });
        eframe::App::ui(&mut self.dock, ui, frame);
        let x = ui.ctx().content_rect().right() - 268.0;
        egui::Window::new("Native egui Window")
            .open(&mut self.window_open)
            .fixed_pos(Pos2::new(x, 205.0))
            .fixed_size(Vec2::new(245.0, 300.0))
            .show(ui.ctx(), |ui| {
                ui.label("Application-owned content");
                ui.horizontal(|ui| {
                    let _ = ui.button("Apply");
                    let _ = ui.button("Reset");
                });
                ui.checkbox(&mut self.selected, "Enabled");
                ui.add(egui::Slider::new(&mut self.number, 0.0..=1.0).text("Value"));
                ui.add(
                    egui::TextEdit::multiline(&mut self.text)
                        .desired_width(f32::INFINITY)
                        .desired_rows(5),
                );
            });
        assert!(self.dock.error_kind().is_none());
        ui.ctx().request_repaint_after(Duration::from_millis(80));
    }
}
fn configure_theme(context: &egui::Context, args: &[String]) {
    context.set_visuals(if args.iter().any(|s| s == "--light") {
        egui::Visuals::light()
    } else {
        egui::Visuals::dark()
    });
    context.global_style_mut(|style| {
        if args.iter().any(|s| s == "--large") {
            for font in style.text_styles.values_mut() {
                font.size *= 2.0;
            }
            style.spacing.interact_size.y = 36.0;
        }
        if args.iter().any(|s| s == "--font36") {
            style.override_font_id = Some(egui::FontId::proportional(36.0));
            style.spacing.interact_size.y = 40.0;
        }
        if args.iter().any(|s| s == "--sentinel") {
            style.visuals.override_text_color = Some(Color32::from_rgb(180, 30, 130));
            style.visuals.window_corner_radius = egui::CornerRadius::same(12);
            style.visuals.menu_corner_radius = egui::CornerRadius::same(12);
            style.spacing.button_padding.y = 6.0;
            style.visuals.window_stroke = egui::Stroke::new(3.0, Color32::from_rgb(50, 140, 190));
        }
    });
    if args.iter().any(|s| s == "--zoom") {
        context.set_zoom_factor(1.5);
    }
}
fn main() -> eframe::Result {
    let args: Vec<_> = std::env::args().collect();
    let selected = if args.iter().any(|s| s == "--select-last") {
        6
    } else {
        2
    };
    let root = DockspaceNode::split(
        DockspaceAxis::Horizontal,
        [
            (DockspaceNode::tabs([ItemId::new(1), ItemId::new(7)]), 0.24),
            (
                DockspaceNode::central_tabs_with_selection(
                    (2..=if args.iter().any(|s| s == "--long-title") {
                        3
                    } else {
                        6
                    })
                        .map(ItemId::new),
                    Some(ItemId::new(selected)),
                )
                .unwrap(),
                0.76,
            ),
        ],
    )
    .unwrap();
    let layout = DockspaceLayout::new([DockspaceSurfaceLayout::new(
        SURFACE,
        DockspaceRootLayout::new(RootId::new(1), root),
    )
    .with_contained(DockspaceContainedLayout::new(
        FloatingPresentationId::new(1),
        DockspaceRootLayout::new(
            RootId::new(2),
            DockspaceNode::tabs([ItemId::new(9), ItemId::new(10)]),
        ),
        LogicalRect::new(440.0, 265.0, 380.0, 310.0).unwrap(),
    ))])
    .unwrap();
    let mut policy = DockPolicy::default();
    policy.set_allow_native_surfaces(true);
    let mut dock_style = DockStyle::default();
    if args.iter().any(|s| s == "--font36" || s == "--large") {
        let context = egui::Context::default();
        configure_theme(&context, &args);
        context
            .run_ui(egui::RawInput::default(), |ui| {
                dock_style = DockStyle::from_egui(ui)
            })
            .drop_without_applying_deltas();
    }
    let session = DockspaceSession::from_layout_with_presentation_config(
        layout,
        policy,
        dock_style.presentation_config().unwrap(),
    )
    .unwrap();
    let dock = NativeDockspaceApp::new(egui::Id::unique("ui-style-audit"),session,SURFACE,Panes{text:"Select an object to inspect its properties.\nContent follows the application's egui theme.".into(),number:0.42,enabled:true,long_title:args.iter().any(|s|s=="--long-title")},dock_style).unwrap();
    let native_host = Some(dock.native_host_handler());
    eframe::run_native(
        "Dockspace UI style audit",
        eframe::NativeOptions {
            native_host,
            renderer: eframe::Renderer::Glow,
            viewport: egui::ViewportBuilder::default().with_inner_size([1280.0, 820.0]),
            ..Default::default()
        },
        Box::new(move |cc| {
            configure_theme(&cc.egui_ctx, &args);
            cc.egui_ctx.enable_accesskit();
            cc.egui_ctx.add_plugin(Recorder {
                path: std::env::var("UI_AUDIT_NODES").unwrap(),
                pass: 0,
            });
            Ok(Box::new(App{dock,window_open:true,selected:true,text:"Select an object to inspect its properties.\nContent follows the application's egui theme.".into(),number:0.42}))
        }),
    )
}
