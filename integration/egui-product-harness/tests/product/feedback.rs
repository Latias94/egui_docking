//! Destination feedback follows the structural target, not the future preview's overlap.

use super::*;

#[derive(Default)]
struct FeedbackPanes {
    bounds: std::collections::BTreeMap<ItemId, Rect>,
    label_override: Option<&'static str>,
    target: Option<egui_dockspace::DockspaceDropTargetFeedback>,
    decorate_target: bool,
    future_tabs: Vec<egui_dockspace::DropPreviewTab>,
    live_tabs: std::collections::BTreeMap<ItemId, (Rect, Rect, Rect, Rect, bool)>,
}

impl PaneView for FeedbackPanes {
    fn title(&self, item: ItemId) -> Option<egui::WidgetText> {
        match item {
            FIRST => Some("A".into()),
            SECOND => Some("B".into()),
            THIRD => Some("C".into()),
            _ => None,
        }
    }

    fn ui(&mut self, item: ItemId, ui: &mut Ui) -> PaneResponse {
        self.bounds.insert(item, ui.available_rect_before_wrap());
        ui.label(format!("Pane {}", item.get()));
        PaneResponse::default()
    }

    fn drop_preview_ui(&mut self, ui: &Ui, preview: &egui_dockspace::DropPreview) {
        self.target = Some(preview.target);
        self.future_tabs.clone_from(&preview.tabs);
        preview.paint(ui);
        if self.decorate_target {
            preview.paint_target(ui);
            let title = |item| self.title(item).unwrap().text().to_owned();
            let label = match preview.target.action() {
                egui_dockspace::DockspaceDropAction::Merge => {
                    format!(
                        "Dock into {}",
                        title(preview.target.selected_item().unwrap())
                    )
                }
                egui_dockspace::DockspaceDropAction::InsertTab {
                    previous: Some(previous),
                    next: Some(next),
                    ..
                } => format!("Insert between {} and {}", title(previous), title(next)),
                _ => "Custom docking feedback".to_owned(),
            };
            preview.paint_label(ui, self.label_override.unwrap_or(&label));
        }
    }

    fn tab_ui(&mut self, ui: &mut Ui, tab: &egui_dockspace::Tab<'_>) -> egui_dockspace::TabVisuals {
        self.live_tabs.insert(
            tab.item,
            (
                tab.rect,
                tab.visible_rect,
                tab.content_rect,
                tab.content_clip_rect,
                tab.selected,
            ),
        );
        tab.paint(ui)
    }
}

fn assert_future_title_painted(output: &FullOutput, future: &egui_dockspace::DropPreviewTab) {
    let expected = future.title.job.text.as_str();
    let clip = future
        .title_clip_rect
        .expect("the visible future member has title space");
    fn find(shape: &egui::Shape, clip: Rect, expected: &str, region: Rect) -> bool {
        match shape {
            egui::Shape::Text(text) if text.galley.job.text == expected => clip
                .intersect(text.galley.rect.translate(text.pos.to_vec2()))
                .intersect(region)
                .is_positive(),
            egui::Shape::Vec(shapes) => shapes
                .iter()
                .any(|shape| find(shape, clip, expected, region)),
            _ => false,
        }
    }
    assert!(
        output
            .shapes
            .iter()
            .any(|shape| find(&shape.shape, shape.clip_rect, expected, clip)),
        "the future target must visibly paint {expected:?} at {clip:?}"
    );
}

fn assert_future_matches_committed(panes: &FeedbackPanes, tabs: &[egui_dockspace::DropPreviewTab]) {
    for tab in tabs {
        assert_eq!(
            panes.live_tabs[&tab.item],
            (
                tab.rect,
                tab.visible_rect
                    .expect("the fixture's future tab is visible"),
                tab.title_rect.unwrap(),
                tab.title_clip_rect.unwrap(),
                tab.selected,
            ),
            "future and committed geometry must agree for {:?}",
            tab.item
        );
    }
}

fn visible_text_bounds(output: &FullOutput, expected: &str) -> Option<Rect> {
    fn find(shape: &egui::Shape, clip: Rect, expected: &str) -> Option<Rect> {
        match shape {
            egui::Shape::Text(text) if text.galley.job.text == expected => {
                let bounds = text.galley.rect.translate(text.pos.to_vec2());
                clip.intersect(bounds).is_positive().then_some(bounds)
            }
            egui::Shape::Vec(shapes) => shapes.iter().find_map(|shape| find(shape, clip, expected)),
            _ => None,
        }
    }

    output
        .shapes
        .iter()
        .find_map(|clipped| find(&clipped.shape, clipped.clip_rect, expected))
}

fn assert_passive_feedback(output: &FullOutput, expected: &str) -> egui::accesskit::NodeId {
    let (id, node) = accesskit_node(output, Role::Label, expected);
    assert!(!node.supports_action(Action::Click));
    assert!(!node.supports_action(Action::Focus));
    assert!(accesskit_node_rect(node).is_positive());
    assert!(
        visible_text_bounds(output, expected).is_some(),
        "{expected:?} must be visibly painted, not only added to the accessibility tree"
    );
    id
}

#[test]
fn disappearing_source_preview_names_the_original_destination_pane() {
    let context = Context::default();
    context.enable_accesskit();
    let root = DockspaceNode::equal_split(
        DockspaceAxis::Horizontal,
        [
            DockspaceNode::tabs([FIRST]),
            DockspaceNode::tabs([SECOND]),
            DockspaceNode::tabs([THIRD]),
        ],
    )
    .expect("the source and two possible destinations form a horizontal split");
    let layout = DockspaceLayout::new([DockspaceSurfaceLayout::new(
        SURFACE,
        DockspaceRootLayout::new(ROOT, root),
    )])
    .unwrap();
    let mut dockspace = Dockspace::builder("product-destination-feedback", layout)
        .style(instrumented_style())
        .build()
        .unwrap();
    let mut panes = FeedbackPanes::default();
    let _ = run_pane_frame(&context, &mut dockspace, &mut panes, Vec::new());
    let stable = run_pane_frame(&context, &mut dockspace, &mut panes, Vec::new());
    let source = tab_center(&stable.output, "A");
    let original_b = panes.bounds[&SECOND];
    let original_c = panes.bounds[&THIRD];
    let _ = run_pane_frame(
        &context,
        &mut dockspace,
        &mut panes,
        vec![Event::PointerMoved(source), pointer_button(source, true)],
    );
    let _ = run_pane_frame(
        &context,
        &mut dockspace,
        &mut panes,
        vec![Event::PointerMoved(source + vec2(16.0, 8.0))],
    );
    let cluster = settle_until(6, || {
        let frame = run_pane_frame(
            &context,
            &mut dockspace,
            &mut panes,
            vec![Event::PointerMoved(original_c.center())],
        );
        let ready = try_node_rect(&frame.output, Role::Label, "Dockspace center drop guide")
            .is_some_and(|guide| original_c.contains(guide.center()));
        (
            frame,
            if ready {
                Vec::new()
            } else {
                vec!["C's presented center guide"]
            },
        )
    })
    .expect("the original destination must expose its center guide");
    let target = node_rect(&cluster.output, Role::Label, "Dockspace center drop guide").center();
    let drop_fill = dockspace.style().visuals.drop_fill.unwrap();
    let preview = settle_until(6, || {
        let frame = run_pane_frame(
            &context,
            &mut dockspace,
            &mut panes,
            vec![Event::PointerMoved(target)],
        );
        let spans_old_panes = rects_with_fill(&frame.output, &[drop_fill])
            .iter()
            .any(|rect| {
                rect.min.x < original_c.min.x - 1.0
                    && rect.max.x >= original_c.max.x - 1.0
                    && rect.intersect(original_b).is_positive()
            });
        (
            frame,
            if spans_old_panes {
                Vec::new()
            } else {
                vec!["the future C preview expands into B after removing the source pane"]
            },
        )
    })
    .expect("the fixture must acquire the enlarged future center preview");
    assert_eq!(root_split(&dockspace).unwrap().1.len(), 3);
    assert!(visible_text_bounds(&preview.output, "Dock into C").is_none());
    assert!(try_node_rect(&preview.output, Role::Label, "Dock into C").is_none());
    let future = panes.future_tabs.clone();
    assert_eq!(
        future.iter().map(|tab| tab.item).collect::<Vec<_>>(),
        vec![THIRD, FIRST]
    );
    let moved_tab = future.iter().find(|tab| tab.item == FIRST).unwrap();
    assert!(moved_tab.selected && moved_tab.is_payload);
    assert_future_title_painted(&preview.output, moved_tab);
    panes.decorate_target = true;
    let preview = run_pane_frame(&context, &mut dockspace, &mut panes, Vec::new());
    let feedback = assert_passive_feedback(&preview.output, "Dock into C");
    assert_eq!(panes.target.unwrap().selected_item(), Some(THIRD));
    assert!(try_node_rect(&preview.output, Role::Label, "Dock into B").is_none());
    assert!(visible_text_bounds(&preview.output, "Dock into B").is_none());

    let held = run_pane_frame(&context, &mut dockspace, &mut panes, Vec::new());
    assert_eq!(
        assert_passive_feedback(&held.output, "Dock into C"),
        feedback
    );
    let _ = run_pane_frame(
        &context,
        &mut dockspace,
        &mut panes,
        vec![pointer_button(target, false)],
    );
    assert_eq!(
        dockspace.view().item(THIRD).unwrap().tabs().items(),
        &[THIRD, FIRST],
        "the passive feedback must not intercept release or retarget it to B"
    );
    assert_eq!(
        dockspace.view().item(SECOND).unwrap().tabs().items(),
        &[SECOND]
    );
    assert_eq!(root_split(&dockspace).unwrap().1.len(), 2);
    let _ = run_pane_frame(&context, &mut dockspace, &mut panes, Vec::new());
    assert_future_matches_committed(&panes, &future);
}

#[test]
fn tab_reorder_shows_the_insertion_gap_and_neighbor_titles() {
    let context = Context::default();
    context.enable_accesskit();
    let layout = DockspaceLayout::new([DockspaceSurfaceLayout::new(
        SURFACE,
        DockspaceRootLayout::new(ROOT, DockspaceNode::central_tabs([FIRST, SECOND, THIRD])),
    )])
    .unwrap();
    let mut dockspace = Dockspace::builder("product-insertion-feedback", layout)
        .style(instrumented_style())
        .build()
        .unwrap();
    let mut panes = Box::new(FeedbackPanes::default());
    let _ = run_pane_frame(&context, &mut dockspace, &mut panes, Vec::new());
    let stable = run_pane_frame(&context, &mut dockspace, &mut panes, Vec::new());
    let a = node_rect(&stable.output, Role::Tab, "A");
    let b = node_rect(&stable.output, Role::Tab, "B");
    let c = node_rect(&stable.output, Role::Tab, "C");
    let source = a.center();
    let target = Pos2::new(c.min.x + 1.0, c.center().y);
    let _ = run_pane_frame(
        &context,
        &mut dockspace,
        &mut panes,
        vec![Event::PointerMoved(source), pointer_button(source, true)],
    );
    let _ = run_pane_frame(
        &context,
        &mut dockspace,
        &mut panes,
        vec![Event::PointerMoved(source + vec2(16.0, 8.0))],
    );
    let drop_fill = dockspace.style().visuals.drop_fill.unwrap();
    let preview = settle_until(6, || {
        let frame = run_pane_frame(
            &context,
            &mut dockspace,
            &mut panes,
            vec![Event::PointerMoved(target)],
        );
        let insertion_marker = rects_with_fill(&frame.output, &[drop_fill])
            .iter()
            .any(|rect| {
                rect.width() > 0.0
                    && rect.width() < rect.height()
                    && rect.height() >= b.height() * 0.5
                    && b.union(c).expand(4.0).contains(rect.center())
            });
        (
            frame,
            if insertion_marker {
                Vec::new()
            } else {
                vec!["a destination tab insertion marker distinct from the pointer ghost"]
            },
        )
    })
    .expect("the real tab gap must paint an insertion marker");
    assert_eq!(tab_items(&dockspace), vec![FIRST, SECOND, THIRD]);
    assert!(visible_text_bounds(&preview.output, "Insert between B and C").is_none());
    assert!(try_node_rect(&preview.output, Role::Label, "Insert between B and C").is_none());
    let future = panes.future_tabs.clone();
    assert_eq!(
        future.iter().map(|tab| tab.item).collect::<Vec<_>>(),
        vec![SECOND, FIRST, THIRD]
    );
    assert_future_title_painted(
        &preview.output,
        future.iter().find(|tab| tab.item == FIRST).unwrap(),
    );
    panes.decorate_target = true;
    let preview = run_pane_frame(&context, &mut dockspace, &mut panes, Vec::new());
    let feedback = assert_passive_feedback(&preview.output, "Insert between B and C");
    assert_eq!(
        panes.target.unwrap().action(),
        egui_dockspace::DockspaceDropAction::InsertTab {
            index: 2,
            previous: Some(SECOND),
            next: Some(THIRD),
        }
    );
    for wrong in ["Insert between A and B", "Insert between A and C"] {
        assert!(try_node_rect(&preview.output, Role::Label, wrong).is_none());
        assert!(visible_text_bounds(&preview.output, wrong).is_none());
    }
    let held = run_pane_frame(&context, &mut dockspace, &mut panes, Vec::new());
    assert_eq!(
        assert_passive_feedback(&held.output, "Insert between B and C"),
        feedback
    );
    panes.label_override = Some("Place A here");
    let customized = run_pane_frame(&context, &mut dockspace, &mut panes, Vec::new());
    assert_eq!(
        assert_passive_feedback(&customized.output, "Place A here"),
        feedback
    );
    let _ = run_pane_frame(
        &context,
        &mut dockspace,
        &mut panes,
        vec![pointer_button(target, false)],
    );
    assert_eq!(
        tab_items(&dockspace),
        vec![SECOND, FIRST, THIRD],
        "neighbor titles describe the insertion after excluding the dragged source"
    );
    let _ = run_pane_frame(&context, &mut dockspace, &mut panes, Vec::new());
    assert_future_matches_committed(&panes, &future);
}
