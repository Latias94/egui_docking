//! An auto-hidden bar remains reachable above ordinary editable pane content.

use dockspace::policy::{DockPolicy, TabBarInteraction, TabBarPolicy, TabBarVisibility};
use egui::accesskit::{Node, Role};
use egui::{Context, Event, Modifiers, PointerButton, Pos2, RawInput, Rect, Ui};
use egui_dockspace::{
    CloseDecision, DockPlacement, Dockspace, DockspaceCloseRequestStatus, DockspaceLayout,
    DockspaceNode, DockspaceRootLayout, DockspaceSurfaceLayout, ItemId, PaneResponse, PaneView,
    RootId, SurfaceId, SurfacePresentationResult,
};

const SURFACE: SurfaceId = SurfaceId::new(1);
const FIRST: ItemId = ItemId::new(1);
const SECOND: ItemId = ItemId::new(2);

#[derive(Default)]
struct Editors(String);

impl PaneView for Editors {
    fn title(&self, item: ItemId) -> Option<egui::WidgetText> {
        Some(if item == FIRST { "First" } else { "Second" }.into())
    }

    fn ui(&mut self, _: ItemId, ui: &mut Ui) -> PaneResponse {
        PaneResponse::from(
            &ui.add_sized(ui.available_size(), egui::TextEdit::multiline(&mut self.0)),
        )
    }
}

struct Harness {
    context: Context,
    dock: Dockspace,
    panes: Editors,
    owned: bool,
    time: f64,
}

impl Harness {
    fn new(owned: bool) -> Self {
        let context = Context::default();
        context.enable_accesskit();
        context.options_mut(|options| options.max_passes = 1.try_into().unwrap());
        let mut policy = DockPolicy::default();
        policy.set_tab_bar(TabBarPolicy::new(
            TabBarVisibility::AutoHideSingle,
            TabBarInteraction::Enabled,
        ));
        let layout = DockspaceLayout::new([DockspaceSurfaceLayout::new(
            SURFACE,
            DockspaceRootLayout::new(RootId::new(1), DockspaceNode::central_tabs([FIRST])),
        )])
        .unwrap();
        Self {
            context,
            dock: Dockspace::builder(("reachable-auto-hidden-tabs", owned), layout)
                .policy(policy)
                .build()
                .unwrap(),
            panes: Editors::default(),
            owned,
            time: 0.0,
        }
    }

    fn frame(&mut self, events: Vec<Event>) -> Vec<Node> {
        self.time += 0.1;
        let input = RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, egui::vec2(640.0, 480.0))),
            focused: true,
            time: Some(self.time),
            events,
            ..RawInput::default()
        };
        let output = if self.owned {
            self.dock
                .run_single_surface(&self.context, input, SURFACE, &mut self.panes)
                .unwrap()
                .0
        } else {
            egui_dockspace::run_ui(&self.context, input, |ui| {
                self.dock
                    .show_single_surface(SURFACE, ui, &mut self.panes)
                    .unwrap();
            })
        };
        let (mut output, receipt) = output.into_parts();
        output.textures_delta.clear();
        let nodes = output
            .platform_output
            .accesskit_update
            .as_ref()
            .unwrap()
            .nodes
            .iter()
            .map(|(_, node)| node.clone())
            .collect();
        output.drop_without_applying_deltas();
        receipt.report(SurfacePresentationResult::Presented);
        nodes
    }

    fn settle(&mut self) -> Vec<Node> {
        let mut nodes = Vec::new();
        for _ in 0..4 {
            nodes = self.frame(Vec::new());
        }
        nodes
    }
}

#[test]
#[allow(
    clippy::cast_possible_truncation,
    reason = "accessible bounds fit the finite test viewport"
)]
fn a_single_tab_can_be_revealed_and_roster_changes_restore_automatic_visibility() {
    for owned in [true, false] {
        let mut harness = Harness::new(owned);
        let nodes = harness.settle();
        assert!(!nodes.iter().any(|node| node.role() == Role::Tab));
        let button = nodes
            .iter()
            .find(|node| node.role() == Role::Button && node.label() == Some("Show the tab bar"))
            .expect("the hidden bar has a real accessible recovery button");
        let bounds = button.bounds().unwrap();
        let position = Pos2::new(
            ((bounds.x0 + bounds.x1) * 0.5) as f32,
            ((bounds.y0 + bounds.y1) * 0.5) as f32,
        );
        harness.frame(vec![
            Event::PointerMoved(position),
            Event::PointerButton {
                pos: position,
                button: PointerButton::Primary,
                pressed: true,
                modifiers: Modifiers::NONE,
            },
            Event::PointerButton {
                pos: position,
                button: PointerButton::Primary,
                pressed: false,
                modifiers: Modifiers::NONE,
            },
        ]);
        for _ in 0..2 {
            let nodes = harness.settle();
            assert_eq!(
                nodes.iter().filter(|node| node.role() == Role::Tab).count(),
                1
            );
            assert!(
                !nodes
                    .iter()
                    .any(|node| node.label() == Some("Show the tab bar"))
            );
        }

        harness
            .dock
            .open_item_current(SECOND, DockPlacement::After(FIRST))
            .unwrap();
        let nodes = harness.settle();
        assert_eq!(
            nodes.iter().filter(|node| node.role() == Role::Tab).count(),
            2
        );
        let requested = harness.dock.request_close_item_current(SECOND).unwrap();
        let DockspaceCloseRequestStatus::Requested(request) = requested.status() else {
            panic!("closing the extra tab opens a real close plan");
        };
        let plan = request.plan().clone();
        harness
            .dock
            .resolve_close(
                plan.request(),
                plan.items()[0].token(),
                CloseDecision::Allow,
            )
            .unwrap();
        let nodes = harness.settle();
        assert!(!nodes.iter().any(|node| node.role() == Role::Tab));
        assert!(
            nodes
                .iter()
                .any(|node| node.label() == Some("Show the tab bar"))
        );
        assert!(harness.dock.view().item(FIRST).unwrap().is_selected());
    }
}
