//! Stationary overflow scrolling through actual official-egui input and output.

use super::*;
use crate::DockStyle;
use dockspace::runtime::{
    SurfacePaintProgramOperation as Operation, SurfacePaintProgramStep as Step,
};

const TARGET: ItemId = ItemId::new(10);

struct OverflowPanes;

impl PaneView for OverflowPanes {
    fn title(&self, item: ItemId) -> Option<egui::WidgetText> {
        Some(if item == FIRST {
            "Source".into()
        } else {
            format!("Target {}", item.get()).into()
        })
    }

    fn ui(&mut self, item: ItemId, ui: &mut Ui) {
        ui.label(format!("Content {}", item.get()));
    }
}

fn frame(
    context: &Context,
    dockspace: &mut Dockspace,
    time: f64,
    events: Vec<Event>,
) -> egui::FullOutput {
    let output = run(context, dockspace, time, events);
    let (mut output, receipt) = output.into_parts();
    receipt.report(crate::SurfacePresentationResult::Presented);
    output.textures_delta.clear();
    output
}

fn run(
    context: &Context,
    dockspace: &mut Dockspace,
    time: f64,
    events: Vec<Event>,
) -> crate::DockspaceOutput {
    let mut raw = input(events);
    raw.time = Some(time);
    crate::run_ui(context, raw, |ui| {
        let response = dockspace
            .show_single_surface(SURFACE, ui, &mut OverflowPanes)
            .unwrap();
        assert!(response.close_requests().next().is_none());
    })
}

#[allow(
    clippy::cast_possible_truncation,
    reason = "the core viewport is bounded by the finite egui test surface"
)]
fn overflow(dockspace: &mut Dockspace) -> (Rect, f64, f64) {
    let mut frame = dockspace.session.begin_host_frame().unwrap();
    let plan = frame.paint_plan(SURFACE).unwrap().unwrap();
    let mut result = None;
    let mut controls = Vec::new();
    plan.visit_surface_program(|step| {
        if let Step::Operation {
            operation: Operation::TabBar(bar),
            ..
        } = step
            && bar.members().any(|member| member.item() == TARGET)
        {
            let rect = bar.viewport();
            result = Some((
                bar.visual_id(),
                Rect::from_min_max(
                    Pos2::new(rect.x() as f32, rect.y() as f32),
                    Pos2::new(rect.max().x() as f32, rect.max().y() as f32),
                ),
                bar.scroll_offset(),
                bar.maximum_scroll_offset(),
            ));
        }
        if let Step::Operation {
            operation: Operation::TabStripControl(control),
            ..
        } = step
        {
            controls.push((control.tab_bar_visual_id(), control.hit_bounds()));
        }
        Ok::<(), ()>(())
    })
    .unwrap();
    let (bar, mut viewport, offset, maximum) = result.unwrap();
    let middle = viewport.center().x;
    for (_, hit) in controls.into_iter().filter(|(owner, _)| *owner == bar) {
        if hit.max().x() <= f64::from(viewport.min.x) || hit.x() >= f64::from(viewport.max.x) {
            continue;
        }
        if hit.x() + hit.width() * 0.5 < f64::from(middle) {
            viewport.min.x = viewport.min.x.max(hit.max().x() as f32);
        } else {
            viewport.max.x = viewport.max.x.min(hit.x() as f32);
        }
    }
    (viewport, offset, maximum)
}

fn setup() -> (Context, Dockspace, Pos2) {
    setup_with_profile(24.0, 160.0)
}

fn setup_with_profile(edge_band: f32, tab_width: f32) -> (Context, Dockspace, Pos2) {
    let context = Context::default();
    context.enable_accesskit();
    context.options_mut(|options| options.max_passes = 1.try_into().unwrap());
    let layout = DockspaceLayout::new([DockspaceSurfaceLayout::new(
        SURFACE,
        DockspaceRootLayout::new(
            ROOT,
            DockspaceNode::equal_split(
                DockspaceAxis::Horizontal,
                [
                    DockspaceNode::tabs([FIRST]),
                    DockspaceNode::tabs((10..22).map(ItemId::new)),
                ],
            )
            .unwrap(),
        ),
    )])
    .unwrap();
    let style = DockStyle {
        tab_min_width: tab_width,
        tab_max_width: tab_width,
        tab_strip_edge_scroll_band: edge_band,
        ..DockStyle::default()
    };
    let mut dockspace = Dockspace::builder("stationary-overflow-product", layout)
        .style(style)
        .build()
        .unwrap();
    let mut source = None;
    for index in 0..8 {
        let output = frame(
            &context,
            &mut dockspace,
            f64::from(index) * 0.05,
            Vec::new(),
        );
        source = try_tab_rect(&output, "Source")
            .map(|rect| rect.center())
            .or(source);
    }
    let source = source.unwrap();
    (context, dockspace, source)
}

#[test]
fn stationary_drag_edge_scroll_repeats_after_presented_output_without_selecting_or_focusing() {
    let (context, mut dockspace, source) = setup();
    let (viewport, initial, maximum) = overflow(&mut dockspace);
    assert!(maximum > 80.0);
    let edge = Pos2::new(viewport.max.x - 2.0, viewport.max.y - 2.0);
    let before = dockspace.version();
    frame(
        &context,
        &mut dockspace,
        1.0,
        vec![Event::PointerMoved(source), pointer_button(source, true)],
    );
    let focus_before = context.memory(|memory| memory.focused());
    frame(
        &context,
        &mut dockspace,
        1.01,
        vec![Event::PointerMoved(edge)],
    );
    let wake = dockspace
        .session
        .pointer_continuation()
        .expect("the overflow edge schedules one finite action");
    assert_eq!(wake.deadline(), std::time::Duration::from_millis(1060));
    frame(&context, &mut dockspace, 1.059, Vec::new());
    assert_eq!(overflow(&mut dockspace).1, initial);
    frame(&context, &mut dockspace, 1.06, Vec::new());
    assert_eq!(overflow(&mut dockspace).1, initial + 40.0);
    assert!(dockspace.session.has_active_gesture());
    assert_eq!(dockspace.version(), before);
    assert!(dockspace.view().item(TARGET).unwrap().is_selected());
    assert_eq!(context.memory(|memory| memory.focused()), focus_before);
    // The next product run accepts the exact newly painted scroll geometry.
    frame(&context, &mut dockspace, 1.08, Vec::new());
    let second = dockspace.session.pointer_continuation().unwrap();
    assert_eq!(second.deadline(), std::time::Duration::from_millis(1110));
    assert_ne!(second.token(), wake.token());
    frame(&context, &mut dockspace, 1.11, Vec::new());
    assert_eq!(overflow(&mut dockspace).1, initial + 80.0);
    assert_eq!(dockspace.version(), before);
    assert!(dockspace.view().item(TARGET).unwrap().is_selected());
    assert_eq!(context.memory(|memory| memory.focused()), focus_before);
    frame(
        &context,
        &mut dockspace,
        1.12,
        vec![Event::Key {
            key: egui::Key::Escape,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        }],
    );
    assert!(!dockspace.session.has_active_gesture());
    assert!(dockspace.session.pointer_continuation().is_none());
    frame(
        &context,
        &mut dockspace,
        1.2,
        vec![pointer_button(edge, false)],
    );
    assert_eq!(dockspace.version(), before);
}

#[test]
fn renderer_result_wakes_the_idle_host_to_accept_the_next_edge_scroll_output() {
    let (context, mut dockspace, source) = setup();
    let (viewport, _, _) = overflow(&mut dockspace);
    let edge = Pos2::new(viewport.max.x - 2.0, viewport.max.y - 2.0);
    frame(
        &context,
        &mut dockspace,
        1.0,
        vec![Event::PointerMoved(source), pointer_button(source, true)],
    );
    frame(
        &context,
        &mut dockspace,
        1.01,
        vec![Event::PointerMoved(edge)],
    );
    frame(&context, &mut dockspace, 1.02, Vec::new());
    frame(&context, &mut dockspace, 1.03, Vec::new());
    assert!(
        dockspace.session.pointer_continuation().is_some(),
        "the initial edge challenge must survive equivalent idle repaints"
    );
    let (output, receipt) = run(&context, &mut dockspace, 1.06, Vec::new()).into_parts();
    output.drop_without_applying_deltas();
    assert_eq!(overflow(&mut dockspace).1, 40.0);
    assert!(dockspace.session.pointer_continuation().is_none());
    let mut pending = Vec::new();
    let mut idle = false;
    for index in 0..16 {
        let (output, receipt) = run(
            &context,
            &mut dockspace,
            1.1 + f64::from(index) * 0.05,
            Vec::new(),
        )
        .into_parts();
        idle = output.viewport_output[&egui::ViewportId::ROOT].repaint_delay
            == std::time::Duration::MAX;
        output.drop_without_applying_deltas();
        pending.push(receipt);
        if idle {
            break;
        }
    }
    assert!(
        idle,
        "an unpresented stationary action must not spin the host"
    );
    let wakes = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let observed = wakes.clone();
    context.set_request_repaint_callback(move |_| {
        observed.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    });
    receipt.report(crate::SurfacePresentationResult::Presented);
    assert!(
        wakes.load(std::sync::atomic::Ordering::Relaxed) > 0,
        "the exact renderer result must wake the otherwise idle host"
    );
    // Settle the produced same-scene emissions in their existing order.
    for receipt in pending {
        receipt.report(crate::SurfacePresentationResult::Presented);
    }
    frame(&context, &mut dockspace, 2.0, Vec::new());
    assert_eq!(overflow(&mut dockspace).1, 80.0);
}

fn semantic_action(output: &egui::FullOutput, role: Role, label: &str) -> Event {
    let (target_node, node) = output
        .platform_output
        .accesskit_update
        .as_ref()
        .unwrap()
        .nodes
        .iter()
        .find(|(_, node)| node.role() == role && node.label() == Some(label))
        .unwrap_or_else(|| panic!("current control {label:?} must be emitted"));
    assert!(node.supports_action(egui::accesskit::Action::Click));
    Event::AccessKitActionRequest(egui::accesskit::ActionRequest {
        action: egui::accesskit::Action::Click,
        target_tree: egui::accesskit::TreeId::ROOT,
        target_node: *target_node,
        data: None,
    })
}

#[test]
fn edge_scroll_yields_to_explicit_scroll_then_menu_selection_and_reveal() {
    let (context, mut dockspace, source) = setup();
    let (viewport, _, _) = overflow(&mut dockspace);
    let edge = Pos2::new(viewport.max.x - 2.0, viewport.max.y - 2.0);
    let original = dockspace.version();
    frame(
        &context,
        &mut dockspace,
        1.0,
        vec![Event::PointerMoved(source), pointer_button(source, true)],
    );
    frame(
        &context,
        &mut dockspace,
        1.01,
        vec![Event::PointerMoved(edge)],
    );
    let wake = dockspace.session.pointer_continuation().unwrap();
    assert_eq!(wake.deadline(), std::time::Duration::from_millis(1060));
    frame(&context, &mut dockspace, 1.06, Vec::new());
    assert_eq!(overflow(&mut dockspace).1, 40.0);
    let output = frame(&context, &mut dockspace, 1.08, Vec::new());
    assert!(dockspace.session.pointer_continuation().is_some());
    let manual = semantic_action(&output, Role::Button, "Scroll tabs forward");
    frame(&context, &mut dockspace, 1.09, vec![manual]);
    assert!(dockspace.session.pointer_continuation().is_none());
    let output = frame(&context, &mut dockspace, 1.1, Vec::new());
    let offset = overflow(&mut dockspace).1;
    assert!(offset > 40.0, "manual scroll uses its own aligned offset");
    assert_eq!(dockspace.version(), original);
    assert!(dockspace.view().item(TARGET).unwrap().is_selected());
    assert!(dockspace.session.has_active_gesture());
    frame(&context, &mut dockspace, 1.2, Vec::new());
    assert_eq!(overflow(&mut dockspace).1, offset);
    assert!(dockspace.session.pointer_continuation().is_none());
    let open = semantic_action(&output, Role::Button, "Show hidden tabs");
    frame(&context, &mut dockspace, 1.3, vec![open]);
    let mut output = frame(&context, &mut dockspace, 1.31, Vec::new());
    for index in 0..4 {
        if output
            .platform_output
            .accesskit_update
            .as_ref()
            .unwrap()
            .nodes
            .iter()
            .any(|(_, node)| node.role() == Role::MenuItem && node.label() == Some("Target 20"))
        {
            break;
        }
        output = frame(
            &context,
            &mut dockspace,
            1.32 + f64::from(index) * 0.01,
            Vec::new(),
        );
    }
    assert!(dockspace.session.pointer_continuation().is_none());
    let select = semantic_action(&output, Role::MenuItem, "Target 20");
    frame(&context, &mut dockspace, 1.4, vec![select]);
    let mut visible = false;
    for index in 0..6 {
        let output = frame(
            &context,
            &mut dockspace,
            1.5 + f64::from(index) * 0.1,
            Vec::new(),
        );
        visible = output
            .platform_output
            .accesskit_update
            .as_ref()
            .unwrap()
            .nodes
            .iter()
            .any(|(_, node)| {
                node.role() == Role::Button && node.label() == Some("Close Target 20")
            });
        if visible {
            break;
        }
    }
    assert!(
        dockspace
            .view()
            .item(ItemId::new(20))
            .unwrap()
            .is_selected()
    );
    assert!(
        visible,
        "menu selection owns reveal and restores an operable close control"
    );
    assert!(!dockspace.session.has_active_gesture());
    assert!(dockspace.session.pointer_continuation().is_none());
    let selected = dockspace.version();
    let final_offset = overflow(&mut dockspace).1;
    frame(
        &context,
        &mut dockspace,
        2.2,
        vec![pointer_button(edge, false)],
    );
    assert_eq!(dockspace.version(), selected);
    assert_eq!(overflow(&mut dockspace).1, final_offset);
}

#[test]
fn edge_scroll_does_not_cross_a_visible_close_control() {
    let (context, mut dockspace, source) = setup_with_profile(1_000.0, 200.0);
    let output = frame(&context, &mut dockspace, 1.0, Vec::new());
    let close = output
        .platform_output
        .accesskit_update
        .as_ref()
        .unwrap()
        .nodes
        .iter()
        .find_map(|(_, node)| {
            (node.role() == Role::Button && node.label() == Some("Close Target 10"))
                .then(|| node.bounds())
                .flatten()
        })
        .expect("the first target tab exposes its close control");
    let close_point = Pos2::new(
        ((close.x0 + close.x1) * 0.5) as f32,
        ((close.y0 + close.y1) * 0.5) as f32,
    );
    let (viewport, _, maximum) = overflow(&mut dockspace);
    assert!(viewport.contains(close_point));
    assert!(close_point.x >= viewport.center().x);
    assert!(maximum > 40.0);
    let version = dockspace.version();
    frame(
        &context,
        &mut dockspace,
        1.1,
        vec![Event::PointerMoved(source), pointer_button(source, true)],
    );
    frame(
        &context,
        &mut dockspace,
        1.11,
        vec![Event::PointerMoved(close_point)],
    );
    frame(&context, &mut dockspace, 1.16, Vec::new());
    assert!(dockspace.session.pointer_continuation().is_none());
    assert_eq!(dockspace.version(), version);
    assert_eq!(overflow(&mut dockspace).1, 0.0);
    frame(
        &context,
        &mut dockspace,
        1.2,
        vec![
            Event::Key {
                key: egui::Key::Escape,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: egui::Modifiers::NONE,
            },
            pointer_button(close_point, false),
        ],
    );
    assert_eq!(dockspace.version(), version);
}
