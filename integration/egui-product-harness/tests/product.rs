#[path = "product/magnetic.rs"]
mod magnetic;
#[path = "support/observations.rs"]
mod observations;
#[path = "support/run.rs"]
mod run_support;

use magnetic::{magnetic_preview_rect, normalized_pane_point, preview_readiness};
use observations::{
    accesskit_node, accesskit_node_rect, contained_rect, contained_title_point, guide_button_count,
    line_segment_count_in, nearest_guide_center, node_center, node_rect, rect_fill_count,
    rect_fill_count_in, rect_with_fill, rects_with_fill, root_split, root_tabs, root_weights,
    selected, splitter_junction_weights, splitter_rect, tab_center, tab_items, try_node_rect,
};
use run_support::{
    FrameOutput, input, run_disabled_frame, run_focus_frame, run_frame,
    run_frame_with_discard_after_dockspace, run_headless, run_pane_frame,
    run_pane_frame_with_mutation, run_two_discarded_dockspace_passes, settle_until,
};

#[path = "product/arrangement_lock.rs"]
mod arrangement_lock;
#[path = "product/close_commands.rs"]
mod close_commands;
#[path = "product/contained_redock.rs"]
mod contained_redock;
#[path = "product/overflow_ownership.rs"]
mod overflow_ownership;

use dockspace::policy::DockPolicy;
use dockspace_host_conformance::scenario::{
    CadenceProfile, ItemAlias, JourneyId, LOCAL_TAB_CLOSE_SPLITTER_AND_CONTAINED,
    MAGNETIC_DOCKING_BASINS, MagneticDockBasin, NATIVE_CREATE_REDOCK_RETIREMENT, ScenarioAdmission,
    ScenarioAxis, ScenarioCapability, ScenarioEdge, ScenarioId, SemanticAction as ScenarioAction,
    SemanticObservation, SemanticSelector,
};
use egui::accesskit::{Action, ActionRequest, Orientation, Role, TreeId};
use egui::{
    Context, Event, FullOutput, Key, Modifiers, PointerButton, Pos2, Rect, RepaintCause, Ui, vec2,
};
use egui_dockspace::{
    CloseDecision, ClosePlanTarget, DockPlacement, DockStyle, Dockspace, DockspaceActionOutcome,
    DockspaceActionStatus, DockspaceAxis, DockspaceCapability, DockspaceCloseRequest,
    DockspaceCloseRequestRejection, DockspaceCloseRequestStatus, DockspaceContainedLayout,
    DockspaceLayout, DockspaceNode, DockspaceRootLayout, DockspaceSurfaceLayout,
    DockspaceUnavailableReason, FloatingPresentationId, ItemId, LogicalRect, PaneFocusState,
    PaneResponse, PaneView, RootId, SurfaceId, SurfacePresentationResult, run_ui,
};
use egui_kittest::{Harness, kittest::Queryable as _};
use std::cell::Cell;
use std::ops::{Deref, DerefMut};
use std::panic::{AssertUnwindSafe, catch_unwind};

const SURFACE: SurfaceId = SurfaceId::new(1);
const ROOT: RootId = RootId::new(1);
const FLOATING_ROOT: RootId = RootId::new(2);
const FRONT_FLOATING_ROOT: RootId = RootId::new(3);
const FLOATING: FloatingPresentationId = FloatingPresentationId::new(1);
const FRONT_FLOATING: FloatingPresentationId = FloatingPresentationId::new(2);
const FIRST: ItemId = ItemId::new(1);
const SECOND: ItemId = ItemId::new(2);
const THIRD: ItemId = ItemId::new(3);
const FOURTH: ItemId = ItemId::new(4);
const FIFTH: ItemId = ItemId::new(5);
const SIXTH: ItemId = ItemId::new(6);

fn surface_ids(dockspace: &Dockspace) -> Vec<SurfaceId> {
    dockspace
        .view()
        .surfaces()
        .map(|surface| surface.id())
        .collect()
}

fn instrumented_style() -> DockStyle {
    let mut style = DockStyle::default();
    style.visuals.drop_guide_fill = Some(egui::Color32::from_rgb(52, 57, 62));
    style.visuals.drop_guide_active_fill = Some(egui::Color32::from_rgb(49, 142, 154));
    style.visuals.splitter_color = Some(egui::Color32::from_rgb(69, 75, 82));
    style.visuals.splitter_hover_color = Some(egui::Color32::from_rgb(89, 157, 165));
    style.visuals.drop_fill = Some(egui::Color32::from_rgba_unmultiplied(49, 142, 154, 72));
    style.visuals.ghost_fill = Some(egui::Color32::from_rgba_unmultiplied(180, 132, 62, 64));
    style
}

struct Panes;

#[derive(Clone, Copy, Debug)]
struct PaneProjectionCallback {
    item: ItemId,
    pass: u64,
    bounds: Rect,
    ids: [egui::Id; 4],
    label_rect: Rect,
    button_rect: Rect,
    button_hit_rect: Rect,
    opacity: f32,
}

#[derive(Default)]
struct ProjectionPanes {
    callbacks: Vec<PaneProjectionCallback>,
}

impl ProjectionPanes {
    fn latest(&self, item: ItemId) -> PaneProjectionCallback {
        self.callbacks
            .iter()
            .rev()
            .find(|callback| callback.item == item)
            .copied()
            .expect("the selected pane published its callback geometry")
    }

    fn assert_frame(
        &self,
        context: &Context,
        frame: &FrameOutput,
        baseline: [PaneProjectionCallback; 2],
        contained_bounds: Rect,
    ) {
        let end_pass = context.cumulative_pass_nr();
        let first_pass = end_pass - frame.output.platform_output.num_completed_passes as u64;
        for (before, expected) in [
            (baseline[0], baseline[0].bounds),
            (baseline[1], contained_bounds),
        ] {
            let current = self.latest(before.item);
            assert_eq!(
                current.bounds, expected,
                "pane {:?} callback available rect must follow the complete live projection, not the durable source or ghost shell",
                before.item,
            );
            for pass in first_pass..end_pass {
                assert_eq!(
                    self.callbacks
                        .iter()
                        .filter(|callback| callback.item == before.item && callback.pass == pass)
                        .count(),
                    1,
                    "selected pane {:?} must render exactly once in pass {pass}",
                    before.item,
                );
            }
            assert_eq!(current.ids, before.ids, "projection must preserve UI IDs");
            assert!(
                current.opacity > 0.0,
                "live pane content must remain visible"
            );
            let delta = expected.min - before.bounds.min;
            assert_eq!(current.label_rect, before.label_rect.translate(delta));
            assert_eq!(current.button_rect, before.button_rect.translate(delta));
            assert_eq!(
                current.button_hit_rect,
                before.button_hit_rect.translate(delta),
                "ordinary child-widget hit geometry follows the same pane projection",
            );
            assert!(expected.contains_rect(current.button_hit_rect));
            for (role, label, id, bounds) in [
                (
                    Role::Pane,
                    format!("Projection pane {}", before.item.get()),
                    current.ids[1],
                    expected,
                ),
                (
                    Role::Label,
                    format!("Projection label {}", before.item.get()),
                    current.ids[2],
                    current.label_rect,
                ),
                (
                    Role::Button,
                    format!("Projection button {}", before.item.get()),
                    current.ids[3],
                    current.button_rect,
                ),
            ] {
                let tree = frame
                    .output
                    .platform_output
                    .accesskit_update
                    .as_ref()
                    .expect("AccessKit is enabled");
                let (_, node) = tree
                    .nodes
                    .iter()
                    .find(|(node_id, _)| *node_id == id.accesskit_id())
                    .unwrap_or_else(|| panic!("missing stable semantic node for {label}"));
                assert_eq!(node.role(), role);
                let text = if role == Role::Label {
                    node.value()
                } else {
                    node.label()
                };
                assert_eq!(text, Some(label.as_str()));
                assert_eq!(
                    accesskit_node_rect(node),
                    bounds,
                    "{label} semantic bounds must agree with the actual callback UI",
                );
            }
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum PaneContentWidget {
    Button,
    TextEdit,
    ScrollArea,
    Canvas,
}

impl PaneContentWidget {
    const ALL: [Self; 4] = [Self::Button, Self::TextEdit, Self::ScrollArea, Self::Canvas];
}

struct TextPanes {
    texts: std::collections::BTreeMap<ItemId, String>,
    pane_rects: std::collections::BTreeMap<ItemId, Cell<Rect>>,
    content_widgets: std::collections::BTreeMap<ItemId, Vec<egui::Id>>,
    widget: PaneContentWidget,
    fill_widget: bool,
    register_content: bool,
    overlapping_blocker: bool,
    responses: std::collections::BTreeMap<ItemId, egui::Response>,
    clicks: std::collections::BTreeMap<ItemId, usize>,
    dragged: std::collections::BTreeSet<ItemId>,
    scroll_offsets: std::collections::BTreeMap<ItemId, f32>,
}

impl TextPanes {
    fn new(items: impl IntoIterator<Item = (ItemId, &'static str)>) -> Self {
        Self {
            texts: items
                .into_iter()
                .map(|(item, text)| (item, text.to_owned()))
                .collect(),
            pane_rects: std::collections::BTreeMap::new(),
            content_widgets: std::collections::BTreeMap::new(),
            widget: PaneContentWidget::TextEdit,
            fill_widget: false,
            register_content: true,
            overlapping_blocker: false,
            responses: std::collections::BTreeMap::new(),
            clicks: std::collections::BTreeMap::new(),
            dragged: std::collections::BTreeSet::new(),
            scroll_offsets: std::collections::BTreeMap::new(),
        }
    }

    fn magnetic(widget: PaneContentWidget) -> Self {
        Self {
            widget,
            fill_widget: true,
            ..Self::new([
                (FIRST, "Target content"),
                (SECOND, "Source content"),
                (THIRD, "Outline"),
                (FOURTH, "North"),
                (FIFTH, "East"),
                (SIXTH, "South"),
            ])
        }
    }

    fn pane_rect(&self, item: ItemId) -> Rect {
        self.pane_rects
            .get(&item)
            .map(Cell::get)
            .expect("the text pane published its content rectangle")
    }

    fn widget_response(&self, item: ItemId) -> &egui::Response {
        self.responses
            .get(&item)
            .expect("the pane published its actual child-widget response")
    }
}

struct FocusPanes {
    target: egui::Id,
    target_rect: Cell<Option<Rect>>,
    pane_rect: Cell<Option<Rect>>,
    expose_target: bool,
    docking_content: bool,
    content_widgets: Vec<egui::Id>,
    overlapping_blocker: bool,
}

impl FocusPanes {
    fn new(id_salt: &'static str, expose_target: bool) -> Self {
        let target = egui::Id::unique((id_salt, "pane-focus-target"));
        Self {
            target,
            target_rect: Cell::new(None),
            pane_rect: Cell::new(None),
            expose_target,
            docking_content: true,
            content_widgets: vec![target],
            overlapping_blocker: false,
        }
    }

    fn target_rect(&self) -> Rect {
        self.target_rect
            .get()
            .expect("the selected pane published its focus target rectangle")
    }

    fn pane_rect(&self) -> Rect {
        self.pane_rect
            .get()
            .expect("the selected pane published its content rectangle")
    }
}

struct KittestProductState {
    dockspace: Dockspace,
    panes: Panes,
    close_requests: Vec<DockspaceCloseRequest>,
    presentation_settlement_capability: Option<DockspaceCapability>,
}

struct KittestDockspaceHarness(Harness<'static, KittestProductState>);

impl KittestDockspaceHarness {
    fn step(&mut self) {
        self.0.step();
    }

    fn run_steps(&mut self, steps: usize) {
        for _ in 0..steps {
            self.step();
        }
    }
}

impl Deref for KittestDockspaceHarness {
    type Target = Harness<'static, KittestProductState>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl DerefMut for KittestDockspaceHarness {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

#[derive(Default)]
struct LateDiscardPlugin {
    remaining: usize,
    panic_once: bool,
    strip_callback_compounds_once: bool,
}

impl egui::plugin::Plugin for LateDiscardPlugin {
    fn debug_name(&self) -> &'static str {
        "egui_dockspace_test::late_discard"
    }

    fn output_hook(&mut self, _context: &Context, output: &mut FullOutput) {
        if self.panic_once {
            self.panic_once = false;
            panic!("late output hook interrupted the egui run");
        }
        if self.strip_callback_compounds_once {
            self.strip_callback_compounds_once = false;
            output
                .shapes
                .retain(|shape| !shape_contains_callback(&shape.shape));
        }
        if self.remaining == 0 {
            return;
        }
        self.remaining -= 1;
        output
            .platform_output
            .request_discard_reasons
            .push(RepaintCause::new_reason(
                "discard after dockspace final-pass settlement runs",
            ));
    }
}

fn shape_contains_callback(shape: &egui::Shape) -> bool {
    match shape {
        egui::Shape::Callback(_) => true,
        egui::Shape::Vec(shapes) => shapes.iter().any(shape_contains_callback),
        egui::Shape::Noop
        | egui::Shape::Circle(_)
        | egui::Shape::Ellipse(_)
        | egui::Shape::LineSegment { .. }
        | egui::Shape::Path(_)
        | egui::Shape::Band(_)
        | egui::Shape::Rect(_)
        | egui::Shape::Text(_)
        | egui::Shape::Mesh(_)
        | egui::Shape::QuadraticBezier(_)
        | egui::Shape::CubicBezier(_) => false,
    }
}

impl PaneView for Panes {
    fn title(&self, item: ItemId) -> Option<egui::WidgetText> {
        match item {
            FIRST => Some("First".into()),
            SECOND => Some("Second".into()),
            THIRD => Some("Third".into()),
            FOURTH => Some("Fourth".into()),
            FIFTH => Some("Fifth".into()),
            SIXTH => Some("Sixth".into()),
            _ => None,
        }
    }

    fn ui(&mut self, item: ItemId, ui: &mut Ui) -> PaneResponse {
        ui.label(format!("Pane {}", item.get()));
        PaneResponse::default()
    }
}

impl PaneView for ProjectionPanes {
    fn title(&self, item: ItemId) -> Option<egui::WidgetText> {
        Panes.title(item)
    }

    fn ui(&mut self, item: ItemId, ui: &mut Ui) -> PaneResponse {
        let bounds = ui.available_rect_before_wrap();
        // The semantic marker measures the real callback UI, never the expected test rect.
        let marker = ui.interact(
            bounds,
            ui.make_persistent_id("projection-pane-bounds"),
            egui::Sense::hover(),
        );
        ui.ctx().accesskit_node_builder(marker.id, |node| {
            node.set_role(Role::Pane);
            node.set_label(format!("Projection pane {}", item.get()));
            node.set_bounds(egui::accesskit::Rect {
                x0: f64::from(bounds.min.x),
                y0: f64::from(bounds.min.y),
                x1: f64::from(bounds.max.x),
                y1: f64::from(bounds.max.y),
            });
        });
        let label = ui.label(format!("Projection label {}", item.get()));
        let button = ui.button(format!("Projection button {}", item.get()));
        self.callbacks.push(PaneProjectionCallback {
            item,
            pass: ui.ctx().cumulative_pass_nr(),
            bounds,
            ids: [ui.scope_id(), marker.id, label.id, button.id],
            label_rect: label.rect,
            button_rect: button.rect,
            button_hit_rect: button.interact_rect,
            opacity: ui.opacity(),
        });
        PaneResponse::from(&label).with_content(&button)
    }
}

impl PaneView for TextPanes {
    fn title(&self, item: ItemId) -> Option<egui::WidgetText> {
        match item {
            FIRST => Some("Editor".into()),
            SECOND => Some("Preview".into()),
            THIRD => Some("Outline".into()),
            _ => Panes.title(item),
        }
    }

    fn ui(&mut self, item: ItemId, ui: &mut Ui) -> PaneResponse {
        let bounds = ui.available_rect_before_wrap();
        self.pane_rects
            .entry(item)
            .or_insert_with(|| Cell::new(Rect::NOTHING))
            .set(bounds);
        self.content_widgets.entry(item).or_default().clear();
        self.responses.remove(&item);
        let Some(text) = self.texts.get_mut(&item) else {
            return PaneResponse::default();
        };
        let response = match self.widget {
            PaneContentWidget::Button => ui.add_sized(bounds.size(), egui::Button::new(&*text)),
            PaneContentWidget::TextEdit => {
                let edit = egui::TextEdit::multiline(text)
                    .desired_width(f32::INFINITY)
                    .desired_rows(if self.fill_widget { 1 } else { 12 });
                if self.fill_widget {
                    ui.add_sized(bounds.size(), edit)
                } else {
                    ui.add(edit)
                }
            }
            PaneContentWidget::ScrollArea => {
                let scroll = egui::ScrollArea::vertical()
                    .id_salt("pane-content-scroll")
                    .max_height(bounds.height())
                    .auto_shrink([false, false])
                    .scroll_source(egui::scroll_area::ScrollSource::ALL)
                    .show(ui, |ui| {
                        ui.add(egui::Label::new(&*text).selectable(false));
                        ui.allocate_space(vec2(ui.available_width(), bounds.height() * 3.0));
                    });
                self.scroll_offsets
                    .insert(item, scroll.state.clamped_offset().y);
                // egui registers its background drag response after the first layout pass.
                let Some(response) = ui.ctx().read_response(scroll.id.with("area")) else {
                    return PaneResponse::default();
                };
                response
            }
            PaneContentWidget::Canvas => {
                let response = ui.allocate_response(bounds.size(), egui::Sense::click_and_drag());
                ui.painter()
                    .rect_filled(response.rect, 0.0, egui::Color32::DARK_GRAY);
                response
            }
        };
        if response.clicked() {
            *self.clicks.entry(item).or_default() += 1;
        }
        if response.dragged() {
            self.dragged.insert(item);
        }
        let mut content = PaneResponse::default();
        if self.register_content {
            content = content.with_content(&response);
            if self.overlapping_blocker {
                // A repeated declaration cannot consume another widget's equal rectangle.
                content = content.with_content(&response);
            }
        }
        if self.overlapping_blocker {
            let _ = ui.interact(
                response.interact_rect,
                response.id.with("unknown-blocker"),
                egui::Sense::click_and_drag(),
            );
        }
        self.content_widgets
            .entry(item)
            .or_default()
            .extend(content.content_widgets());
        self.responses.insert(item, response);
        content
    }
}

impl PaneView for FocusPanes {
    fn title(&self, item: ItemId) -> Option<egui::WidgetText> {
        Panes.title(item)
    }

    fn ui(&mut self, item: ItemId, ui: &mut Ui) -> PaneResponse {
        let pane_rect = ui.available_rect_before_wrap();
        self.pane_rect.set(Some(pane_rect));
        if item == SECOND {
            let rect = Rect::from_min_size(pane_rect.min, vec2(120.0, 24.0));
            self.target_rect.set(Some(rect));
            let _ = ui.interact(rect, self.target, egui::Sense::click_and_drag());
            if self.overlapping_blocker {
                let _ = ui.interact(
                    rect,
                    self.target.with("unknown-blocker"),
                    egui::Sense::click_and_drag(),
                );
            }
        }
        ui.label(format!("Pane {}", item.get()));
        if item == SECOND && self.docking_content {
            PaneResponse::default().with_content_ids(self.content_widgets.iter().copied())
        } else {
            PaneResponse::default()
        }
    }

    fn focus_target(&self, item: ItemId) -> Option<egui::Id> {
        if item != SECOND {
            return None;
        }
        self.expose_target.then_some(self.target)
    }

    fn focus_state(&self, item: ItemId, context: &Context) -> PaneFocusState {
        if item != SECOND || !self.expose_target {
            return PaneFocusState::Unknown;
        }
        if context.input(|input| input.focused)
            && context.memory(|memory| memory.has_focus(self.target))
        {
            PaneFocusState::Focused
        } else {
            PaneFocusState::Unfocused
        }
    }
}

fn kittest_harness(id: &'static str, layout: DockspaceLayout) -> KittestDockspaceHarness {
    let dockspace = Dockspace::builder(id, layout)
        .build()
        .expect("the kittest product facade initializes");
    let harness = Harness::builder()
        .with_size(vec2(800.0, 600.0))
        .with_max_steps(8)
        .build_ui_state(
            |ui, state: &mut KittestProductState| {
                let response = state
                    .dockspace
                    .show_single_surface(SURFACE, ui, &mut state.panes)
                    .expect("the kittest product frame advances");
                state
                    .close_requests
                    .extend(response.close_request_events().iter().cloned());
                state.presentation_settlement_capability =
                    Some(response.presentation_settlement_capability());
            },
            KittestProductState {
                dockspace,
                panes: Panes,
                close_requests: Vec::new(),
                presentation_settlement_capability: None,
            },
        );
    let mut harness = KittestDockspaceHarness(harness);
    harness.run_steps(2);
    harness
}

fn layout() -> DockspaceLayout {
    DockspaceLayout::new([DockspaceSurfaceLayout::new(
        SURFACE,
        DockspaceRootLayout::new(ROOT, DockspaceNode::central_tabs([FIRST, SECOND])),
    )])
    .expect("the product fixture is valid")
}

fn editor_layout() -> DockspaceLayout {
    let main = DockspaceNode::split(
        DockspaceAxis::Horizontal,
        [
            (DockspaceNode::tabs([THIRD]), 0.24),
            (DockspaceNode::central_tabs([FIRST, SECOND]), 0.76),
        ],
    )
    .expect("the editor split fixture is valid");
    DockspaceLayout::new([DockspaceSurfaceLayout::new(
        SURFACE,
        DockspaceRootLayout::new(ROOT, main),
    )])
    .expect("the editor product layout is valid")
}

fn overflow_layout() -> DockspaceLayout {
    DockspaceLayout::new([DockspaceSurfaceLayout::new(
        SURFACE,
        DockspaceRootLayout::new(
            ROOT,
            DockspaceNode::central_tabs([FIRST, SECOND, THIRD, FOURTH]),
        ),
    )])
    .expect("the overflow product fixture is valid")
}

fn long_overflow_layout() -> DockspaceLayout {
    DockspaceLayout::new([DockspaceSurfaceLayout::new(
        SURFACE,
        DockspaceRootLayout::new(ROOT, DockspaceNode::central_tabs((1..=40).map(ItemId::new))),
    )])
    .expect("the long overflow product fixture is valid")
}

fn split_layout() -> DockspaceLayout {
    DockspaceLayout::new([DockspaceSurfaceLayout::new(
        SURFACE,
        DockspaceRootLayout::new(
            ROOT,
            DockspaceNode::equal_split(
                DockspaceAxis::Horizontal,
                [DockspaceNode::tabs([FIRST]), DockspaceNode::tabs([SECOND])],
            )
            .expect("the product split fixture is valid"),
        ),
    )])
    .expect("the product split layout is valid")
}

fn shared_semantic_layout() -> DockspaceLayout {
    DockspaceLayout::new([DockspaceSurfaceLayout::new(
        SURFACE,
        DockspaceRootLayout::new(
            ROOT,
            DockspaceNode::equal_split(
                DockspaceAxis::Horizontal,
                [
                    DockspaceNode::tabs([FIRST, SECOND]),
                    DockspaceNode::tabs([THIRD]),
                ],
            )
            .expect("the shared semantic split is valid"),
        ),
    )
    .with_contained(DockspaceContainedLayout::new(
        FLOATING,
        DockspaceRootLayout::new(FLOATING_ROOT, DockspaceNode::tabs([FOURTH])),
        LogicalRect::new(500.0, 260.0, 240.0, 220.0)
            .expect("the shared contained rectangle is valid"),
    ))])
    .expect("the shared semantic product layout is valid")
}

fn splitter_junction_layout() -> DockspaceLayout {
    let column = |top, bottom| {
        DockspaceNode::equal_split(
            DockspaceAxis::Vertical,
            [DockspaceNode::tabs([top]), DockspaceNode::tabs([bottom])],
        )
        .expect("the product splitter column is valid")
    };
    DockspaceLayout::new([DockspaceSurfaceLayout::new(
        SURFACE,
        DockspaceRootLayout::new(
            ROOT,
            DockspaceNode::equal_split(
                DockspaceAxis::Horizontal,
                [column(FIRST, SECOND), column(THIRD, FOURTH)],
            )
            .expect("the product splitter junction fixture is valid"),
        ),
    )])
    .expect("the product splitter junction layout is valid")
}

fn contained_layout() -> DockspaceLayout {
    contained_layout_at(
        LogicalRect::new(500.0, 260.0, 240.0, 220.0)
            .expect("the contained product fixture rectangle is valid"),
    )
}

fn contained_layout_at(rect: LogicalRect) -> DockspaceLayout {
    DockspaceLayout::new([DockspaceSurfaceLayout::new(
        SURFACE,
        DockspaceRootLayout::new(ROOT, DockspaceNode::central_tabs([FIRST])),
    )
    .with_contained(DockspaceContainedLayout::new(
        FLOATING,
        DockspaceRootLayout::new(FLOATING_ROOT, DockspaceNode::tabs([SECOND])),
        rect,
    ))])
    .expect("the contained product layout is valid")
}

fn stacked_contained_layout() -> DockspaceLayout {
    let rear =
        LogicalRect::new(80.0, 80.0, 300.0, 220.0).expect("the rear contained rectangle is valid");
    let front = LogicalRect::new(300.0, 240.0, 300.0, 220.0)
        .expect("the front contained rectangle is valid");
    DockspaceLayout::new([DockspaceSurfaceLayout::new(
        SURFACE,
        DockspaceRootLayout::new(ROOT, DockspaceNode::central_tabs([FIRST, THIRD])),
    )
    .with_contained(DockspaceContainedLayout::new(
        FLOATING,
        DockspaceRootLayout::new(FLOATING_ROOT, DockspaceNode::tabs([SECOND])),
        rear,
    ))
    .with_contained(DockspaceContainedLayout::new(
        FRONT_FLOATING,
        DockspaceRootLayout::new(FRONT_FLOATING_ROOT, DockspaceNode::tabs([FOURTH])),
        front,
    ))])
    .expect("the stacked contained product layout is valid")
}

fn fully_occluded_stacked_contained_layout() -> DockspaceLayout {
    let rear = LogicalRect::new(220.0, 180.0, 240.0, 180.0)
        .expect("the rear contained rectangle is valid");
    let front = LogicalRect::new(180.0, 140.0, 320.0, 260.0)
        .expect("the front contained rectangle is valid");
    DockspaceLayout::new([DockspaceSurfaceLayout::new(
        SURFACE,
        DockspaceRootLayout::new(ROOT, DockspaceNode::central_tabs([FIRST, THIRD])),
    )
    .with_contained(DockspaceContainedLayout::new(
        FLOATING,
        DockspaceRootLayout::new(FLOATING_ROOT, DockspaceNode::tabs([SECOND])),
        rear,
    ))
    .with_contained(DockspaceContainedLayout::new(
        FRONT_FLOATING,
        DockspaceRootLayout::new(FRONT_FLOATING_ROOT, DockspaceNode::tabs([FOURTH])),
        front,
    ))])
    .expect("the fully occluded contained product layout is valid")
}

fn pointer_button(pos: Pos2, pressed: bool) -> Event {
    Event::PointerButton {
        pos,
        button: PointerButton::Primary,
        pressed,
        modifiers: Modifiers::NONE,
    }
}

fn mouse_wheel(delta: egui::Vec2) -> Event {
    Event::MouseWheel {
        unit: egui::MouseWheelUnit::Point,
        delta,
        phase: egui::TouchPhase::Move,
        source: egui::MouseWheelSource::Wheel,
        modifiers: Modifiers::NONE,
    }
}

fn key_press(key: Key) -> Vec<Event> {
    [true, false]
        .into_iter()
        .map(|pressed| Event::Key {
            key,
            physical_key: Some(key),
            pressed,
            repeat: false,
            modifiers: Modifiers::NONE,
        })
        .collect()
}

fn accesskit_action(target_node: egui::accesskit::NodeId, action: Action) -> Event {
    Event::AccessKitActionRequest(ActionRequest {
        action,
        target_tree: TreeId::ROOT,
        target_node,
        data: None,
    })
}

#[test]
fn default_features_render_a_ready_product_surface() {
    let context = Context::default();
    context.enable_accesskit();
    let mut dockspace = Dockspace::builder("product-ready", layout())
        .build()
        .expect("the product facade initializes");
    let mut panes = Panes;

    let first = run_frame(&context, &mut dockspace, &mut panes, Vec::new());
    assert!(
        first.output.platform_output.accesskit_update.is_some(),
        "the bootstrap frame still publishes a valid accessibility tree",
    );
    let ready = run_frame(&context, &mut dockspace, &mut panes, Vec::new());
    let _ = tab_center(&ready.output, "First");
    let _ = tab_center(&ready.output, "Second");
}

#[test]
fn default_visuals_follow_the_current_egui_theme() {
    let context = Context::default();
    let dark_panel = egui::Color32::from_rgb(17, 23, 31);
    let dark_bar = egui::Color32::from_rgb(5, 7, 11);
    let mut dark = egui::Visuals::dark();
    dark.panel_fill = dark_panel;
    dark.extreme_bg_color = dark_bar;
    context.set_visuals(dark);

    let mut dockspace = Dockspace::builder("product-theme-following", layout())
        .build()
        .expect("the themed product facade initializes");
    let mut panes = Panes;
    let _ = run_frame(&context, &mut dockspace, &mut panes, Vec::new());
    let dark_output = run_frame(&context, &mut dockspace, &mut panes, Vec::new());
    assert!(rect_fill_count(&dark_output.output, dark_panel) > 0);
    assert!(rect_fill_count(&dark_output.output, dark_bar) > 0);

    let light_panel = egui::Color32::from_rgb(241, 244, 248);
    let light_bar = egui::Color32::from_rgb(211, 217, 225);
    let mut light = egui::Visuals::light();
    light.panel_fill = light_panel;
    light.extreme_bg_color = light_bar;
    let expected_light_bar: egui::Color32 =
        (egui::Rgba::from(light_panel) * egui::Rgba::from_gray(0.8)).into();
    context.set_visuals(light);
    let light_output = run_frame(&context, &mut dockspace, &mut panes, Vec::new());
    assert!(rect_fill_count(&light_output.output, light_panel) > 0);
    assert!(rect_fill_count(&light_output.output, expected_light_bar) > 0);
    assert_eq!(rect_fill_count(&light_output.output, dark_panel), 0);
    assert_eq!(rect_fill_count(&light_output.output, dark_bar), 0);
}

#[test]
fn contained_panes_use_the_egui_window_fill() {
    let context = Context::default();
    let panel_fill = egui::Color32::from_rgb(23, 31, 43);
    let window_fill = egui::Color32::from_rgb(67, 79, 101);
    let mut visuals = egui::Visuals::dark();
    visuals.panel_fill = panel_fill;
    visuals.window_fill = window_fill;
    context.set_visuals(visuals);
    let mut dockspace = Dockspace::builder("product-contained-window-fill", contained_layout())
        .build()
        .expect("the themed contained facade initializes");
    let mut panes = Panes;

    let _ = run_frame(&context, &mut dockspace, &mut panes, Vec::new());
    let ready = run_frame(&context, &mut dockspace, &mut panes, Vec::new());
    let contained_content = |rect: &egui::epaint::RectShape| {
        rect.rect.min.x >= 500.0 && rect.rect.min.y > 280.0 && rect.rect.width() > 200.0
    };

    assert!(ready.output.shapes.iter().any(|shape| {
        matches!(
            &shape.shape,
            egui::Shape::Rect(rect) if rect.fill == window_fill && contained_content(rect)
        )
    }));
    assert!(!ready.output.shapes.iter().any(|shape| {
        matches!(
            &shape.shape,
            egui::Shape::Rect(rect) if rect.fill == panel_fill && contained_content(rect)
        )
    }));
}

#[test]
fn frontmost_contained_title_uses_the_egui_open_window_visual() {
    let context = Context::default();
    let window_fill = egui::Color32::from_rgb(45, 55, 72);
    let active_title_fill = egui::Color32::from_rgb(121, 77, 148);
    let mut visuals = egui::Visuals::dark();
    visuals.window_fill = window_fill;
    visuals.widgets.open.weak_bg_fill = active_title_fill;
    context.set_visuals(visuals);
    let mut dockspace =
        Dockspace::builder("product-contained-active-title", stacked_contained_layout())
            .build()
            .expect("the stacked contained facade initializes");
    let mut panes = Panes;

    let _ = run_frame(&context, &mut dockspace, &mut panes, Vec::new());
    let ready = run_frame(&context, &mut dockspace, &mut panes, Vec::new());
    let front = dockspace
        .view()
        .contained(FRONT_FLOATING)
        .expect("the front contained window remains present")
        .rect();
    let border = dockspace.style().floating_border_width;
    let title = Rect::from_min_size(
        Pos2::new(
            front.min().x() as f32 + border,
            front.min().y() as f32 + border,
        ),
        vec2(
            front.width() as f32 - 2.0 * border,
            dockspace.style().floating_title_height,
        ),
    );

    assert_eq!(rect_fill_count(&ready.output, active_title_fill), 1);
    assert!(
        rect_with_fill(&ready.output, active_title_fill)
            .is_some_and(|rect| title.contains(rect.center())),
        "only the core-frontmost contained title receives egui's open-window fill",
    );
}

#[test]
fn visual_override_wins_without_detaching_unspecified_theme_tokens() {
    let context = Context::default();
    let panel = egui::Color32::from_rgb(36, 41, 47);
    let bar = egui::Color32::from_rgb(9, 13, 17);
    let workspace_override = egui::Color32::from_rgb(71, 37, 91);
    let mut visuals = egui::Visuals::dark();
    visuals.panel_fill = panel;
    visuals.extreme_bg_color = bar;
    context.set_visuals(visuals);
    let mut style = DockStyle::default();
    style.visuals.workspace_fill = Some(workspace_override);
    let mut dockspace = Dockspace::builder("product-theme-override", layout())
        .style(style)
        .build()
        .expect("the overridden product facade initializes");
    let mut panes = Panes;

    let _ = run_frame(&context, &mut dockspace, &mut panes, Vec::new());
    let ready = run_frame(&context, &mut dockspace, &mut panes, Vec::new());
    assert!(rect_fill_count(&ready.output, workspace_override) > 0);
    assert!(rect_fill_count(&ready.output, bar) > 0);
    assert!(rect_fill_count(&ready.output, panel) > 0);
}

#[test]
fn default_splitter_visuals_follow_egui_response_state() {
    let context = Context::default();
    let idle = egui::Color32::from_rgb(19, 29, 39);
    let hovered = egui::Color32::from_rgb(61, 127, 149);
    let active = egui::Color32::from_rgb(191, 97, 53);
    let mut visuals = egui::Visuals::dark();
    visuals.extreme_bg_color = idle;
    visuals.widgets.hovered.fg_stroke = egui::Stroke::new(2.0, hovered);
    visuals.widgets.active.fg_stroke = egui::Stroke::new(3.0, active);
    context.set_visuals(visuals);
    let mut dockspace = Dockspace::builder("product-response-splitter", split_layout())
        .build()
        .expect("the response-driven splitter facade initializes");
    let mut panes = Panes;

    let _ = run_frame(&context, &mut dockspace, &mut panes, Vec::new());
    let stable = run_frame(&context, &mut dockspace, &mut panes, Vec::new());
    let splitter = splitter_rect(&stable.output, idle, hovered);
    let point = splitter.center();
    let hovered_output = run_frame(
        &context,
        &mut dockspace,
        &mut panes,
        vec![Event::PointerMoved(point)],
    );
    assert!(rect_fill_count(&hovered_output.output, hovered) > 0);

    let active_output = run_frame(
        &context,
        &mut dockspace,
        &mut panes,
        vec![pointer_button(point, true)],
    );
    assert!(rect_fill_count(&active_output.output, active) > 0);
}

#[test]
fn default_close_visuals_use_egui_interaction_strokes() {
    let context = Context::default();
    context.enable_accesskit();
    let idle = egui::Color32::from_rgb(31, 47, 59);
    let hovered = egui::Color32::from_rgb(79, 143, 167);
    let active = egui::Color32::from_rgb(211, 101, 67);
    let mut visuals = egui::Visuals::dark();
    visuals.widgets.inactive.fg_stroke = egui::Stroke::new(1.0, idle);
    visuals.widgets.hovered.fg_stroke = egui::Stroke::new(2.0, hovered);
    visuals.widgets.active.fg_stroke = egui::Stroke::new(3.0, active);
    context.set_visuals(visuals);
    let mut dockspace = Dockspace::builder("product-response-close", layout())
        .build()
        .expect("the response-driven close facade initializes");
    let mut panes = Panes;

    let _ = run_frame(&context, &mut dockspace, &mut panes, Vec::new());
    let stable = run_frame(&context, &mut dockspace, &mut panes, Vec::new());
    let close = node_rect(&stable.output, Role::Button, "Close First");
    assert!(line_segment_count_in(&stable.output, idle, close.expand(4.0)) >= 2);

    let hovered_output = run_frame(
        &context,
        &mut dockspace,
        &mut panes,
        vec![Event::PointerMoved(close.center())],
    );
    assert!(line_segment_count_in(&hovered_output.output, hovered, close.expand(4.0)) >= 2);

    let active_output = run_frame(
        &context,
        &mut dockspace,
        &mut panes,
        vec![pointer_button(close.center(), true)],
    );
    assert!(line_segment_count_in(&active_output.output, active, close.expand(4.0)) >= 2);
}

#[test]
fn selected_tab_text_override_does_not_leak_into_close_interaction() {
    let context = Context::default();
    context.enable_accesskit();
    let tab_text = egui::Color32::from_rgb(43, 157, 181);
    let selected_text = egui::Color32::from_rgb(229, 83, 61);
    let hovered_text = egui::Color32::from_rgb(103, 211, 139);
    let mut visuals = egui::Visuals::dark();
    visuals.widgets.hovered.fg_stroke = egui::Stroke::new(2.0, hovered_text);
    context.set_visuals(visuals);
    let mut style = DockStyle::default();
    style.visuals.tab_text_color = Some(tab_text);
    style.visuals.tab_active_text_color = Some(selected_text);
    let mut dockspace = Dockspace::builder("product-close-color-roles", layout())
        .style(style)
        .build()
        .expect("the role-specific close facade initializes");
    let mut panes = Panes;

    let _ = run_frame(&context, &mut dockspace, &mut panes, Vec::new());
    let stable = run_frame(&context, &mut dockspace, &mut panes, Vec::new());
    let close = node_rect(&stable.output, Role::Button, "Close First");
    let hovered = run_frame(
        &context,
        &mut dockspace,
        &mut panes,
        vec![Event::PointerMoved(close.center())],
    );

    assert!(line_segment_count_in(&hovered.output, hovered_text, close.expand(4.0)) >= 2);
    assert_eq!(
        line_segment_count_in(&hovered.output, tab_text, close.expand(4.0)),
        0,
        "the idle tab text override must not replace egui's hovered control stroke",
    );
    assert_eq!(
        line_segment_count_in(&hovered.output, selected_text, close.expand(4.0)),
        0,
        "selected-tab text color is not a generic hovered-control color",
    );
}

#[test]
fn inactive_tab_close_hides_only_its_paint_until_emphasized() {
    let context = Context::default();
    context.enable_accesskit();
    let idle = egui::Color32::from_rgb(31, 47, 59);
    let hovered = egui::Color32::from_rgb(79, 143, 167);
    let mut visuals = egui::Visuals::dark();
    visuals.widgets.inactive.fg_stroke = egui::Stroke::new(1.0, idle);
    visuals.widgets.hovered.fg_stroke = egui::Stroke::new(2.0, hovered);
    context.set_visuals(visuals);
    let mut dockspace = Dockspace::builder("product-inactive-close-visibility", layout())
        .build()
        .expect("the close visibility facade initializes");
    let mut panes = Panes;

    let _ = run_frame(&context, &mut dockspace, &mut panes, Vec::new());
    let stable = run_frame(&context, &mut dockspace, &mut panes, Vec::new());
    let close = node_rect(&stable.output, Role::Button, "Close Second");
    let first_tab = node_rect(&stable.output, Role::Tab, "First");
    let second_tab = node_rect(&stable.output, Role::Tab, "Second");
    assert_eq!(
        line_segment_count_in(&stable.output, idle, close.expand(4.0)),
        0,
        "the inactive close keeps its exact response bounds without idle icon noise"
    );

    let hover = run_frame(
        &context,
        &mut dockspace,
        &mut panes,
        vec![Event::PointerMoved(close.center())],
    );
    assert!(line_segment_count_in(&hover.output, hovered, close.expand(4.0)) >= 2);
    assert_eq!(
        node_rect(&hover.output, Role::Button, "Close Second"),
        close,
        "paint visibility must not change the reserved close geometry"
    );
    assert_eq!(
        node_rect(&hover.output, Role::Tab, "First"),
        first_tab,
        "close hover must not move the selected tab row"
    );
    assert_eq!(
        node_rect(&hover.output, Role::Tab, "Second"),
        second_tab,
        "close hover must not change the inactive tab row height"
    );
}

#[test]
fn tab_close_semantics_follow_the_exact_close_region_occlusion() {
    let base_context = Context::default();
    base_context.enable_accesskit();
    let mut base = Dockspace::builder("product-close-occlusion-basis", layout())
        .build()
        .expect("the close-occlusion basis initializes");
    let mut base_panes = Panes;
    let _ = run_frame(&base_context, &mut base, &mut base_panes, Vec::new());
    let stable = run_frame(&base_context, &mut base, &mut base_panes, Vec::new());
    let close = node_rect(&stable.output, Role::Button, "Close Second");
    let occluder = LogicalRect::new(f64::from((close.min.x - 2.0).max(0.0)), 0.0, 180.0, 160.0)
        .expect("the close-region occluder is valid");
    let occluded_layout = DockspaceLayout::new([DockspaceSurfaceLayout::new(
        SURFACE,
        DockspaceRootLayout::new(ROOT, DockspaceNode::central_tabs([FIRST, SECOND])),
    )
    .with_contained(DockspaceContainedLayout::new(
        FLOATING,
        DockspaceRootLayout::new(FLOATING_ROOT, DockspaceNode::tabs([FOURTH])),
        occluder,
    ))])
    .expect("the close-region occlusion layout is valid");

    let context = Context::default();
    context.enable_accesskit();
    let mut dockspace = Dockspace::builder("product-close-exact-occlusion", occluded_layout)
        .build()
        .expect("the close-region occlusion facade initializes");
    let mut panes = Panes;
    let _ = run_frame(&context, &mut dockspace, &mut panes, Vec::new());
    let stable = run_frame(&context, &mut dockspace, &mut panes, Vec::new());
    let (_, tab) = accesskit_node(&stable.output, Role::Tab, "Second");
    assert!(!tab.is_disabled());
    assert!(tab.supports_action(Action::Click));
    let (close, close_node) = accesskit_node(&stable.output, Role::Button, "Close Second");
    assert!(close_node.is_disabled());
    assert!(!close_node.supports_action(Action::Click));

    let attempted = run_frame(
        &context,
        &mut dockspace,
        &mut panes,
        vec![accesskit_action(close, Action::Click)],
    );
    assert!(
        attempted.close_requests.is_empty(),
        "an occluded exact close region must not publish an executable close request",
    );
}

#[test]
fn default_features_expose_item_centric_contained_actions() {
    let context = Context::default();
    let mut dockspace = Dockspace::builder("product-contained-actions", layout())
        .build()
        .expect("the product facade initializes");
    let mut panes = Panes;

    let _ = run_frame(&context, &mut dockspace, &mut panes, Vec::new());
    let _ = run_frame(&context, &mut dockspace, &mut panes, Vec::new());

    let first_float = dockspace
        .float_item_current(SECOND, SURFACE)
        .expect("the first item floats through the product facade");
    assert!(
        matches!(
            first_float.status(),
            DockspaceActionStatus::Applied(DockspaceActionOutcome::Floated { item: SECOND, .. })
        ),
        "the core-derived item float must apply: {first_float:?}",
    );
    let second_floating = dockspace
        .view()
        .item(SECOND)
        .and_then(|item| item.contained())
        .expect("the floated item owns a contained presentation");

    let _ = run_frame(&context, &mut dockspace, &mut panes, Vec::new());
    let _ = run_frame(&context, &mut dockspace, &mut panes, Vec::new());
    let second_float = dockspace
        .float_item_current(FIRST, SURFACE)
        .expect("the singleton main root floats through the product facade");
    assert!(
        matches!(
            second_float.status(),
            DockspaceActionStatus::Applied(DockspaceActionOutcome::Floated { item: FIRST, .. })
        ),
        "the second core-derived item float must apply: {second_float:?}",
    );
    let first_floating = dockspace
        .view()
        .item(FIRST)
        .and_then(|item| item.contained())
        .expect("the second floated item owns a contained presentation");
    assert_eq!(
        dockspace
            .view()
            .surface(SURFACE)
            .expect("the surface remains available")
            .contained()
            .map(|contained| contained.id())
            .collect::<Vec<_>>(),
        vec![second_floating, first_floating],
    );

    dockspace
        .raise_contained_current(SECOND)
        .expect("contained raise through the product facade");
    assert_eq!(
        dockspace
            .view()
            .surface(SURFACE)
            .expect("the surface remains available")
            .contained()
            .map(|contained| contained.id())
            .collect::<Vec<_>>(),
        vec![first_floating, second_floating],
    );
}

#[test]
fn default_features_bring_contained_content_into_current_ready_bounds() {
    let context = Context::default();
    let offscreen = LogicalRect::new(900.0, 700.0, 240.0, 220.0)
        .expect("the offscreen contained rectangle is valid");
    let mut dockspace = Dockspace::builder(
        "product-contained-bring-into-view",
        contained_layout_at(offscreen),
    )
    .build()
    .expect("the product contained facade initializes");
    let mut panes = Panes;
    let roster = dockspace
        .view()
        .surface(SURFACE)
        .expect("the surface is present")
        .contained()
        .map(|contained| contained.id())
        .collect::<Vec<_>>();

    let _ = run_frame(&context, &mut dockspace, &mut panes, Vec::new());
    let _ = run_frame(&context, &mut dockspace, &mut panes, Vec::new());
    let result = dockspace
        .bring_contained_into_view_current(SECOND)
        .expect("the public product action uses current ready bounds");
    assert!(
        matches!(
            result.status(),
            DockspaceActionStatus::Applied(DockspaceActionOutcome::ContainedBoundsUpdated {
                root: FLOATING_ROOT,
                surface: SURFACE,
                floating: FLOATING,
                changed: true,
            })
        ),
        "unexpected bring-into-view status: {:?}",
        result.status(),
    );

    let rect = dockspace
        .view()
        .contained(FLOATING)
        .expect("the contained presentation remains available")
        .rect();
    assert_eq!(rect.size(), offscreen.size());
    assert!(rect.min().x() >= 0.0 && rect.min().y() >= 0.0);
    assert!(rect.max().x() <= 800.0 && rect.max().y() <= 600.0);
    assert_eq!(
        dockspace
            .view()
            .surface(SURFACE)
            .expect("the surface remains present")
            .contained()
            .map(|contained| contained.id())
            .collect::<Vec<_>>(),
        roster,
        "bring-into-view must not change contained stacking order",
    );
}

#[test]
fn default_features_contained_chrome_exposes_focusable_accessibility_controls() {
    let context = Context::default();
    context.enable_accesskit();
    let mut dockspace = Dockspace::builder("product-contained-accessibility", contained_layout())
        .build()
        .expect("the product contained facade initializes");
    let mut panes = Panes;

    let _ = run_frame(&context, &mut dockspace, &mut panes, Vec::new());
    let stable = run_frame(&context, &mut dockspace, &mut panes, Vec::new());
    let (title, title_node) = accesskit_node(&stable.output, Role::TitleBar, "Second");
    assert!(title_node.supports_action(Action::Focus));
    assert_eq!(
        title_node.description(),
        Some("Drag to move floating window"),
        "the title bar must expose its direct-manipulation affordance"
    );
    let (_, close_node) = accesskit_node(&stable.output, Role::Button, "Close floating Second");
    assert!(close_node.supports_action(Action::Focus));
    assert!(close_node.supports_action(Action::Click));

    let focused = run_frame(
        &context,
        &mut dockspace,
        &mut panes,
        vec![accesskit_action(title, Action::Focus)],
    );
    assert_eq!(
        focused
            .output
            .platform_output
            .accesskit_update
            .as_ref()
            .expect("AccessKit remains enabled")
            .focus,
        title,
    );
}

#[test]
fn contained_move_resize_select_focus_and_dock_back_preserve_exact_ownership() {
    let context = Context::default();
    context.enable_accesskit();
    let mut dockspace = Dockspace::builder("product-contained-lifecycle-focus", contained_layout())
        .build()
        .expect("the contained lifecycle facade initializes");
    let mut panes = FocusPanes::new("product-contained-lifecycle-focus", true);

    let _ = run_focus_frame(&context, &mut dockspace, &mut panes, Vec::new());
    let _ = run_focus_frame(&context, &mut dockspace, &mut panes, Vec::new());
    let initial = contained_rect(&dockspace);
    let move_source = contained_title_point(&dockspace);
    let move_target = move_source + vec2(-80.0, 60.0);
    let _ = run_focus_frame(
        &context,
        &mut dockspace,
        &mut panes,
        vec![
            Event::PointerMoved(move_source),
            pointer_button(move_source, true),
        ],
    );
    let _ = run_focus_frame(
        &context,
        &mut dockspace,
        &mut panes,
        vec![Event::PointerMoved(move_target)],
    );
    let _ = run_focus_frame(
        &context,
        &mut dockspace,
        &mut panes,
        vec![
            Event::PointerMoved(move_target),
            pointer_button(move_target, false),
        ],
    );
    let _ = run_focus_frame(&context, &mut dockspace, &mut panes, Vec::new());
    let moved = contained_rect(&dockspace);
    assert_eq!(moved.size(), initial.size());
    assert_eq!(moved.min().x(), initial.min().x() - 80.0);
    assert_eq!(moved.min().y(), initial.min().y() + 60.0);
    assert_eq!(
        dockspace
            .view()
            .item(SECOND)
            .and_then(|item| item.contained()),
        Some(FLOATING),
        "contained motion must preserve exact item ownership"
    );

    let resize_source = Pos2::new(moved.max().x() as f32 - 1.0, moved.min().y() as f32 + 100.0);
    let resize_target = resize_source + vec2(30.0, 0.0);
    let _ = run_focus_frame(
        &context,
        &mut dockspace,
        &mut panes,
        vec![
            Event::PointerMoved(resize_source),
            pointer_button(resize_source, true),
        ],
    );
    let _ = run_focus_frame(
        &context,
        &mut dockspace,
        &mut panes,
        vec![Event::PointerMoved(resize_target)],
    );
    let _ = run_focus_frame(
        &context,
        &mut dockspace,
        &mut panes,
        vec![
            Event::PointerMoved(resize_target),
            pointer_button(resize_target, false),
        ],
    );
    let (stable, _) = run_focus_frame(&context, &mut dockspace, &mut panes, Vec::new());
    let resized = contained_rect(&dockspace);
    assert_eq!(resized.min(), moved.min());
    assert!(resized.width() > moved.width());
    assert_eq!(resized.height(), moved.height());
    assert_eq!(
        dockspace
            .view()
            .item(SECOND)
            .and_then(|item| item.contained()),
        Some(FLOATING),
        "contained resize must preserve exact item ownership"
    );

    let (second, _) = accesskit_node(&stable.output, Role::Tab, "Second");
    let _ = run_focus_frame(
        &context,
        &mut dockspace,
        &mut panes,
        vec![accesskit_action(second, Action::Click)],
    );
    let _ = run_focus_frame(&context, &mut dockspace, &mut panes, Vec::new());
    assert!(
        !context.memory(|memory| memory.has_focus(panes.target)),
        "the selection update publishes its intent before requesting pane focus"
    );
    let (_focused, capability) = run_focus_frame(&context, &mut dockspace, &mut panes, Vec::new());
    assert_eq!(capability, DockspaceCapability::Supported);
    assert!(context.memory(|memory| memory.has_focus(panes.target)));

    // Accept the exact presented focus acknowledgement before changing roots.
    let _ = run_focus_frame(&context, &mut dockspace, &mut panes, Vec::new());
    dockspace
        .dock_root_back_current(FLOATING_ROOT)
        .expect("the explicit Dock Back command preserves the source content");

    assert!(dockspace.view().contained(FLOATING).is_none());
    let tabs = dockspace
        .view()
        .root(ROOT)
        .and_then(|root| root.content())
        .and_then(|content| content.tabs())
        .expect("dock-back recovers the contained pane into the main tabs");
    assert_eq!(tabs.items(), &[FIRST, SECOND]);
    assert_eq!(
        tabs.selected(),
        Some(SECOND),
        "core must preserve the exact recovered pane instead of selecting the first visible tab"
    );
    assert!(
        context.memory(|memory| memory.has_focus(panes.target)),
        "same-context dock-back must preserve the exact focused pane target"
    );

    context.memory_mut(|memory| memory.surrender_focus(panes.target));
    assert!(!context.memory(|memory| memory.has_focus(panes.target)));
    let _ = run_focus_frame(&context, &mut dockspace, &mut panes, Vec::new());
    assert!(
        context.memory(|memory| memory.has_focus(panes.target)),
        "dock-back publishes a fresh request for the exact recovered pane"
    );
    let _ = run_focus_frame(&context, &mut dockspace, &mut panes, Vec::new());
    assert!(context.memory(|memory| memory.has_focus(panes.target)));
    context.memory_mut(|memory| memory.surrender_focus(panes.target));
    assert!(!context.memory(|memory| memory.has_focus(panes.target)));
    let _ = run_focus_frame(&context, &mut dockspace, &mut panes, Vec::new());
    assert!(
        !context.memory(|memory| memory.has_focus(panes.target)),
        "the terminal Focused acknowledgement settles recovery without reclaiming focus"
    );
}

#[test]
fn presentation_menu_floats_and_docks_back_with_truthful_native_availability() {
    let context = Context::default();
    context.enable_accesskit();
    let mut dockspace = Dockspace::builder("product-presentation-menu", layout())
        .build()
        .expect("the presentation-menu facade initializes");
    let mut panes = Panes;

    let _ = run_frame(&context, &mut dockspace, &mut panes, Vec::new());
    let stable = run_frame(&context, &mut dockspace, &mut panes, Vec::new());
    let (menu, menu_node) = accesskit_node(&stable.output, Role::Button, "Presentation commands");
    assert!(!menu_node.is_disabled());

    let sizing = run_frame(
        &context,
        &mut dockspace,
        &mut panes,
        vec![accesskit_action(menu, Action::Click)],
    );
    assert!(sizing.close_requests.is_empty());
    let opened = run_frame(&context, &mut dockspace, &mut panes, Vec::new());
    assert!(opened.close_requests.is_empty());
    let (float, float_node) = accesskit_node(&opened.output, Role::MenuItem, "Float");
    let (_, native_node) = accesskit_node(&opened.output, Role::MenuItem, "Move to New Window");
    let (_, dock_back_node) = accesskit_node(&opened.output, Role::MenuItem, "Dock Back");
    assert!(
        !float_node.is_disabled(),
        "{float_node:#?}; description={:?}",
        float_node.description()
    );
    assert!(native_node.is_disabled());
    assert!(dock_back_node.is_disabled());
    assert_eq!(
        surface_ids(&dockspace),
        [SURFACE],
        "the product starts with one same-context logical surface"
    );

    let floated = run_frame(
        &context,
        &mut dockspace,
        &mut panes,
        vec![accesskit_action(float, Action::Click)],
    );
    assert!(floated.close_requests.is_empty());
    assert!(dockspace.view().item(SECOND).unwrap().contained().is_none());
    let floated = run_frame(&context, &mut dockspace, &mut panes, Vec::new());
    assert!(floated.close_requests.is_empty());
    let floating_id = dockspace
        .view()
        .item(SECOND)
        .and_then(|item| item.contained())
        .expect("Float must move the complete root into an egui-native contained window");
    for item in [FIRST, SECOND] {
        let location = dockspace
            .view()
            .item(item)
            .expect("Float preserves every item");
        assert_eq!(location.contained(), Some(floating_id));
        assert_eq!(location.tabs().items(), &[FIRST, SECOND]);
        assert_eq!(location.tabs().selected(), Some(FIRST));
    }
    assert_eq!(
        surface_ids(&dockspace),
        [SURFACE],
        "same-context Float must not synthesize a native logical surface or binding"
    );

    let floating = run_frame(&context, &mut dockspace, &mut panes, Vec::new());
    assert!(floating.close_requests.is_empty());
    let (menu, _) = accesskit_node(&floating.output, Role::Button, "Presentation commands");
    let sizing = run_frame(
        &context,
        &mut dockspace,
        &mut panes,
        vec![accesskit_action(menu, Action::Click)],
    );
    assert!(sizing.close_requests.is_empty());
    let opened = run_frame(&context, &mut dockspace, &mut panes, Vec::new());
    assert!(opened.close_requests.is_empty());
    let (_, float_node) = accesskit_node(&opened.output, Role::MenuItem, "Float");
    let (_, native_node) = accesskit_node(&opened.output, Role::MenuItem, "Move to New Window");
    let (dock_back, dock_back_node) = accesskit_node(&opened.output, Role::MenuItem, "Dock Back");
    assert!(float_node.is_disabled());
    assert!(
        native_node.is_disabled(),
        "contained ownership does not promote the product into a native host"
    );
    assert!(!dock_back_node.is_disabled());

    let docked = run_frame(
        &context,
        &mut dockspace,
        &mut panes,
        vec![accesskit_action(dock_back, Action::Click)],
    );
    assert!(
        docked.close_requests.is_empty(),
        "Dock Back must move the root without requesting permission to destroy its content",
    );
    assert!(dockspace.view().contained(floating_id).is_some());
    let docked = run_frame(&context, &mut dockspace, &mut panes, Vec::new());
    assert!(docked.close_requests.is_empty());
    assert!(dockspace.view().contained(floating_id).is_none());
    let main = dockspace
        .view()
        .surface(SURFACE)
        .and_then(|surface| surface.main_root())
        .expect("Dock Back recovers the complete root as the main presentation");
    for item in [FIRST, SECOND] {
        let location = dockspace
            .view()
            .item(item)
            .expect("Dock Back preserves every item");
        assert_eq!(location.root(), main.id());
        assert_eq!(location.surface(), SURFACE);
        assert_eq!(location.contained(), None);
        assert_eq!(location.tabs().items(), &[FIRST, SECOND]);
        assert_eq!(location.tabs().selected(), Some(FIRST));
    }
    assert_eq!(
        surface_ids(&dockspace),
        [SURFACE],
        "same-context Dock Back must preserve the sole logical surface"
    );
    let settled = run_frame(&context, &mut dockspace, &mut panes, Vec::new());
    assert!(
        settled.close_requests.is_empty(),
        "Dock Back must not publish a delayed close plan"
    );
}

#[test]
fn contained_presentation_menu_exposes_and_executes_directional_commands() {
    let cases = [
        (
            "Dock Left",
            Some((DockspaceAxis::Horizontal, vec![vec![SECOND], vec![FIRST]])),
            None,
        ),
        (
            "Dock Right",
            Some((DockspaceAxis::Horizontal, vec![vec![FIRST], vec![SECOND]])),
            None,
        ),
        (
            "Dock Top",
            Some((DockspaceAxis::Vertical, vec![vec![SECOND], vec![FIRST]])),
            None,
        ),
        (
            "Dock Bottom",
            Some((DockspaceAxis::Vertical, vec![vec![FIRST], vec![SECOND]])),
            None,
        ),
        ("Dock Center", None, Some(vec![FIRST, SECOND])),
    ];

    for (label, expected_split, expected_tabs) in cases {
        let context = Context::default();
        context.enable_accesskit();
        let mut dockspace = Dockspace::builder(
            ("product-directional-presentation-menu", label),
            contained_layout(),
        )
        .build()
        .expect("the directional presentation-menu facade initializes");
        let mut panes = Panes;

        let _ = run_frame(&context, &mut dockspace, &mut panes, Vec::new());
        let stable = run_frame(&context, &mut dockspace, &mut panes, Vec::new());
        let contained_rect = dockspace
            .view()
            .contained(FLOATING)
            .expect("the contained presentation remains available")
            .rect();
        let tree = stable
            .output
            .platform_output
            .accesskit_update
            .as_ref()
            .expect("AccessKit is enabled");
        let (menu, menu_node) = tree
            .nodes
            .iter()
            .find_map(|(id, node)| {
                let bounds = node.bounds()?;
                let center_x = (bounds.x0 + bounds.x1) * 0.5;
                let center_y = (bounds.y0 + bounds.y1) * 0.5;
                (node.role() == Role::Button
                    && node.label() == Some("Presentation commands")
                    && center_x >= contained_rect.min().x()
                    && center_x <= contained_rect.max().x()
                    && center_y >= contained_rect.min().y()
                    && center_y <= contained_rect.max().y())
                .then_some((*id, node))
            })
            .expect("the contained title exposes its presentation menu");
        assert!(!menu_node.is_disabled());

        let _ = run_frame(
            &context,
            &mut dockspace,
            &mut panes,
            vec![accesskit_action(menu, Action::Click)],
        );
        let opened = run_frame(&context, &mut dockspace, &mut panes, Vec::new());
        for command_label in [
            "Dock Left",
            "Dock Right",
            "Dock Top",
            "Dock Bottom",
            "Dock Center",
        ] {
            let (_, node) = accesskit_node(&opened.output, Role::MenuItem, command_label);
            assert!(!node.is_disabled(), "{command_label} must be executable",);
            assert!(
                node.supports_action(Action::Click),
                "{command_label} must expose an AccessKit click action",
            );
        }
        let (command, _) = accesskit_node(&opened.output, Role::MenuItem, label);

        let _ = run_frame(
            &context,
            &mut dockspace,
            &mut panes,
            vec![accesskit_action(command, Action::Click)],
        );
        assert!(dockspace.view().contained(FLOATING).is_some());
        let _ = run_frame(&context, &mut dockspace, &mut panes, Vec::new());
        assert!(
            dockspace.view().contained(FLOATING).is_none(),
            "{label} must consume the contained presentation",
        );
        assert_eq!(
            root_split(&dockspace),
            expected_split,
            "{label} must use the core-derived split target",
        );
        assert_eq!(
            root_tabs(&dockspace),
            expected_tabs,
            "{label} must use the core-derived central target",
        );
    }
}

#[test]
fn open_presentation_menu_closes_immediately_when_ui_becomes_disabled() {
    let context = Context::default();
    context.enable_accesskit();
    let mut dockspace = Dockspace::builder("product-open-disabled-presentation-menu", layout())
        .build()
        .expect("the presentation-menu facade initializes");
    let mut panes = Panes;

    let _ = run_frame(&context, &mut dockspace, &mut panes, Vec::new());
    let stable = run_frame(&context, &mut dockspace, &mut panes, Vec::new());
    let (menu, _) = accesskit_node(&stable.output, Role::Button, "Presentation commands");
    let _ = run_frame(
        &context,
        &mut dockspace,
        &mut panes,
        vec![accesskit_action(menu, Action::Click)],
    );
    let opened = run_frame(&context, &mut dockspace, &mut panes, Vec::new());
    let (_, opened_anchor) = accesskit_node(&opened.output, Role::Button, "Presentation commands");
    assert_eq!(opened_anchor.is_expanded(), Some(true));
    let _ = accesskit_node(&opened.output, Role::MenuItem, "Float");

    let disabled = run_disabled_frame(&context, &mut dockspace, &mut panes, Vec::new());
    let (_, disabled_anchor) =
        accesskit_node(&disabled.output, Role::Button, "Presentation commands");
    assert!(disabled_anchor.is_disabled());
    assert_eq!(
        disabled_anchor.is_expanded(),
        Some(false),
        "the disabled anchor must synchronously report its popup as collapsed",
    );
    let tree = disabled
        .output
        .platform_output
        .accesskit_update
        .as_ref()
        .expect("AccessKit is enabled");
    assert!(
        tree.nodes
            .iter()
            .all(|(_, node)| node.role() != Role::MenuItem),
        "the popup must stop rendering as soon as its anchor becomes disabled",
    );
}

#[test]
fn open_main_presentation_menu_closes_immediately_when_anchor_becomes_occluded() {
    let context = Context::default();
    context.enable_accesskit();
    let initial_rect = LogicalRect::new(40.0, 0.0, 180.0, 180.0)
        .expect("the initial contained rectangle is valid");
    let mut dockspace = Dockspace::builder(
        "product-open-occluded-presentation-menu",
        contained_layout_at(initial_rect),
    )
    .build()
    .expect("the presentation-menu occlusion facade initializes");
    let mut panes = Panes;

    let _ = run_frame(&context, &mut dockspace, &mut panes, Vec::new());
    let stable = run_frame(&context, &mut dockspace, &mut panes, Vec::new());
    let stable_tree = stable
        .output
        .platform_output
        .accesskit_update
        .as_ref()
        .expect("AccessKit is enabled");
    let (main_menu, main_anchor) = stable_tree
        .nodes
        .iter()
        .find_map(|(id, node)| {
            let is_main_anchor = node.role() == Role::Button
                && node.label() == Some("Presentation commands")
                && node.bounds().is_some_and(|bounds| bounds.x0 > 600.0);
            is_main_anchor.then_some((*id, node))
        })
        .expect("the unobscured main root exposes its presentation menu");
    assert!(!main_anchor.is_disabled());
    let (right_resize, _) =
        accesskit_node(&stable.output, Role::Splitter, "Resize floating right edge");

    let _ = run_frame(
        &context,
        &mut dockspace,
        &mut panes,
        vec![accesskit_action(main_menu, Action::Click)],
    );
    let opened = run_frame(&context, &mut dockspace, &mut panes, Vec::new());
    let opened_tree = opened
        .output
        .platform_output
        .accesskit_update
        .as_ref()
        .expect("AccessKit is enabled");
    let opened_anchor = opened_tree
        .nodes
        .iter()
        .find_map(|(id, node)| (*id == main_menu).then_some(node))
        .expect("the opened main presentation anchor remains stable");
    assert_eq!(opened_anchor.is_expanded(), Some(true));
    let _ = accesskit_node(&opened.output, Role::MenuItem, "Float");

    let mut occluded = None;
    for _ in 0..64 {
        let output = run_frame(
            &context,
            &mut dockspace,
            &mut panes,
            vec![accesskit_action(right_resize, Action::Increment)],
        );
        let tree = output
            .output
            .platform_output
            .accesskit_update
            .as_ref()
            .expect("AccessKit is enabled");
        let anchor_disabled = tree
            .nodes
            .iter()
            .find_map(|(id, node)| (*id == main_menu).then_some(node.is_disabled()))
            .expect("the main presentation anchor remains described");
        if anchor_disabled {
            occluded = Some(output);
            break;
        }
    }
    let occluded = occluded.expect("semantic resizing eventually occludes the main menu anchor");
    let occluded_tree = occluded
        .output
        .platform_output
        .accesskit_update
        .as_ref()
        .expect("AccessKit is enabled");
    let occluded_anchor = occluded_tree
        .nodes
        .iter()
        .find_map(|(id, node)| (*id == main_menu).then_some(node))
        .expect("the occluded main presentation anchor remains described");
    assert!(occluded_anchor.is_disabled());
    assert_eq!(
        occluded_anchor.is_expanded(),
        Some(false),
        "the occluded anchor must synchronously report its popup as collapsed",
    );
    assert!(
        occluded_tree
            .nodes
            .iter()
            .all(|(_, node)| node.role() != Role::MenuItem),
        "the stale popup must not remain above or intercept the front contained window",
    );
    assert!(
        occluded_tree.nodes.iter().any(|(id, node)| {
            *id != main_menu
                && node.role() == Role::Button
                && node.label() == Some("Presentation commands")
                && !node.is_disabled()
        }),
        "the front contained window keeps its own presentation anchor operable",
    );
}

#[test]
fn discarded_presentation_menu_activation_commits_exactly_once() {
    let context = Context::default();
    context.enable_accesskit();
    context.options_mut(|options| {
        options.max_passes = 4.try_into().expect("four is non-zero");
    });
    let mut dockspace = Dockspace::builder("product-discarded-presentation-menu", layout())
        .build()
        .expect("the multipass presentation-menu facade initializes");
    let mut panes = Panes;

    let _ = run_frame(&context, &mut dockspace, &mut panes, Vec::new());
    let stable = run_frame(&context, &mut dockspace, &mut panes, Vec::new());
    let (menu, _) = accesskit_node(&stable.output, Role::Button, "Presentation commands");
    let _ = run_frame(
        &context,
        &mut dockspace,
        &mut panes,
        vec![accesskit_action(menu, Action::Click)],
    );
    let opened = run_frame(&context, &mut dockspace, &mut panes, Vec::new());
    let (float, float_node) = accesskit_node(&opened.output, Role::MenuItem, "Float");
    assert!(!float_node.is_disabled());

    context
        .plugin_or_default::<LateDiscardPlugin>()
        .lock()
        .remaining = 2;
    let passes = run_two_discarded_dockspace_passes(
        &context,
        &mut dockspace,
        &mut panes,
        vec![accesskit_action(float, Action::Click)],
    );
    assert_eq!(passes, 3, "the terminal pass intentionally omits dockspace");
    assert_eq!(
        dockspace
            .view()
            .surface(SURFACE)
            .unwrap()
            .contained()
            .count(),
        0
    );
    let delivered = run_frame(&context, &mut dockspace, &mut panes, Vec::new());
    assert!(delivered.close_requests.is_empty());
    let surface = dockspace
        .view()
        .surface(SURFACE)
        .expect("the logical surface remains present");
    assert_eq!(
        surface.contained().count(),
        1,
        "discarded passes must retain the local command once without replaying it"
    );
    assert!(surface.main_root().is_none());
    let committed_version = dockspace.version();
    for _ in 0..2 {
        run_frame(&context, &mut dockspace, &mut panes, Vec::new());
        assert_eq!(dockspace.version(), committed_version);
        assert_eq!(
            dockspace
                .view()
                .surface(SURFACE)
                .unwrap()
                .contained()
                .count(),
            1
        );
    }
}

#[test]
fn disabled_ui_does_not_open_or_activate_presentation_commands() {
    let context = Context::default();
    context.enable_accesskit();
    let mut dockspace = Dockspace::builder("product-disabled-presentation-menu", layout())
        .build()
        .expect("the disabled presentation-menu facade initializes");
    let mut panes = Panes;

    let _ = run_frame(&context, &mut dockspace, &mut panes, Vec::new());
    let stable = run_frame(&context, &mut dockspace, &mut panes, Vec::new());
    let (menu, _) = accesskit_node(&stable.output, Role::Button, "Presentation commands");
    let disabled = run_disabled_frame(
        &context,
        &mut dockspace,
        &mut panes,
        vec![accesskit_action(menu, Action::Click)],
    );
    let (_, node) = accesskit_node(&disabled.output, Role::Button, "Presentation commands");
    assert!(node.is_disabled());
    assert!(!node.supports_action(Action::Click));
    assert!(
        dockspace
            .view()
            .item(SECOND)
            .and_then(|item| item.contained())
            .is_none(),
        "a delayed accessibility click must not float content through a disabled Ui",
    );

    let after = run_frame(&context, &mut dockspace, &mut panes, Vec::new());
    let tree = after
        .output
        .platform_output
        .accesskit_update
        .as_ref()
        .expect("AccessKit is enabled");
    assert!(
        tree.nodes
            .iter()
            .all(|(_, node)| node.role() != Role::MenuItem),
        "the disabled anchor must not leave its popup open",
    );
}

#[test]
fn fully_occluded_main_presentation_menu_is_accessibility_disabled() {
    let context = Context::default();
    context.enable_accesskit();
    let layout = DockspaceLayout::new([DockspaceSurfaceLayout::new(
        SURFACE,
        DockspaceRootLayout::new(ROOT, DockspaceNode::central_tabs([FIRST])),
    )
    .with_contained(DockspaceContainedLayout::new(
        FLOATING,
        DockspaceRootLayout::new(FLOATING_ROOT, DockspaceNode::tabs([SECOND])),
        LogicalRect::new(620.0, 0.0, 180.0, 180.0)
            .expect("the occluding contained rectangle is valid"),
    ))])
    .expect("the presentation-menu occlusion layout is valid");
    let mut dockspace = Dockspace::builder("product-occluded-presentation-menu", layout)
        .build()
        .expect("the occluded presentation-menu facade initializes");
    let mut panes = Panes;

    let _ = run_frame(&context, &mut dockspace, &mut panes, Vec::new());
    let stable = run_frame(&context, &mut dockspace, &mut panes, Vec::new());
    let tree = stable
        .output
        .platform_output
        .accesskit_update
        .as_ref()
        .expect("AccessKit is enabled");
    let anchors = tree
        .nodes
        .iter()
        .filter(|(_, node)| {
            node.role() == Role::Button && node.label() == Some("Presentation commands")
        })
        .collect::<Vec<_>>();
    assert_eq!(anchors.len(), 2);
    let (occluded_id, occluded) = anchors
        .iter()
        .copied()
        .find(|(_, node)| node.is_disabled())
        .expect("the main anchor under the front contained window is disabled");
    assert!(!occluded.supports_action(Action::Click));
    assert!(
        anchors.iter().any(|(_, node)| !node.is_disabled()),
        "the front contained title keeps its own command menu operable",
    );

    let _ = run_frame(
        &context,
        &mut dockspace,
        &mut panes,
        vec![accesskit_action(*occluded_id, Action::Click)],
    );
    assert!(
        dockspace
            .view()
            .item(FIRST)
            .and_then(|item| item.contained())
            .is_none(),
        "an occluded main command anchor must not execute through the front window",
    );
}

#[test]
fn disabled_ui_ignores_delayed_contained_accesskit_actions() {
    let context = Context::default();
    context.enable_accesskit();
    let mut dockspace = Dockspace::builder("product-disabled-contained-close", contained_layout())
        .build()
        .expect("the product contained facade initializes");
    let mut panes = Panes;

    let _ = run_frame(&context, &mut dockspace, &mut panes, Vec::new());
    let stable = run_frame(&context, &mut dockspace, &mut panes, Vec::new());
    let (close, _) = accesskit_node(&stable.output, Role::Button, "Close floating Second");
    let (right, _) = accesskit_node(&stable.output, Role::Splitter, "Resize floating right edge");
    let before = contained_rect(&dockspace);
    let disabled = run_disabled_frame(
        &context,
        &mut dockspace,
        &mut panes,
        vec![
            accesskit_action(close, Action::Click),
            accesskit_action(right, Action::Increment),
        ],
    );

    assert!(disabled.close_requests.is_empty());
    assert_eq!(contained_rect(&dockspace), before);
    let (_, resize_node) = accesskit_node(
        &disabled.output,
        Role::Splitter,
        "Resize floating right edge",
    );
    assert!(!resize_node.supports_action(Action::Focus));
    assert!(!resize_node.supports_action(Action::Increment));
    assert!(!resize_node.supports_action(Action::Decrement));
    let after = run_frame(&context, &mut dockspace, &mut panes, Vec::new());
    assert!(after.close_requests.is_empty());
    assert_eq!(
        contained_rect(&dockspace),
        before,
        "disabled input cannot enter the next-run action queue"
    );
}

#[test]
fn fully_occluded_rear_contained_chrome_is_accessibility_disabled() {
    let context = Context::default();
    context.enable_accesskit();
    let mut dockspace = Dockspace::builder(
        "product-fully-occluded-contained-chrome",
        fully_occluded_stacked_contained_layout(),
    )
    .build()
    .expect("the fully occluded contained facade initializes");
    let mut panes = Panes;

    let _ = run_frame(&context, &mut dockspace, &mut panes, Vec::new());
    let stable = run_frame(&context, &mut dockspace, &mut panes, Vec::new());
    let tree = stable
        .output
        .platform_output
        .accesskit_update
        .as_ref()
        .expect("AccessKit is enabled");
    let (rear_title_id, rear_title) = accesskit_node(&stable.output, Role::TitleBar, "Second");
    let (rear_close_id, rear_close) =
        accesskit_node(&stable.output, Role::Button, "Close floating Second");
    let (_, front_title) = accesskit_node(&stable.output, Role::TitleBar, "Fourth");
    let (_, front_close) = accesskit_node(&stable.output, Role::Button, "Close floating Fourth");
    let right_edges = tree
        .nodes
        .iter()
        .filter(|(_, node)| {
            node.role() == Role::Splitter && node.label() == Some("Resize floating right edge")
        })
        .collect::<Vec<_>>();
    assert_eq!(right_edges.len(), 2);
    let (rear_right_id, rear_right) = right_edges
        .iter()
        .copied()
        .min_by(|(_, left), (_, right)| {
            left.bounds()
                .expect("the rear resize edge has bounds")
                .x1
                .total_cmp(&right.bounds().expect("the front resize edge has bounds").x1)
        })
        .expect("the rear resize edge is described");
    let (_, front_right) = right_edges
        .iter()
        .copied()
        .max_by(|(_, left), (_, right)| {
            left.bounds()
                .expect("the rear resize edge has bounds")
                .x1
                .total_cmp(&right.bounds().expect("the front resize edge has bounds").x1)
        })
        .expect("the front resize edge is described");

    for node in [rear_title, rear_close, rear_right] {
        assert!(node.is_disabled());
        assert!(!node.supports_action(Action::Focus));
    }
    assert!(!rear_close.supports_action(Action::Click));
    assert!(!rear_right.supports_action(Action::Increment));
    assert!(!rear_right.supports_action(Action::Decrement));
    assert!(!front_title.is_disabled());
    assert!(!front_close.is_disabled());
    assert!(front_close.supports_action(Action::Click));
    assert!(front_right.supports_action(Action::Increment));
    assert!(front_right.supports_action(Action::Decrement));

    let before_rect = contained_rect(&dockspace);
    let before_roster = dockspace
        .view()
        .surface(SURFACE)
        .expect("the stacked surface remains present")
        .contained()
        .map(|contained| contained.id())
        .collect::<Vec<_>>();
    let _ = run_frame(
        &context,
        &mut dockspace,
        &mut panes,
        vec![
            accesskit_action(*rear_right_id, Action::Increment),
            accesskit_action(rear_title_id, Action::Focus),
        ],
    );
    assert_eq!(contained_rect(&dockspace), before_rect);
    assert_eq!(
        dockspace
            .view()
            .surface(SURFACE)
            .expect("the stacked surface remains present")
            .contained()
            .map(|contained| contained.id())
            .collect::<Vec<_>>(),
        before_roster,
    );
    let _ = run_frame(
        &context,
        &mut dockspace,
        &mut panes,
        vec![accesskit_action(rear_close_id, Action::Click)],
    );
    assert!(dockspace.view().contained(FLOATING).is_some());
}

#[test]
fn default_features_contained_cardinal_resize_supports_accesskit_and_keyboard() {
    let context = Context::default();
    context.enable_accesskit();
    let mut dockspace =
        Dockspace::builder("product-contained-resize-semantics", contained_layout())
            .build()
            .expect("the product contained facade initializes");
    let mut panes = Panes;

    let _ = run_frame(&context, &mut dockspace, &mut panes, Vec::new());
    let stable = run_frame(&context, &mut dockspace, &mut panes, Vec::new());
    let tree = stable
        .output
        .platform_output
        .accesskit_update
        .as_ref()
        .expect("AccessKit is enabled");
    let resize_nodes = tree
        .nodes
        .iter()
        .filter(|(_, node)| node.role() == Role::Splitter)
        .collect::<Vec<_>>();
    assert_eq!(
        resize_nodes.len(),
        4,
        "only cardinal contained edges are focusable controls",
    );
    for (_, node) in resize_nodes {
        assert!(node.supports_action(Action::Focus));
        assert!(node.supports_action(Action::Increment));
        assert!(node.supports_action(Action::Decrement));
    }

    let (right, _) = accesskit_node(&stable.output, Role::Splitter, "Resize floating right edge");
    let (top, _) = accesskit_node(&stable.output, Role::Splitter, "Resize floating top edge");
    let initial = contained_rect(&dockspace);
    let _ = run_frame(
        &context,
        &mut dockspace,
        &mut panes,
        vec![accesskit_action(right, Action::Increment)],
    );
    assert_eq!(contained_rect(&dockspace), initial);
    let _ = run_frame(&context, &mut dockspace, &mut panes, Vec::new());
    let widened = contained_rect(&dockspace);
    assert_eq!(widened.min(), initial.min());
    assert!(widened.width() > initial.width());

    let _ = run_frame(
        &context,
        &mut dockspace,
        &mut panes,
        vec![accesskit_action(top, Action::Focus)],
    );
    let _ = run_frame(&context, &mut dockspace, &mut panes, Vec::new());
    let _ = run_frame(
        &context,
        &mut dockspace,
        &mut panes,
        key_press(Key::ArrowUp),
    );
    assert_eq!(contained_rect(&dockspace), widened);
    let _ = run_frame(&context, &mut dockspace, &mut panes, Vec::new());
    let raised = contained_rect(&dockspace);
    assert!(raised.min().y() < widened.min().y());
    assert_eq!(raised.max().y(), widened.max().y());
}

#[test]
fn contained_resize_policy_denial_advertises_no_adjustment_actions() {
    let context = Context::default();
    context.enable_accesskit();
    let mut policy = dockspace::policy::DockPolicy::default();
    policy.set_allow_contained_transform(false);
    let mut dockspace = Dockspace::builder("product-contained-resize-disabled", contained_layout())
        .policy(policy)
        .build()
        .expect("the policy-disabled contained facade initializes");
    let mut panes = Panes;

    let _ = run_frame(&context, &mut dockspace, &mut panes, Vec::new());
    let stable = run_frame(&context, &mut dockspace, &mut panes, Vec::new());
    let tree = stable
        .output
        .platform_output
        .accesskit_update
        .as_ref()
        .expect("AccessKit is enabled");
    let resize_nodes = tree
        .nodes
        .iter()
        .filter(|(_, node)| node.role() == Role::Splitter)
        .collect::<Vec<_>>();
    assert_eq!(resize_nodes.len(), 4);
    for (_, node) in &resize_nodes {
        assert!(!node.supports_action(Action::Focus));
        assert!(!node.supports_action(Action::Increment));
        assert!(!node.supports_action(Action::Decrement));
    }

    let before = contained_rect(&dockspace);
    let right = resize_nodes
        .iter()
        .find_map(|(id, node)| (node.label() == Some("Resize floating right edge")).then_some(*id))
        .expect("the disabled right edge remains described");
    let _ = run_frame(
        &context,
        &mut dockspace,
        &mut panes,
        vec![accesskit_action(right, Action::Increment)],
    );
    assert_eq!(contained_rect(&dockspace), before);
}

#[test]
fn default_features_click_selects_a_tab() {
    let context = Context::default();
    context.enable_accesskit();
    let mut dockspace = Dockspace::builder("product-click", layout())
        .build()
        .expect("the product facade initializes");
    let mut panes = Panes;

    let _ = run_frame(&context, &mut dockspace, &mut panes, Vec::new());
    let ready = run_frame(&context, &mut dockspace, &mut panes, Vec::new());
    let pointer = tab_center(&ready.output, "Second");
    let _ = run_frame(
        &context,
        &mut dockspace,
        &mut panes,
        vec![
            Event::PointerMoved(pointer),
            Event::PointerButton {
                pos: pointer,
                button: PointerButton::Primary,
                pressed: true,
                modifiers: Modifiers::NONE,
            },
            Event::PointerButton {
                pos: pointer,
                button: PointerButton::Primary,
                pressed: false,
                modifiers: Modifiers::NONE,
            },
        ],
    );

    assert_eq!(selected(&dockspace), Some(SECOND));
}

#[test]
fn default_features_overflow_menu_opens_and_selects_hidden_tab() {
    let context = Context::default();
    context.enable_accesskit();
    let style = DockStyle {
        tab_min_width: 220.0,
        tab_max_width: 220.0,
        ..DockStyle::default()
    };
    let mut dockspace = Dockspace::builder("product-overflow-menu", overflow_layout())
        .style(style)
        .build()
        .expect("the overflow product facade initializes");
    let mut panes = Panes;

    let _ = run_frame(&context, &mut dockspace, &mut panes, Vec::new());
    let stable = run_frame(&context, &mut dockspace, &mut panes, Vec::new());
    let (control, control_node) = accesskit_node(&stable.output, Role::Button, "Show hidden tabs");
    assert!(control_node.supports_action(Action::Click));
    let control_point = node_center(&stable.output, Role::Button, "Show hidden tabs");

    let _ = run_frame(
        &context,
        &mut dockspace,
        &mut panes,
        vec![
            Event::PointerMoved(control_point),
            pointer_button(control_point, true),
            pointer_button(control_point, false),
        ],
    );
    let menu = run_frame(&context, &mut dockspace, &mut panes, Vec::new());
    let (row, row_node) = accesskit_node(&menu.output, Role::MenuItem, "Fourth");
    assert!(row_node.supports_action(Action::Click));
    assert!(row_node.supports_action(Action::ScrollIntoView));
    let row_point = node_center(&menu.output, Role::MenuItem, "Fourth");
    let _ = run_frame(
        &context,
        &mut dockspace,
        &mut panes,
        vec![
            Event::PointerMoved(row_point),
            pointer_button(row_point, true),
            pointer_button(row_point, false),
        ],
    );

    assert!(
        dockspace
            .view()
            .item(FOURTH)
            .expect("fourth item remains open")
            .is_selected(),
        "activating a menu row selects the hidden item"
    );
    let _ = (control, row);
}

#[test]
fn default_features_overflow_menu_supports_accesskit_and_keyboard() {
    let context = Context::default();
    context.enable_accesskit();
    let style = DockStyle {
        tab_min_width: 220.0,
        tab_max_width: 220.0,
        ..DockStyle::default()
    };
    let mut dockspace = Dockspace::builder("product-overflow-menu-inputs", overflow_layout())
        .style(style)
        .build()
        .expect("the overflow product facade initializes");
    let mut panes = Panes;

    let _ = run_frame(&context, &mut dockspace, &mut panes, Vec::new());
    let stable = run_frame(&context, &mut dockspace, &mut panes, Vec::new());
    let (control, control_node) = accesskit_node(&stable.output, Role::Button, "Show hidden tabs");
    assert!(control_node.supports_action(Action::Click));
    let _ = run_frame(
        &context,
        &mut dockspace,
        &mut panes,
        vec![accesskit_action(control, Action::Click)],
    );

    let menu = run_frame(&context, &mut dockspace, &mut panes, Vec::new());
    let (_, row_node) = accesskit_node(&menu.output, Role::MenuItem, "Fourth");
    assert!(row_node.supports_action(Action::Focus));
    assert!(row_node.supports_action(Action::ScrollIntoView));

    let _ = run_frame(&context, &mut dockspace, &mut panes, key_press(Key::End));
    let _ = run_frame(&context, &mut dockspace, &mut panes, key_press(Key::Enter));
    assert_eq!(selected(&dockspace), Some(FIRST));
    let _ = run_frame(&context, &mut dockspace, &mut panes, Vec::new());

    assert!(
        dockspace
            .view()
            .item(FOURTH)
            .expect("fourth item remains open")
            .is_selected(),
        "keyboard activation selects the focused hidden item"
    );
}

#[test]
fn official_wheel_does_not_promote_final_hover_to_delivery_authority() {
    let context = Context::default();
    context.enable_accesskit();
    let style = DockStyle {
        tab_min_width: 220.0,
        tab_max_width: 220.0,
        ..DockStyle::default()
    };
    let mut dockspace =
        Dockspace::builder("product-final-hover-wheel-authority", overflow_layout())
            .style(style)
            .build()
            .expect("the overflow product facade initializes");
    let mut panes = Panes;

    let _ = run_frame(&context, &mut dockspace, &mut panes, Vec::new());
    let stable = run_frame(&context, &mut dockspace, &mut panes, Vec::new());
    let tab_point = tab_center(&stable.output, "First");
    let application_point = tab_point + vec2(0.0, 120.0);
    let input = input(vec![
        Event::PointerMoved(application_point),
        mouse_wheel(vec2(0.0, -120.0)),
        Event::PointerMoved(tab_point),
    ]);
    let before = Cell::new(egui::Vec2::ZERO);
    let after = Cell::new(egui::Vec2::ZERO);
    let mut scroll_receivers_current = true;
    let mut output = context.run_ui(input, |ui| {
        before.set(ui.input(|input| input.smooth_scroll_delta()));
        let response = dockspace
            .show_single_surface(SURFACE, ui, &mut panes)
            .expect("the product frame advances");
        scroll_receivers_current = response
            .interaction_capabilities()
            .scroll_receivers_current();
        after.set(ui.input(|input| input.smooth_scroll_delta()));
    });
    output.textures_delta.clear();

    assert_ne!(
        before.get(),
        egui::Vec2::ZERO,
        "the official egui run exposes one aggregate scroll delta",
    );
    assert_eq!(
        after.get(),
        before.get(),
        "final hover cannot authorize Dockspace to consume an uncorrelated wheel aggregate",
    );
    assert!(
        !scroll_receivers_current,
        "official egui cannot advertise event-time scroll receiver authority",
    );
    let settled = run_frame(&context, &mut dockspace, &mut panes, Vec::new());
    let (_, backward) = accesskit_node(&settled.output, Role::Button, "Scroll tabs backward");
    assert!(
        backward.is_disabled(),
        "an uncorrelated official wheel aggregate must not advance tab-strip scroll state",
    );
}

#[test]
fn disabled_ancestor_blocks_tab_strip_wheel_input() {
    let context = Context::default();
    context.enable_accesskit();
    let style = DockStyle {
        tab_min_width: 220.0,
        tab_max_width: 220.0,
        ..DockStyle::default()
    };
    let mut dockspace =
        Dockspace::builder("product-disabled-ancestor-tab-wheel", overflow_layout())
            .style(style)
            .build()
            .expect("the disabled-ancestor tab-wheel facade initializes");
    let mut panes = Panes;

    let _ = run_frame(&context, &mut dockspace, &mut panes, Vec::new());
    let stable = run_frame(&context, &mut dockspace, &mut panes, Vec::new());
    let pointer = tab_center(&stable.output, "First");
    let (_, backward) = accesskit_node(&stable.output, Role::Button, "Scroll tabs backward");
    assert!(backward.is_disabled());

    let disabled = run_disabled_frame(
        &context,
        &mut dockspace,
        &mut panes,
        vec![Event::PointerMoved(pointer), mouse_wheel(vec2(0.0, -120.0))],
    );
    let (_, menu) = accesskit_node(&disabled.output, Role::Button, "Show hidden tabs");
    assert!(menu.is_disabled());
    assert!(!menu.supports_action(Action::Click));

    let settled = run_frame(&context, &mut dockspace, &mut panes, Vec::new());
    let (_, backward) = accesskit_node(&settled.output, Role::Button, "Scroll tabs backward");
    assert!(
        backward.is_disabled(),
        "wheel input under a disabled ancestor must not advance tab-strip scroll state",
    );
}

#[test]
fn disabled_ancestor_suppresses_tab_list_menu_semantics_and_keyboard() {
    let context = Context::default();
    context.enable_accesskit();
    let style = DockStyle {
        tab_min_width: 220.0,
        tab_max_width: 220.0,
        ..DockStyle::default()
    };
    let mut dockspace = Dockspace::builder("product-disabled-ancestor-tab-menu", overflow_layout())
        .style(style)
        .build()
        .expect("the disabled-ancestor tab-menu facade initializes");
    let mut panes = Panes;

    let _ = run_frame(&context, &mut dockspace, &mut panes, Vec::new());
    let stable = run_frame(&context, &mut dockspace, &mut panes, Vec::new());
    let (control, _) = accesskit_node(&stable.output, Role::Button, "Show hidden tabs");
    let _ = run_frame(
        &context,
        &mut dockspace,
        &mut panes,
        vec![accesskit_action(control, Action::Click)],
    );
    let menu = run_frame(&context, &mut dockspace, &mut panes, Vec::new());
    let (fourth, _) = accesskit_node(&menu.output, Role::MenuItem, "Fourth");

    let disabled = run_disabled_frame(
        &context,
        &mut dockspace,
        &mut panes,
        vec![
            accesskit_action(fourth, Action::Focus),
            accesskit_action(fourth, Action::ScrollIntoView),
        ],
    );
    let (_, row) = accesskit_node(&disabled.output, Role::MenuItem, "Fourth");
    assert!(row.is_disabled());
    assert!(!row.supports_action(Action::Click));
    assert!(!row.supports_action(Action::Focus));
    assert!(!row.supports_action(Action::ScrollIntoView));

    let _ = run_disabled_frame(&context, &mut dockspace, &mut panes, key_press(Key::Enter));
    assert_eq!(
        selected(&dockspace),
        Some(FIRST),
        "disabled menu focus and keyboard activation must not select another tab",
    );
}

#[test]
fn partially_clipped_menu_row_scroll_into_view_reaches_core_before_interact_consumes_it() {
    let context = Context::default();
    context.enable_accesskit();
    context.all_styles_mut(|style| {
        style.spacing.item_spacing.y = 0.0;
    });
    let style = DockStyle {
        tab_min_width: 220.0,
        tab_max_width: 220.0,
        ..DockStyle::default()
    };
    let mut dockspace = Dockspace::builder(
        "product-partially-clipped-menu-reveal",
        long_overflow_layout(),
    )
    .style(style)
    .build()
    .expect("the partially clipped menu facade initializes");
    let mut panes = Panes;

    let _ = run_frame(&context, &mut dockspace, &mut panes, Vec::new());
    let stable = run_frame(&context, &mut dockspace, &mut panes, Vec::new());
    let (control, _) = accesskit_node(&stable.output, Role::Button, "Show hidden tabs");
    let _ = run_frame(
        &context,
        &mut dockspace,
        &mut panes,
        vec![accesskit_action(control, Action::Click)],
    );
    let menu = run_frame(&context, &mut dockspace, &mut panes, Vec::new());
    let menu_rect = node_rect(&menu.output, Role::Menu, "Open tabs");
    let tree = menu
        .output
        .platform_output
        .accesskit_update
        .as_ref()
        .expect("AccessKit is enabled");
    let mut rows = tree
        .nodes
        .iter()
        .filter(|(_, node)| node.role() == Role::MenuItem)
        .map(|(id, node)| (*id, accesskit_node_rect(node)))
        .collect::<Vec<_>>();
    rows.sort_by(|(_, left), (_, right)| left.min.y.total_cmp(&right.min.y));
    let first_row = rows.first().expect("the long menu exposes its first row").1;
    let vertical_padding = first_row.min.y - menu_rect.min.y;
    let viewport_max_y = menu_rect.max.y - vertical_padding;
    let (partial_row, partial_rect) = rows
        .iter()
        .copied()
        .find(|(_, rect)| rect.min.y < viewport_max_y && rect.max.y > viewport_max_y)
        .expect("the long menu has one bottom-edge partially clipped row");
    assert!(partial_rect.min.y < viewport_max_y);
    assert!(partial_rect.max.y > viewport_max_y);
    let (_, scrollbar) = accesskit_node(&menu.output, Role::ScrollBar, "Open tabs scroll position");
    let before = scrollbar
        .numeric_value()
        .expect("the menu scrollbar publishes its current value");

    let _ = run_frame(
        &context,
        &mut dockspace,
        &mut panes,
        vec![accesskit_action(partial_row, Action::ScrollIntoView)],
    );
    let settled = run_frame(&context, &mut dockspace, &mut panes, Vec::new());
    let (_, scrollbar) = accesskit_node(
        &settled.output,
        Role::ScrollBar,
        "Open tabs scroll position",
    );
    assert!(
        scrollbar
            .numeric_value()
            .expect("the settled scrollbar publishes its current value")
            > before,
        "ScrollIntoView must reveal the clipped remainder through core-owned menu scroll",
    );
}

#[test]
fn tab_list_menu_scrollbar_pointer_drag_updates_core_owned_scroll() {
    let context = Context::default();
    context.enable_accesskit();
    let style = DockStyle {
        tab_min_width: 220.0,
        tab_max_width: 220.0,
        ..DockStyle::default()
    };
    let mut dockspace = Dockspace::builder(
        "product-menu-scrollbar-pointer-drag",
        long_overflow_layout(),
    )
    .style(style)
    .build()
    .expect("the pointer-scrollbar facade initializes");
    let mut panes = Panes;

    let _ = run_frame(&context, &mut dockspace, &mut panes, Vec::new());
    let stable = run_frame(&context, &mut dockspace, &mut panes, Vec::new());
    let (control, _) = accesskit_node(&stable.output, Role::Button, "Show hidden tabs");
    let _ = run_frame(
        &context,
        &mut dockspace,
        &mut panes,
        vec![accesskit_action(control, Action::Click)],
    );
    let menu = run_frame(&context, &mut dockspace, &mut panes, Vec::new());
    let track = node_rect(&menu.output, Role::ScrollBar, "Open tabs scroll position");
    let (_, scrollbar) = accesskit_node(&menu.output, Role::ScrollBar, "Open tabs scroll position");
    let before = scrollbar
        .numeric_value()
        .expect("the menu scrollbar publishes its current value");
    let source = Pos2::new(track.center().x, track.min.y + 6.0);
    let target = Pos2::new(track.center().x, track.max.y - 6.0);

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
        vec![Event::PointerMoved(target)],
    );
    let _ = run_frame(
        &context,
        &mut dockspace,
        &mut panes,
        vec![Event::PointerMoved(target), pointer_button(target, false)],
    );
    let settled = run_frame(&context, &mut dockspace, &mut panes, Vec::new());
    let (_, scrollbar) = accesskit_node(
        &settled.output,
        Role::ScrollBar,
        "Open tabs scroll position",
    );
    assert!(
        scrollbar
            .numeric_value()
            .expect("the settled scrollbar publishes its current value")
            > before,
        "dragging the visible scrollbar thumb must update core-owned menu scroll",
    );
}

#[test]
fn default_features_keyboard_navigation_keeps_tab_focus_with_selection() {
    let context = Context::default();
    context.enable_accesskit();
    let mut dockspace = Dockspace::builder("product-keyboard-navigation", layout())
        .build()
        .expect("the product facade initializes");
    let mut panes = Panes;

    let _ = run_frame(&context, &mut dockspace, &mut panes, Vec::new());
    let stable = run_frame(&context, &mut dockspace, &mut panes, Vec::new());
    let first = tab_center(&stable.output, "First");
    let _ = run_frame(
        &context,
        &mut dockspace,
        &mut panes,
        vec![
            Event::PointerMoved(first),
            pointer_button(first, true),
            pointer_button(first, false),
        ],
    );

    let _ = run_frame(
        &context,
        &mut dockspace,
        &mut panes,
        key_press(Key::ArrowRight),
    );
    assert_eq!(selected(&dockspace), Some(FIRST));
    let _ = run_frame(&context, &mut dockspace, &mut panes, Vec::new());
    assert_eq!(selected(&dockspace), Some(SECOND));

    let _ = run_frame(&context, &mut dockspace, &mut panes, Vec::new());
    let _ = run_frame(&context, &mut dockspace, &mut panes, key_press(Key::Home));
    assert_eq!(selected(&dockspace), Some(SECOND));
    let _ = run_frame(&context, &mut dockspace, &mut panes, Vec::new());
    assert_eq!(selected(&dockspace), Some(FIRST));

    let _ = run_frame(&context, &mut dockspace, &mut panes, Vec::new());
    let _ = run_frame(&context, &mut dockspace, &mut panes, key_press(Key::End));
    assert_eq!(selected(&dockspace), Some(FIRST));
    let _ = run_frame(&context, &mut dockspace, &mut panes, Vec::new());
    assert_eq!(selected(&dockspace), Some(SECOND));
}

#[test]
fn tab_drag_uses_egui_grab_cursors() {
    let context = Context::default();
    context.enable_accesskit();
    let mut dockspace = Dockspace::builder("product-tab-drag-cursors", layout())
        .build()
        .expect("the product facade initializes");
    let mut panes = Panes;

    let _ = run_frame(&context, &mut dockspace, &mut panes, Vec::new());
    let stable = run_frame(&context, &mut dockspace, &mut panes, Vec::new());
    let tab = node_rect(&stable.output, Role::Tab, "First");
    let source = tab.center();
    let hovered = run_frame(
        &context,
        &mut dockspace,
        &mut panes,
        vec![Event::PointerMoved(source)],
    );
    assert_eq!(
        hovered.output.platform_output.cursor_icon,
        egui::CursorIcon::Grab
    );

    let _ = run_frame(
        &context,
        &mut dockspace,
        &mut panes,
        vec![pointer_button(source, true)],
    );
    let dragging = run_frame(
        &context,
        &mut dockspace,
        &mut panes,
        vec![Event::PointerMoved(source + vec2(12.0, 0.0))],
    );
    assert_eq!(
        dragging.output.platform_output.cursor_icon,
        egui::CursorIcon::Grabbing
    );
}

#[test]
fn tab_accessibility_bounds_match_the_exact_drag_receiver() {
    let context = Context::default();
    context.enable_accesskit();
    let mut dockspace = Dockspace::builder("product-tab-accessibility-drag-bounds", layout())
        .build()
        .expect("the product facade initializes");
    let mut panes = Panes;

    let _ = run_frame(&context, &mut dockspace, &mut panes, Vec::new());
    let stable = run_frame(&context, &mut dockspace, &mut panes, Vec::new());
    let tab = node_rect(&stable.output, Role::Tab, "First");
    let close = node_rect(&stable.output, Role::Button, "Close First");

    assert!(tab.is_positive());
    assert!(close.is_positive());
    assert!(
        !tab.intersect(close).is_positive(),
        "the Tab semantic hit bounds must exclude its disjoint close receiver; tab={tab:?}, close={close:?}"
    );
}

#[test]
fn active_tab_drag_leaves_an_accessible_gap_and_paints_one_pointer_ghost() {
    let context = Context::default();
    context.enable_accesskit();
    context.options_mut(|options| {
        options.max_passes = 4.try_into().expect("four is non-zero");
    });
    let tab_fill = egui::Color32::from_rgb(37, 73, 109);
    let active_fill = egui::Color32::from_rgb(53, 101, 149);
    let ghost_fill = egui::Color32::from_rgba_unmultiplied(219, 139, 57, 96);
    let ghost_offset = vec2(17.0, 11.0);
    let mut style = DockStyle::default();
    style.visuals.tab_fill = Some(tab_fill);
    style.visuals.tab_active_fill = Some(active_fill);
    style.visuals.ghost_fill = Some(ghost_fill);
    style.ghost_offset = ghost_offset;
    let mut dockspace = Dockspace::builder("product-tab-drag-decoration", layout())
        .style(style)
        .build()
        .expect("the decorated drag facade initializes");
    let mut panes = Panes;

    let _ = run_frame(&context, &mut dockspace, &mut panes, Vec::new());
    let stable = run_frame(&context, &mut dockspace, &mut panes, Vec::new());
    let source_rect = node_rect(&stable.output, Role::Tab, "First");
    let close_rect = node_rect(&stable.output, Role::Button, "Close First");
    let source = source_rect.center();
    assert!(rect_fill_count_in(&stable.output, active_fill, source_rect) > 0);

    let armed = run_frame(
        &context,
        &mut dockspace,
        &mut panes,
        vec![Event::PointerMoved(source), pointer_button(source, true)],
    );
    assert!(
        rect_fill_count_in(&armed.output, active_fill, source_rect) > 0,
        "an armed press must not hide the source tab"
    );
    assert_eq!(rect_fill_count(&armed.output, ghost_fill), 0);
    assert_eq!(
        node_rect(&armed.output, Role::Tab, "First"),
        source_rect,
        "arming a drag must not change the tab receiver or accessibility bounds"
    );
    assert_eq!(
        node_rect(&armed.output, Role::Button, "Close First"),
        close_rect,
        "arming a drag must not change the disjoint close receiver bounds"
    );

    let dragged = source + vec2(24.0, 8.0);
    let dragging = run_frame(
        &context,
        &mut dockspace,
        &mut panes,
        vec![Event::PointerMoved(dragged)],
    );
    assert_eq!(
        rect_fill_count_in(&dragging.output, active_fill, source_rect),
        0,
        "the active source tab must become a real visual gap"
    );
    assert_eq!(
        rect_fill_count(&dragging.output, ghost_fill),
        1,
        "one non-interactive pointer ghost represents the dragged tab"
    );
    let ghost = rect_with_fill(&dragging.output, ghost_fill).expect("the tab ghost is painted");
    assert!(
        ghost.min.distance(dragged + ghost_offset) <= 0.01,
        "the pointer ghost follows the current pointer plus the configured style offset"
    );
    let _ = accesskit_node(&dragging.output, Role::Tab, "First");
    let _ = accesskit_node(&dragging.output, Role::Button, "Close First");
    assert_eq!(
        node_rect(&dragging.output, Role::Tab, "First"),
        source_rect,
        "crossing the drag threshold must preserve the tab semantic bounds behind the gap"
    );
    assert_eq!(
        node_rect(&dragging.output, Role::Button, "Close First"),
        close_rect,
        "crossing the drag threshold must preserve close geometry and semantics"
    );
    assert_eq!(
        dragging.output.platform_output.cursor_icon,
        egui::CursorIcon::Grabbing
    );

    let _ = run_frame(&context, &mut dockspace, &mut panes, key_press(Key::Escape));
    let cancelled = run_frame(&context, &mut dockspace, &mut panes, Vec::new());
    assert!(rect_fill_count_in(&cancelled.output, active_fill, source_rect) > 0);
    assert_eq!(rect_fill_count(&cancelled.output, ghost_fill), 0);
}

#[test]
fn rejected_drag_keeps_the_source_visible_without_a_ghost() {
    let context = Context::default();
    context.enable_accesskit();
    context.options_mut(|options| {
        options.max_passes = 4.try_into().expect("four is non-zero");
    });
    let active_fill = egui::Color32::from_rgb(61, 109, 157);
    let ghost_fill = egui::Color32::from_rgba_unmultiplied(231, 145, 53, 96);
    let mut style = DockStyle::default();
    style.visuals.tab_fill = Some(active_fill);
    style.visuals.tab_active_fill = Some(active_fill);
    style.visuals.tab_hover_fill = Some(active_fill);
    style.visuals.ghost_fill = Some(ghost_fill);
    let mut policy = dockspace::policy::DockPolicy::default();
    let mut first = dockspace::policy::DockItemRule::new();
    first.set_source_enabled(false);
    policy.set_item_rule(FIRST, first);
    let mut dockspace = Dockspace::builder("product-rejected-tab-drag-decoration", layout())
        .style(style)
        .policy(policy)
        .build()
        .expect("the source-disabled drag facade initializes");
    let mut panes = Panes;

    let _ = run_frame(&context, &mut dockspace, &mut panes, Vec::new());
    let stable = run_frame(&context, &mut dockspace, &mut panes, Vec::new());
    let source_rect = node_rect(&stable.output, Role::Tab, "First");
    let source = source_rect.center();
    let _ = run_frame(
        &context,
        &mut dockspace,
        &mut panes,
        vec![Event::PointerMoved(source), pointer_button(source, true)],
    );
    let rejected = run_frame(
        &context,
        &mut dockspace,
        &mut panes,
        vec![Event::PointerMoved(source + vec2(24.0, 8.0))],
    );

    assert!(rect_fill_count_in(&rejected.output, active_fill, source_rect) > 0);
    assert_eq!(rect_fill_count(&rejected.output, ghost_fill), 0);
    assert_ne!(
        rejected.output.platform_output.cursor_icon,
        egui::CursorIcon::Grabbing,
        "a rejected source must not advertise an active core drag"
    );
}

#[test]
fn unused_tab_bar_space_starts_the_exact_group_drag() {
    let context = Context::default();
    context.enable_accesskit();
    context.options_mut(|options| {
        options.max_passes = 4.try_into().expect("four is non-zero");
    });
    let group_fill = egui::Color32::from_rgb(71, 107, 157);
    let ghost_fill = egui::Color32::from_rgba_unmultiplied(227, 151, 61, 96);
    let mut style = instrumented_style();
    style.visuals.tab_active_fill = Some(group_fill);
    style.visuals.ghost_fill = Some(ghost_fill);
    let mut dockspace = Dockspace::builder("product-tab-group-trailing-drag", contained_layout())
        .style(style)
        .build()
        .expect("the product facade initializes");
    let mut panes = Panes;

    let _ = run_frame(&context, &mut dockspace, &mut panes, Vec::new());
    let stable = run_frame(&context, &mut dockspace, &mut panes, Vec::new());
    let second = node_rect(&stable.output, Role::Tab, "Second");
    let close = node_rect(&stable.output, Role::Button, "Close Second");
    let title = node_rect(&stable.output, Role::TitleBar, "Second");
    let source = Pos2::new((second.max.x + 80.0).min(760.0), second.center().y);
    assert!(
        source.x > second.max.x,
        "the fixture must leave a positive trailing tab-bar region"
    );

    let hovered = run_frame(
        &context,
        &mut dockspace,
        &mut panes,
        vec![Event::PointerMoved(source)],
    );
    assert_eq!(
        hovered.output.platform_output.cursor_icon,
        egui::CursorIcon::Grab,
        "unused tab-bar space must expose the same group drag affordance as the leading grip"
    );
    assert_eq!(node_rect(&hovered.output, Role::Tab, "Second"), second);
    assert_eq!(
        node_rect(&hovered.output, Role::Button, "Close Second"),
        close
    );
    assert_eq!(node_rect(&hovered.output, Role::TitleBar, "Second"), title);

    let armed = run_frame(
        &context,
        &mut dockspace,
        &mut panes,
        vec![pointer_button(source, true)],
    );
    assert!(rect_fill_count_in(&armed.output, group_fill, second) > 0);
    assert_eq!(rect_fill_count(&armed.output, ghost_fill), 0);
    assert_eq!(node_rect(&armed.output, Role::Tab, "Second"), second);
    assert_eq!(
        node_rect(&armed.output, Role::Button, "Close Second"),
        close
    );
    assert_eq!(node_rect(&armed.output, Role::TitleBar, "Second"), title);
    let dragging = run_frame(
        &context,
        &mut dockspace,
        &mut panes,
        vec![Event::PointerMoved(source + vec2(12.0, 0.0))],
    );
    assert_eq!(
        dragging.output.platform_output.cursor_icon,
        egui::CursorIcon::Grabbing
    );
    assert!(
        rect_fill_count_in(
            &dragging.output,
            group_fill,
            second.translate(vec2(12.0, 0.0))
        ) > 0
    );
    assert_eq!(rect_fill_count(&dragging.output, ghost_fill), 0);
    assert_eq!(
        node_rect(&dragging.output, Role::Tab, "Second"),
        second.translate(vec2(12.0, 0.0)),
        "the complete group moves its real tab receiver"
    );
    assert_eq!(
        node_rect(&dragging.output, Role::Button, "Close Second"),
        close.translate(vec2(12.0, 0.0)),
        "the complete group moves its real close receiver"
    );
    assert_eq!(
        node_rect(&dragging.output, Role::TitleBar, "Second"),
        title.translate(vec2(12.0, 0.0)),
        "the complete group moves contained-title accessibility bounds"
    );

    let passive = dockspace
        .style()
        .visuals
        .drop_guide_fill
        .expect("the fixture fixes its passive guide color");
    let active = dockspace
        .style()
        .visuals
        .drop_guide_active_fill
        .expect("the fixture fixes its active guide color");
    let preview = run_frame(
        &context,
        &mut dockspace,
        &mut panes,
        vec![Event::PointerMoved(Pos2::new(400.0, 528.0))],
    );
    assert!(
        guide_button_count(&preview.output, passive, active) >= 4,
        "the trailing region must enter the same core-owned group drag as the leading grip"
    );
    let moved_title = node_rect(&preview.output, Role::TitleBar, "Second");
    let delta = moved_title.min - title.min;
    assert_eq!(
        node_rect(&preview.output, Role::Tab, "Second"),
        second.translate(delta),
        "the full source remains aligned with its title"
    );
    assert!(rect_fill_count_in(&preview.output, group_fill, second.translate(delta)) > 0);
    assert_eq!(
        node_rect(&preview.output, Role::Button, "Close Second"),
        close.translate(delta)
    );
    assert_eq!(
        rect_fill_count(&preview.output, ghost_fill),
        0,
        "the real complete source replaces the duplicate header ghost"
    );
    let _ = accesskit_node(&preview.output, Role::Tab, "Second");
    let _ = accesskit_node(&preview.output, Role::Button, "Close Second");
    let tree = preview
        .output
        .platform_output
        .accesskit_update
        .as_ref()
        .expect("AccessKit is enabled");
    let pane_label = tree
        .nodes
        .iter()
        .find_map(|(_, node)| {
            (node.role() == Role::Label && node.value() == Some("Pane 2")).then_some(node)
        })
        .expect("the projected pane keeps its application accessibility node");
    assert!(
        !pane_label.is_disabled(),
        "the projected source must not disable application controls or surrender pane focus"
    );
}

#[test]
fn contained_title_drag_paints_a_full_window_preview_in_the_same_run() {
    let context = Context::default();
    context.enable_accesskit();
    context.options_mut(|options| {
        options.max_passes = 4.try_into().expect("four is non-zero");
    });
    let source_fill = egui::Color32::from_rgb(83, 59, 137);
    let ghost_fill = egui::Color32::from_rgba_unmultiplied(235, 157, 67, 96);
    let mut style = DockStyle::default();
    style.visuals.floating_fill = Some(source_fill);
    style.visuals.ghost_fill = Some(ghost_fill);
    let mut dockspace = Dockspace::builder("product-contained-drag-decoration", contained_layout())
        .style(style)
        .build()
        .expect("the decorated contained facade initializes");
    let mut panes = Panes;

    let _ = run_frame(&context, &mut dockspace, &mut panes, Vec::new());
    let stable = run_frame(&context, &mut dockspace, &mut panes, Vec::new());
    let source_bounds = contained_rect(&dockspace);
    let source_rect = Rect::from_min_max(
        Pos2::new(
            source_bounds.min().x() as f32,
            source_bounds.min().y() as f32,
        ),
        Pos2::new(
            source_bounds.max().x() as f32,
            source_bounds.max().y() as f32,
        ),
    );
    let source = contained_title_point(&dockspace);
    assert!(rect_fill_count_in(&stable.output, source_fill, source_rect) > 0);

    let armed = run_frame(
        &context,
        &mut dockspace,
        &mut panes,
        vec![Event::PointerMoved(source), pointer_button(source, true)],
    );
    assert!(rect_fill_count_in(&armed.output, source_fill, source_rect) > 0);
    assert_eq!(rect_fill_count(&armed.output, ghost_fill), 0);

    let dragging = run_frame(
        &context,
        &mut dockspace,
        &mut panes,
        vec![Event::PointerMoved(source + vec2(24.0, 8.0))],
    );
    let projected =
        rect_with_fill(&dragging.output, source_fill).expect("the real root is painted");
    assert_eq!(rect_fill_count(&dragging.output, ghost_fill), 0);
    assert_eq!(projected.size(), source_rect.size());
    assert!(
        projected
            .min
            .distance(source_rect.translate(vec2(24.0, 8.0)).min)
            <= 0.01,
        "the full-window preview preserves the title-bar grab offset"
    );
    let _ = accesskit_node(&dragging.output, Role::TitleBar, "Second");
    let _ = accesskit_node(&dragging.output, Role::Button, "Close floating Second");
    assert_eq!(
        dragging.output.platform_output.cursor_icon,
        egui::CursorIcon::Grabbing
    );
}

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
fn explicit_touch_cancel_terminates_the_host_gesture_and_allows_a_fresh_press() {
    let context = Context::default();
    context.enable_accesskit();
    let mut dockspace = Dockspace::builder("product-raw-touch-cancel", layout())
        .style(instrumented_style())
        .build()
        .expect("the touch cancellation fixture initializes");
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
    let before = dockspace.version();
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
    let cancelled = run_frame(
        &context,
        &mut dockspace,
        &mut panes,
        vec![
            Event::Touch {
                device_id: egui::TouchDeviceId(1),
                id: egui::TouchId(1),
                phase: egui::TouchPhase::Cancel,
                pos: target,
                force: None,
            },
            Event::PointerGone,
        ],
    );
    assert_eq!(dockspace.version(), before);
    assert_eq!(root_tabs(&dockspace), Some(vec![FIRST, SECOND]));
    assert_eq!(rect_fill_count(&cancelled.output, ghost), 0);
    assert_ne!(
        cancelled.output.platform_output.cursor_icon,
        egui::CursorIcon::Grabbing
    );
    let _ = run_frame(&context, &mut dockspace, &mut panes, Vec::new());
    let _ = run_frame(
        &context,
        &mut dockspace,
        &mut panes,
        vec![
            Event::PointerMoved(source),
            pointer_button(source, true),
            Event::PointerMoved(target),
            pointer_button(target, false),
        ],
    );
    let _ = run_frame(&context, &mut dockspace, &mut panes, Vec::new());
    assert_eq!(root_tabs(&dockspace), Some(vec![SECOND, FIRST]));
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
fn rear_contained_selected_tab_press_raises_before_any_drag_motion() {
    let context = Context::default();
    context.enable_accesskit();
    context.options_mut(|options| {
        options.max_passes = 4.try_into().expect("four is non-zero");
    });
    let mut dockspace = Dockspace::builder(
        "product-contained-first-effective-press",
        stacked_contained_layout(),
    )
    .build()
    .expect("the stacked contained facade initializes");
    let mut panes = Panes;

    let _ = run_frame(&context, &mut dockspace, &mut panes, Vec::new());
    let stable = run_frame(&context, &mut dockspace, &mut panes, Vec::new());
    let source = node_center(&stable.output, Role::Tab, "Second");
    let pressed = run_frame(
        &context,
        &mut dockspace,
        &mut panes,
        vec![Event::PointerMoved(source), pointer_button(source, true)],
    );

    let _ = accesskit_node(&pressed.output, Role::TitleBar, "Second");
    let _ = accesskit_node(&pressed.output, Role::TitleBar, "Fourth");
    assert_eq!(
        dockspace
            .view()
            .surface(SURFACE)
            .expect("the stacked surface remains present")
            .contained()
            .map(|contained| contained.id())
            .collect::<Vec<_>>(),
        [FRONT_FLOATING, FLOATING],
        "the selected rear tab raises its complete contained root on the initial press",
    );
}

#[test]
fn rear_contained_child_press_uses_event_time_position_even_when_released_in_the_same_frame() {
    for (registered, blocked) in [(false, false), (true, false), (true, true)] {
        let context = Context::default();
        context.enable_accesskit();
        context.options_mut(|options| {
            options.max_passes = 4.try_into().expect("four is non-zero");
        });
        let mut dockspace = Dockspace::builder(
            "product-contained-child-first-effective-press",
            stacked_contained_layout(),
        )
        .build()
        .expect("the stacked contained facade initializes");
        let mut panes = FocusPanes::new("contained-child-first-effective-press", true);
        panes.docking_content = registered;
        panes.overlapping_blocker = blocked;
        if blocked {
            panes.content_widgets.push(panes.target);
        }
        let qualified = registered && !blocked;

        let _ = run_focus_frame(&context, &mut dockspace, &mut panes, Vec::new());
        let _ = run_focus_frame(&context, &mut dockspace, &mut panes, Vec::new());
        let source = panes.target_rect().center();
        let released_at = contained_title_point(&dockspace);
        let _ = run_focus_frame(
            &context,
            &mut dockspace,
            &mut panes,
            vec![
                Event::PointerMoved(source),
                pointer_button(source, true),
                Event::PointerMoved(released_at),
                pointer_button(released_at, false),
            ],
        );

        assert_eq!(
            dockspace
                .view()
                .surface(SURFACE)
                .expect("the stacked surface remains present")
                .contained()
                .map(|contained| contained.id())
                .collect::<Vec<_>>(),
            if qualified {
                [FRONT_FLOATING, FLOATING]
            } else {
                [FLOATING, FRONT_FLOATING]
            },
            "only explicitly registered application content can raise its containing presentation",
        );
        assert!(
            !context.memory(|memory| memory.has_focus(panes.target)),
            "the input update publishes the focus obligation before encoding can observe it"
        );
        let _ = run_focus_frame(&context, &mut dockspace, &mut panes, Vec::new());
        assert_eq!(
            context.memory(|memory| memory.has_focus(panes.target)),
            qualified,
            "only a qualified event-time pane hit focuses the exact target"
        );
        let _ = run_focus_frame(&context, &mut dockspace, &mut panes, Vec::new());
        context.memory_mut(|memory| memory.surrender_focus(panes.target));
        assert!(!context.memory(|memory| memory.has_focus(panes.target)));
        let _ = run_focus_frame(&context, &mut dockspace, &mut panes, Vec::new());
        assert!(
            !context.memory(|memory| memory.has_focus(panes.target)),
            "acknowledged focus cannot replay after surrender; registered={registered}, blocked={blocked}"
        );
    }
}

#[test]
fn default_features_close_request_can_be_resolved() {
    let context = Context::default();
    context.enable_accesskit();
    let mut dockspace = Dockspace::builder("product-close", layout())
        .build()
        .expect("the product facade initializes");
    let mut panes = Panes;

    let _ = run_frame(&context, &mut dockspace, &mut panes, Vec::new());
    let ready = run_frame(&context, &mut dockspace, &mut panes, Vec::new());
    let pointer = node_center(&ready.output, Role::Button, "Close Second");
    let _ = run_frame(
        &context,
        &mut dockspace,
        &mut panes,
        vec![
            Event::PointerMoved(pointer),
            Event::PointerButton {
                pos: pointer,
                button: PointerButton::Primary,
                pressed: true,
                modifiers: Modifiers::NONE,
            },
        ],
    );
    let released = run_frame(
        &context,
        &mut dockspace,
        &mut panes,
        vec![
            Event::PointerMoved(pointer),
            Event::PointerButton {
                pos: pointer,
                button: PointerButton::Primary,
                pressed: false,
                modifiers: Modifiers::NONE,
            },
        ],
    );

    let request = released
        .close_requests
        .first()
        .expect("the close button opens one plan");
    let item = request.plan().items()[0];
    assert_eq!(item.item(), SECOND);
    dockspace
        .resolve_close(request.plan().request(), item.token(), CloseDecision::Allow)
        .expect("the close decision commits");
    assert!(dockspace.view().item(SECOND).is_none());
}

#[test]
fn default_features_programmatic_root_close_uses_the_product_facade() {
    let mut root_dockspace = Dockspace::builder("product-programmatic-root-close", layout())
        .build()
        .expect("the root-close product facade initializes");
    let root_request = root_dockspace
        .request_close_root_current(ROOT)
        .expect("the root close request is reduced");
    let DockspaceCloseRequestStatus::Requested(request) = root_request.status() else {
        panic!("the root close request must open a plan")
    };
    assert_eq!(
        request.plan().target(),
        egui_dockspace::ClosePlanTarget::Root { root: ROOT }
    );
    for item in request.plan().items() {
        root_dockspace
            .resolve_close(request.plan().request(), item.token(), CloseDecision::Allow)
            .expect("each root close decision commits");
    }
    assert!(root_dockspace.view().root(ROOT).is_none());
}

#[test]
fn default_features_programmatic_close_reports_stale_and_root_rejection() {
    let mut stale_dockspace = Dockspace::builder("product-programmatic-close-stale", layout())
        .build()
        .expect("the stale-close product facade initializes");
    let prepared = stale_dockspace.prepare_close_item(FIRST);
    let expected = prepared.expected_version();
    let selection = stale_dockspace
        .select_item_current(SECOND)
        .expect("selection advances the product revision");
    assert!(matches!(
        selection.status(),
        DockspaceActionStatus::Applied(DockspaceActionOutcome::Selected {
            item: SECOND,
            changed: true,
        })
    ));
    let stale = stale_dockspace
        .submit_prepared_close_request(prepared)
        .expect("the stale close request is reduced");
    assert!(matches!(
        stale.status(),
        DockspaceCloseRequestStatus::Stale {
            expected: actual_expected,
            accepted,
        } if *actual_expected == expected && *accepted == stale.mutation().after()
    ));

    let mut policy = dockspace::policy::DockPolicy::default();
    policy.set_close_capability(dockspace::policy::CloseCapability::Disabled);
    let mut disabled_dockspace =
        Dockspace::builder("product-programmatic-root-close-disabled", layout())
            .policy(policy)
            .build()
            .expect("the disabled-close product facade initializes");
    let rejected = disabled_dockspace
        .request_close_root_current(ROOT)
        .expect("the disabled root close request is reduced");
    assert_eq!(
        rejected.status(),
        &DockspaceCloseRequestStatus::Rejected(DockspaceCloseRequestRejection::ItemCloseDisabled {
            target: ClosePlanTarget::Root { root: ROOT },
            item: FIRST,
        })
    );
}

#[test]
fn stale_prepared_dock_action_is_inert_in_the_product_executor() {
    let mut dockspace = Dockspace::builder("product-stale-dock-action", layout())
        .build()
        .expect("the stale dock-action facade initializes");
    let prepared = dockspace.prepare_dock_item(SECOND, DockPlacement::Before(FIRST));
    let expected = prepared.expected_version();
    let selection = dockspace
        .select_item_current(SECOND)
        .expect("selection advances the product revision");
    assert!(matches!(
        selection.status(),
        DockspaceActionStatus::Applied(DockspaceActionOutcome::Selected {
            item: SECOND,
            changed: true,
        })
    ));

    let stale = dockspace
        .submit_prepared_action(prepared)
        .expect("the stale dock action is reduced inertly");
    assert!(matches!(
        stale.status(),
        DockspaceActionStatus::Stale {
            expected: actual_expected,
            accepted,
        } if *actual_expected == expected && *accepted == stale.mutation().after()
    ));
    assert_eq!(root_tabs(&dockspace), Some(vec![FIRST, SECOND]));
    assert_eq!(selected(&dockspace), Some(SECOND));
}

#[test]
fn default_features_paint_outer_guides_and_dock_each_edge() {
    for (name, target, expected_axis, expected_items) in [
        (
            "left",
            Pos2::new(72.0, 300.0),
            DockspaceAxis::Horizontal,
            vec![vec![SECOND], vec![FIRST]],
        ),
        (
            "right",
            Pos2::new(728.0, 300.0),
            DockspaceAxis::Horizontal,
            vec![vec![FIRST], vec![SECOND]],
        ),
        (
            "top",
            Pos2::new(400.0, 72.0),
            DockspaceAxis::Vertical,
            vec![vec![SECOND], vec![FIRST]],
        ),
        (
            "bottom",
            Pos2::new(400.0, 528.0),
            DockspaceAxis::Vertical,
            vec![vec![FIRST], vec![SECOND]],
        ),
    ] {
        let context = Context::default();
        context.enable_accesskit();
        let mut dockspace = Dockspace::builder(("product-guide", name), split_layout())
            .style(instrumented_style())
            .build()
            .expect("the product guide facade initializes");
        let mut panes = Panes;

        let _ = run_frame(&context, &mut dockspace, &mut panes, Vec::new());
        let stable = run_frame(&context, &mut dockspace, &mut panes, Vec::new());
        let source = tab_center(&stable.output, "Second");
        let passive = dockspace
            .style()
            .visuals
            .drop_guide_fill
            .expect("the fixture fixes its passive guide color");
        let active = dockspace
            .style()
            .visuals
            .drop_guide_active_fill
            .expect("the fixture fixes its active guide color");

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
            vec![Event::PointerMoved(target)],
        );
        let _ = run_frame(
            &context,
            &mut dockspace,
            &mut panes,
            vec![Event::PointerMoved(target)],
        );
        let painted = run_frame(
            &context,
            &mut dockspace,
            &mut panes,
            vec![Event::PointerMoved(target)],
        );
        assert!(
            guide_button_count(&painted.output, passive, active) >= 4,
            "an active drag must paint at least the complete outer four-way guide",
        );
        assert!(
            painted.output.shapes.iter().any(|shape| matches!(
                &shape.shape,
                egui::Shape::Rect(rect) if rect.fill == active
            )),
            "the exact hovered guide must use the active visual state",
        );

        let _ = run_frame(
            &context,
            &mut dockspace,
            &mut panes,
            vec![Event::PointerMoved(target), pointer_button(target, false)],
        );
        assert_eq!(
            root_split(&dockspace),
            Some((expected_axis, expected_items)),
            "the {name} guide must commit the same edge target shown in preview",
        );
    }
}

#[test]
fn default_features_center_guide_merges_tabs() {
    let context = Context::default();
    context.enable_accesskit();
    let mut dockspace = Dockspace::builder("product-guide-center", split_layout())
        .style(instrumented_style())
        .build()
        .expect("the product guide facade initializes");
    let mut panes = Panes;

    let _ = run_frame(&context, &mut dockspace, &mut panes, Vec::new());
    let stable = run_frame(&context, &mut dockspace, &mut panes, Vec::new());
    let source = tab_center(&stable.output, "Second");
    let first = tab_center(&stable.output, "First");
    let passive = dockspace
        .style()
        .visuals
        .drop_guide_fill
        .expect("the fixture fixes its passive guide color");
    let active = dockspace
        .style()
        .visuals
        .drop_guide_active_fill
        .expect("the fixture fixes its active guide color");

    let _ = run_frame(
        &context,
        &mut dockspace,
        &mut panes,
        vec![Event::PointerMoved(source), pointer_button(source, true)],
    );
    for _ in 0..2 {
        let _ = run_frame(
            &context,
            &mut dockspace,
            &mut panes,
            vec![Event::PointerMoved(first)],
        );
    }
    let cluster = run_frame(
        &context,
        &mut dockspace,
        &mut panes,
        vec![Event::PointerMoved(first)],
    );
    let (_, guide_node) =
        accesskit_node(&cluster.output, Role::Label, "Dockspace center drop guide");
    assert!(!guide_node.supports_action(Action::Click));
    assert!(!guide_node.supports_action(Action::Focus));
    let target = node_rect(&cluster.output, Role::Label, "Dockspace center drop guide").center();
    let painted_center =
        nearest_guide_center(&cluster.output, passive, active, Pos2::new(200.0, 314.0));
    assert_eq!(target, painted_center);
    for _ in 0..2 {
        let _ = run_frame(
            &context,
            &mut dockspace,
            &mut panes,
            vec![Event::PointerMoved(target)],
        );
    }
    let _ = run_frame(
        &context,
        &mut dockspace,
        &mut panes,
        vec![Event::PointerMoved(target), pointer_button(target, false)],
    );

    assert_eq!(root_tabs(&dockspace), Some(vec![FIRST, SECOND]));
}

#[test]
fn docked_tab_drag_to_a_non_target_region_creates_contained_floating() {
    let context = Context::default();
    context.enable_accesskit();
    context.options_mut(|options| {
        options.max_passes = 4.try_into().expect("four is non-zero");
    });
    let mut dockspace = Dockspace::builder("product-local-tab-contained", layout())
        .style(instrumented_style())
        .build()
        .expect("the local contained tab facade initializes");
    let mut panes = FocusPanes::new("product-local-tab-contained", false);

    let _ = run_focus_frame(&context, &mut dockspace, &mut panes, Vec::new());
    let stable = run_focus_frame(&context, &mut dockspace, &mut panes, Vec::new()).0;
    let source = tab_center(&stable.output, "First");
    let pane = panes.pane_rect();
    let target = Pos2::new(
        egui::lerp(pane.x_range(), 0.65),
        egui::lerp(pane.y_range(), 0.65),
    );

    let _ = run_focus_frame(
        &context,
        &mut dockspace,
        &mut panes,
        vec![Event::PointerMoved(source), pointer_button(source, true)],
    );
    let _ = run_focus_frame(
        &context,
        &mut dockspace,
        &mut panes,
        vec![Event::PointerMoved(source + vec2(16.0, 8.0))],
    );
    let mut preview = None;
    for _ in 0..3 {
        preview = Some(
            run_focus_frame(
                &context,
                &mut dockspace,
                &mut panes,
                vec![Event::PointerMoved(target)],
            )
            .0,
        );
    }
    let preview = preview.expect("the drag publishes a terminal target frame");
    let drop_fill = dockspace
        .style()
        .visuals
        .drop_fill
        .expect("the fixture fixes its preview fill");
    assert_eq!(
        rect_fill_count(&preview.output, drop_fill),
        1,
        "a no-target local tab drag must paint one contained preview before release"
    );
    let _ = run_focus_frame(
        &context,
        &mut dockspace,
        &mut panes,
        vec![Event::PointerMoved(target), pointer_button(target, false)],
    );

    let first = dockspace
        .view()
        .item(FIRST)
        .expect("the dragged tab remains present");
    assert_eq!(first.surface(), SURFACE);
    assert_ne!(first.root(), ROOT);
    assert!(first.contained().is_some());
    let second = dockspace
        .view()
        .item(SECOND)
        .expect("the undragged tab remains in the main root");
    assert_eq!(second.root(), ROOT);
    assert_eq!(second.contained(), None);
}

#[test]
fn docked_tab_drag_released_outside_the_surface_cancels_without_floating() {
    let context = Context::default();
    context.enable_accesskit();
    context.options_mut(|options| {
        options.max_passes = 4.try_into().expect("four is non-zero");
    });
    let mut dockspace = Dockspace::builder("product-local-tab-outside", layout())
        .style(instrumented_style())
        .build()
        .expect("the outside local tab facade initializes");
    let mut panes = Panes;

    let _ = run_frame(&context, &mut dockspace, &mut panes, Vec::new());
    let stable = run_frame(&context, &mut dockspace, &mut panes, Vec::new());
    let source = tab_center(&stable.output, "First");
    let outside = Pos2::new(840.0, 640.0);

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
        vec![Event::PointerMoved(source + vec2(16.0, 8.0))],
    );
    let mut outside_frame = None;
    for _ in 0..3 {
        outside_frame = Some(run_frame(
            &context,
            &mut dockspace,
            &mut panes,
            vec![Event::PointerMoved(outside)],
        ));
    }
    let outside_frame = outside_frame.expect("the outside drag reaches a terminal product frame");
    let drop_fill = dockspace
        .style()
        .visuals
        .drop_fill
        .expect("the fixture fixes its preview fill");
    assert_eq!(
        rect_fill_count(&outside_frame.output, drop_fill),
        0,
        "surface-external local coordinates cannot paint a contained preview"
    );

    let _ = run_frame(
        &context,
        &mut dockspace,
        &mut panes,
        vec![Event::PointerMoved(outside), pointer_button(outside, false)],
    );
    let _ = run_frame(&context, &mut dockspace, &mut panes, Vec::new());

    assert_eq!(root_tabs(&dockspace), Some(vec![FIRST, SECOND]));
    let surface = dockspace
        .view()
        .surface(SURFACE)
        .expect("the logical surface remains present");
    assert_eq!(surface.contained().count(), 0);
}

#[test]
fn docked_group_drag_to_a_non_target_region_rehomes_the_complete_root() {
    let context = Context::default();
    context.enable_accesskit();
    context.options_mut(|options| {
        options.max_passes = 4.try_into().expect("four is non-zero");
    });
    let mut dockspace = Dockspace::builder("product-local-group-contained", layout())
        .style(instrumented_style())
        .build()
        .expect("the local contained group facade initializes");
    let mut panes = Panes;

    let _ = run_frame(&context, &mut dockspace, &mut panes, Vec::new());
    let stable = run_frame(&context, &mut dockspace, &mut panes, Vec::new());
    let second = node_rect(&stable.output, Role::Tab, "Second");
    let source = Pos2::new((second.max.x + 80.0).min(760.0), second.center().y);
    let target = Pos2::new(620.0, 440.0);

    let hovered = run_frame(
        &context,
        &mut dockspace,
        &mut panes,
        vec![Event::PointerMoved(source)],
    );
    assert_eq!(
        hovered.output.platform_output.cursor_icon,
        egui::CursorIcon::Grab
    );
    let _ = run_frame(
        &context,
        &mut dockspace,
        &mut panes,
        vec![pointer_button(source, true)],
    );
    let dragging = run_frame(
        &context,
        &mut dockspace,
        &mut panes,
        vec![Event::PointerMoved(source + vec2(16.0, 0.0))],
    );
    assert_eq!(
        dragging.output.platform_output.cursor_icon,
        egui::CursorIcon::Grabbing
    );
    let mut preview = None;
    for _ in 0..3 {
        preview = Some(run_frame(
            &context,
            &mut dockspace,
            &mut panes,
            vec![Event::PointerMoved(target)],
        ));
    }
    let preview = preview.expect("the drag publishes a terminal target frame");
    let drop_fill = dockspace
        .style()
        .visuals
        .drop_fill
        .expect("the fixture fixes its preview fill");
    assert_eq!(
        rect_fill_count(&preview.output, drop_fill),
        1,
        "a no-target local group drag must paint one contained preview before release"
    );
    let _ = run_frame(
        &context,
        &mut dockspace,
        &mut panes,
        vec![Event::PointerMoved(target), pointer_button(target, false)],
    );

    let surface = dockspace
        .view()
        .surface(SURFACE)
        .expect("the logical surface remains present");
    assert!(surface.main_root().is_none());
    assert_eq!(surface.contained().count(), 1);
    for item in [FIRST, SECOND] {
        let location = dockspace
            .view()
            .item(item)
            .expect("the complete dragged root keeps every item");
        assert_eq!(location.root(), ROOT);
        assert!(location.contained().is_some());
    }
}

#[test]
fn default_features_splitter_tracks_pointer_before_release() {
    let context = Context::default();
    let mut dockspace = Dockspace::builder("product-live-splitter", split_layout())
        .style(instrumented_style())
        .build()
        .expect("the product splitter facade initializes");
    let mut panes = Panes;

    let _ = run_frame(&context, &mut dockspace, &mut panes, Vec::new());
    let ready = run_frame(&context, &mut dockspace, &mut panes, Vec::new());
    let idle = dockspace
        .style()
        .visuals
        .splitter_color
        .expect("the fixture fixes its idle splitter color");
    let active = dockspace
        .style()
        .visuals
        .splitter_hover_color
        .expect("the fixture fixes its active splitter color");
    let initial_splitter = splitter_rect(&ready.output, idle, active);
    let initial_weights = root_weights(&dockspace).expect("the fixture root remains split");
    let source = initial_splitter.center();
    let moved = source + vec2(80.0, 0.0);

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
    let live = run_frame(&context, &mut dockspace, &mut panes, Vec::new());
    let live_splitter = splitter_rect(&live.output, idle, active);

    assert!(
        live_splitter.center().x > initial_splitter.center().x + 40.0,
        "transient splitter geometry must follow the pointer before durable commit",
    );
    assert_eq!(
        root_weights(&dockspace),
        Some(initial_weights.clone()),
        "pointer motion must not mutate durable weights before release",
    );

    let _ = run_frame(
        &context,
        &mut dockspace,
        &mut panes,
        vec![Event::PointerMoved(moved), pointer_button(moved, false)],
    );
    assert_ne!(
        root_weights(&dockspace),
        Some(initial_weights),
        "release commits the transient splitter proposal",
    );
}

#[test]
fn default_features_splitter_junction_tracks_both_axes_and_commits_once() {
    let context = Context::default();
    context.enable_accesskit();
    let mut dockspace =
        Dockspace::builder("product-live-splitter-junction", splitter_junction_layout())
            .build()
            .expect("the product splitter junction facade initializes");
    let mut panes = Panes;

    let _ = run_frame(&context, &mut dockspace, &mut panes, Vec::new());
    let ready = run_frame(&context, &mut dockspace, &mut panes, Vec::new());
    let initial = node_rect(&ready.output, Role::Group, "Resize pane grid");
    let source = initial.center();
    let moved = source + vec2(70.0, 50.0);
    let before_version = dockspace.version();

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
    let live = run_frame(&context, &mut dockspace, &mut panes, Vec::new());
    let live_junction = node_rect(&live.output, Role::Group, "Resize pane grid");

    assert!(
        live_junction.center().x > initial.center().x + 35.0
            && live_junction.center().y > initial.center().y + 25.0,
        "the atomic junction preview must follow both pointer axes",
    );
    assert_eq!(
        dockspace.version(),
        before_version,
        "junction motion remains transient until release",
    );

    let _ = run_frame(
        &context,
        &mut dockspace,
        &mut panes,
        vec![Event::PointerMoved(moved), pointer_button(moved, false)],
    );
    assert_ne!(
        dockspace.version(),
        before_version,
        "one junction release commits both axes atomically",
    );
    let committed_version = dockspace.version();
    let _ = run_frame(&context, &mut dockspace, &mut panes, Vec::new());
    assert_eq!(dockspace.version(), committed_version);
}

#[test]
fn default_features_splitter_junction_exposes_independent_axis_semantics() {
    let context = Context::default();
    context.enable_accesskit();
    let mut dockspace = Dockspace::builder(
        "product-splitter-junction-semantics",
        splitter_junction_layout(),
    )
    .build()
    .expect("the product splitter junction facade initializes");
    let mut panes = Panes;

    let _ = run_frame(&context, &mut dockspace, &mut panes, Vec::new());
    let stable = run_frame(&context, &mut dockspace, &mut panes, Vec::new());
    let (_, group_node) = accesskit_node(&stable.output, Role::Group, "Resize pane grid");
    assert_eq!(
        group_node.description(),
        Some(
            "Drag to resize rows and columns, or use the horizontal and vertical splitter controls",
        ),
    );
    let (horizontal, horizontal_node) = accesskit_node(
        &stable.output,
        Role::Splitter,
        "Resize pane grid horizontally",
    );
    let (vertical, vertical_node) = accesskit_node(
        &stable.output,
        Role::Splitter,
        "Resize pane grid vertically",
    );
    assert_eq!(horizontal_node.orientation(), Some(Orientation::Vertical));
    assert_eq!(vertical_node.orientation(), Some(Orientation::Horizontal));
    assert!(group_node.children().contains(&horizontal));
    assert!(group_node.children().contains(&vertical));
    for node in [horizontal_node, vertical_node] {
        assert!(node.supports_action(Action::Focus));
        assert!(node.supports_action(Action::Increment));
        assert!(node.supports_action(Action::Decrement));
    }

    let (before_root, before_columns) =
        splitter_junction_weights(&dockspace).expect("the product junction fixture remains split");
    let _ = run_frame(
        &context,
        &mut dockspace,
        &mut panes,
        vec![accesskit_action(horizontal, Action::Increment)],
    );
    assert_eq!(
        splitter_junction_weights(&dockspace),
        Some((before_root.clone(), before_columns.clone()))
    );
    let _ = run_frame(&context, &mut dockspace, &mut panes, Vec::new());
    let (horizontal_root, horizontal_columns) = splitter_junction_weights(&dockspace)
        .expect("the horizontally adjusted junction remains split");
    assert!(horizontal_root[0] > before_root[0]);
    assert_eq!(horizontal_columns, before_columns);

    let _ = run_frame(
        &context,
        &mut dockspace,
        &mut panes,
        vec![accesskit_action(vertical, Action::Focus)],
    );
    let _ = run_frame(&context, &mut dockspace, &mut panes, Vec::new());
    let _ = run_frame(
        &context,
        &mut dockspace,
        &mut panes,
        key_press(Key::ArrowDown),
    );
    assert_eq!(
        splitter_junction_weights(&dockspace),
        Some((horizontal_root.clone(), horizontal_columns.clone()))
    );
    let _ = run_frame(&context, &mut dockspace, &mut panes, Vec::new());
    let (vertical_root, vertical_columns) = splitter_junction_weights(&dockspace)
        .expect("the vertically adjusted junction remains split");
    assert_eq!(vertical_root, horizontal_root);
    assert!(
        vertical_columns
            .iter()
            .zip(horizontal_columns.iter())
            .all(|(after, before)| after[0] > before[0]),
        "Down Arrow advances every vertical-axis incident splitter together"
    );
}

#[test]
fn default_features_splitter_supports_accesskit_and_keyboard_adjustment() {
    let context = Context::default();
    context.enable_accesskit();
    let mut dockspace = Dockspace::builder("product-splitter-semantics", split_layout())
        .build()
        .expect("the product splitter facade initializes");
    let mut panes = Panes;

    let _ = run_frame(&context, &mut dockspace, &mut panes, Vec::new());
    let stable = run_frame(&context, &mut dockspace, &mut panes, Vec::new());
    let (splitter, node) = accesskit_node(&stable.output, Role::Splitter, "Resize panes");
    assert!(node.supports_action(Action::Focus));
    assert!(node.supports_action(Action::Increment));
    assert!(node.supports_action(Action::Decrement));
    let initial = root_weights(&dockspace).expect("the fixture root remains split");

    let _ = run_frame(
        &context,
        &mut dockspace,
        &mut panes,
        vec![accesskit_action(splitter, Action::Increment)],
    );
    assert_eq!(root_weights(&dockspace), Some(initial.clone()));
    let _ = run_frame(&context, &mut dockspace, &mut panes, Vec::new());
    let increased = root_weights(&dockspace).expect("the fixture root remains split");
    assert!(increased[0] > initial[0]);

    let _ = run_frame(
        &context,
        &mut dockspace,
        &mut panes,
        vec![accesskit_action(splitter, Action::Focus)],
    );
    let _ = run_frame(&context, &mut dockspace, &mut panes, Vec::new());
    let _ = run_frame(
        &context,
        &mut dockspace,
        &mut panes,
        key_press(Key::ArrowLeft),
    );
    assert_eq!(root_weights(&dockspace), Some(increased.clone()));
    let _ = run_frame(&context, &mut dockspace, &mut panes, Vec::new());
    let decreased = root_weights(&dockspace).expect("the fixture root remains split");
    assert!(decreased[0] < increased[0]);
}

#[test]
fn disabled_ancestor_suppresses_splitter_keyboard_and_accesskit_adjustment() {
    let context = Context::default();
    context.enable_accesskit();
    let mut dockspace = Dockspace::builder("product-disabled-ancestor-splitter", split_layout())
        .build()
        .expect("the disabled-ancestor splitter facade initializes");
    let mut panes = Panes;

    let _ = run_frame(&context, &mut dockspace, &mut panes, Vec::new());
    let stable = run_frame(&context, &mut dockspace, &mut panes, Vec::new());
    let (splitter, _) = accesskit_node(&stable.output, Role::Splitter, "Resize panes");
    let _ = run_frame(
        &context,
        &mut dockspace,
        &mut panes,
        vec![accesskit_action(splitter, Action::Focus)],
    );
    let before = root_weights(&dockspace).expect("the disabled fixture root remains split");
    let mut events = vec![accesskit_action(splitter, Action::Increment)];
    events.extend(key_press(Key::ArrowRight));

    let disabled = run_disabled_frame(&context, &mut dockspace, &mut panes, events);
    let (_, node) = accesskit_node(&disabled.output, Role::Splitter, "Resize panes");

    assert!(node.is_disabled());
    assert!(!node.supports_action(Action::Focus));
    assert!(!node.supports_action(Action::Increment));
    assert!(!node.supports_action(Action::Decrement));
    assert_eq!(
        root_weights(&dockspace),
        Some(before),
        "a disabled ancestor must suppress delayed semantic and keyboard adjustment",
    );
}

#[test]
fn default_features_escape_cancels_one_active_drag_and_clears_preview() {
    let context = Context::default();
    context.enable_accesskit();
    let mut dockspace = Dockspace::builder("product-escape", split_layout())
        .style(instrumented_style())
        .build()
        .expect("the product drag facade initializes");
    let mut panes = Panes;

    let _ = run_frame(&context, &mut dockspace, &mut panes, Vec::new());
    let stable = run_frame(&context, &mut dockspace, &mut panes, Vec::new());
    let source = tab_center(&stable.output, "Second");
    let target = Pos2::new(400.0, 72.0);
    let passive = dockspace
        .style()
        .visuals
        .drop_guide_fill
        .expect("the fixture fixes its passive guide color");
    let active = dockspace
        .style()
        .visuals
        .drop_guide_active_fill
        .expect("the fixture fixes its active guide color");

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
            vec![Event::PointerMoved(target)],
        );
    }
    let preview = run_frame(
        &context,
        &mut dockspace,
        &mut panes,
        vec![Event::PointerMoved(target)],
    );
    assert!(guide_button_count(&preview.output, passive, active) >= 4);

    let _ = run_frame(&context, &mut dockspace, &mut panes, key_press(Key::Escape));
    let cancelled = run_frame(&context, &mut dockspace, &mut panes, Vec::new());
    assert_eq!(guide_button_count(&cancelled.output, passive, active), 0);
    assert_eq!(
        context.dragged_id(),
        None,
        "Escape must clear egui's local drag owner together with the core gesture"
    );
    assert_eq!(
        root_split(&dockspace),
        Some((DockspaceAxis::Horizontal, vec![vec![FIRST], vec![SECOND]],)),
    );

    let _ = run_frame(
        &context,
        &mut dockspace,
        &mut panes,
        vec![Event::PointerMoved(target), pointer_button(target, false)],
    );
    assert_eq!(
        root_split(&dockspace),
        Some((DockspaceAxis::Horizontal, vec![vec![FIRST], vec![SECOND]],)),
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
fn pane_focus_observation_settles_only_after_the_terminal_egui_pass() {
    let context = Context::default();
    context.enable_accesskit();
    let mut dockspace = Dockspace::builder("product-pane-focus", layout())
        .build()
        .expect("the pane-focus product facade initializes");
    let mut panes = FocusPanes::new("product-pane-focus", true);

    let _ = run_focus_frame(&context, &mut dockspace, &mut panes, Vec::new());
    let (stable, _) = run_focus_frame(&context, &mut dockspace, &mut panes, Vec::new());
    let (second, _) = accesskit_node(&stable.output, Role::Tab, "Second");
    let _ = run_focus_frame(
        &context,
        &mut dockspace,
        &mut panes,
        vec![accesskit_action(second, Action::Click)],
    );
    assert_eq!(selected(&dockspace), Some(FIRST));
    assert!(!context.memory(|memory| memory.has_focus(panes.target)));

    let _ = run_focus_frame(&context, &mut dockspace, &mut panes, Vec::new());
    assert_eq!(selected(&dockspace), Some(SECOND));
    assert!(
        !context.memory(|memory| memory.has_focus(panes.target)),
        "the delivery update publishes its intent before requesting focus"
    );
    let (_, capability) = run_focus_frame(&context, &mut dockspace, &mut panes, Vec::new());
    assert_eq!(capability, DockspaceCapability::Supported);
    assert!(context.memory(|memory| memory.has_focus(panes.target)));

    let _ = run_focus_frame(&context, &mut dockspace, &mut panes, Vec::new());
    assert!(context.memory(|memory| memory.has_focus(panes.target)));
    context.memory_mut(|memory| memory.surrender_focus(panes.target));
    assert!(!context.memory(|memory| memory.has_focus(panes.target)));
    let _ = run_focus_frame(&context, &mut dockspace, &mut panes, Vec::new());
    assert!(
        !context.memory(|memory| memory.has_focus(panes.target)),
        "the exact terminal acknowledgement completes the request without reclaiming focus",
    );
}

#[test]
fn unavailable_pane_focus_binding_terminates_without_guessing_a_target() {
    let context = Context::default();
    context.enable_accesskit();
    let mut dockspace = Dockspace::builder("product-pane-focus-unavailable", layout())
        .build()
        .expect("the unavailable-focus product facade initializes");
    let mut panes = FocusPanes::new("product-pane-focus-unavailable", false);

    let _ = run_focus_frame(&context, &mut dockspace, &mut panes, Vec::new());
    let (stable, _) = run_focus_frame(&context, &mut dockspace, &mut panes, Vec::new());
    let (second, _) = accesskit_node(&stable.output, Role::Tab, "Second");
    let _ = run_focus_frame(
        &context,
        &mut dockspace,
        &mut panes,
        vec![accesskit_action(second, Action::Click)],
    );

    let _ = run_focus_frame(&context, &mut dockspace, &mut panes, Vec::new());
    assert_eq!(selected(&dockspace), Some(SECOND));
    assert!(
        !context.memory(|memory| memory.has_focus(panes.target)),
        "the selection publication cannot guess an unavailable focus target"
    );
    let (_, capability) = run_focus_frame(&context, &mut dockspace, &mut panes, Vec::new());
    assert_eq!(
        capability,
        DockspaceCapability::Unavailable(DockspaceUnavailableReason::PaneFocusBindingUnavailable,),
    );
    assert!(!context.memory(|memory| memory.has_focus(panes.target)));

    let _ = run_focus_frame(&context, &mut dockspace, &mut panes, Vec::new());
    panes.expose_target = true;
    context.memory_mut(|memory| memory.surrender_focus(panes.target));
    let _ = run_focus_frame(&context, &mut dockspace, &mut panes, Vec::new());
    assert!(
        !context.memory(|memory| memory.has_focus(panes.target)),
        "an acknowledged unavailable request cannot replay when its binding appears",
    );
    dockspace
        .select_item_current(SECOND)
        .expect("a fresh explicit selection publishes a new focus request");
    let (_, capability) = run_focus_frame(&context, &mut dockspace, &mut panes, Vec::new());
    assert_eq!(capability, DockspaceCapability::Supported);
    assert!(
        context.memory(|memory| memory.has_focus(panes.target)),
        "the newly available binding accepts a fresh request"
    );
    let _ = run_focus_frame(&context, &mut dockspace, &mut panes, Vec::new());
    context.memory_mut(|memory| memory.surrender_focus(panes.target));
    let _ = run_focus_frame(&context, &mut dockspace, &mut panes, Vec::new());
    assert!(!context.memory(|memory| memory.has_focus(panes.target)));
}

#[test]
fn discarded_pane_focus_observation_cannot_complete_the_request() {
    let context = Context::default();
    context.enable_accesskit();
    context.options_mut(|options| {
        options.max_passes = 4.try_into().expect("four is non-zero");
    });
    let mut dockspace = Dockspace::builder("product-discarded-pane-focus", layout())
        .build()
        .expect("the discarded-focus product facade initializes");
    let mut panes = FocusPanes::new("product-discarded-pane-focus", true);

    let _ = run_focus_frame(&context, &mut dockspace, &mut panes, Vec::new());
    let _ = run_focus_frame(&context, &mut dockspace, &mut panes, Vec::new());
    dockspace
        .select_item_current(SECOND)
        .expect("the exact pane-focus request precedes the discarded UI passes");
    assert!(!context.memory(|memory| memory.has_focus(panes.target)));

    context
        .plugin_or_default::<LateDiscardPlugin>()
        .lock()
        .remaining = 2;
    let passes =
        run_two_discarded_dockspace_passes(&context, &mut dockspace, &mut panes, Vec::new());
    assert_eq!(passes, 3);
    context.memory_mut(|memory| memory.surrender_focus(panes.target));
    assert!(!context.memory(|memory| memory.has_focus(panes.target)));

    let _ = run_focus_frame(&context, &mut dockspace, &mut panes, Vec::new());
    assert!(
        context.memory(|memory| memory.has_focus(panes.target)),
        "discarded Focused observations leave the request pending for the next legal encoding",
    );
    let _ = run_focus_frame(&context, &mut dockspace, &mut panes, Vec::new());
    assert!(context.memory(|memory| memory.has_focus(panes.target)));
    context.memory_mut(|memory| memory.surrender_focus(panes.target));
    assert!(!context.memory(|memory| memory.has_focus(panes.target)));
    let _ = run_focus_frame(&context, &mut dockspace, &mut panes, Vec::new());
    assert!(
        !context.memory(|memory| memory.has_focus(panes.target)),
        "the later terminal acknowledgement completes the retried request without reclaiming focus"
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
fn default_features_unowned_run_ui_is_capability_negative() {
    let context = Context::default();
    let mut dockspace = Dockspace::builder("product-automatic-run-settlement", layout())
        .build()
        .expect("the product automatic-settlement facade initializes");
    let mut panes = Panes;
    let mut presentation_capability = None;
    let mut pane_focus_capability = None;
    let mut local_actions_current = false;

    for _ in 0..3 {
        let mut output = context.run_ui(input(Vec::new()), |ui| {
            let response = dockspace
                .show_single_surface(SURFACE, ui, &mut panes)
                .expect("the paint-only product frame advances");
            presentation_capability = Some(response.presentation_settlement_capability());
            pane_focus_capability = Some(response.pane_focus_capability());
            local_actions_current |= response.interaction_capabilities().local_actions_current();
        });
        output.textures_delta.clear();
    }

    assert!(
        local_actions_current,
        "ordinary eframe-style callbacks keep current-pass local actions",
    );
    assert_eq!(
        presentation_capability,
        Some(DockspaceCapability::Unavailable(
            DockspaceUnavailableReason::PresentationSettlementRequired,
        )),
        "an unowned egui callback cannot mint terminal presentation proof",
    );
    assert_eq!(
        pane_focus_capability,
        Some(DockspaceCapability::Unavailable(
            DockspaceUnavailableReason::PresentationSettlementRequired,
        )),
        "pane focus remains capability-negative without terminal presentation proof",
    );
}

#[test]
fn unowned_splitter_junction_advertises_only_available_semantic_controls() {
    let context = Context::default();
    context.enable_accesskit();
    let mut dockspace = Dockspace::builder(
        "product-unowned-splitter-junction",
        splitter_junction_layout(),
    )
    .build()
    .expect("the unowned junction facade initializes");
    let mut panes = Panes;
    let mut stable = None;

    for _ in 0..3 {
        let mut output = context.run_ui(input(Vec::new()), |ui| {
            dockspace
                .show_single_surface(SURFACE, ui, &mut panes)
                .expect("the unowned junction frame advances");
        });
        if output.platform_output.accesskit_update.is_some() {
            stable = Some(output.platform_output.accesskit_update.clone());
        }
        output.textures_delta.clear();
    }

    let update = stable
        .flatten()
        .expect("the unowned junction publishes accessibility semantics");
    let group = update
        .nodes
        .iter()
        .find_map(|(_, node)| {
            (node.role() == Role::Group && node.label() == Some("Resize pane grid")).then_some(node)
        })
        .expect("the unowned junction publishes its semantic group");
    assert_eq!(
        group.description(),
        Some("Use the horizontal and vertical splitter controls to resize rows and columns"),
    );
}

#[test]
fn unowned_splitter_junction_describes_one_enabled_axis_exactly() {
    let context = Context::default();
    context.enable_accesskit();
    let mut policy = DockPolicy::default();
    policy.set_allow_resize_axis(DockspaceAxis::Vertical, false);
    let mut dockspace = Dockspace::builder(
        "product-unowned-single-axis-junction",
        splitter_junction_layout(),
    )
    .policy(policy)
    .build()
    .expect("the single-axis junction facade initializes");
    let mut panes = Panes;
    let mut stable = None;

    for _ in 0..3 {
        let mut output = context.run_ui(input(Vec::new()), |ui| {
            dockspace
                .show_single_surface(SURFACE, ui, &mut panes)
                .expect("the single-axis junction frame advances");
        });
        if output.platform_output.accesskit_update.is_some() {
            stable = Some(output.platform_output.accesskit_update.clone());
        }
        output.textures_delta.clear();
    }

    let update = stable
        .flatten()
        .expect("the single-axis junction publishes accessibility semantics");
    let node_by_label = |role, label| {
        update
            .nodes
            .iter()
            .find_map(|(_, node)| {
                (node.role() == role && node.label() == Some(label)).then_some(node)
            })
            .expect("the requested junction semantic node is present")
    };
    let group = node_by_label(Role::Group, "Resize pane grid");
    assert_eq!(
        group.description(),
        Some("Use the horizontal splitter control to resize columns"),
    );
    let horizontal = node_by_label(Role::Splitter, "Resize pane grid horizontally");
    assert!(!horizontal.is_disabled());
    assert!(horizontal.supports_action(Action::Increment));
    let vertical = node_by_label(Role::Splitter, "Resize pane grid vertically");
    assert!(vertical.is_disabled());
    assert!(!vertical.supports_action(Action::Increment));
    assert!(!vertical.supports_action(Action::Decrement));
}

#[test]
fn unowned_run_ui_does_not_start_a_preview_qualified_tab_drag() {
    let context = Context::default();
    context.enable_accesskit();
    let mut dockspace = Dockspace::builder("product-unowned-tab-drag", layout())
        .build()
        .expect("the unowned tab-drag facade initializes");
    let mut panes = Panes;
    let before = root_tabs(&dockspace).expect("the root remains a tab group");

    let mut run_unowned = |events| {
        let mut capability = None;
        let mut dragged_id = None;
        let mut output = context.run_ui(input(events), |ui| {
            let response = dockspace
                .show_single_surface(SURFACE, ui, &mut panes)
                .expect("the unowned product frame advances");
            capability = Some(response.presentation_settlement_capability());
            dragged_id = ui.ctx().dragged_id();
        });
        output.textures_delta.clear();
        (output, capability, dragged_id)
    };

    let _ = run_unowned(Vec::new());
    let (stable, capability, _) = run_unowned(Vec::new());
    assert_eq!(
        capability,
        Some(DockspaceCapability::Unavailable(
            DockspaceUnavailableReason::PresentationSettlementRequired,
        )),
    );
    let source = tab_center(&stable, "Second");
    let target = tab_center(&stable, "First");

    let (_, _, pressed_dragged_id) = run_unowned(vec![
        Event::PointerMoved(source),
        pointer_button(source, true),
    ]);
    let (_, _, moved_dragged_id) = run_unowned(vec![Event::PointerMoved(target)]);
    let _ = run_unowned(vec![
        Event::PointerMoved(target),
        pointer_button(target, false),
    ]);

    assert!(
        pressed_dragged_id.is_none() && moved_dragged_id.is_none(),
        "a capability-negative tab remains clickable without claiming egui drag ownership",
    );

    assert_eq!(
        root_tabs(&dockspace).expect("the root remains a tab group"),
        before,
        "an unowned callback must not begin or commit a preview-qualified tab drag",
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

#[test]
fn default_features_contained_resize_commits_a_painted_preview() {
    for (direction, delta) in [
        ("East", vec2(30.0, 0.0)),
        ("North", vec2(0.0, -24.0)),
        ("South", vec2(0.0, 24.0)),
        ("West", vec2(-30.0, 0.0)),
        ("NorthEast", vec2(30.0, -24.0)),
        ("SouthEast", vec2(30.0, 24.0)),
        ("SouthWest", vec2(-30.0, 24.0)),
        ("NorthWest", vec2(-30.0, -24.0)),
    ] {
        let context = Context::default();
        context.enable_accesskit();
        let mut dockspace =
            Dockspace::builder(("product-contained-resize", direction), contained_layout())
                .style(instrumented_style())
                .build()
                .expect("the product contained facade initializes");
        let mut panes = ProjectionPanes::default();

        let _ = run_pane_frame(&context, &mut dockspace, &mut panes, Vec::new());
        let stable = run_pane_frame(&context, &mut dockspace, &mut panes, Vec::new());
        let baseline = [panes.latest(FIRST), panes.latest(SECOND)];
        panes.assert_frame(&context, &stable, baseline, baseline[1].bounds);
        let initial = contained_rect(&dockspace);
        let north = node_rect(&stable.output, Role::Splitter, "Resize floating top edge");
        let east = node_rect(&stable.output, Role::Splitter, "Resize floating right edge");
        let south = node_rect(
            &stable.output,
            Role::Splitter,
            "Resize floating bottom edge",
        );
        let west = node_rect(&stable.output, Role::Splitter, "Resize floating left edge");
        // Cardinal semantic bounds also locate the actual corner bands without guessed insets.
        let source = Pos2::new(
            if delta.x < 0.0 {
                west.center().x
            } else if delta.x > 0.0 {
                east.center().x
            } else {
                north.center().x
            },
            if delta.y < 0.0 {
                north.center().y
            } else if delta.y > 0.0 {
                south.center().y
            } else {
                east.center().y
            },
        );
        let destination = source + delta;
        let min_delta = delta.min(egui::Vec2::ZERO);
        let max_delta = delta.max(egui::Vec2::ZERO);
        let projected_content = Rect::from_min_max(
            baseline[1].bounds.min + min_delta,
            baseline[1].bounds.max + max_delta,
        );

        let armed = run_pane_frame(
            &context,
            &mut dockspace,
            &mut panes,
            vec![Event::PointerMoved(source), pointer_button(source, true)],
        );
        assert_eq!(contained_rect(&dockspace), initial, "{direction}: press");
        panes.assert_frame(&context, &armed, baseline, baseline[1].bounds);

        for events in [vec![Event::PointerMoved(destination)], Vec::new()] {
            let preview = run_pane_frame(&context, &mut dockspace, &mut panes, events);
            assert_eq!(
                contained_rect(&dockspace),
                initial,
                "{direction}: even a presented resize remains transient before release",
            );
            panes.assert_frame(&context, &preview, baseline, projected_content);
            let top_right_delta = vec2(max_delta.x, min_delta.y);
            let bottom_left_delta = vec2(min_delta.x, max_delta.y);
            for (role, label, min_shift, max_shift) in [
                (Role::TitleBar, "Second", min_delta, top_right_delta),
                (Role::Tab, "Second", min_delta, min_delta),
                (
                    Role::Splitter,
                    "Resize floating top edge",
                    min_delta,
                    top_right_delta,
                ),
                (
                    Role::Splitter,
                    "Resize floating right edge",
                    top_right_delta,
                    max_delta,
                ),
                (
                    Role::Splitter,
                    "Resize floating bottom edge",
                    bottom_left_delta,
                    max_delta,
                ),
                (
                    Role::Splitter,
                    "Resize floating left edge",
                    min_delta,
                    bottom_left_delta,
                ),
            ] {
                let (before_id, before) = accesskit_node(&stable.output, role, label);
                let (during_id, during) = accesskit_node(&preview.output, role, label);
                let before = accesskit_node_rect(before);
                assert_eq!(during_id, before_id, "{direction}: {label} identity");
                assert_eq!(
                    accesskit_node_rect(during),
                    Rect::from_min_max(before.min + min_shift, before.max + max_shift),
                    "{direction}: {label} follows the live content projection",
                );
            }
        }

        let released = run_pane_frame(
            &context,
            &mut dockspace,
            &mut panes,
            vec![pointer_button(destination, false)],
        );
        panes.assert_frame(&context, &released, baseline, projected_content);
        let settled = run_pane_frame(&context, &mut dockspace, &mut panes, Vec::new());
        panes.assert_frame(&context, &settled, baseline, projected_content);
        let committed = contained_rect(&dockspace);
        assert_eq!(
            committed,
            LogicalRect::new(
                initial.min().x() + f64::from(min_delta.x),
                initial.min().y() + f64::from(min_delta.y),
                initial.width() + f64::from(max_delta.x - min_delta.x),
                initial.height() + f64::from(max_delta.y - min_delta.y),
            )
            .expect("the outward resize keeps a valid contained rectangle"),
            "{direction}: release commits exactly the projected edges",
        );
        let idle = run_pane_frame(&context, &mut dockspace, &mut panes, Vec::new());
        panes.assert_frame(&context, &idle, baseline, projected_content);
        assert_eq!(contained_rect(&dockspace), committed, "{direction}: idle");
    }
}

#[test]
fn rear_contained_title_drag_raises_without_a_bootstrap_mask() {
    let context = Context::default();
    context.enable_accesskit();
    let mut dockspace = Dockspace::builder(
        "product-contained-activation-continuity",
        stacked_contained_layout(),
    )
    .build()
    .expect("the stacked contained facade initializes");
    let mut panes = Panes;

    let _ = run_frame(&context, &mut dockspace, &mut panes, Vec::new());
    let stable = run_frame(&context, &mut dockspace, &mut panes, Vec::new());
    let source = node_center(&stable.output, Role::TitleBar, "Second");
    let moved = source + vec2(24.0, 0.0);

    let _ = run_frame(
        &context,
        &mut dockspace,
        &mut panes,
        vec![Event::PointerMoved(source), pointer_button(source, true)],
    );
    let dragged = run_frame(
        &context,
        &mut dockspace,
        &mut panes,
        vec![Event::PointerMoved(moved)],
    );

    let _ = accesskit_node(&dragged.output, Role::TitleBar, "Second");
    let _ = accesskit_node(&dragged.output, Role::TitleBar, "Fourth");
    assert_eq!(
        dockspace
            .view()
            .surface(SURFACE)
            .expect("the stacked surface remains present")
            .contained()
            .map(|contained| contained.id())
            .collect::<Vec<_>>(),
        [FRONT_FLOATING, FLOATING],
        "the rear contained root raises without replacing the terminal pass with bootstrap paint",
    );
}

#[test]
fn default_features_contained_title_moves_without_a_dock_target() {
    let context = Context::default();
    context.enable_accesskit();
    let mut dockspace = Dockspace::builder("product-contained-move", contained_layout())
        .style(instrumented_style())
        .build()
        .expect("the product contained facade initializes");
    let mut panes = ProjectionPanes::default();

    let _ = run_pane_frame(&context, &mut dockspace, &mut panes, Vec::new());
    let stable = run_pane_frame(&context, &mut dockspace, &mut panes, Vec::new());
    let baseline = [panes.latest(FIRST), panes.latest(SECOND)];
    panes.assert_frame(&context, &stable, baseline, baseline[1].bounds);
    let initial = contained_rect(&dockspace);
    let source = node_center(&stable.output, Role::TitleBar, "Second");
    let delta = vec2(-120.0, 80.0);
    let destination = source + delta;
    let projected_content = baseline[1].bounds.translate(delta);

    let armed = run_pane_frame(
        &context,
        &mut dockspace,
        &mut panes,
        vec![Event::PointerMoved(source), pointer_button(source, true)],
    );
    assert_eq!(contained_rect(&dockspace), initial);
    panes.assert_frame(&context, &armed, baseline, baseline[1].bounds);

    for events in [vec![Event::PointerMoved(destination)], Vec::new()] {
        let moving = run_pane_frame(&context, &mut dockspace, &mut panes, events);
        assert_eq!(
            contained_rect(&dockspace),
            initial,
            "even a presented contained move remains transient before release",
        );
        panes.assert_frame(&context, &moving, baseline, projected_content);
        for (role, label) in [
            (Role::TitleBar, "Second"),
            (Role::Tab, "Second"),
            (Role::Button, "Close floating Second"),
        ] {
            let (before_id, before) = accesskit_node(&stable.output, role, label);
            let (during_id, during) = accesskit_node(&moving.output, role, label);
            assert_eq!(during_id, before_id, "{label} keeps its semantic identity");
            assert_eq!(
                accesskit_node_rect(during),
                accesskit_node_rect(before).translate(delta),
                "{label} follows the same complete pane projection",
            );
        }
    }

    let released = run_pane_frame(
        &context,
        &mut dockspace,
        &mut panes,
        vec![pointer_button(destination, false)],
    );
    panes.assert_frame(&context, &released, baseline, projected_content);
    let settled = run_pane_frame(&context, &mut dockspace, &mut panes, Vec::new());
    panes.assert_frame(&context, &settled, baseline, projected_content);
    let committed = contained_rect(&dockspace);
    assert_eq!(committed.width(), initial.width());
    assert_eq!(committed.height(), initial.height());
    assert_eq!(committed.min().x(), initial.min().x() + f64::from(delta.x));
    assert_eq!(committed.min().y(), initial.min().y() + f64::from(delta.y));
    let idle = run_pane_frame(&context, &mut dockspace, &mut panes, Vec::new());
    panes.assert_frame(&context, &idle, baseline, projected_content);
    assert_eq!(contained_rect(&dockspace), committed);
}

#[test]
fn default_features_contained_title_redocks_through_the_canonical_guide() {
    let context = Context::default();
    let mut dockspace = Dockspace::builder("product-contained-redock", contained_layout())
        .build()
        .expect("the product contained facade initializes");
    let mut panes = Panes;

    let _ = run_frame(&context, &mut dockspace, &mut panes, Vec::new());
    let _ = run_frame(&context, &mut dockspace, &mut panes, Vec::new());
    let source = contained_title_point(&dockspace);
    let target = Pos2::new(400.0, 72.0);

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
            vec![Event::PointerMoved(target)],
        );
    }
    let _ = run_frame(
        &context,
        &mut dockspace,
        &mut panes,
        vec![Event::PointerMoved(target), pointer_button(target, false)],
    );
    let _ = run_frame(&context, &mut dockspace, &mut panes, Vec::new());

    assert_eq!(
        root_split(&dockspace),
        Some((DockspaceAxis::Vertical, vec![vec![SECOND], vec![FIRST]],)),
    );
    assert!(dockspace.view().contained(FLOATING).is_none());
}

#[test]
fn contained_tab_redocks_through_an_ordinary_pane_basin_without_guide_aiming() {
    let context = Context::default();
    context.enable_accesskit();
    context.options_mut(|options| {
        options.max_passes = 4.try_into().expect("four is non-zero");
    });
    let mut dockspace = Dockspace::builder("product-contained-magnetic-redock", contained_layout())
        .style(instrumented_style())
        .build()
        .expect("the contained magnetic-redock facade initializes");
    let mut panes = Panes;

    let _ = run_frame(&context, &mut dockspace, &mut panes, Vec::new());
    let stable = run_frame(&context, &mut dockspace, &mut panes, Vec::new());
    let source = node_center(&stable.output, Role::Tab, "Second");
    let root_tab = node_rect(&stable.output, Role::Tab, "First");
    let target = Pos2::new(root_tab.max.x + 280.0, root_tab.max.y + 300.0);
    let source_rect = contained_rect(&dockspace);
    assert!(
        !Rect::from_min_size(
            Pos2::new(source_rect.x() as f32, source_rect.y() as f32),
            vec2(source_rect.width() as f32, source_rect.height() as f32),
        )
        .contains(target),
        "the first magnetic observation must leave the contained source rectangle"
    );
    let passive = dockspace
        .style()
        .visuals
        .drop_guide_fill
        .expect("the fixture fixes its passive guide color");
    let active = dockspace
        .style()
        .visuals
        .drop_guide_active_fill
        .expect("the fixture fixes its active guide color");
    let drop_fill = dockspace
        .style()
        .visuals
        .drop_fill
        .expect("the fixture fixes its preview fill");

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
        vec![Event::PointerMoved(target)],
    );

    let mut preview = None;
    for _ in 0..8 {
        let frame = run_frame(
            &context,
            &mut dockspace,
            &mut panes,
            vec![Event::PointerMoved(target)],
        );
        let pane_merge = rect_with_fill(&frame.output, drop_fill).is_some_and(|rect| {
            rect.contains(target) && rect.width() > source_rect.width() as f32 * 2.0
        });
        if pane_merge {
            preview = Some(frame);
            break;
        }
    }
    let preview = preview
        .expect("the ordinary pane point publishes a full-pane Dock preview before the deadline");
    assert!(
        preview.output.shapes.iter().any(|shape| matches!(
            &shape.shape,
            egui::Shape::Rect(rect) if rect.fill == active
        )),
        "the magnetic pane candidate highlights its explanatory guide"
    );
    assert!(
        preview.output.shapes.iter().all(|shape| !matches!(
            &shape.shape,
            egui::Shape::Rect(rect)
                if (rect.fill == passive || rect.fill == active) && rect.rect.contains(target)
        )),
        "the contained redock journey must not aim at an explicit guide button"
    );

    let _ = run_frame(
        &context,
        &mut dockspace,
        &mut panes,
        vec![Event::PointerMoved(target), pointer_button(target, false)],
    );
    let mut redocked = false;
    for _ in 0..8 {
        if root_tabs(&dockspace) == Some(vec![FIRST, SECOND])
            && dockspace.view().contained(FLOATING).is_none()
        {
            redocked = true;
            break;
        }
        let _ = run_frame(&context, &mut dockspace, &mut panes, Vec::new());
    }

    assert!(
        redocked,
        "a contained tab released in the ordinary pane basin must redock without guide aiming"
    );
    let moved = dockspace
        .view()
        .item(SECOND)
        .expect("the redocked contained item remains open");
    assert_eq!(moved.surface(), SURFACE);
    assert_eq!(moved.root(), ROOT);
    assert_eq!(moved.contained(), None);
    assert!(moved.is_selected());
    assert_eq!(context.dragged_id(), None);
}

#[test]
fn default_features_replace_policy_and_style_atomically() {
    let mut dockspace = Dockspace::builder("product-online-configuration", layout())
        .build()
        .expect("the product configuration facade initializes");
    let initial_style = dockspace.style().clone();
    let mut replacement_style = initial_style.clone();
    replacement_style.tab_bar_height += 8.0;
    replacement_style.visuals.tab_active_fill = Some(egui::Color32::from_rgb(12, 34, 56));

    let style_mutation = dockspace
        .set_style(replacement_style.clone())
        .expect("valid style replacement commits");
    assert_eq!(dockspace.style(), &replacement_style);
    assert!(!style_mutation.workspace_changed());
    assert!(style_mutation.published_state_changed());
    assert_eq!(style_mutation.affected_surfaces(), &[SURFACE]);

    let version_before_visual_only_style = dockspace.version();
    let mut visual_only_style = replacement_style.clone();
    visual_only_style.visuals.tab_active_fill = Some(egui::Color32::from_rgb(78, 90, 123));
    let visual_only_mutation = dockspace
        .set_style(visual_only_style.clone())
        .expect("visual-only style replacement commits");
    assert_eq!(dockspace.version(), version_before_visual_only_style);
    assert_eq!(dockspace.style(), &visual_only_style);
    assert!(!visual_only_mutation.workspace_changed());
    assert!(visual_only_mutation.published_state_changed());
    assert_eq!(visual_only_mutation.affected_surfaces(), &[SURFACE]);

    let mut replacement_policy = dockspace.policy().clone();
    replacement_policy.set_allow_tab_merge(false);
    let policy_mutation = dockspace
        .set_policy(replacement_policy.clone())
        .expect("valid policy replacement commits");
    assert_eq!(dockspace.policy(), &replacement_policy);
    assert!(policy_mutation.published_state_changed());

    let version_before_invalid_style = dockspace.version();
    let mut invalid_style = visual_only_style.clone();
    invalid_style.tab_min_width = invalid_style.tab_max_width + 1.0;
    assert!(dockspace.set_style(invalid_style).is_err());
    assert_eq!(dockspace.version(), version_before_invalid_style);
    assert_eq!(dockspace.style(), &visual_only_style);
}

#[test]
fn shared_local_interaction_scenario_maps_to_egui_product_headless_semantics() {
    let mut harness = kittest_harness(
        "product-kittest-local-interactions",
        shared_semantic_layout(),
    );
    assert_eq!(
        LOCAL_TAB_CLOSE_SPLITTER_AND_CONTAINED.id(),
        ScenarioId::LocalTabCloseSplitterAndContained,
    );
    assert_eq!(
        LOCAL_TAB_CLOSE_SPLITTER_AND_CONTAINED.journey(),
        JourneyId::L08,
    );
    let supported = [
        ScenarioCapability::LocalTabSelection,
        ScenarioCapability::LocalItemClose,
        ScenarioCapability::LocalSplitterAdjustment,
        ScenarioCapability::LocalContainedResize,
    ];
    assert_eq!(
        LOCAL_TAB_CLOSE_SPLITTER_AND_CONTAINED.admission(&supported),
        ScenarioAdmission::Ready,
        "egui-product-headless admits the shared scenario only from declared capabilities",
    );
    assert_eq!(
        NATIVE_CREATE_REDOCK_RETIREMENT.admission(&supported),
        ScenarioAdmission::Unsupported {
            missing: ScenarioCapability::NativeWindowLifecycle,
        },
        "the single-surface product executor must reject native-only scenarios before input",
    );
    let mut observations = Vec::new();

    for step in LOCAL_TAB_CLOSE_SPLITTER_AND_CONTAINED.steps() {
        let (item, label) = match step.selector() {
            SemanticSelector::Tab(ItemAlias::Primary)
            | SemanticSelector::TabClose(ItemAlias::Primary) => (FIRST, "First"),
            SemanticSelector::Tab(ItemAlias::Secondary)
            | SemanticSelector::TabClose(ItemAlias::Secondary) => (SECOND, "Second"),
            SemanticSelector::Tab(ItemAlias::Contained)
            | SemanticSelector::TabClose(ItemAlias::Contained) => (FOURTH, "Fourth"),
            SemanticSelector::Splitter(_) | SemanticSelector::ContainedResize { .. } => {
                (FOURTH, "Fourth")
            }
        };
        match (step.selector(), step.action()) {
            (SemanticSelector::Tab(alias), ScenarioAction::Activate) => {
                harness
                    .get_by_role_and_label(Role::Tab, label)
                    .click_accesskit();
                harness.run_steps(2);
                assert_eq!(selected(&harness.state().dockspace), Some(item));
                assert_eq!(alias, ItemAlias::Secondary);
                observations.push(step.expected());
            }
            (SemanticSelector::TabClose(alias), ScenarioAction::Activate) => {
                harness
                    .get_by_role_and_label(Role::Button, &format!("Close {label}"))
                    .click_accesskit();
                harness.run_steps(2);
                let request = harness
                    .state_mut()
                    .close_requests
                    .pop()
                    .expect("the semantic close control opens one close plan");
                assert_eq!(request.plan().items()[0].item(), item);
                assert_eq!(alias, ItemAlias::Secondary);
                observations.push(step.expected());
            }
            (SemanticSelector::Splitter(ScenarioAxis::Horizontal), ScenarioAction::Increment) => {
                let before = root_weights(&harness.state().dockspace)
                    .expect("the shared product root remains split");
                harness
                    .get_by_role_and_label(Role::Splitter, "Resize panes")
                    .focus();
                harness.key_press(Key::ArrowRight);
                harness.run_steps(2);
                let after = root_weights(&harness.state().dockspace)
                    .expect("the adjusted product root remains split");
                assert!(after[0] > before[0]);
                observations.push(step.expected());
            }
            (
                SemanticSelector::ContainedResize {
                    item: ItemAlias::Contained,
                    edge: ScenarioEdge::East,
                },
                ScenarioAction::Increment,
            ) => {
                let before = contained_rect(&harness.state().dockspace);
                harness
                    .get_by_role_and_label(Role::Splitter, "Resize floating right edge")
                    .focus();
                harness.key_press(Key::ArrowRight);
                harness.run_steps(2);
                let after = contained_rect(&harness.state().dockspace);
                assert_eq!(after.min(), before.min());
                assert!(after.width() > before.width());
                assert_eq!(after.height(), before.height());
                observations.push(step.expected());
            }
            (selector, action) => {
                panic!("unsupported shared semantic step: {selector:?} {action:?}")
            }
        }
    }

    assert_eq!(
        observations,
        LOCAL_TAB_CLOSE_SPLITTER_AND_CONTAINED.expected_observations(),
        "the egui-product-headless executor must map every shared semantic step",
    );
}

#[test]
fn disabled_tab_bar_is_paint_only_for_pointer_keyboard_and_accessibility() {
    let context = Context::default();
    context.enable_accesskit();
    let mut policy = dockspace::policy::DockPolicy::default();
    let mut target = dockspace::policy::DockTargetRule::default();
    target.set_tab_bar(dockspace::policy::TabBarPolicy::new(
        dockspace::policy::TabBarVisibility::Visible,
        dockspace::policy::TabBarInteraction::Disabled,
    ));
    policy.set_target_rule(dockspace::policy::DockTargetRuleKey::Item(FIRST), target);
    let mut dockspace = Dockspace::builder("product-disabled-tab-bar", layout())
        .policy(policy)
        .build()
        .expect("the paint-only tab-bar facade initializes");
    let mut panes = Panes;

    let _ = run_frame(&context, &mut dockspace, &mut panes, Vec::new());
    let stable = run_frame(&context, &mut dockspace, &mut panes, Vec::new());
    assert_eq!(selected(&dockspace), Some(FIRST));
    let (second_id, second_node) = accesskit_node(&stable.output, Role::Tab, "Second");
    assert!(second_node.is_disabled());
    assert!(!second_node.supports_action(Action::Focus));
    assert!(!second_node.supports_action(Action::Click));
    let second = node_rect(&stable.output, Role::Tab, "Second").center();

    let _ = run_frame(
        &context,
        &mut dockspace,
        &mut panes,
        vec![
            Event::PointerMoved(second),
            pointer_button(second, true),
            pointer_button(second, false),
        ],
    );
    assert_eq!(selected(&dockspace), Some(FIRST));

    let _ = run_frame(
        &context,
        &mut dockspace,
        &mut panes,
        vec![
            accesskit_action(second_id, Action::Focus),
            accesskit_action(second_id, Action::Click),
        ],
    );
    let _ = run_frame(&context, &mut dockspace, &mut panes, key_press(Key::Enter));
    assert_eq!(selected(&dockspace), Some(FIRST));
}

#[test]
fn kittest_reports_unowned_contained_drag_but_keeps_semantic_resize() {
    let mut harness = kittest_harness("product-kittest-contained", contained_layout());
    let initial = contained_rect(&harness.state().dockspace);
    assert_eq!(
        harness.state().presentation_settlement_capability,
        Some(DockspaceCapability::Unavailable(
            DockspaceUnavailableReason::PresentationSettlementRequired,
        )),
        "egui_kittest owns its run boundary, so drag settlement must remain unavailable",
    );
    let source = harness
        .get_by_role_and_label(Role::TitleBar, "Second")
        .rect()
        .center();
    let destination = source + vec2(-80.0, 60.0);

    harness.hover_at(source);
    harness.step();
    harness.drag_at(source);
    harness.step();
    harness.hover_at(destination);
    harness.step();
    harness.drop_at(destination);
    harness.run_steps(3);

    let after_drag = contained_rect(&harness.state().dockspace);
    assert_eq!(
        after_drag, initial,
        "an unowned callback must not start a preview-qualified contained drag",
    );
    assert_eq!(
        surface_ids(&harness.state().dockspace),
        [SURFACE],
        "same-context contained motion must not synthesize a native logical surface"
    );

    harness
        .get_by_role_and_label(Role::Splitter, "Resize floating right edge")
        .focus();
    harness.run_steps(2);
    harness.key_press(Key::ArrowRight);
    harness.run_steps(3);

    let resized = contained_rect(&harness.state().dockspace);
    assert_eq!(resized.min(), after_drag.min());
    assert!(resized.width() > after_drag.width());
    assert_eq!(resized.height(), after_drag.height());
    assert_eq!(
        surface_ids(&harness.state().dockspace),
        [SURFACE],
        "same-context contained resize must preserve the sole logical surface"
    );
}

#[test]
fn kittest_adjusts_splitter_junction_axes_independently() {
    let mut harness = kittest_harness("product-kittest-junction", splitter_junction_layout());
    let (initial_root, initial_columns) = splitter_junction_weights(&harness.state().dockspace)
        .expect("the kittest junction fixture remains split");

    harness
        .get_by_role_and_label(Role::Splitter, "Resize pane grid horizontally")
        .focus();
    harness.run_steps(2);
    harness.key_press(Key::ArrowRight);
    harness.run_steps(3);

    let (horizontal_root, horizontal_columns) =
        splitter_junction_weights(&harness.state().dockspace)
            .expect("the horizontally adjusted kittest junction remains split");
    assert!(horizontal_root[0] > initial_root[0]);
    assert_eq!(horizontal_columns, initial_columns);

    harness
        .get_by_role_and_label(Role::Splitter, "Resize pane grid vertically")
        .focus();
    harness.run_steps(2);
    harness.key_press(Key::ArrowDown);
    harness.run_steps(3);

    let (vertical_root, vertical_columns) = splitter_junction_weights(&harness.state().dockspace)
        .expect("the vertically adjusted kittest junction remains split");
    assert_eq!(vertical_root, horizontal_root);
    assert!(
        vertical_columns
            .iter()
            .zip(horizontal_columns.iter())
            .all(|(after, before)| after[0] > before[0])
    );
}
