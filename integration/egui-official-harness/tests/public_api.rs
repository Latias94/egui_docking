use std::time::Duration;

use dockspace::command::WorkspaceCommand;
use dockspace::graph::{Node, RootRecord, SurfacePresentation, Workspace};
use dockspace::ids::{ItemId, RootId, SurfaceId};
use egui::accesskit::{Action, Role};
use egui::{Context, RawInput, Rect, Ui, ViewportId, vec2};
use egui_dockspace::backend::{EguiFrameScheduleKey, EguiPresentationResult};
use egui_dockspace::{Dockspace, DockspaceCommandOutcome, PaneView};

const SURFACE: SurfaceId = SurfaceId::new(1);
const ROOT: RootId = RootId::new(1);
const ITEM: ItemId = ItemId::new(1);

#[derive(Default)]
struct SmokePanes {
    paint_count: usize,
}

impl PaneView for SmokePanes {
    fn title(&self, item: ItemId) -> Option<egui::WidgetText> {
        (item == ITEM).then(|| "Official egui pane".into())
    }

    fn ui(&mut self, item: ItemId, ui: &mut Ui) {
        if item == ITEM {
            self.paint_count += 1;
            ui.label("Rendered through the public egui_dockspace facade");
        }
    }
}

fn workspace() -> Workspace {
    let mut builder = Workspace::builder();
    let tabs = builder.insert_node(Node::tabs([ITEM]));
    builder.set_root(ROOT, RootRecord::new(tabs).with_central(tabs));
    builder.set_surface(SURFACE, SurfacePresentation::with_main(ROOT));
    builder.build().expect("the smoke workspace is valid")
}

fn raw_input() -> RawInput {
    RawInput {
        screen_rect: Some(Rect::from_min_size(egui::Pos2::ZERO, vec2(800.0, 600.0))),
        ..RawInput::default()
    }
}

#[test]
fn official_egui_consumes_the_public_single_surface_facade() {
    let _native_options = eframe::NativeOptions::default();
    let context = Context::default();
    context.enable_accesskit();
    let mut dockspace = Dockspace::builder("official-egui-consumer", workspace())
        .build()
        .expect("the public facade accepts a valid workspace");
    let root_node = dockspace
        .workspace()
        .root(ROOT)
        .expect("the fixture root exists")
        .node;
    let source = dockspace
        .workspace()
        .capture_item_source(ROOT, root_node, ITEM)
        .expect("the fixture item source is current");
    let command = dockspace
        .submit_command(WorkspaceCommand::Select { source })
        .expect("the public command boundary reduces one checked command");
    assert!(matches!(
        command.outcome(),
        DockspaceCommandOutcome::Applied(_)
    ));
    assert!(!command.mutation().workspace_changed());
    let mut panes = SmokePanes::default();
    let mut last_repaint_delay = Duration::ZERO;
    let mut last_tree = None;
    let mut saw_interactive_pass = false;

    for _ in 0..4 {
        let mut output = context.run_ui(raw_input(), |ui| {
            let response = dockspace
                .show_single_surface(SURFACE, ui, &mut panes)
                .expect("the public facade advances an official-egui frame");
            saw_interactive_pass |= response.interactions_current();
            assert!(response.missing_panes().is_empty());
            assert!(response.capture_errors().is_empty());
        });
        last_repaint_delay = output
            .viewport_output
            .get(&ViewportId::ROOT)
            .expect("the root viewport has output")
            .repaint_delay;
        last_tree = output.platform_output.accesskit_update.take();
        output.textures_delta.clear();
    }

    assert!(panes.paint_count > 0, "the selected pane must paint");
    assert_ne!(
        last_repaint_delay,
        Duration::ZERO,
        "a stable local-response frame must not spin",
    );
    assert!(
        saw_interactive_pass,
        "a Ready official-egui frame must accept local Response actions",
    );

    let tree = last_tree.expect("AccessKit output is enabled");
    let mut actionable_tabs = 0;
    for (_, node) in &tree.nodes {
        if node.role() == Role::Tab && !node.is_disabled() {
            actionable_tabs += 1;
            assert!(node.supports_action(Action::Focus));
            assert!(node.supports_action(Action::Click));
        }
    }
    assert!(
        actionable_tabs > 0,
        "the smoke must expose actionable docking tabs",
    );
}

#[test]
fn official_egui_consumes_the_outer_presentation_protocol() {
    let context = Context::default();
    let mut dockspace = Dockspace::builder("official-egui-outer-host", workspace())
        .build()
        .expect("the public facade accepts a valid workspace");
    let mut panes = SmokePanes::default();

    let mut bootstrap = dockspace
        .begin_outer_frame(EguiFrameScheduleKey::new(1, 0))
        .expect("outer frame begins");
    bootstrap
        .run_surface(SURFACE, &context, raw_input(), &mut panes)
        .expect("the outer host owns the surface run");
    let (_, outputs) = bootstrap
        .finish()
        .expect("bootstrap frame commits")
        .into_parts();
    assert!(
        outputs
            .iter()
            .all(|output| !output.has_presentation_obligation()),
        "bootstrap paint must not fabricate a presentation obligation",
    );
    for output in outputs {
        output.settle_with(|_, mut full_output| {
            full_output.textures_delta.clear();
            EguiPresentationResult::Dropped
        });
    }

    let mut ready = dockspace
        .begin_outer_frame(EguiFrameScheduleKey::new(2, 0))
        .expect("ready frame begins");
    ready
        .run_surface(SURFACE, &context, raw_input(), &mut panes)
        .expect("the ready host owns the surface run");
    let (_, pending) = ready.finish().expect("ready frame commits").into_parts();
    pending
        .into_iter()
        .next()
        .expect("ready paint has a settlement obligation")
        .settle_with(|_, mut full_output| {
            full_output.textures_delta.clear();
            EguiPresentationResult::Presented
        });

    let mut settled = dockspace
        .begin_outer_frame(EguiFrameScheduleKey::new(3, 0))
        .expect("settlement frame begins");
    settled
        .run_surface(SURFACE, &context, raw_input(), &mut panes)
        .expect("the settlement host owns the surface run");
    let (host, pending) = settled
        .finish()
        .expect("settlement frame commits")
        .into_parts();
    assert_eq!(host.presentation_summary().retired_presented_eligible(), 1);
    pending
        .into_iter()
        .next()
        .expect("the next output has a settlement obligation")
        .settle_with(|_, mut full_output| {
            full_output.textures_delta.clear();
            EguiPresentationResult::Dropped
        });
}
