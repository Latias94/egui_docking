//! Finite viewports retain item ownership through overflow, collapse and recovery.

use std::collections::BTreeMap;

use dockspace::geometry::{LogicalPoint, LogicalRect, LogicalSize};
use dockspace::model::{
    DockspaceAxis as Axis, DockspaceLayout, DockspaceNode as Node, DockspaceRootLayout,
    DockspaceSurfaceLayout, ItemId, RootId, SurfaceId,
};
use dockspace::policy::DockPolicy;
use dockspace::runtime::{
    DockspaceDropDirection as Direction, DockspaceGuideScope as Scope, DockspaceHostFrame,
    DockspacePreviewVisual, DockspaceReceiverDescriptor, DockspaceSession, HostFrameReport,
    HostPointerInput, HostPointerRoute, PaneOverflowPolicy, PointerDeviceKind,
    PointerEventMetadata, SurfaceMeasurementAnswer as Answer,
    SurfaceMeasurementRequest as Question, SurfacePaintProgramOperation as Operation,
    SurfacePaintProgramStep as Step, SurfacePointerButton as Button, SurfacePointerCancelReason,
    SurfacePointerCapture, SurfacePointerEvent as Event, SurfacePointerId,
    SurfacePointerReceiverFacts, SurfacePresentationResult, SurfaceUnavailableReason,
    TabStripMetrics,
};
use dockspace_host_harness::{OwnedRecord, RecordingRenderer};

const SOURCE: SurfaceId = SurfaceId::new(1);
const TARGET: SurfaceId = SurfaceId::new(2);
const SOURCE_ANCHOR: ItemId = ItemId::new(100);
const TARGET_ANCHOR: ItemId = ItemId::new(200);
const POINTER: SurfacePointerId = SurfacePointerId::new(1);

fn bounds(width: f64, height: f64) -> LogicalRect {
    LogicalRect::new(0.0, 0.0, width, height).unwrap()
}

fn point(x: f64, y: f64) -> LogicalPoint {
    LogicalPoint::new(x, y).unwrap()
}

fn center(rect: LogicalRect) -> LogicalPoint {
    point(
        rect.x() + rect.width() * 0.5,
        rect.y() + rect.height() * 0.5,
    )
}

fn minimum(item: Option<ItemId>) -> LogicalSize {
    let (width, height) = if item.is_some_and(|item| item.get().is_multiple_of(3)) {
        (280.0, 180.0)
    } else {
        (64.0, 48.0)
    };
    LogicalSize::new(width, height).unwrap()
}

fn session(source_items: &[ItemId], target: Node) -> DockspaceSession {
    let layout = DockspaceLayout::new([
        DockspaceSurfaceLayout::new(
            SOURCE,
            DockspaceRootLayout::new(
                RootId::new(1),
                Node::central_tabs(source_items.iter().copied().chain([SOURCE_ANCHOR])),
            ),
        ),
        DockspaceSurfaceLayout::new(TARGET, DockspaceRootLayout::new(RootId::new(2), target)),
    ])
    .unwrap();
    let mut session = DockspaceSession::from_layout(layout, DockPolicy::default()).unwrap();
    session.enable_host_pointer().unwrap();
    session
}

fn commit(mut frame: DockspaceHostFrame<'_>) -> HostFrameReport {
    frame
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .unwrap();
    frame.commit().unwrap()
}

fn overflow_policy(session: &mut DockspaceSession, policy: PaneOverflowPolicy) {
    let config = session
        .presentation_config()
        .to_builder()
        .pane_overflow_policy(policy)
        .build()
        .unwrap();
    let mut frame = session.begin_host_frame().unwrap();
    frame.replace_presentation_config(config).unwrap();
    commit(frame);
}

#[track_caller]
fn measure(
    session: &mut DockspaceSession,
    target: LogicalRect,
    pane_minimum: impl Fn(Option<ItemId>) -> LogicalSize,
) {
    let mut frame = session.begin_host_frame().unwrap();
    for (surface, rect) in [(SOURCE, bounds(900.0, 600.0)), (TARGET, target)] {
        frame
            .measure_surface_with(surface, |question| match question {
                Question::DockBounds { .. } | Question::PopupPlaneBounds { .. } => {
                    Answer::Bounds(rect)
                }
                Question::PaneMinimum { item, .. } => Answer::PaneMinimum(pane_minimum(item)),
                Question::TabIntrinsic { item, .. } => {
                    Answer::TabIntrinsic(if item.get().is_multiple_of(2) {
                        4096.0
                    } else {
                        96.0
                    })
                }
                Question::TabStrip { .. } => {
                    Answer::TabStrip(TabStripMetrics::new(0.0, 0.0).unwrap())
                }
            })
            .unwrap_or_else(|error| {
                panic!("measurement failed for {surface:?}, target {target:?}: {error:?}")
            });
    }
    commit(frame);
}

#[derive(Default)]
struct Snapshot {
    tabs: BTreeMap<ItemId, DockspaceReceiverDescriptor>,
    centers: BTreeMap<ItemId, LogicalPoint>,
    panes: BTreeMap<ItemId, LogicalRect>,
    inner_guide_slots: Vec<usize>,
    splitters: Vec<(bool, bool)>,
    outer: Vec<(Direction, LogicalRect)>,
    previews: Vec<LogicalRect>,
}

impl Snapshot {
    fn outer(&self, direction: Direction) -> LogicalPoint {
        center(
            self.outer
                .iter()
                .find(|(side, _)| *side == direction)
                .expect("a visible root retains its outer edge targets")
                .1,
        )
    }
}

fn assert_geometry(rect: LogicalRect) {
    for scalar in [
        rect.x(),
        rect.y(),
        rect.width(),
        rect.height(),
        rect.max().x(),
        rect.max().y(),
    ] {
        assert!(scalar.is_finite(), "non-finite geometry: {rect:?}");
    }
    assert!(rect.width() >= 0.0 && rect.height() >= 0.0, "{rect:?}");
}

fn paint_present(session: &mut DockspaceSession) -> BTreeMap<SurfaceId, Snapshot> {
    let mut snapshots = BTreeMap::new();
    let mut frame = session.begin_host_frame().unwrap();
    for surface in [SOURCE, TARGET] {
        let Some(plan) = frame.paint_plan(surface).unwrap() else {
            continue;
        };
        let mut snapshot = Snapshot::default();
        plan.visit_surface_program(|step| {
            match step {
                Step::Operation {
                    operation: Operation::Tab(tab),
                    ..
                } => {
                    if let Some(receiver) = plan.receiver_for_tab_body(tab) {
                        snapshot.tabs.insert(tab.item(), receiver);
                    }
                    if let Some(receiver) = plan.center_drop_receiver_for_item(tab.item()) {
                        snapshot.centers.insert(tab.item(), receiver.center());
                    }
                }
                Step::Operation {
                    operation: Operation::Pane(pane),
                    ..
                } => {
                    if let Some(item) = pane.selected() {
                        snapshot.panes.insert(item, pane.bounds());
                    }
                }
                Step::Operation {
                    operation: Operation::Splitter(splitter),
                    ..
                } => snapshot.splitters.push((
                    splitter.operable(),
                    plan.receiver_for_splitter(splitter).is_some(),
                )),
                Step::DropGuide { structural, .. } if structural.scope() == Scope::Outer => {
                    snapshot.outer.extend(
                        structural
                            .targets()
                            .map(|target| (target.direction(), target.hit_bounds())),
                    );
                }
                Step::DropGuide { structural, .. } if structural.scope() == Scope::Inner => {
                    snapshot
                        .inner_guide_slots
                        .push(structural.targets().count());
                }
                Step::DragPreview(preview) => {
                    if let DockspacePreviewVisual::Dock {
                        surface: owner,
                        rect,
                        ..
                    } = preview.visual()
                    {
                        assert_eq!(owner, surface);
                        snapshot.previews.push(rect);
                    }
                }
                _ => {}
            }
            Ok::<(), ()>(())
        })
        .unwrap();
        let (recording, receipt) = RecordingRenderer::execute(plan).unwrap();
        for record in recording.records {
            match record {
                OwnedRecord::Geometry { bounds, .. } | OwnedRecord::PaneContent { bounds, .. } => {
                    assert_geometry(bounds);
                }
                OwnedRecord::Preview(DockspacePreviewVisual::Dock { rect, .. }) => {
                    assert_geometry(rect)
                }
                _ => {}
            }
        }
        frame.confirm_surface_painted(receipt).unwrap();
        snapshots.insert(surface, snapshot);
    }
    let outputs = commit(frame).take_painted_outputs();
    assert_eq!(outputs.len(), snapshots.len());
    for output in outputs {
        session
            .report_surface_presentation(output, SurfacePresentationResult::Presented)
            .unwrap();
    }
    commit(session.begin_host_frame().unwrap());
    snapshots
}

fn assert_items(session: &DockspaceSession, expected: &[ItemId]) {
    let mut actual = Vec::new();
    for surface in session.view().surfaces() {
        for root in surface
            .main_root()
            .into_iter()
            .chain(surface.contained().filter_map(|floating| floating.root()))
        {
            let mut nodes = vec![root.content().unwrap()];
            while let Some(node) = nodes.pop() {
                if let Some(tabs) = node.tabs() {
                    actual.extend_from_slice(tabs.items());
                } else {
                    nodes.extend(node.split().unwrap().children());
                }
            }
        }
    }
    actual.sort();
    let mut expected = expected.to_vec();
    expected.sort();
    assert_eq!(
        actual, expected,
        "every item must have exactly one owning leaf"
    );
}

fn press(session: &mut DockspaceSession, descriptor: DockspaceReceiverDescriptor) {
    let presented = session.presented_surface(SOURCE).unwrap();
    let receiver = session.bind_presented_receiver(&descriptor).unwrap();
    let route = HostPointerRoute::new(presented, receiver.center());
    let mut frame = session.begin_host_frame().unwrap();
    frame
        .submit_host_pointer(
            HostPointerInput::new(
                POINTER,
                Event::ButtonPressed(Button::Primary),
                route,
                route,
                SurfacePointerCapture::ProviderEndpoint,
                SurfacePointerReceiverFacts::delivery(&receiver).with_clear_hover_lane(&presented),
            )
            .with_metadata(PointerEventMetadata::untimed(PointerDeviceKind::Mouse)),
        )
        .unwrap();
    commit(frame);
    assert!(session.has_active_gesture());
}

fn target_edge(session: &mut DockspaceSession, event: Event, aim: LogicalPoint) -> HostFrameReport {
    let source = session.presented_surface(SOURCE).unwrap();
    let target = session.presented_surface(TARGET).unwrap();
    let mut frame = session.begin_host_frame().unwrap();
    frame
        .submit_host_pointer(
            HostPointerInput::new(
                POINTER,
                event,
                HostPointerRoute::new(source, point(850.0, 550.0)),
                HostPointerRoute::new(target, aim),
                SurfacePointerCapture::ProviderEndpoint,
                SurfacePointerReceiverFacts::unknown().with_clear_hover_lane(&target),
            )
            .with_metadata(PointerEventMetadata::untimed(PointerDeviceKind::Mouse)),
        )
        .unwrap();
    commit(frame)
}

#[test]
fn repeated_edge_splits_then_center_merges_survive_minimum_and_title_overflow() {
    for policy in [PaneOverflowPolicy::Clip, PaneOverflowPolicy::Compress] {
        repeated_splits_and_merges(policy);
    }
}

fn repeated_splits_and_merges(policy: PaneOverflowPolicy) {
    let items = (1..=8).map(ItemId::new).collect::<Vec<_>>();
    let expected = items
        .iter()
        .copied()
        .chain([SOURCE_ANCHOR, TARGET_ANCHOR])
        .collect::<Vec<_>>();
    let compact = bounds(180.0, 130.0);
    let mut session = session(&items, Node::central_tabs([TARGET_ANCHOR]));
    overflow_policy(&mut session, policy);
    for (index, &item) in items.iter().enumerate() {
        let mut frame = session.begin_host_frame().unwrap();
        frame.select_item_current(item).unwrap();
        commit(frame);
        measure(&mut session, compact, minimum);
        let snapshots = paint_present(&mut session);
        let direction = [Direction::Right, Direction::Bottom][index % 2];
        let aim = snapshots[&TARGET].outer(direction);
        press(&mut session, snapshots[&SOURCE].tabs[&item]);
        target_edge(&mut session, Event::Moved, aim);
        let preview = paint_present(&mut session)[&TARGET].previews.clone();
        assert_eq!(
            preview.len(),
            1,
            "edge split {index} must retain an exact preview"
        );
        assert!(
            target_edge(&mut session, Event::ButtonReleased(Button::Primary), aim)
                .workspace_changed()
        );
        assert!(!session.has_active_gesture());
        assert_eq!(session.view().item(item).unwrap().surface(), TARGET);
        measure(&mut session, compact, minimum);
        let snapshots = paint_present(&mut session);
        assert_eq!(preview[0], snapshots[&TARGET].panes[&item]);
        assert_items(&session, &expected);
    }

    // Restore enough room to operate every clipped leaf, then merge the groups.
    measure(&mut session, bounds(2400.0, 1800.0), minimum);
    let restored = paint_present(&mut session);
    assert!(
        restored[&TARGET]
            .panes
            .values()
            .all(|rect| rect.width() > 0.0 && rect.height() > 0.0)
    );
    for (index, &item) in items.iter().enumerate() {
        let mut frame = session.begin_host_frame().unwrap();
        frame
            .dock_item_current(
                item,
                dockspace::model::DockPlacement::Center(dockspace::model::DockAnchor::Item(
                    TARGET_ANCHOR,
                )),
            )
            .unwrap();
        assert!(commit(frame).workspace_changed());
        measure(&mut session, compact, minimum);
        let snapshots = paint_present(&mut session);
        assert_eq!(snapshots[&TARGET].panes.len(), items.len() - index);
        if policy == PaneOverflowPolicy::Clip {
            assert!(
                snapshots[&TARGET].tabs.contains_key(&item),
                "the selected overflowing tab must remain reachable: merge={index}",
            );
        } else {
            assert!(
                snapshots[&TARGET]
                    .panes
                    .values()
                    .all(|rect| { rect.width() > 0.0 && rect.height() > 0.0 })
            );
        }
        assert_items(&session, &expected);
        // Nine compressed panes can be smaller than their tab chrome. Enlarging
        // the host must restore the selected tab without changing the layout.
        let version = session.version();
        measure(&mut session, bounds(2400.0, 1800.0), minimum);
        let recovered = paint_present(&mut session);
        assert!(recovered[&TARGET].tabs.contains_key(&item));
        assert_eq!(session.version(), version);
    }
    measure(&mut session, compact, minimum);
    let merged = paint_present(&mut session);
    assert!(merged[&TARGET].tabs.contains_key(items.last().unwrap()));
    assert_eq!(
        session
            .view()
            .item(TARGET_ANCHOR)
            .unwrap()
            .tabs()
            .items()
            .len(),
        items.len() + 1
    );
}

#[test]
fn compact_two_leaf_root_keeps_complete_inner_guide_geometry() {
    let ordinary = ItemId::new(1);
    for axis in [Axis::Horizontal, Axis::Vertical] {
        for (x, y, width, height) in [
            (0.0, 0.0, 180.0, 130.0),
            (17.0, 29.0, 180.0, 130.0),
            (13.25, -37.5, 179.5, 129.75),
            (-320.5, -240.25, 97.125, 72.875),
            (23.75, -4.5, 32.75, 120.5),
            (-0.5, 11.125, 240.25, 31.5),
        ] {
            let target = Node::split(
                axis,
                [
                    (Node::tabs([ordinary]), 0.2),
                    (Node::central_tabs([TARGET_ANCHOR]), 0.8),
                ],
            )
            .unwrap();
            let mut session = session(&[], target);
            let viewport = LogicalRect::new(x, y, width, height).unwrap();
            measure(&mut session, viewport, minimum);
            let snapshot = paint_present(&mut session);
            assert_eq!(snapshot[&TARGET].panes.len(), 2, "{axis:?}, {viewport:?}");
            assert!(
                snapshot[&TARGET].inner_guide_slots.contains(&5),
                "the ordinary pane must keep all five guide slots: {axis:?}, {viewport:?}"
            );
            assert!(
                session.presented_surface(TARGET).is_some(),
                "{axis:?}, {viewport:?}"
            );
            assert_items(&session, &[SOURCE_ANCHOR, TARGET_ANCHOR, ordinary]);
        }
    }
}

#[test]
fn constrained_layout_keeps_every_pane_reachable_and_restores_original_weights() {
    let items = [ItemId::new(1), ItemId::new(2), TARGET_ANCHOR];
    let target = Node::split(
        Axis::Horizontal,
        [
            (Node::tabs([items[0]]), 0.2),
            (Node::tabs([items[1]]), 0.3),
            (Node::central_tabs([items[2]]), 0.5),
        ],
    )
    .unwrap();
    let mut session = session(&[], target);
    overflow_policy(&mut session, PaneOverflowPolicy::Compress);
    let original = session.version();
    let weights = || {
        session
            .view()
            .root(RootId::new(2))
            .unwrap()
            .content()
            .unwrap()
            .split()
            .unwrap()
            .weights()
            .collect::<Vec<_>>()
    };
    let original_weights = weights();
    let compact = bounds(240.0, 180.0);
    measure(&mut session, compact, |_| {
        LogicalSize::new(200.0, 90.0).unwrap()
    });
    let snapshot = paint_present(&mut session);
    assert_eq!(snapshot[&TARGET].splitters, [(false, false); 2]);
    for item in items {
        let pane = snapshot[&TARGET].panes[&item];
        assert!(
            pane.width() > 0.0 && pane.height() > 0.0,
            "a constrained pane must remain reachable: {item:?}, {pane:?}"
        );
        assert!(snapshot[&TARGET].tabs.contains_key(&item));
    }
    measure(&mut session, bounds(1200.0, 700.0), |_| {
        LogicalSize::new(200.0, 90.0).unwrap()
    });
    let restored = paint_present(&mut session);
    assert_eq!(restored[&TARGET].splitters, [(true, true); 2]);
    assert_eq!(session.version(), original);
    assert_eq!(
        session
            .view()
            .root(RootId::new(2))
            .unwrap()
            .content()
            .unwrap()
            .split()
            .unwrap()
            .weights()
            .collect::<Vec<_>>(),
        original_weights
    );
    assert!(restored[&TARGET].panes[&items[2]].width() > 500.0);
    assert_items(&session, &[SOURCE_ANCHOR, items[0], items[1], items[2]]);
}

#[test]
fn zero_area_target_revokes_drop_preview_and_recovers_without_losing_the_drag_source() {
    let item = ItemId::new(1);
    let second = ItemId::new(201);
    let third = ItemId::new(202);
    let target = Node::split(
        Axis::Horizontal,
        [
            (Node::tabs([TARGET_ANCHOR]), 0.2),
            (
                Node::equal_split(
                    Axis::Vertical,
                    [Node::tabs([second]), Node::central_tabs([third])],
                )
                .unwrap(),
                0.8,
            ),
        ],
    )
    .unwrap();
    let mut session = session(&[item], target);
    let expected = [item, SOURCE_ANCHOR, TARGET_ANCHOR, second, third];
    measure(&mut session, bounds(900.0, 600.0), minimum);
    let snapshots = paint_present(&mut session);
    press(&mut session, snapshots[&SOURCE].tabs[&item]);
    target_edge(
        &mut session,
        Event::Moved,
        snapshots[&TARGET].outer(Direction::Right),
    );
    assert_eq!(paint_present(&mut session)[&TARGET].previews.len(), 1);
    for collapsed in [bounds(0.0, 0.0), bounds(0.0, 100.0), bounds(100.0, 0.0)] {
        measure(&mut session, collapsed, minimum);
        assert!(!paint_present(&mut session).contains_key(&TARGET));
        assert!(session.presented_surface(TARGET).is_none());
        assert!(session.host_pointer_hit(TARGET, point(0.0, 0.0)).is_none());
        let source = session.presented_surface(SOURCE).unwrap();
        let mut frame = session.begin_host_frame().unwrap();
        frame
            .submit_host_pointer(HostPointerInput::new(
                POINTER,
                Event::Moved,
                HostPointerRoute::new(source, point(850.0, 550.0)),
                HostPointerRoute::Unknown,
                SurfacePointerCapture::ProviderEndpoint,
                SurfacePointerReceiverFacts::unknown(),
            ))
            .unwrap();
        assert!(!commit(frame).workspace_changed());
        assert!(
            paint_present(&mut session)
                .values()
                .all(|snapshot| snapshot.previews.is_empty())
        );
        assert_items(&session, &expected);
    }
    let mut frame = session.begin_host_frame().unwrap();
    frame
        .submit_host_pointer(HostPointerInput::new(
            POINTER,
            Event::StreamCancelled(SurfacePointerCancelReason::ExplicitCancellation),
            HostPointerRoute::Unknown,
            HostPointerRoute::Unknown,
            SurfacePointerCapture::ProviderEndpoint,
            SurfacePointerReceiverFacts::unknown(),
        ))
        .unwrap();
    assert!(!commit(frame).workspace_changed());
    assert!(!session.has_active_gesture());
    assert_eq!(session.view().item(item).unwrap().surface(), SOURCE);

    measure(&mut session, bounds(3.0, 7.0), minimum);
    let tiny = paint_present(&mut session);
    assert_eq!(tiny[&TARGET].panes.len(), 3);
    assert!(
        tiny[&TARGET]
            .panes
            .values()
            .any(|rect| rect.width() == 0.0 || rect.height() == 0.0)
    );
    assert_items(&session, &expected);
    measure(&mut session, bounds(900.0, 600.0), minimum);
    let restored = paint_present(&mut session);
    assert!(
        restored[&TARGET]
            .panes
            .values()
            .all(|rect| rect.width() > 0.0 && rect.height() > 0.0)
    );
    let aim = restored[&TARGET].centers[&second];
    press(&mut session, restored[&SOURCE].tabs[&item]);
    target_edge(&mut session, Event::Moved, aim);
    assert_eq!(paint_present(&mut session)[&TARGET].previews.len(), 1);
    assert!(
        target_edge(&mut session, Event::ButtonReleased(Button::Primary), aim).workspace_changed()
    );
    assert_eq!(
        session.view().item(item).unwrap().tabs().items(),
        [second, item]
    );
    assert!(!session.has_active_gesture());
    assert_items(&session, &expected);
}

#[test]
fn drag_retargets_after_the_viewport_shrinks_and_the_incoming_minimum_changes() {
    let item = ItemId::new(1);
    let mut session = session(&[item], Node::central_tabs([TARGET_ANCHOR]));
    measure(&mut session, bounds(700.0, 500.0), minimum);
    let snapshots = paint_present(&mut session);
    press(&mut session, snapshots[&SOURCE].tabs[&item]);
    target_edge(
        &mut session,
        Event::Moved,
        snapshots[&TARGET].outer(Direction::Right),
    );
    assert_eq!(paint_present(&mut session)[&TARGET].previews.len(), 1);
    let changed = |candidate| {
        if candidate == Some(item) {
            LogicalSize::new(512.0, 384.0).unwrap()
        } else {
            minimum(candidate)
        }
    };
    let small = bounds(96.0, 72.0);
    measure(&mut session, small, changed);
    let resized = paint_present(&mut session);
    assert!(session.has_active_gesture());
    let aim = resized[&TARGET].outer(Direction::Right);
    target_edge(&mut session, Event::Moved, aim);
    let preview = paint_present(&mut session)[&TARGET].previews.clone();
    assert_eq!(preview.len(), 1);
    assert!(
        target_edge(&mut session, Event::ButtonReleased(Button::Primary), aim).workspace_changed()
    );
    measure(&mut session, small, changed);
    let final_scene = paint_present(&mut session);
    assert_eq!(preview[0], final_scene[&TARGET].panes[&item]);
    assert_eq!(session.view().item(item).unwrap().surface(), TARGET);
    assert!(!session.has_active_gesture());
    assert_items(&session, &[item, SOURCE_ANCHOR, TARGET_ANCHOR]);
}
