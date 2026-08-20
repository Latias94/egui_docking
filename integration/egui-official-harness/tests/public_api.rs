use dockspace::policy::DockPolicy;
use egui::accesskit::{Action, ActionRequest, Role, TreeId};
use egui::{Context, RawInput, Rect, Ui, vec2};
use egui_dockspace::{
    Dockspace, DockspaceActionOutcome, DockspaceActionStatus, DockspaceAxis, DockspaceCapability,
    DockspaceLayout, DockspaceNode, DockspaceRootLayout, DockspaceSurfaceLayout,
    DockspaceUnavailableReason, ItemId, PaneView, RootId, SurfaceId,
};

const SURFACE: SurfaceId = SurfaceId::new(1);
const ROOT: RootId = RootId::new(1);
const ITEM: ItemId = ItemId::new(1);
const SECOND_ITEM: ItemId = ItemId::new(2);

#[derive(Default)]
struct SmokePanes {
    paint_count: usize,
}

impl PaneView for SmokePanes {
    fn title(&self, item: ItemId) -> Option<egui::WidgetText> {
        match item {
            ITEM => Some("Official egui pane".into()),
            SECOND_ITEM => Some("Second official pane".into()),
            _ => None,
        }
    }

    fn ui(&mut self, item: ItemId, ui: &mut Ui) {
        if matches!(item, ITEM | SECOND_ITEM) {
            self.paint_count += 1;
            let target = pane_focus_target(item);
            let rect = Rect::from_min_size(ui.available_rect_before_wrap().min, vec2(160.0, 24.0));
            let _ = ui.interact(rect, target, egui::Sense::click_and_drag());
            ui.label("Rendered through the public egui_dockspace facade");
        }
    }

    fn focus_target(&self, item: ItemId) -> Option<egui::Id> {
        matches!(item, ITEM | SECOND_ITEM).then(|| pane_focus_target(item))
    }
}

fn pane_focus_target(item: ItemId) -> egui::Id {
    egui::Id::new(("official-egui-pane-focus", item.get()))
}

fn product_layout() -> DockspaceLayout {
    DockspaceLayout::new([DockspaceSurfaceLayout::new(
        SURFACE,
        DockspaceRootLayout::new(ROOT, DockspaceNode::central_tabs([ITEM, SECOND_ITEM])),
    )])
    .expect("the smoke layout is valid")
}

fn raw_input() -> RawInput {
    RawInput {
        screen_rect: Some(Rect::from_min_size(egui::Pos2::ZERO, vec2(800.0, 600.0))),
        ..RawInput::default()
    }
}

fn accesskit_activate(target_node: egui::accesskit::NodeId) -> egui::Event {
    egui::Event::AccessKitActionRequest(ActionRequest {
        action: Action::Click,
        target_tree: TreeId::ROOT,
        target_node,
        data: None,
    })
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
        let input = raw_input();
        let mut output = context.run_ui(input, |ui| {
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

#[test]
fn official_eframe_app_settles_without_raw_input_or_full_output() {
    struct OfficialApp {
        dockspace: Dockspace,
        panes: SmokePanes,
        local_actions_current: bool,
        pane_focus_capability: Option<DockspaceCapability>,
    }

    impl eframe::App for OfficialApp {
        fn ui(&mut self, ui: &mut Ui, _frame: &mut eframe::Frame) {
            let response = self
                .dockspace
                .show_single_surface(SURFACE, ui, &mut self.panes)
                .expect("the eframe application advances its Dockspace frame");
            self.local_actions_current |=
                response.interaction_capabilities().local_actions_current();
            self.pane_focus_capability = Some(response.pane_focus_capability());
        }
    }

    let context = Context::default();
    context.enable_accesskit();
    let mut app = OfficialApp {
        dockspace: Dockspace::builder("official-eframe-app", product_layout())
            .build()
            .expect("the official eframe application initializes"),
        panes: SmokePanes::default(),
        local_actions_current: false,
        pane_focus_capability: None,
    };
    let mut frame = eframe::Frame::_new_kittest();

    let mut stable_second_tab = None;
    for _ in 0..4 {
        let mut output = context.run_ui(raw_input(), |ui| {
            eframe::App::ui(&mut app, ui, &mut frame);
        });
        if let Some(node) = output
            .platform_output
            .accesskit_update
            .as_ref()
            .and_then(|update| {
                update.nodes.iter().find_map(|(id, node)| {
                    (node.role() == Role::Tab && node.label() == Some("Second official pane"))
                        .then_some(*id)
                })
            })
        {
            stable_second_tab = Some(node);
        }
        output.textures_delta.clear();
    }
    let second_tab = stable_second_tab.expect("the stable eframe output exposes the second tab");
    let mut selection_input = raw_input();
    selection_input.events.push(accesskit_activate(second_tab));
    let mut output = context.run_ui(selection_input, |ui| {
        eframe::App::ui(&mut app, ui, &mut frame);
    });
    output.textures_delta.clear();
    for _ in 0..3 {
        let mut output = context.run_ui(raw_input(), |ui| {
            eframe::App::ui(&mut app, ui, &mut frame);
        });
        output.textures_delta.clear();
    }

    assert!(app.panes.paint_count > 0, "the eframe pane must paint");
    assert!(
        app.local_actions_current,
        "an ordinary eframe App keeps current-pass local actions"
    );
    assert_ne!(
        app.pane_focus_capability,
        Some(DockspaceCapability::Unavailable(
            DockspaceUnavailableReason::PresentationSettlementRequired,
        )),
        "the App callback needs no RawInput or FullOutput settlement token"
    );
    assert!(
        app.dockspace
            .view()
            .item(SECOND_ITEM)
            .is_some_and(|item| item.is_selected()),
        "the AccessKit action must select the second tab through App::ui",
    );
    assert!(
        context.memory(|memory| memory.has_focus(pane_focus_target(SECOND_ITEM))),
        "the next successful App::ui frames must settle the exact pane-focus request",
    );
}
