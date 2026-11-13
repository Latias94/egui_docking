//! A standalone workbench built with the public retained host.

use std::rc::Rc;

use anyhow::Result;
use dockspace::{
    geometry::LogicalRect,
    model::{
        DockspaceAxis, DockspaceContainedLayout, DockspaceLayout, DockspaceNode,
        DockspaceRootLayout, DockspaceSurfaceLayout, FloatingPresentationId, ItemId, RootId,
        SurfaceId,
    },
    policy::{CloseCapability, DockPolicy},
    runtime::{DockPresentationConfig, DockspaceSession},
};
use open_gpui::{
    AnyElement, App, Bounds, Context, Hsla, IntoElement, Render, Window, WindowBounds,
    WindowOptions, div, prelude::*, px, size,
};

use crate::{
    DockPane, DockspaceColors, GpuiDockspace, GpuiDockspaceConfig, SharedHost, presentation, smoke,
};

pub(crate) const SURFACE: SurfaceId = SurfaceId::new(1);

fn example_host(cx: &mut App) -> Result<GpuiDockspace> {
    let content = DockspaceNode::split(
        DockspaceAxis::Horizontal,
        [
            (
                DockspaceNode::central_tabs([ItemId::new(1), ItemId::new(2)]),
                0.68,
            ),
            (DockspaceNode::tabs([ItemId::new(3)]), 0.32),
        ],
    )?;
    let layout = DockspaceLayout::new([DockspaceSurfaceLayout::new(
        SURFACE,
        DockspaceRootLayout::new(RootId::new(1), content),
    )
    .with_contained(DockspaceContainedLayout::new(
        FloatingPresentationId::new(1),
        DockspaceRootLayout::new(RootId::new(2), DockspaceNode::tabs([ItemId::new(4)])),
        LogicalRect::new(590.0, 330.0, 280.0, 220.0)?,
    ))])?;
    let mut policy = DockPolicy::default();
    policy.set_close_capability(CloseCapability::Disabled);
    policy.set_allow_native_surfaces(false);
    let geometry = DockPresentationConfig::builder()
        .tab_bar_height(27.0)
        .tab_group_grip_extent(20.0)
        .tab_horizontal_padding(9.0)
        .tab_min_width(68.0)
        .guide_extent(28.0)
        .guide_gap(10.0)
        .guide_hit_padding(4.0)
        .guide_outer_inset(48.0)
        .build()?;
    let session = DockspaceSession::from_layout_with_presentation_config(layout, policy, geometry)?;
    let config = GpuiDockspaceConfig::default();
    let panes = [
        (1, "Editor"),
        (2, "Preview"),
        (3, "Inspector"),
        (4, "Tools"),
    ]
    .into_iter()
    .map(|(id, title)| {
        let view = cx.new(|_| CounterPanel {
            title,
            accent: config.colors.accent,
            colors: config.colors.clone(),
            clicks: 0,
            button_probe: None,
        });
        DockPane::new(ItemId::new(id), title, view)
    })
    .collect::<Vec<_>>();
    GpuiDockspace::new(session, SURFACE, panes, config, cx)
}

pub(crate) struct CounterPanel {
    pub(crate) title: &'static str,
    pub(crate) accent: Hsla,
    colors: DockspaceColors,
    pub(crate) clicks: usize,
    pub(crate) button_probe: Option<smoke::ButtonProbe>,
}

impl Render for CounterPanel {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let heading = match self.title {
            "Editor" => "workspace.rs",
            "Preview" => "Workspace preview",
            "Inspector" => "Document",
            _ => "Quick tools",
        };
        div()
            .size_full()
            .overflow_hidden()
            .flex()
            .flex_col()
            .p_3()
            .gap_3()
            .bg(self.colors.pane)
            .text_size(px(12.0))
            .text_color(self.colors.text)
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap_2()
                    .child(div().min_w_0().truncate().child(heading))
                    .child(
                        div()
                            .id("increment")
                            .debug_selector(|| format!("counter:{}", self.title))
                            .flex_shrink_0()
                            .px_2()
                            .py_1()
                            .rounded_sm()
                            .border_1()
                            .border_color(self.colors.border)
                            .bg(self.colors.guide)
                            .text_color(self.colors.text)
                            .hover(|button| button.bg(self.colors.hovered_tab))
                            .active(|button| {
                                button.bg(self.colors.active_guide).text_color(self.accent)
                            })
                            .cursor_pointer()
                            .on_click(cx.listener(|panel, _, _, cx| {
                                panel.clicks += 1;
                                cx.notify();
                            }))
                            .when_some(self.button_probe.clone(), |button, probe| {
                                button.relative().child(
                                    open_gpui::canvas(
                                        |_, _, _| {},
                                        move |bounds, (), _, _| probe.set(Some(bounds)),
                                    )
                                    .absolute()
                                    .top_0()
                                    .bottom_0()
                                    .left_0()
                                    .right_0(),
                                )
                            })
                            .child(format!("Increment · {}", self.clicks)),
                    ),
            )
            .child(self.content())
    }
}

impl CounterPanel {
    fn content(&self) -> AnyElement {
        match self.title {
            "Editor" => {
                let lines = [
                    "use dockspace::model::{DockspaceNode, ItemId};",
                    "",
                    "fn documents() -> DockspaceNode {",
                    "    let editor = ItemId::new(1);",
                    "    let preview = ItemId::new(2);",
                    "",
                    "    DockspaceNode::central_tabs([editor, preview])",
                    "}",
                ];
                div()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .py_2()
                    .children(lines.into_iter().enumerate().map(|(index, line)| {
                        div()
                            .flex()
                            .gap_3()
                            .whitespace_nowrap()
                            .font_family("monospace")
                            .child(
                                div()
                                    .w_4()
                                    .text_color(self.colors.muted_text)
                                    .child((index + 1).to_string()),
                            )
                            .child(
                                div()
                                    .text_color(if line.starts_with("use ") {
                                        self.accent
                                    } else {
                                        self.colors.text
                                    })
                                    .child(line),
                            )
                    }))
                    .into_any_element()
            }
            "Inspector" => {
                let fields = [
                    ("Language", "Rust"),
                    ("Encoding", "UTF-8"),
                    ("Line endings", "LF"),
                    ("Status", "Saved"),
                ];
                div()
                    .flex()
                    .flex_col()
                    .gap_3()
                    .children(fields.into_iter().map(|(label, value)| {
                        div()
                            .flex()
                            .justify_between()
                            .gap_2()
                            .child(div().text_color(self.colors.muted_text).child(label))
                            .child(value)
                    }))
                    .into_any_element()
            }
            "Preview" => div()
                .flex()
                .flex_col()
                .gap_3()
                .p_3()
                .rounded_md()
                .border_1()
                .border_color(self.colors.border)
                .bg(self.colors.guide)
                .child(div().text_size(px(16.0)).child("A workspace that adapts"))
                .child(
                    div()
                        .text_color(self.colors.muted_text)
                        .child("Group documents, split your view, and keep tools nearby."),
                )
                .child(
                    div()
                        .flex()
                        .gap_2()
                        .children(["Editor", "Inspector", "Tools"].map(|label| {
                            div()
                                .px_2()
                                .py_1()
                                .rounded_sm()
                                .bg(self.colors.background)
                                .child(label)
                        })),
                )
                .into_any_element(),
            _ => div()
                .flex()
                .flex_col()
                .gap_2()
                .child(
                    div()
                        .flex()
                        .justify_between()
                        .child(div().text_color(self.colors.muted_text).child("Workspace"))
                        .child("4 panels"),
                )
                .child(
                    div()
                        .flex()
                        .justify_between()
                        .child(
                            div()
                                .text_color(self.colors.muted_text)
                                .child("Background tasks"),
                        )
                        .child("Idle"),
                )
                .into_any_element(),
        }
    }
}

pub struct Workbench {
    pub(crate) host: SharedHost,
    dockspace: GpuiDockspace,
}

impl Workbench {
    pub fn new(cx: &mut App) -> Result<Self> {
        let dockspace = example_host(cx)?;
        Ok(Self {
            host: dockspace.host.clone(),
            dockspace,
        })
    }
}

impl Render for Workbench {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .size_full()
            .overflow_hidden()
            .child(self.dockspace.element())
    }
}

/// Starts one native window. Logical floating windows stay inside this window.
pub fn run() {
    run_application(None);
}

/// Checks real-window presentation, retained controls, tab selection and splitter input.
pub fn run_smoke() {
    run_application(Some(smoke::Smoke::default()));
}

/// Checks presentation and interaction before closing through the native window.
pub fn run_smoke_close() {
    run_application(Some(smoke::Smoke::close_window()));
}

fn run_application(smoke: Option<smoke::Smoke>) {
    let completion = smoke.clone();
    open_gpui_platform::application().run(move |cx| {
        let bounds = Bounds::centered(None, size(px(1100.0), px(760.0)), cx);
        let result = cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                titlebar: Some(open_gpui::TitlebarOptions {
                    title: Some("Dockspace · GPUI workbench".into()),
                    ..Default::default()
                }),
                ..Default::default()
            },
            |window, cx| {
                let workbench = Workbench::new(cx).expect("the example layout is valid");
                let weak = Rc::downgrade(&workbench.host);
                let closed_smoke = smoke.clone();
                cx.on_window_closed(move |cx, _| {
                    if let Some(host) = weak.upgrade() {
                        presentation::close(&mut host.borrow_mut());
                    }
                    if let Some(smoke) = &closed_smoke {
                        smoke.require_complete("window closed before exact presentation completed");
                    }
                    cx.quit();
                })
                .detach();
                if let Some(smoke) = &smoke {
                    smoke.install(&workbench.host, window, cx);
                }
                cx.new(|_| workbench)
            },
        );
        if let Err(error) = result {
            if let Some(smoke) = &smoke {
                smoke.fail(format!("cannot open the GPUI workbench: {error:#}"));
            }
            eprintln!("Cannot open the GPUI workbench: {error:#}");
            cx.quit();
        } else {
            cx.activate(true);
        }
    });
    if let Some(smoke) = completion {
        smoke.require_complete("the native application returned before presentation completed");
    }
}
