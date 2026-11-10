//! Deep product declarations and measured scenes must not depend on the call stack.

use std::process::Command;

use dockspace::geometry::{LogicalRect, LogicalSize};
use dockspace::model::{
    DockspaceAxis, DockspaceContainedLayout, DockspaceLayout, DockspaceLayoutError, DockspaceNode,
    DockspaceRootLayout, DockspaceSurfaceLayout, DockspaceView, FloatingPresentationId, ItemId,
    RootId, SurfaceId,
};
use dockspace::policy::DockPolicy;
use dockspace::runtime::{
    DockPresentationConfig, DockspaceSession, SurfaceMeasurementAnswer as Answer,
    SurfaceMeasurementRequest as Question, SurfacePaintProgramOperation as Operation,
    SurfacePaintProgramStep as Step, SurfacePresentationResult, SurfaceUnavailableReason,
    TabStripMetrics,
};

const DEPTH: usize = 10_000;
const SURFACE: SurfaceId = SurfaceId::new(1);
const ROOT: RootId = RootId::new(1);
const FLOATING: FloatingPresentationId = FloatingPresentationId::new(1);
const FIRST: ItemId = ItemId::new(1);
const SECOND: ItemId = ItemId::new(2);
const SUBPROCESS: &str = "DOCKSPACE_DEEP_LAYOUT_SUBPROCESS";

fn subprocess(test: &str, run: impl FnOnce()) {
    if std::env::var(SUBPROCESS).as_deref() == Ok(test) {
        run();
        return;
    }
    // Isolate stack-overflow aborts without enlarging the test thread's stack.
    let output = Command::new(std::env::current_exe().unwrap())
        .args(["--exact", test, "--nocapture"])
        .env(SUBPROCESS, test)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{test} failed: {}\n{}\n{}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
}

fn axis(depth: usize) -> DockspaceAxis {
    if depth.is_multiple_of(2) {
        DockspaceAxis::Horizontal
    } else {
        DockspaceAxis::Vertical
    }
}

fn side_item(depth: usize) -> ItemId {
    ItemId::new(u64::try_from(depth).unwrap() + 3)
}

fn declaration() -> DockspaceNode {
    let mut node =
        DockspaceNode::central_tabs_with_selection([FIRST, SECOND], Some(SECOND)).unwrap();
    for depth in 0..DEPTH {
        node = DockspaceNode::equal_split(
            axis(depth),
            [node, DockspaceNode::tabs([side_item(depth)])],
        )
        .unwrap();
    }
    node
}

fn surface(id: SurfaceId, root: RootId, node: DockspaceNode) -> DockspaceSurfaceLayout {
    DockspaceSurfaceLayout::new(id, DockspaceRootLayout::new(root, node))
}

fn assert_declaration(view: DockspaceView<'_>) {
    let root = view.root(ROOT).unwrap();
    let mut node = root.content().unwrap();
    for depth in (0..DEPTH).rev() {
        let split = node
            .split()
            .expect("alternating axes must preserve every split");
        assert_eq!(split.axis(), axis(depth));
        assert_eq!(split.weights().collect::<Vec<_>>(), [0.5, 0.5]);
        let mut children = split.children();
        node = children.next().unwrap();
        let side = children.next().unwrap().tabs().unwrap();
        assert!(children.next().is_none());
        assert_eq!(side.items(), [side_item(depth)]);
        assert_eq!(side.selected(), Some(side_item(depth)));
        assert!(!side.is_central());
    }
    let central = node.tabs().unwrap();
    assert_eq!(central.items(), [FIRST, SECOND]);
    assert_eq!(central.selected(), Some(SECOND));
    assert!(central.is_central());
    assert_eq!(
        root.central().unwrap().tabs().unwrap().items(),
        central.items()
    );
}

#[test]
fn deep_declarations_clone_compile_and_drop() {
    subprocess("deep_declarations_clone_compile_and_drop", || {
        let node = declaration();
        let cloned = node.clone();
        assert!(format!("{node:#?}").contains("child_count: 2"));
        drop(node);

        let original = surface(SURFACE, ROOT, cloned);
        let cloned = original.clone();
        assert!(format!("{original:#?}").contains("DockspaceNode"));
        for surface in [original, cloned] {
            let layout = DockspaceLayout::new([surface]).unwrap();
            assert_declaration(layout.view());
            let debug = format!("{layout:#?}");
            assert!(!debug.contains("NodeId"));
            assert!(!debug.contains("Workspace {"));
        }
    });
}

#[test]
fn deep_public_views_have_bounded_debug_output() {
    subprocess("deep_public_views_have_bounded_debug_output", || {
        let layout = DockspaceLayout::new([surface(SURFACE, ROOT, declaration())]).unwrap();
        let view = layout.view();
        let surface = view.surface(SURFACE).unwrap();
        let root = surface.main_root().unwrap();
        let node = root.content().unwrap();
        for debug in [
            format!("{view:?}"),
            format!("{surface:?}"),
            format!("{root:?}"),
            format!("{node:?}"),
            format!("{:?}", node.split().unwrap()),
        ] {
            assert!(debug.contains("child_count: 2"), "{debug}");
            assert!(
                debug.len() < 1_024,
                "diagnostics must not expand the forest"
            );
            assert!(!debug.contains("NodeId"));
        }
    });
}

#[test]
fn deep_declaration_errors_release_unvisited_subtrees() {
    subprocess("deep_declaration_errors_release_unvisited_subtrees", || {
        assert_eq!(
            DockspaceNode::equal_split(DockspaceAxis::Horizontal, [declaration()]).unwrap_err(),
            DockspaceLayoutError::SplitTooFewChildren { children: 1 },
        );
        assert_eq!(
            DockspaceNode::split(
                DockspaceAxis::Horizontal,
                [
                    (declaration(), f32::NAN),
                    (DockspaceNode::tabs([ItemId::new(99_999)]), 1.0),
                ],
            )
            .unwrap_err(),
            DockspaceLayoutError::InvalidSplitWeights,
        );

        let duplicate = ItemId::new(99_999);
        let invalid = DockspaceNode::equal_split(
            DockspaceAxis::Horizontal,
            [
                DockspaceNode::tabs([duplicate]),
                DockspaceNode::tabs([duplicate]),
                declaration(),
            ],
        )
        .unwrap();
        assert_eq!(
            DockspaceLayout::new([surface(SURFACE, ROOT, invalid)]).unwrap_err(),
            DockspaceLayoutError::DuplicateItem { item: duplicate },
        );

        assert_eq!(
            DockspaceLayout::new([
                surface(SURFACE, ROOT, DockspaceNode::tabs([duplicate])),
                surface(SURFACE, RootId::new(2), declaration()),
                surface(SurfaceId::new(2), RootId::new(3), declaration()),
            ])
            .unwrap_err(),
            DockspaceLayoutError::DuplicateSurface { surface: SURFACE },
        );

        let invalid = DockspaceNode::equal_split(
            DockspaceAxis::Horizontal,
            [DockspaceNode::central_tabs([duplicate]), declaration()],
        )
        .unwrap();
        assert_eq!(
            DockspaceLayout::new([surface(SURFACE, ROOT, invalid)]).unwrap_err(),
            DockspaceLayoutError::MultipleCentralRegions { root: ROOT },
        );
    });
}

#[test]
fn deep_public_declaration_compiles_a_complete_measured_scene() {
    subprocess(
        "deep_public_declaration_compiles_a_complete_measured_scene",
        || {
            let config = DockPresentationConfig::default();
            let leaf_width = config.minimum_pane_size().width().max(64.0);
            let leaf_height =
                config.minimum_pane_size().height().max(48.0) + config.tab_bar_height();
            let pairs = f64::from(u32::try_from(DEPTH / 2).unwrap());
            let width = leaf_width + pairs * (leaf_width + config.splitter_thickness());
            let height = leaf_height + pairs * (leaf_height + config.splitter_thickness());
            let border = config.floating_border_width();
            let outer = LogicalRect::new(
                20.0,
                20.0,
                width + 2.0 * border,
                height + config.floating_title_height() + 2.0 * border,
            )
            .unwrap();
            let bounds =
                LogicalRect::new(0.0, 0.0, outer.max().x() + 20.0, outer.max().y() + 20.0).unwrap();
            let layout = DockspaceLayout::new([DockspaceSurfaceLayout::rootless(SURFACE)
                .with_contained(DockspaceContainedLayout::new(
                    FLOATING,
                    DockspaceRootLayout::new(ROOT, declaration()),
                    outer,
                ))])
            .unwrap();
            let mut session = DockspaceSession::from_layout(layout, DockPolicy::default()).unwrap();
            assert_declaration(session.view());
            let version = session.version();
            let mut pane_questions = 0;
            let mut frame = session.begin_host_frame().unwrap();
            frame
                .measure_surface_with(SURFACE, |question| match question {
                    Question::DockBounds { .. } | Question::PopupPlaneBounds { .. } => {
                        Answer::Bounds(bounds)
                    }
                    Question::PaneMinimum { .. } => {
                        pane_questions += 1;
                        Answer::PaneMinimum(LogicalSize::new(64.0, 48.0).unwrap())
                    }
                    Question::TabIntrinsic { .. } => Answer::TabIntrinsic(56.0),
                    Question::TabStrip { .. } => {
                        Answer::TabStrip(TabStripMetrics::new(0.0, 0.0).unwrap())
                    }
                })
                .unwrap();
            assert!(!frame.commit().unwrap().workspace_changed());
            assert_eq!(pane_questions, DEPTH + 2);

            let mut frame = session.begin_host_frame().unwrap();
            let plan = frame.paint_plan(SURFACE).unwrap().unwrap();
            let mut pane_count = 0;
            let mut splitter_count = 0;
            let mut central_bounds = None;
            let mut minimum = None;
            let receipt = plan
                .execute_surface_program(|step| {
                    match step {
                        Step::RootBegin { root, .. } => {
                            let contained = root.contained().unwrap();
                            assert_eq!(contained.floating(), FLOATING);
                            minimum = Some(contained.minimum_size());
                            assert_eq!(contained.content_bounds().width(), width);
                            assert_eq!(contained.content_bounds().height(), height);
                        }
                        Step::Operation {
                            operation: Operation::Pane(pane),
                            ..
                        } => {
                            pane_count += 1;
                            assert_eq!(pane.root(), ROOT);
                            assert!(pane.content_bounds().width() >= leaf_width);
                            assert!(
                                pane.content_bounds().height()
                                    >= leaf_height - config.tab_bar_height()
                            );
                            if pane.selected() == Some(SECOND) {
                                central_bounds = Some(pane.bounds());
                            }
                        }
                        Step::Operation {
                            operation: Operation::Splitter(splitter),
                            ..
                        } => {
                            splitter_count += 1;
                            let (before, after) = splitter.minimum_extents();
                            assert!(before > 0.0 && after > 0.0);
                        }
                        _ => {}
                    }
                    Ok::<(), ()>(())
                })
                .unwrap();
            assert_eq!(pane_count, DEPTH + 1);
            assert_eq!(splitter_count, DEPTH);
            let central = central_bounds.unwrap();
            assert_eq!(central.width(), leaf_width);
            assert_eq!(central.height(), leaf_height);
            assert_eq!(minimum.unwrap().width(), outer.width());
            assert_eq!(minimum.unwrap().height(), outer.height());
            frame.confirm_surface_painted(receipt).unwrap();
            let mut outputs = frame.commit().unwrap().take_painted_outputs();
            assert_eq!(outputs.len(), 1);
            session
                .report_surface_presentation(
                    outputs.pop().unwrap(),
                    SurfacePresentationResult::Presented,
                )
                .unwrap();
            let mut frame = session.begin_host_frame().unwrap();
            frame
                .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
                .unwrap();
            assert!(!frame.commit().unwrap().workspace_changed());
            assert_eq!(session.version(), version);
        },
    );
}
