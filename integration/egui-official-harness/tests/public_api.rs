use dockspace::model::{
    DockspaceAxis, DockspaceLayout, DockspaceNode, DockspaceRootLayout, DockspaceSurfaceLayout,
    ItemId, RootId, SurfaceId,
};
use dockspace::policy::DockPolicy;
use egui::{Context, RawInput, Rect, Ui, vec2};
use egui_dockspace::{Dockspace, DockspaceActionOutcome, DockspaceActionStatus, PaneView};

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
fn official_egui_consumes_the_default_product_facade() {
    let _native_options = eframe::NativeOptions::default();
    let context = Context::default();
    let mut policy = DockPolicy::default();
    policy.set_allow_resize_axis(DockspaceAxis::Vertical, false);
    let mut dockspace = Dockspace::builder("official-egui-consumer", product_layout())
        .policy(policy)
        .build()
        .expect("the public facade accepts a valid product layout");
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

    let mut panes = SmokePanes::default();
    let mut local_actions_current = false;
    for _ in 0..4 {
        let mut output = context.run_ui(raw_input(), |ui| {
            let response = dockspace
                .show_single_surface(SURFACE, ui, &mut panes)
                .expect("the public facade advances an official-egui frame");
            assert!(response.missing_panes().is_empty());
            local_actions_current |= response.interaction_capabilities().local_actions_current();
        });
        output.textures_delta.clear();
    }

    assert!(panes.paint_count > 0, "the product pane must paint");
    assert!(
        local_actions_current,
        "the product facade must become interactive after bootstrap"
    );
}
