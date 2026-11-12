//! Consumer tests exercise the public retained host with application-owned views.

use std::{cell::Cell, rc::Rc};

use dockspace::{
    geometry::LogicalSize,
    model::{
        DockspaceAxis, DockspaceLayout, DockspaceNode, DockspaceRootLayout, DockspaceSurfaceLayout,
        ItemId, RootId, SurfaceId,
    },
    policy::{CloseCapability, DockPolicy},
    runtime::DockspaceSession,
};
use dockspace_gpui_harness::{DockPane, GpuiDockspace, GpuiDockspaceConfig};
use open_gpui::{
    App, Context, Entity, IntoElement, Modifiers, MouseButton, Render, RequestFrameOptions,
    SharedString, TestAppContext, VisualTestContext, Window, div, point, prelude::*, px, size,
};

const SURFACE: SurfaceId = SurfaceId::new(83);
const FIRST: ItemId = ItemId::new(201);
const SECOND: ItemId = ItemId::new(509);

struct Content {
    item: ItemId,
    clicks: usize,
}

impl Render for Content {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let item = self.item;
        div()
            .size_full()
            .debug_selector(move || format!("application-pane:{}", item.get()))
            .child(
                div()
                    .id("application-button")
                    .debug_selector(move || format!("application-button:{}", item.get()))
                    .w(px(180.0))
                    .h(px(35.0))
                    .on_click(cx.listener(|pane, _, _, cx| {
                        pane.clicks += 1;
                        cx.notify();
                    }))
                    .child(format!("{} clicks", self.clicks)),
            )
    }
}

struct Embedded {
    dock: GpuiDockspace,
    visible: Rc<Cell<bool>>,
}

struct RepeatedHost {
    dock: GpuiDockspace,
}

impl Render for RepeatedHost {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .size_full()
            .flex()
            .flex_col()
            .child(self.dock.element())
            .child(self.dock.element())
    }
}

impl Render for Embedded {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .size_full()
            .p_5()
            .when(self.visible.get(), |root| root.child(self.dock.element()))
    }
}

fn session() -> DockspaceSession {
    session_with_content(DockspaceNode::tabs([FIRST, SECOND]))
}

fn session_with_content(content: DockspaceNode) -> DockspaceSession {
    let layout = DockspaceLayout::new([DockspaceSurfaceLayout::new(
        SURFACE,
        DockspaceRootLayout::new(RootId::new(37), content),
    )])
    .unwrap();
    let mut policy = DockPolicy::default();
    policy.set_allow_native_surfaces(false);
    policy.set_close_capability(CloseCapability::Disabled);
    DockspaceSession::from_layout(layout, policy).unwrap()
}

fn host(cx: &mut App, config: GpuiDockspaceConfig) -> (GpuiDockspace, [Entity<Content>; 2]) {
    let views = [FIRST, SECOND].map(|item| cx.new(|_| Content { item, clicks: 0 }));
    let title = |item: ItemId| SharedString::from(format!("Document {}", item.get()));
    let panes = [
        DockPane::new(FIRST, title(FIRST), views[0].clone())
            .with_minimum_size(LogicalSize::new(230.0, 110.0).unwrap()),
        DockPane::new(SECOND, title(SECOND), views[1].clone()),
    ];
    let dock = GpuiDockspace::new(session(), SURFACE, panes, config, cx).unwrap();
    (dock, views)
}

fn frames(visual: &mut VisualTestContext, dock: &GpuiDockspace) {
    for _ in 0..4 {
        assert!(visual.simulate_frame(RequestFrameOptions {
            require_presentation: true,
            force_render: true,
            ..Default::default()
        }));
        assert!(dock.error().is_none(), "{:?}", dock.error());
    }
}

#[open_gpui::test]
fn an_application_can_embed_its_own_ids_views_and_dynamic_titles(cx: &mut TestAppContext) {
    cx.defer_next_window_frame_requests();
    let (dock, views) = cx.update(|app| host(app, GpuiDockspaceConfig::default()));
    let visible = Rc::new(Cell::new(true));
    let window = cx.open_window(size(px(1100.0), px(760.0)), |_, _| Embedded {
        dock: dock.clone(),
        visible,
    });
    let mut visual = VisualTestContext::from_window(window.into(), cx);
    visual.update(|window, _| {
        let _ = window.activate_window();
    });
    frames(&mut visual, &dock);
    let button = visual.debug_bounds("application-button:201").unwrap();
    assert!(f32::from(button.origin.x) >= 20.0);
    visual.simulate_click(button.center(), Modifiers::default());
    frames(&mut visual, &dock);
    assert_eq!(views[0].read_with(&visual, |pane, _| pane.clicks), 1);

    for (tab, selected) in [("dockspace:tab:509", SECOND), ("dockspace:tab:201", FIRST)] {
        let bounds = visual.debug_bounds(tab).expect("stable item selector");
        visual.simulate_click(bounds.center(), Modifiers::default());
        frames(&mut visual, &dock);
        assert!(dock.with_view(|view| view.item(selected).unwrap().is_selected()));
    }
    assert_eq!(views[0].read_with(&visual, |pane, _| pane.clicks), 1);
    assert!(!visual.did_quit());
}

#[open_gpui::test]
fn removing_and_reinserting_the_host_keeps_application_views_alive(cx: &mut TestAppContext) {
    cx.defer_next_window_frame_requests();
    let (dock, views) = cx.update(|app| host(app, GpuiDockspaceConfig::default()));
    let visible = Rc::new(Cell::new(true));
    let window = cx.open_window(size(px(1100.0), px(760.0)), |_, _| Embedded {
        dock: dock.clone(),
        visible: visible.clone(),
    });
    let mut visual = VisualTestContext::from_window(window.into(), cx);
    visual.update(|window, _| {
        let _ = window.activate_window();
    });
    frames(&mut visual, &dock);
    let button = visual.debug_bounds("application-button:201").unwrap();
    visual.simulate_click(button.center(), Modifiers::default());
    frames(&mut visual, &dock);

    visible.set(false);
    visual.update(|window, _| window.refresh());
    frames(&mut visual, &dock);
    assert!(!dock.has_active_gesture());
    assert!(!visual.did_quit());
    visible.set(true);
    visual.update(|window, _| window.refresh());
    frames(&mut visual, &dock);
    let button = visual.debug_bounds("application-button:201").unwrap();
    visual.simulate_click(button.center(), Modifiers::default());
    frames(&mut visual, &dock);
    assert_eq!(views[0].read_with(&visual, |pane, _| pane.clicks), 2);
}

#[open_gpui::test]
fn configured_font_size_changes_the_measured_tab_geometry(cx: &mut TestAppContext) {
    cx.defer_next_window_frame_requests();
    let mut widths = Vec::new();
    for text_size in [14.0, 22.0] {
        let config = GpuiDockspaceConfig {
            font: open_gpui::font("Helvetica"),
            text_size: px(text_size),
            ..Default::default()
        };
        let (dock, _) = cx.update(|app| host(app, config));
        let window = cx.open_window(size(px(1600.0), px(760.0)), |_, _| Embedded {
            dock: dock.clone(),
            visible: Rc::new(Cell::new(true)),
        });
        let mut visual = VisualTestContext::from_window(window.into(), cx);
        frames(&mut visual, &dock);
        let tab = visual.debug_bounds("dockspace:tab:201").unwrap();
        widths.push(f32::from(tab.size.width));
    }
    assert!(
        widths[1] > widths[0] * 1.3,
        "the same configured font and size must drive layout and chrome: {widths:?}"
    );
}

#[open_gpui::test]
fn pane_minimum_size_limits_a_real_splitter_drag(cx: &mut TestAppContext) {
    cx.defer_next_window_frame_requests();
    let dock = cx.update(|app| {
        let content = DockspaceNode::split(
            DockspaceAxis::Horizontal,
            [
                (DockspaceNode::tabs([FIRST]), 0.5),
                (DockspaceNode::tabs([SECOND]), 0.5),
            ],
        )
        .unwrap();
        let panes = [FIRST, SECOND].map(|item| {
            let view = app.new(|_| Content { item, clicks: 0 });
            DockPane::new(item, "Application pane", view)
                .with_minimum_size(LogicalSize::new(230.0, 110.0).unwrap())
        });
        GpuiDockspace::new(
            session_with_content(content),
            SURFACE,
            panes,
            GpuiDockspaceConfig::default(),
            app,
        )
        .unwrap()
    });
    let window = cx.open_window(size(px(1100.0), px(760.0)), |_, _| Embedded {
        dock: dock.clone(),
        visible: Rc::new(Cell::new(true)),
    });
    let mut visual = VisualTestContext::from_window(window.into(), cx);
    visual.update(|window, _| {
        let _ = window.activate_window();
    });
    frames(&mut visual, &dock);
    let left = visual.debug_bounds("application-pane:201").unwrap();
    let right = visual.debug_bounds("application-pane:509").unwrap();
    let start = point(
        (left.origin.x + left.size.width + right.origin.x) / 2.0,
        left.center().y,
    );
    visual.simulate_mouse_down(start, MouseButton::Left, Modifiers::default());
    frames(&mut visual, &dock);
    let end = point(px(25.0), start.y);
    visual.simulate_mouse_move(end, MouseButton::Left, Modifiers::default());
    frames(&mut visual, &dock);
    visual.simulate_mouse_up(end, MouseButton::Left, Modifiers::default());
    frames(&mut visual, &dock);
    let resized = visual.debug_bounds("application-pane:201").unwrap();
    assert!(resized.size.width >= px(230.0));
    assert!(resized.size.width < left.size.width - px(100.0));
    assert!(!dock.has_active_gesture());
}

#[open_gpui::test]
fn application_can_replace_the_cancel_key(cx: &mut TestAppContext) {
    cx.defer_next_window_frame_requests();
    let mut config = GpuiDockspaceConfig::default();
    config.input.cancel = Some(open_gpui::Keystroke {
        key: "q".into(),
        modifiers: Modifiers::default(),
        key_char: None,
    });
    let (dock, _) = cx.update(|app| host(app, config));
    let window = cx.open_window(size(px(1100.0), px(760.0)), |_, _| Embedded {
        dock: dock.clone(),
        visible: Rc::new(Cell::new(true)),
    });
    let mut visual = VisualTestContext::from_window(window.into(), cx);
    visual.update(|window, _| {
        let _ = window.activate_window();
    });
    frames(&mut visual, &dock);
    let tab = visual.debug_bounds("dockspace:tab:509").unwrap();
    visual.simulate_mouse_down(tab.center(), MouseButton::Left, Modifiers::default());
    frames(&mut visual, &dock);
    visual.simulate_mouse_move(
        tab.center() + point(px(40.0), px(100.0)),
        MouseButton::Left,
        Modifiers::default(),
    );
    frames(&mut visual, &dock);
    assert!(dock.has_active_gesture());
    visual.simulate_keystrokes("escape");
    frames(&mut visual, &dock);
    assert!(
        dock.has_active_gesture(),
        "the replaced default must not cancel"
    );
    visual.simulate_keystrokes("q");
    frames(&mut visual, &dock);
    assert!(!dock.has_active_gesture());
}

#[open_gpui::test]
fn a_cloned_handle_cannot_mount_the_same_session_in_another_window(cx: &mut TestAppContext) {
    cx.defer_next_window_frame_requests();
    let (dock, views) = cx.update(|app| host(app, GpuiDockspaceConfig::default()));
    let first = cx.open_window(size(px(1100.0), px(760.0)), |_, _| Embedded {
        dock: dock.clone(),
        visible: Rc::new(Cell::new(true)),
    });
    let mut first = VisualTestContext::from_window(first.into(), cx);
    frames(&mut first, &dock);
    let second = cx.open_window(size(px(1100.0), px(760.0)), |_, _| Embedded {
        dock: dock.clone(),
        visible: Rc::new(Cell::new(true)),
    });
    let mut second = VisualTestContext::from_window(second.into(), cx);
    frames(&mut second, &dock);
    assert!(second.debug_bounds("application-button:201").is_none());

    first.update(|window, _| {
        let _ = window.activate_window();
    });
    frames(&mut first, &dock);
    let button = first.debug_bounds("application-button:201").unwrap();
    first.simulate_click(button.center(), Modifiers::default());
    frames(&mut first, &dock);
    assert_eq!(views[0].read_with(&first, |pane, _| pane.clicks), 1);
    assert!(dock.with_view(|view| view.item(FIRST).unwrap().is_selected()));
}

#[open_gpui::test]
fn closing_an_embedding_window_retires_its_gesture_without_owning_app_exit(
    cx: &mut TestAppContext,
) {
    cx.defer_next_window_frame_requests();
    cx.update(|app| app.set_quit_mode(open_gpui::QuitMode::Explicit));
    let (dock, _) = cx.update(|app| host(app, GpuiDockspaceConfig::default()));
    let window = cx.open_window(size(px(1100.0), px(760.0)), |_, _| Embedded {
        dock: dock.clone(),
        visible: Rc::new(Cell::new(true)),
    });
    let mut visual = VisualTestContext::from_window(window.into(), cx);
    visual.update(|window, _| {
        let _ = window.activate_window();
    });
    frames(&mut visual, &dock);
    let tab = visual.debug_bounds("dockspace:tab:509").unwrap();
    visual.simulate_mouse_down(tab.center(), MouseButton::Left, Modifiers::default());
    frames(&mut visual, &dock);
    assert!(dock.has_active_gesture());
    visual.update(|window, app| window.remove_window(app));
    assert!(!dock.has_active_gesture());
    assert!(!visual.did_quit());
}

#[open_gpui::test]
fn duplicate_siblings_cannot_withdraw_the_first_mounts_candidate(cx: &mut TestAppContext) {
    cx.defer_next_window_frame_requests();
    let (dock, views) = cx.update(|app| host(app, GpuiDockspaceConfig::default()));
    let window = cx.open_window(size(px(1100.0), px(760.0)), |_, _| RepeatedHost {
        dock: dock.clone(),
    });
    let mut visual = VisualTestContext::from_window(window.into(), cx);
    visual.update(|window, _| {
        let _ = window.activate_window();
    });
    frames(&mut visual, &dock);
    let button = visual.debug_bounds("application-button:201").unwrap();
    visual.simulate_click(button.center(), Modifiers::default());
    frames(&mut visual, &dock);
    assert_eq!(views[0].read_with(&visual, |pane, _| pane.clicks), 1);
    let tab = visual.debug_bounds("dockspace:tab:509").unwrap();
    visual.simulate_click(tab.center(), Modifiers::default());
    frames(&mut visual, &dock);
    assert!(dock.with_view(|view| view.item(SECOND).unwrap().is_selected()));
}
