//! Descriptive OS captions derived from the committed product view.

use dockspace::model::{
    DockspaceNodeView, DockspaceRootView, DockspaceTabsView, DockspaceView, SurfaceId,
};
use egui_dockspace::PaneView;

pub(crate) fn window_title(
    view: DockspaceView<'_>,
    panes: &impl PaneView,
    surface: SurfaceId,
) -> String {
    let mut title = String::new();
    if let Some(surface) = view.surface(surface) {
        if let Some(node) = surface.main_root().and_then(DockspaceRootView::content) {
            append_selected_titles(&mut title, node, panes);
        }
        for contained in surface.contained() {
            if let Some(node) = contained.root().and_then(DockspaceRootView::content) {
                append_selected_titles(&mut title, node, panes);
            }
        }
    }
    if title.is_empty() {
        format!("Panel {}", surface.get())
    } else {
        title
    }
}

fn append_selected_titles(title: &mut String, node: DockspaceNodeView<'_>, panes: &impl PaneView) {
    let mut pending = vec![node];
    while let Some(node) = pending.pop() {
        if let Some(item) = node.tabs().and_then(DockspaceTabsView::selected) {
            if !title.is_empty() {
                title.push_str(" · ");
            }
            if let Some(caption) = panes
                .title(item)
                .filter(|caption| !caption.text().trim().is_empty())
            {
                title.push_str(caption.text().trim());
            } else {
                use std::fmt::Write as _;
                let _ = write!(title, "Pane {}", item.get());
            }
        } else if let Some(split) = node.split() {
            let base = pending.len();
            pending.extend(split.children());
            pending[base..].reverse();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use dockspace::model::{
        DockspaceAxis, DockspaceLayout, DockspaceNode, DockspaceRootLayout, DockspaceSurfaceLayout,
        ItemId, RootId,
    };
    use dockspace::policy::DockPolicy;
    use dockspace::runtime::{DockspaceSession, SurfaceUnavailableReason};

    const SURFACE: SurfaceId = SurfaceId::new(2);
    const FIRST: ItemId = ItemId::new(1);
    const SECOND: ItemId = ItemId::new(2);

    struct Panes;

    impl PaneView for Panes {
        fn title(&self, item: ItemId) -> Option<eframe::egui::WidgetText> {
            match item {
                FIRST => Some("Inspector".into()),
                SECOND => Some("预览".into()),
                _ => None,
            }
        }

        fn ui(&mut self, _: ItemId, _: &mut eframe::egui::Ui) -> egui_dockspace::PaneResponse {
            egui_dockspace::PaneResponse::default()
        }
    }

    fn session(content: DockspaceNode) -> DockspaceSession {
        DockspaceSession::from_layout(
            DockspaceLayout::new([DockspaceSurfaceLayout::new(
                SURFACE,
                DockspaceRootLayout::new(RootId::new(2), content),
            )])
            .unwrap(),
            DockPolicy::default(),
        )
        .unwrap()
    }

    #[test]
    fn selecting_another_pane_changes_the_caption_without_replacing_its_surface() {
        let mut session = session(DockspaceNode::tabs([FIRST, SECOND]));
        assert_eq!(window_title(session.view(), &Panes, SURFACE), "Inspector");
        let action = session.prepare_select_item(SECOND);
        let mut frame = session.begin_host_frame().unwrap();
        frame.submit_prepared_action(action).unwrap();
        frame
            .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
            .unwrap();
        frame.commit().unwrap();
        assert_eq!(window_title(session.view(), &Panes, SURFACE), "预览");
        assert_eq!(session.view().surface(SURFACE).unwrap().id(), SURFACE);
    }

    #[test]
    fn split_captions_include_selected_leaves_and_missing_content_has_a_fallback() {
        let session = session(
            DockspaceNode::equal_split(
                DockspaceAxis::Horizontal,
                [
                    DockspaceNode::tabs([FIRST, SECOND]),
                    DockspaceNode::tabs([ItemId::new(9)]),
                ],
            )
            .unwrap(),
        );
        assert_eq!(
            window_title(session.view(), &Panes, SURFACE),
            "Inspector · Pane 9"
        );
        assert_eq!(
            window_title(session.view(), &Panes, SurfaceId::new(8)),
            "Panel 8"
        );
    }

    #[test]
    fn deep_window_caption_preserves_leaf_order_without_call_stack_traversal() {
        const TEST: &str = "window_title::tests::deep_window_caption_preserves_leaf_order_without_call_stack_traversal";
        const CHILD: &str = "DOCKSPACE_DEEP_CAPTION_SUBPROCESS";
        if std::env::var(CHILD).as_deref() != Ok(TEST) {
            // A recursive traversal can abort the process on stack overflow.
            let output = std::process::Command::new(std::env::current_exe().unwrap())
                .args(["--exact", TEST, "--nocapture"])
                .env(CHILD, TEST)
                .output()
                .unwrap();
            assert!(
                output.status.success(),
                "{TEST} failed: {}\n{}\n{}",
                output.status,
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr),
            );
            return;
        }

        let mut node = DockspaceNode::central_tabs([FIRST]);
        let mut expected = "Inspector".to_owned();
        for depth in 0_u64..10_000 {
            let axis = if depth.is_multiple_of(2) {
                DockspaceAxis::Horizontal
            } else {
                DockspaceAxis::Vertical
            };
            let item = depth + 3;
            node =
                DockspaceNode::equal_split(axis, [node, DockspaceNode::tabs([ItemId::new(item)])])
                    .unwrap();
            use std::fmt::Write as _;
            write!(expected, " · Pane {item}").unwrap();
        }
        let layout = DockspaceLayout::new([DockspaceSurfaceLayout::new(
            SURFACE,
            DockspaceRootLayout::new(RootId::new(2), node),
        )])
        .unwrap();
        assert_eq!(window_title(layout.view(), &Panes, SURFACE), expected);
    }
}
