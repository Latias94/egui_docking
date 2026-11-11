//! Visible tabs retain accessible geometry when policy disables interaction.

use dockspace::policy::{DockPolicy, TabBarInteraction, TabBarPolicy, TabBarVisibility};
use egui::accesskit::{Action, ActionRequest, NodeId, Role, TreeId};
use egui::{Context, Event, FullOutput, Modifiers, PointerButton, Pos2, RawInput, Rect, Shape, Ui};
use egui_dockspace::{
    Dockspace, DockspaceLayout, DockspaceNode, DockspaceRootLayout, DockspaceSurfaceLayout, ItemId,
    PaneResponse, PaneView, RootId, SurfaceId, SurfacePresentationResult,
};

const SURFACE: SurfaceId = SurfaceId::new(1);
const FIRST: ItemId = ItemId::new(1);
const SECOND: ItemId = ItemId::new(2);

struct Panes;

impl PaneView for Panes {
    fn title(&self, item: ItemId) -> Option<egui::WidgetText> {
        Some(if item == FIRST { "First" } else { "Second" }.into())
    }

    fn ui(&mut self, _item: ItemId, _ui: &mut Ui) -> PaneResponse {
        PaneResponse::default()
    }
}

fn dockspace() -> Dockspace {
    let layout = DockspaceLayout::new([DockspaceSurfaceLayout::new(
        SURFACE,
        DockspaceRootLayout::new(RootId::new(1), DockspaceNode::tabs([FIRST, SECOND])),
    )])
    .unwrap();
    let mut policy = DockPolicy::default();
    policy.set_tab_bar(TabBarPolicy::new(
        TabBarVisibility::Visible,
        TabBarInteraction::Disabled,
    ));
    Dockspace::builder("policy-disabled-tab-accessibility", layout)
        .policy(policy)
        .build()
        .unwrap()
}

fn run_frame(
    context: &Context,
    dockspace: &mut Dockspace,
    terminal_owned: bool,
    events: Vec<Event>,
) -> FullOutput {
    let input = RawInput {
        screen_rect: Some(Rect::from_min_size(Pos2::ZERO, egui::vec2(800.0, 480.0))),
        time: Some(context.input(|input| input.time) + 1.0),
        events,
        ..RawInput::default()
    };
    let mut show = |ui: &mut Ui| {
        let response = dockspace
            .show_single_surface(SURFACE, ui, &mut Panes)
            .unwrap();
        assert!(response.manipulation_feedback().is_empty());
    };
    if terminal_owned {
        let (output, receipt) = egui_dockspace::run_ui(context, input, show).into_parts();
        receipt.report(SurfacePresentationResult::Presented);
        output
    } else {
        context.run_ui(input, &mut show)
    }
}

fn disabled_tab(output: &FullOutput, title: &str) -> (NodeId, egui::accesskit::Rect) {
    let (id, node) = output
        .platform_output
        .accesskit_update
        .as_ref()
        .unwrap()
        .nodes
        .iter()
        .find(|(_, node)| node.role() == Role::Tab && node.label() == Some(title))
        .expect("a policy-disabled tab remains in the accessibility tree");
    assert!(node.is_disabled());
    assert!(!node.supports_action(Action::Click));
    assert!(!node.supports_action(Action::Focus));
    let bounds = node.bounds().unwrap();
    assert!(bounds.x0.is_finite() && bounds.y0.is_finite());
    assert!(bounds.x1 > bounds.x0 && bounds.y1 > bounds.y0);
    assert!(bounds.x0 >= 0.0 && bounds.y0 >= 0.0);
    assert!(bounds.x1 <= 800.0 && bounds.y1 <= 480.0);
    (*id, bounds)
}

fn title_occurrences(shape: &Shape, title: &str) -> usize {
    match shape {
        Shape::Text(text) => usize::from(text.galley.job.text == title),
        Shape::Vec(shapes) => shapes
            .iter()
            .map(|shape| title_occurrences(shape, title))
            .sum(),
        _ => 0,
    }
}

#[test]
#[allow(
    clippy::cast_possible_truncation,
    reason = "the checked accessible rectangle lies inside the 800 by 480 egui viewport"
)]
fn policy_disabled_tabs_keep_visible_accessibility_and_hover_without_activation() {
    for terminal_owned in [false, true] {
        let context = Context::default();
        context.enable_accesskit();
        let mut dockspace = dockspace();
        for _ in 0..3 {
            run_frame(&context, &mut dockspace, terminal_owned, Vec::new())
                .drop_without_applying_deltas();
        }
        let output = run_frame(&context, &mut dockspace, terminal_owned, Vec::new());
        let (_, first_bounds) = disabled_tab(&output, "First");
        let (second, bounds) = disabled_tab(&output, "Second");
        let point = Pos2::new(
            ((bounds.x0 + bounds.x1) * 0.5) as f32,
            ((bounds.y0 + bounds.y1) * 0.5) as f32,
        );
        output.drop_without_applying_deltas();

        run_frame(
            &context,
            &mut dockspace,
            terminal_owned,
            vec![Event::PointerMoved(point)],
        )
        .drop_without_applying_deltas();
        let mut tooltip_visible = false;
        for _ in 0..4 {
            // Even a same-position move restarts egui's tooltip delay.
            let output = run_frame(&context, &mut dockspace, terminal_owned, Vec::new());
            tooltip_visible |= output
                .shapes
                .iter()
                .map(|clipped| title_occurrences(&clipped.shape, "Second"))
                .sum::<usize>()
                > 1;
            output.drop_without_applying_deltas();
        }
        assert!(
            context.input(|input| input.pointer.time_since_last_movement())
                >= context.global_style().interaction.tooltip_delay
        );
        assert!(
            tooltip_visible,
            "the visible tab retains its hover tooltip, terminal_owned={terminal_owned}"
        );

        for pressed in [true, false] {
            let output = run_frame(
                &context,
                &mut dockspace,
                terminal_owned,
                vec![
                    Event::PointerMoved(point),
                    Event::PointerButton {
                        pos: point,
                        button: PointerButton::Primary,
                        pressed,
                        modifiers: Modifiers::NONE,
                    },
                ],
            );
            assert_eq!(disabled_tab(&output, "Second").1, bounds);
            output.drop_without_applying_deltas();
        }

        let destination = Pos2::new(
            ((first_bounds.x0 + first_bounds.x1) * 0.5) as f32,
            ((first_bounds.y0 + first_bounds.y1) * 0.5) as f32,
        );
        assert!(point.distance(destination) > 32.0);
        let version = dockspace.version();
        for events in [
            vec![
                Event::PointerMoved(point),
                Event::PointerButton {
                    pos: point,
                    button: PointerButton::Primary,
                    pressed: true,
                    modifiers: Modifiers::NONE,
                },
            ],
            vec![Event::PointerMoved(destination)],
            vec![Event::PointerButton {
                pos: destination,
                button: PointerButton::Primary,
                pressed: false,
                modifiers: Modifiers::NONE,
            }],
        ] {
            run_frame(&context, &mut dockspace, terminal_owned, events)
                .drop_without_applying_deltas();
            assert!(context.dragged_id().is_none());
            assert_eq!(dockspace.version(), version);
            let surface = dockspace.view().surface(SURFACE).unwrap();
            assert_eq!(surface.contained_count(), 0);
            let tabs = surface
                .main_root()
                .unwrap()
                .content()
                .unwrap()
                .tabs()
                .unwrap();
            assert_eq!(tabs.items(), [FIRST, SECOND]);
            assert_eq!(tabs.selected(), Some(FIRST));
        }

        let output = run_frame(
            &context,
            &mut dockspace,
            terminal_owned,
            [Action::Click, Action::Focus]
                .map(|action| {
                    Event::AccessKitActionRequest(ActionRequest {
                        action,
                        target_tree: TreeId::ROOT,
                        target_node: second,
                        data: None,
                    })
                })
                .into(),
        );
        disabled_tab(&output, "Second");
        output.drop_without_applying_deltas();
        run_frame(&context, &mut dockspace, terminal_owned, Vec::new())
            .drop_without_applying_deltas();
        assert!(dockspace.view().item(FIRST).unwrap().is_selected());
        assert!(!dockspace.view().item(SECOND).unwrap().is_selected());
        assert_ne!(
            context
                .memory(egui::Memory::focused)
                .map(|id| id.accesskit_id()),
            Some(second)
        );
    }
}
