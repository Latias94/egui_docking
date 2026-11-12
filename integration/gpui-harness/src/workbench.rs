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
    runtime::DockspaceSession,
};
use open_gpui::{
    App, Bounds, Context, IntoElement, Render, Window, WindowBounds, WindowOptions, div,
    prelude::*, px, rgb, size,
};

use crate::{DockPane, GpuiDockspace, GpuiDockspaceConfig, SharedHost, presentation, smoke};

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
    let session = DockspaceSession::from_layout(layout, policy)?;
    let panes = [
        (1, "Editor", 0x2563eb),
        (2, "Preview", 0x0f766e),
        (3, "Inspector", 0x7c3aed),
        (4, "Tools", 0xb45309),
    ]
    .into_iter()
    .map(|(id, title, accent)| {
        let view = cx.new(|_| CounterPanel {
            title,
            accent,
            clicks: 0,
            button_probe: None,
        });
        DockPane::new(ItemId::new(id), title, view)
    })
    .collect::<Vec<_>>();
    GpuiDockspace::new(session, SURFACE, panes, GpuiDockspaceConfig::default(), cx)
}

pub(crate) struct CounterPanel {
    pub(crate) title: &'static str,
    pub(crate) accent: u32,
    pub(crate) clicks: usize,
    pub(crate) button_probe: Option<smoke::ButtonProbe>,
}

impl Render for CounterPanel {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .size_full()
            .overflow_hidden()
            .flex()
            .flex_col()
            .p_4()
            .gap_3()
            .bg(rgb(0xf5f6f8))
            .text_color(rgb(0x253247))
            .child(div().text_lg().child(self.title))
            .child("Drag a tab to dock or float. Drag a divider to resize.")
            .child(
                div()
                    .id("increment")
                    .debug_selector(|| format!("counter:{}", self.title))
                    .px_3()
                    .py_2()
                    .rounded_md()
                    .bg(rgb(self.accent))
                    .text_color(rgb(0xffffff))
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
                    .child(format!("GPUI button · {} clicks", self.clicks)),
            )
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
