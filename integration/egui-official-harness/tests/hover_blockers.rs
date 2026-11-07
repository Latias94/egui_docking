//! Public, official-egui qualification of the output-bound docking-hover lane.

use egui::accesskit::Role;
use egui::{Context, Event, FullOutput, Id, Pos2, Rect, Sense, Ui, vec2};
use egui_dockspace::{
    DockStyle, Dockspace, DockspaceAxis, DockspaceLayout, DockspaceNode, DockspaceRootLayout,
    DockspaceSurfaceLayout, ItemId, PaneView, RootId, SurfaceId, SurfacePresentationResult, run_ui,
};

const SURFACE: SurfaceId = SurfaceId::new(1);
const SOURCE: ItemId = ItemId::new(1);
const SOURCE_PEER: ItemId = ItemId::new(2);
const TARGET: ItemId = ItemId::new(3);
const PREVIEW: egui::Color32 = egui::Color32::from_rgb(31, 137, 143);
const GUIDE: egui::Color32 = egui::Color32::from_rgb(71, 79, 87);
const ACTIVE_GUIDE: egui::Color32 = egui::Color32::from_rgb(39, 169, 177);

#[derive(Default)]
struct CanvasPanes {
    content: Option<Rect>,
}

impl PaneView for CanvasPanes {
    fn title(&self, item: ItemId) -> Option<egui::WidgetText> {
        Some(
            match item {
                SOURCE => "Source",
                SOURCE_PEER => "Source peer",
                _ => "Target canvas",
            }
            .into(),
        )
    }

    fn ui(&mut self, item: ItemId, ui: &mut Ui) -> egui_dockspace::PaneResponse {
        if item == TARGET {
            let (rect, response) = ui.allocate_exact_size(ui.available_size(), Sense::drag());
            self.content = Some(response.interact_rect);
            ui.painter()
                .rect_filled(rect, 0.0, egui::Color32::DARK_GRAY);
            egui_dockspace::PaneResponse::from(&response)
        } else {
            ui.label("Application source content");
            egui_dockspace::PaneResponse::default()
        }
    }
}

#[derive(Clone, Copy, Debug)]
enum Blocker {
    Clear,
    SameLayer,
    Foreground,
    Popup,
    Modal,
}

impl Blocker {
    fn show(self, ui: &mut Ui, target: Rect) -> Option<Rect> {
        let bounds = Rect::from_center_size(target_point(target), vec2(140.0, 90.0));
        match self {
            Self::Clear => None,
            Self::SameLayer => Some(
                ui.interact(
                    bounds,
                    Id::unique("explicit-docking-blocker"),
                    Sense::click_and_drag(),
                )
                .interact_rect,
            ),
            Self::Foreground => Some(
                egui::Area::new(Id::unique("foreign-foreground"))
                    .order(egui::Order::Foreground)
                    .fixed_pos(bounds.min)
                    .movable(false)
                    .show(ui.ctx(), |ui| {
                        ui.allocate_exact_size(bounds.size(), Sense::hover());
                    })
                    .response
                    .rect,
            ),
            Self::Popup => Some(
                egui::Popup::new(
                    Id::unique("foreign-popup"),
                    ui.ctx().clone(),
                    bounds,
                    ui.layer_id(),
                )
                .at_position(bounds.min)
                .frame(egui::Frame::NONE)
                .show(|ui| {
                    ui.allocate_exact_size(bounds.size(), Sense::click());
                })
                .expect("the always-open popup is visible")
                .response
                .rect,
            ),
            Self::Modal => {
                let modal = egui::Modal::new(Id::unique("foreign-modal")).show(ui.ctx(), |ui| {
                    ui.label("The modal blocks the entire composed UI, not just its contents");
                });
                Some(modal.backdrop_response.interact_rect)
            }
        }
    }
}

struct Fixture {
    context: Context,
    dock: Dockspace,
    panes: CanvasPanes,
}

impl Fixture {
    fn new() -> Self {
        let context = Context::default();
        context.enable_accesskit();
        let layout = DockspaceLayout::new([DockspaceSurfaceLayout::new(
            SURFACE,
            DockspaceRootLayout::new(
                RootId::new(1),
                DockspaceNode::equal_split(
                    DockspaceAxis::Horizontal,
                    [
                        DockspaceNode::tabs([SOURCE, SOURCE_PEER]),
                        DockspaceNode::tabs([TARGET]),
                    ],
                )
                .expect("the two-pane layout is valid"),
            ),
        )])
        .expect("the official consumer layout is valid");
        let mut style = DockStyle::default();
        style.visuals.drop_fill = Some(PREVIEW);
        style.visuals.drop_guide_fill = Some(GUIDE);
        style.visuals.drop_guide_active_fill = Some(ACTIVE_GUIDE);
        let dock = Dockspace::builder("official-hover-blockers", layout)
            .style(style)
            .build()
            .expect("the public product facade initializes");
        Self {
            context,
            dock,
            panes: CanvasPanes::default(),
        }
    }

    fn frame(&mut self, events: Vec<Event>, blocker: Blocker) -> (FullOutput, Option<Rect>) {
        let mut blocker_bounds = None;
        let (mut output, receipt) = run_ui(
            &self.context,
            egui::RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(800.0, 600.0))),
                events,
                ..Default::default()
            },
            |ui| {
                self.dock
                    .show_single_surface(SURFACE, ui, &mut self.panes)
                    .expect("the official-egui frame advances");
                blocker_bounds = blocker.show(ui, self.panes.content.expect("the target rendered"));
            },
        )
        .into_parts();
        receipt.report(SurfacePresentationResult::Presented);
        output.textures_delta.clear();
        (output, blocker_bounds)
    }

    fn arm(&mut self) -> Pos2 {
        self.frame(Vec::new(), Blocker::Clear);
        let (output, _) = self.frame(Vec::new(), Blocker::Clear);
        let tab = output
            .platform_output
            .accesskit_update
            .as_ref()
            .unwrap()
            .nodes
            .iter()
            .find_map(|(_, node)| {
                (node.role() == Role::Tab && node.label() == Some("Source"))
                    .then(|| node.bounds().expect("the tab has semantic bounds"))
            })
            .expect("the actual source tab is present");
        let source = Pos2::new(
            ((tab.x0 + tab.x1) * 0.5) as f32,
            ((tab.y0 + tab.y1) * 0.5) as f32,
        );
        self.frame(
            vec![Event::PointerMoved(source), button(source, true)],
            Blocker::Clear,
        );
        let target = self.panes.content.unwrap();
        // Away from all guide icons, but inside the target's magnetic center basin.
        target_point(target)
    }

    fn source_pane(&self) -> Vec<ItemId> {
        self.dock
            .view()
            .item(SOURCE)
            .expect("the source survives")
            .tabs()
            .items()
            .to_vec()
    }
}

fn target_point(rect: Rect) -> Pos2 {
    rect.min + rect.size() * 0.65
}

fn button(pos: Pos2, pressed: bool) -> Event {
    Event::PointerButton {
        pos,
        button: egui::PointerButton::Primary,
        pressed,
        modifiers: egui::Modifiers::NONE,
    }
}

fn has_preview(output: &FullOutput) -> bool {
    fn is_preview(shape: &egui::Shape) -> bool {
        match shape {
            egui::Shape::Rect(rect) => rect.fill == PREVIEW,
            egui::Shape::Vec(shapes) => shapes.iter().any(is_preview),
            _ => false,
        }
    }
    output.shapes.iter().any(|shape| is_preview(&shape.shape))
}

fn assert_non_guide(output: &FullOutput, point: Pos2) {
    fn guide_contains(shape: &egui::Shape, point: Pos2) -> bool {
        match shape {
            egui::Shape::Rect(rect) => {
                matches!(rect.fill, GUIDE | ACTIVE_GUIDE) && rect.rect.contains(point)
            }
            egui::Shape::Vec(shapes) => shapes.iter().any(|shape| guide_contains(shape, point)),
            _ => false,
        }
    }
    assert!(
        output
            .shapes
            .iter()
            .all(|shape| !guide_contains(&shape.shape, point)),
        "the journey must use the real content basin, not an explicit guide"
    );
}

#[test]
fn official_hover_stops_at_composed_ui_blockers_and_never_falls_back() {
    for blocker in [
        Blocker::Clear,
        Blocker::SameLayer,
        Blocker::Foreground,
        Blocker::Popup,
        Blocker::Modal,
    ] {
        let mut fixture = Fixture::new();
        let target = fixture.arm();
        let before = fixture.dock.version();
        // Establish the blocker in a completed pass before moving onto it.
        fixture.frame(Vec::new(), blocker);
        let (_, bounds) = fixture.frame(Vec::new(), blocker);
        if let Some(bounds) = bounds {
            assert!(
                bounds.contains(target),
                "{blocker:?}: the actual composed blocker covers the input point"
            );
        }
        let mut preview = false;
        for _ in 0..8 {
            let (output, _) = fixture.frame(vec![Event::PointerMoved(target)], blocker);
            assert_non_guide(&output, target);
            preview |= has_preview(&output);
        }
        let clear = matches!(blocker, Blocker::Clear);
        assert_eq!(
            preview, clear,
            "{blocker:?}: only ordinary registered content passes docking hover"
        );
        fixture.frame(
            vec![Event::PointerMoved(target), button(target, false)],
            blocker,
        );
        for _ in 0..3 {
            fixture.frame(Vec::new(), blocker);
        }
        if clear {
            assert_ne!(fixture.dock.version(), before);
            assert_eq!(fixture.source_pane(), vec![TARGET, SOURCE]);
        } else {
            assert_eq!(
                fixture.dock.version(),
                before,
                "{blocker:?}: a blocked release cannot change layout"
            );
            assert_eq!(fixture.source_pane(), vec![SOURCE, SOURCE_PEER]);
        }
    }
}

#[test]
fn official_blocked_release_cannot_deliver_an_earlier_clear_preview() {
    for blocker in [
        Blocker::SameLayer,
        Blocker::Foreground,
        Blocker::Popup,
        Blocker::Modal,
    ] {
        let mut fixture = Fixture::new();
        let target = fixture.arm();
        let before = fixture.dock.version();
        let mut preview = false;
        for _ in 0..8 {
            preview |= has_preview(
                &fixture
                    .frame(vec![Event::PointerMoved(target)], Blocker::Clear)
                    .0,
            );
        }
        assert!(
            preview,
            "the exact ordinary-content preview existed before the blocker"
        );
        fixture.frame(Vec::new(), blocker);
        let (_, bounds) = fixture.frame(Vec::new(), blocker);
        assert!(
            bounds.unwrap().contains(target),
            "{blocker:?}: the real blocker covers release"
        );
        fixture.frame(vec![button(target, false)], blocker);
        for _ in 0..3 {
            fixture.frame(Vec::new(), blocker);
        }
        assert_eq!(
            fixture.dock.version(),
            before,
            "{blocker:?}: no stale-preview delivery"
        );
        assert_eq!(fixture.source_pane(), vec![SOURCE, SOURCE_PEER]);
        // Removing the blocker after release must not resurrect the old pending gesture.
        for _ in 0..3 {
            fixture.frame(Vec::new(), Blocker::Clear);
        }
        assert_eq!(fixture.dock.version(), before);
    }
}

#[test]
fn official_unrelated_completed_pass_cannot_lend_widgets_to_an_older_output() {
    let mut fixture = Fixture::new();
    let target = fixture.arm();
    let before = fixture.dock.version();
    let mut preview = false;
    for _ in 0..8 {
        preview |= has_preview(
            &fixture
                .frame(vec![Event::PointerMoved(target)], Blocker::Clear)
                .0,
        );
    }
    assert!(
        preview,
        "the last presented docking output authorized a preview"
    );
    // egui advances, but this output does not contain a dockspace surface.
    let (mut output, receipt) = run_ui(
        &fixture.context,
        egui::RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(800.0, 600.0))),
            ..Default::default()
        },
        |ui| {
            ui.label("An unrelated composed UI pass");
        },
    )
    .into_parts();
    receipt.report(SurfacePresentationResult::Presented);
    output.textures_delta.clear();
    fixture.frame(vec![button(target, false)], Blocker::Clear);
    for _ in 0..4 {
        fixture.frame(Vec::new(), Blocker::Clear);
    }
    assert_eq!(fixture.dock.version(), before);
    assert_eq!(fixture.source_pane(), vec![SOURCE, SOURCE_PEER]);
}
