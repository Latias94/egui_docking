use std::time::Duration;

use dockspace::backend::graph::{Node, RootRecord, SurfacePresentation, Workspace};
use dockspace::backend::ids::{ItemId, RootId, SurfaceId};
use egui::accesskit::{Action, Role};
use egui::{Context, RawInput, Rect, Ui, ViewportId, vec2};
use egui_dockspace::backend::{EguiFrameScheduleKey, EguiRendererOutputDisposition};
use egui_dockspace::{
    Dockspace, DockspaceActionOutcome, DockspaceActionStatus, DockspaceLayout, DockspaceNode,
    DockspaceRootLayout, DockspaceSurfaceLayout, PaneView,
};

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

fn product_layout() -> DockspaceLayout {
    DockspaceLayout::new([DockspaceSurfaceLayout::new(
        SURFACE,
        DockspaceRootLayout::new(ROOT, DockspaceNode::central_tabs([ITEM])),
    )])
    .expect("the smoke layout is valid")
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
    let mut dockspace = Dockspace::builder("official-egui-consumer", product_layout())
        .build()
        .expect("the public facade accepts a valid workspace");
    let view = dockspace.view();
    assert_eq!(
        view.surface(SURFACE)
            .and_then(|surface| surface.main_root())
            .map(|root| root.id()),
        Some(ROOT),
    );
    let prepared = dockspace.prepare_select_item(ITEM);
    assert_eq!(prepared.expected_version(), dockspace.version());
    let action = dockspace
        .submit_prepared_action(prepared)
        .expect("the product action boundary reduces one checked action");
    assert!(matches!(
        action.status(),
        DockspaceActionStatus::Applied(DockspaceActionOutcome::Selected {
            item: ITEM,
            changed: false,
        })
    ));
    assert!(!action.mutation().workspace_changed());
    let mut panes = SmokePanes::default();
    let mut last_repaint_delay = Duration::ZERO;
    let mut last_tree = None;
    let mut saw_local_actions_pass = false;

    for _ in 0..4 {
        let mut output = context.run_ui(raw_input(), |ui| {
            let response = dockspace
                .show_single_surface(SURFACE, ui, &mut panes)
                .expect("the public facade advances an official-egui frame");
            let capabilities = response.interaction_capabilities();
            saw_local_actions_pass |= capabilities.local_actions_current();
            assert!(!capabilities.retained_presentation_current());
            assert!(!capabilities.pointer_receivers_current());
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
        saw_local_actions_pass,
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
    let mut dockspace = Dockspace::backend_builder("official-egui-outer-host", workspace())
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
            .all(|output| !output.has_renderer_admission_obligation()),
        "bootstrap paint must not fabricate a renderer admission obligation",
    );
    outputs.submit_with(|_, _, _| EguiRendererOutputDisposition::RejectedUnconsumed);

    let mut ready = dockspace
        .begin_outer_frame(EguiFrameScheduleKey::new(2, 0))
        .expect("ready frame begins");
    ready
        .run_surface(SURFACE, &context, raw_input(), &mut panes)
        .expect("the ready host owns the surface run");
    let (_, pending) = ready.finish().expect("ready frame commits").into_parts();
    assert_eq!(pending.len(), 1, "ready paint has one renderer output");
    assert!(
        pending
            .iter()
            .all(|output| output.has_renderer_admission_obligation()),
        "ready paint has a renderer admission obligation",
    );
    pending.submit_with(|_, _, _| EguiRendererOutputDisposition::Accepted);

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
    assert_eq!(pending.len(), 1, "the next frame has one renderer output");
    assert!(
        pending
            .iter()
            .all(|output| output.has_renderer_admission_obligation()),
        "the next output has a renderer admission obligation",
    );
    pending.submit_with(|_, _, _| EguiRendererOutputDisposition::RejectedUnconsumed);
}
