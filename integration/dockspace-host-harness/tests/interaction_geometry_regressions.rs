//! Public host regressions for projected docking and splitter hit geometry.

use std::collections::BTreeMap;

use dockspace::geometry::{LogicalPoint, LogicalRect, LogicalSize};
use dockspace::model::{
    DockspaceAxis as Axis, DockspaceContainedLayout, DockspaceLayout, DockspaceNode as Node,
    DockspaceRootLayout, DockspaceSurfaceLayout, FloatingPresentationId, ItemId, RootId, SurfaceId,
};
use dockspace::policy::{
    DockPolicy, DockSurfaceRule, TabBarInteraction, TabBarPolicy, TabBarVisibility,
};
use dockspace::runtime::*;

const SOURCE: SurfaceId = SurfaceId::new(1);
const TARGET: SurfaceId = SurfaceId::new(2);
const A: ItemId = ItemId::new(1);
const Z: ItemId = ItemId::new(2);
const B: ItemId = ItemId::new(10);
const C: ItemId = ItemId::new(11);
const FLOATING: FloatingPresentationId = FloatingPresentationId::new(1);
const POINTER: SurfacePointerId = SurfacePointerId::new(1);

fn rect(x: f64, y: f64, width: f64, height: f64) -> LogicalRect {
    LogicalRect::new(x, y, width, height).unwrap()
}

fn point(x: f64, y: f64) -> LogicalPoint {
    LogicalPoint::new(x, y).unwrap()
}

fn size(width: f64, height: f64) -> LogicalSize {
    LogicalSize::new(width, height).unwrap()
}

fn center(bounds: LogicalRect) -> LogicalPoint {
    point(
        bounds.x() + bounds.width() * 0.5,
        bounds.y() + bounds.height() * 0.5,
    )
}

fn finish(mut frame: DockspaceHostFrame<'_>) -> HostFrameReport {
    frame
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .unwrap();
    frame.commit().unwrap()
}

fn new_session(
    layout: DockspaceLayout,
    policy: DockPolicy,
    config: DockPresentationConfig,
) -> DockspaceSession {
    let mut session =
        DockspaceSession::from_layout_with_presentation_config(layout, policy, config).unwrap();
    session.enable_host_pointer().unwrap();
    session
}

fn measure(
    session: &mut DockspaceSession,
    bounds: &[(SurfaceId, LogicalRect)],
    minimum: impl Fn(Option<ItemId>) -> LogicalSize,
) {
    let mut frame = session.begin_host_frame().unwrap();
    for &(surface, bounds) in bounds {
        frame
            .measure_surface_with(surface, |question| match question {
                SurfaceMeasurementRequest::DockBounds { .. }
                | SurfaceMeasurementRequest::PopupPlaneBounds { .. } => {
                    SurfaceMeasurementAnswer::Bounds(bounds)
                }
                SurfaceMeasurementRequest::PaneMinimum { item, .. } => {
                    SurfaceMeasurementAnswer::PaneMinimum(minimum(item))
                }
                SurfaceMeasurementRequest::TabIntrinsic { .. } => {
                    SurfaceMeasurementAnswer::TabIntrinsic(96.0)
                }
                SurfaceMeasurementRequest::TabStrip { .. } => {
                    SurfaceMeasurementAnswer::TabStrip(TabStripMetrics::new(0.0, 0.0).unwrap())
                }
            })
            .unwrap();
    }
    finish(frame);
}

#[derive(Clone, Copy)]
struct SplitterVisual {
    visual_id: DockspaceVisualId,
    draw_bounds: LogicalRect,
    hit_bounds: LogicalRect,
    receiver: DockspaceReceiverDescriptor,
}

#[derive(Clone, Copy)]
struct JunctionVisual {
    visual_id: DockspaceVisualId,
    hit_bounds: LogicalRect,
    arm_count: usize,
    receiver: DockspaceReceiverDescriptor,
}

#[derive(Default)]
struct Snapshot {
    title: Option<DockspaceReceiverDescriptor>,
    tabs: BTreeMap<ItemId, DockspaceReceiverDescriptor>,
    tab_bars: Vec<LogicalRect>,
    panes: BTreeMap<ItemId, LogicalRect>,
    gaps: Vec<DockspaceReceiverDescriptor>,
    splitters: Vec<SplitterVisual>,
    junctions: Vec<JunctionVisual>,
    guides: Vec<(
        RootId,
        DockspaceGuideScope,
        DockspaceDropDirection,
        LogicalRect,
    )>,
    previews: Vec<LogicalRect>,
}

fn render_present(session: &mut DockspaceSession) -> BTreeMap<SurfaceId, Snapshot> {
    let mut frame = session.begin_host_frame().unwrap();
    let mut snapshots = BTreeMap::new();
    for surface in frame.surfaces() {
        let Some(plan) = frame.paint_plan(surface).unwrap() else {
            continue;
        };
        let mut snapshot = Snapshot::default();
        let receipt = plan
            .execute_surface_program(|step| {
                match step {
                    SurfacePaintProgramStep::Operation { operation, .. } => match operation {
                        SurfacePaintProgramOperation::ContainedControls(record) => {
                            snapshot.title = plan.receiver_for_contained_title(record);
                        }
                        SurfacePaintProgramOperation::Tab(record) => {
                            if let Some(receiver) = plan.receiver_for_tab_body(record) {
                                snapshot.tabs.insert(record.item(), receiver);
                            }
                        }
                        SurfacePaintProgramOperation::TabBar(record) => {
                            snapshot.tab_bars.push(record.bounds());
                        }
                        SurfacePaintProgramOperation::Pane(record) => {
                            if let Some(item) = record.selected() {
                                snapshot.panes.insert(item, record.bounds());
                            }
                        }
                        SurfacePaintProgramOperation::Splitter(record) => {
                            if let Some(receiver) = plan.receiver_for_splitter(record) {
                                snapshot.splitters.push(SplitterVisual {
                                    visual_id: record.visual_id(),
                                    draw_bounds: record.draw_bounds(),
                                    hit_bounds: record.hit_bounds(),
                                    receiver,
                                });
                            }
                        }
                        SurfacePaintProgramOperation::SplitterJunction(record) => {
                            if let Some(receiver) = plan.receiver_for_splitter_junction(record) {
                                snapshot.junctions.push(JunctionVisual {
                                    visual_id: record.visual_id(),
                                    hit_bounds: record.hit_bounds(),
                                    arm_count: record.arm_count(),
                                    receiver,
                                });
                            }
                        }
                        _ => {}
                    },
                    SurfacePaintProgramStep::DropTarget(receiver) => snapshot.gaps.push(receiver),
                    SurfacePaintProgramStep::DropGuide { structural, .. } => {
                        snapshot.guides.extend(structural.targets().map(|target| {
                            (
                                structural.root(),
                                structural.scope(),
                                target.direction(),
                                target.hit_bounds(),
                            )
                        }));
                    }
                    SurfacePaintProgramStep::DragPreview(preview) => {
                        if let DockspacePreviewVisual::Dock { rect, .. } = preview.visual() {
                            snapshot.previews.push(rect);
                        }
                    }
                    _ => {}
                }
                Ok::<(), ()>(())
            })
            .unwrap();
        frame.confirm_surface_painted(receipt).unwrap();
        snapshots.insert(surface, snapshot);
    }
    let outputs = finish(frame).take_painted_outputs();
    for output in outputs {
        session
            .report_surface_presentation(output, SurfacePresentationResult::Presented)
            .unwrap();
    }
    finish(session.begin_host_frame().unwrap());
    snapshots
}

fn paint_visual_for_receiver(
    snapshot: &Snapshot,
    receiver: DockspaceReceiverDescriptor,
) -> Option<DockspaceVisualId> {
    snapshot
        .splitters
        .iter()
        .find(|visual| visual.receiver == receiver)
        .map(|visual| visual.visual_id)
        .or_else(|| {
            snapshot
                .junctions
                .iter()
                .find(|visual| visual.receiver == receiver)
                .map(|visual| visual.visual_id)
        })
}

fn resolved_resize_visual(
    session: &DockspaceSession,
    snapshot: &Snapshot,
    position: LogicalPoint,
    device: PointerDeviceKind,
) -> Option<DockspaceVisualId> {
    let hit = session.host_pointer_hit_for_device(SOURCE, position, device)?;
    let receiver = hit.drag_delivery()?.descriptor();
    paint_visual_for_receiver(snapshot, receiver)
}

fn expected_base_resize_visual(
    snapshot: &Snapshot,
    position: LogicalPoint,
) -> Option<DockspaceVisualId> {
    let nearest_junction = snapshot
        .junctions
        .iter()
        .filter(|visual| visual.hit_bounds.contains(position))
        .min_by(|first, second| {
            distance_squared(center(first.hit_bounds), position)
                .total_cmp(&distance_squared(center(second.hit_bounds), position))
                .then_with(|| first.visual_id.cmp(&second.visual_id))
        });
    if let Some(junction) = nearest_junction {
        return Some(junction.visual_id);
    }

    snapshot
        .splitters
        .iter()
        .filter(|visual| visual.hit_bounds.contains(position))
        .min_by(|first, second| {
            distance_squared_to_rect(first.draw_bounds, position)
                .total_cmp(&distance_squared_to_rect(second.draw_bounds, position))
                .then_with(|| first.visual_id.cmp(&second.visual_id))
        })
        .map(|visual| visual.visual_id)
}

fn distance_squared(first: LogicalPoint, second: LogicalPoint) -> f64 {
    let dx = first.x() - second.x();
    let dy = first.y() - second.y();
    dx * dx + dy * dy
}

fn distance_squared_to_rect(bounds: LogicalRect, point: LogicalPoint) -> f64 {
    let dx = if point.x() < bounds.x() {
        bounds.x() - point.x()
    } else if point.x() > bounds.max().x() {
        point.x() - bounds.max().x()
    } else {
        0.0
    };
    let dy = if point.y() < bounds.y() {
        bounds.y() - point.y()
    } else if point.y() > bounds.max().y() {
        point.y() - bounds.max().y()
    } else {
        0.0
    };
    dx * dx + dy * dy
}

fn input<'a>(
    event: SurfacePointerEvent,
    source: PresentedDockspaceSurface,
    source_point: LogicalPoint,
    target: PresentedDockspaceSurface,
    target_point: LogicalPoint,
    facts: SurfacePointerReceiverFacts<'a>,
    millis: u64,
) -> HostPointerInput<'a> {
    HostPointerInput::new(
        POINTER,
        event,
        HostPointerRoute::new(source, source_point),
        HostPointerRoute::new(target, target_point),
        SurfacePointerCapture::ProviderEndpoint,
        facts,
    )
    .with_metadata(PointerEventMetadata::new(
        PointerDeviceKind::Unknown,
        std::time::Duration::from_millis(millis),
    ))
}

fn press(
    session: &mut DockspaceSession,
    surface: SurfaceId,
    descriptor: DockspaceReceiverDescriptor,
) {
    let receiver = session.bind_presented_receiver(&descriptor).unwrap();
    let presented = session.presented_surface(surface).unwrap();
    let mut frame = session.begin_host_frame().unwrap();
    frame
        .submit_host_pointer(input(
            SurfacePointerEvent::ButtonPressed(SurfacePointerButton::Primary),
            presented,
            receiver.center(),
            presented,
            receiver.center(),
            SurfacePointerReceiverFacts::delivery(&receiver).with_clear_hover_lane(&presented),
            0,
        ))
        .unwrap();
    finish(frame);
    assert!(session.has_active_gesture());
}

#[test]
fn hidden_draggable_source_requests_exact_intrinsic_and_reaches_visible_gap() {
    let layout = DockspaceLayout::new([
        DockspaceSurfaceLayout::rootless(SOURCE).with_contained(DockspaceContainedLayout::new(
            FLOATING,
            DockspaceRootLayout::new(RootId::new(1), Node::tabs([A])),
            rect(40.0, 40.0, 400.0, 300.0),
        )),
        DockspaceSurfaceLayout::new(
            TARGET,
            DockspaceRootLayout::new(RootId::new(2), Node::central_tabs([B, C])),
        ),
    ])
    .unwrap();
    let mut policy = DockPolicy::default();
    let mut source_rule = DockSurfaceRule::new();
    source_rule.set_tab_bar(TabBarPolicy::new(
        TabBarVisibility::Hidden,
        TabBarInteraction::Enabled,
    ));
    policy.set_surface_rule(SOURCE, source_rule);
    let mut session = new_session(layout, policy, DockPresentationConfig::default());

    let mut saw_hidden_source_intrinsic = false;
    let mut frame = session.begin_host_frame().unwrap();
    for (surface, bounds) in [
        (SOURCE, rect(0.0, 0.0, 800.0, 600.0)),
        (TARGET, rect(0.0, 0.0, 400.0, 300.0)),
    ] {
        frame
            .measure_surface_with(surface, |question| match question {
                SurfaceMeasurementRequest::DockBounds { .. }
                | SurfaceMeasurementRequest::PopupPlaneBounds { .. } => {
                    SurfaceMeasurementAnswer::Bounds(bounds)
                }
                SurfaceMeasurementRequest::PaneMinimum { .. } => {
                    SurfaceMeasurementAnswer::PaneMinimum(size(64.0, 48.0))
                }
                SurfaceMeasurementRequest::TabIntrinsic { item, .. } => {
                    saw_hidden_source_intrinsic |= surface == SOURCE && item == A;
                    SurfaceMeasurementAnswer::TabIntrinsic(96.0)
                }
                SurfaceMeasurementRequest::TabStrip { .. } => {
                    SurfaceMeasurementAnswer::TabStrip(TabStripMetrics::new(0.0, 0.0).unwrap())
                }
            })
            .unwrap();
    }
    finish(frame);
    assert!(
        saw_hidden_source_intrinsic,
        "hidden draggable source must be measured"
    );

    let snapshots = render_present(&mut session);
    assert!(!snapshots[&SOURCE].tabs.contains_key(&A));
    let source = snapshots[&SOURCE].title.unwrap();
    let gap = snapshots[&TARGET]
        .gaps
        .iter()
        .find(|receiver| receiver.bounds().y() == 0.0 && receiver.bounds().height() == 28.0)
        .unwrap();
    let aim = gap.center();
    press(&mut session, SOURCE, source);
    let source_surface = session.presented_surface(SOURCE).unwrap();
    let target_surface = session.presented_surface(TARGET).unwrap();
    let mut frame = session.begin_host_frame().unwrap();
    let result = frame.submit_host_pointer(input(
        SurfacePointerEvent::Moved,
        source_surface,
        point(700.0, 500.0),
        target_surface,
        aim,
        SurfacePointerReceiverFacts::unknown().with_clear_hover_lane(&target_surface),
        10,
    ));
    assert!(
        result.is_ok(),
        "missing projected geometry must not poison input: {result:?}"
    );
    finish(frame);
    assert_eq!(render_present(&mut session)[&TARGET].previews.len(), 1);
}

#[test]
fn overflowing_edge_preview_matches_the_final_clipped_pane() {
    let bounds = [
        (SOURCE, rect(0.0, 0.0, 800.0, 600.0)),
        (TARGET, rect(0.0, 0.0, 200.0, 300.0)),
    ];
    let layout = DockspaceLayout::new([
        DockspaceSurfaceLayout::new(
            SOURCE,
            DockspaceRootLayout::new(RootId::new(1), Node::central_tabs([A, Z])),
        ),
        DockspaceSurfaceLayout::new(
            TARGET,
            DockspaceRootLayout::new(RootId::new(2), Node::central_tabs([B])),
        ),
    ])
    .unwrap();
    let mut session = new_session(
        layout,
        DockPolicy::default(),
        DockPresentationConfig::default(),
    );
    let minimum = |item| {
        if item == Some(A) {
            size(300.0, 48.0)
        } else {
            size(64.0, 48.0)
        }
    };
    measure(&mut session, &bounds, minimum);
    let snapshots = render_present(&mut session);
    let source = snapshots[&SOURCE].tabs[&A];
    let aim = center(
        snapshots[&TARGET]
            .guides
            .iter()
            .find(|(_, scope, direction, _)| {
                *scope == DockspaceGuideScope::Outer && *direction == DockspaceDropDirection::Right
            })
            .unwrap()
            .3,
    );
    press(&mut session, SOURCE, source);
    let source_surface = session.presented_surface(SOURCE).unwrap();
    let target_surface = session.presented_surface(TARGET).unwrap();
    let mut frame = session.begin_host_frame().unwrap();
    frame
        .submit_host_pointer(input(
            SurfacePointerEvent::Moved,
            source_surface,
            point(700.0, 500.0),
            target_surface,
            aim,
            SurfacePointerReceiverFacts::unknown().with_clear_hover_lane(&target_surface),
            10,
        ))
        .unwrap();
    finish(frame);
    let preview = render_present(&mut session)[&TARGET].previews[0];

    let source_surface = session.presented_surface(SOURCE).unwrap();
    let target_surface = session.presented_surface(TARGET).unwrap();
    let mut frame = session.begin_host_frame().unwrap();
    frame
        .submit_host_pointer(input(
            SurfacePointerEvent::ButtonReleased(SurfacePointerButton::Primary),
            source_surface,
            point(700.0, 500.0),
            target_surface,
            aim,
            SurfacePointerReceiverFacts::unknown().with_clear_hover_lane(&target_surface),
            20,
        ))
        .unwrap();
    finish(frame);
    measure(&mut session, &bounds, minimum);
    let final_pane = render_present(&mut session)[&TARGET].panes[&A];
    assert_eq!(session.view().item(A).unwrap().surface(), TARGET);
    assert_eq!(preview, final_pane);
}

#[test]
fn nested_parallel_mouse_bands_keep_the_drawn_outer_splitter_reachable() {
    let inner = Node::split(
        Axis::Horizontal,
        [(Node::tabs([A]), 0.95), (Node::tabs([Z]), 0.05)],
    )
    .unwrap();
    let left = Node::equal_split(Axis::Vertical, [inner, Node::tabs([B])]).unwrap();
    let root = Node::equal_split(Axis::Horizontal, [left, Node::tabs([C])]).unwrap();
    let layout = DockspaceLayout::new([DockspaceSurfaceLayout::new(
        SOURCE,
        DockspaceRootLayout::new(RootId::new(1), root),
    )])
    .unwrap();
    let config = DockPresentationConfig::builder()
        .minimum_pane_size(10.0, 10.0)
        .splitter_hit_extent(64.0)
        .build()
        .unwrap();
    let mut session = new_session(layout, DockPolicy::default(), config);
    measure(
        &mut session,
        &[(SOURCE, rect(0.0, 0.0, 400.0, 400.0))],
        |_| size(10.0, 10.0),
    );
    let snapshot = render_present(&mut session);
    let snapshot = &snapshot[&SOURCE];
    let expected = snapshot
        .splitters
        .iter()
        .find(|visual| visual.draw_bounds.height() > 300.0)
        .unwrap();
    let aim = point(center(expected.draw_bounds).x(), 100.0);
    assert!(expected.draw_bounds.contains(aim));
    assert_eq!(
        resolved_resize_visual(&session, snapshot, aim, PointerDeviceKind::Mouse),
        Some(expected.visual_id)
    );
}

#[test]
fn near_coincident_t_junctions_keep_mouse_and_touch_resize_receivers() {
    let left = Node::equal_split(Axis::Vertical, [Node::tabs([A]), Node::tabs([Z])]).unwrap();
    let right = Node::split(
        Axis::Vertical,
        [(Node::tabs([B]), 0.501), (Node::tabs([C]), 0.499)],
    )
    .unwrap();
    let root = Node::equal_split(Axis::Horizontal, [left, right]).unwrap();
    let layout = DockspaceLayout::new([DockspaceSurfaceLayout::new(
        SOURCE,
        DockspaceRootLayout::new(RootId::new(1), root),
    )])
    .unwrap();
    let mut session = new_session(
        layout,
        DockPolicy::default(),
        DockPresentationConfig::default(),
    );
    measure(
        &mut session,
        &[(SOURCE, rect(0.0, 0.0, 800.0, 600.0))],
        |_| size(64.0, 48.0),
    );
    let snapshots = render_present(&mut session);
    let snapshot = &snapshots[&SOURCE];
    let splitters = &snapshot.splitters;
    let vertical = splitters
        .iter()
        .copied()
        .find(|visual| visual.draw_bounds.height() > 500.0)
        .unwrap();
    let left_horizontal = splitters
        .iter()
        .copied()
        .find(|visual| visual.draw_bounds.x() == 0.0 && visual.draw_bounds.width() > 300.0)
        .unwrap();
    let right_horizontal = splitters
        .iter()
        .copied()
        .find(|visual| {
            visual.draw_bounds.x() > vertical.draw_bounds.x()
                && visual.draw_bounds.width() > 300.0
                && visual.draw_bounds.height() < visual.draw_bounds.width()
        })
        .unwrap();

    // Recursive projection keeps painted splitter bases disjoint. Perpendicular
    // child handles meet the parent at two exact, near-coincident junctions.
    for (index, first) in splitters.iter().enumerate() {
        for second in splitters.iter().skip(index + 1) {
            let overlaps_x = first.draw_bounds.x() < second.draw_bounds.max().x()
                && second.draw_bounds.x() < first.draw_bounds.max().x();
            let overlaps_y = first.draw_bounds.y() < second.draw_bounds.max().y()
                && second.draw_bounds.y() < first.draw_bounds.max().y();
            assert!(
                !overlaps_x || !overlaps_y,
                "painted splitter bases overlap: {:?} and {:?}",
                first.visual_id,
                second.visual_id
            );
        }
    }
    assert_eq!(snapshot.junctions.len(), 2);
    assert!(
        snapshot
            .junctions
            .iter()
            .all(|junction| junction.arm_count == 3)
    );
    let mut junctions = snapshot.junctions.clone();
    junctions.sort_by(|first, second| {
        center(first.hit_bounds)
            .y()
            .total_cmp(&center(second.hit_bounds).y())
    });
    let first_junction_center = center(junctions[0].hit_bounds);
    let second_junction_center = center(junctions[1].hit_bounds);
    assert!((first_junction_center.y() - second_junction_center.y()).abs() < 1.0);
    for (junction, horizontal) in junctions.iter().zip([left_horizontal, right_horizontal]) {
        assert!((center(junction.hit_bounds).x() - center(vertical.draw_bounds).x()).abs() < 1e-9);
        assert!(
            (center(junction.hit_bounds).y() - center(horizontal.draw_bounds).y()).abs() < 1e-9
        );
    }

    let center_x = center(vertical.draw_bounds).x();
    let x_samples = [
        vertical.draw_bounds.x() - 0.25,
        vertical.draw_bounds.x(),
        center_x,
        vertical.draw_bounds.max().x() - 0.25,
        vertical.draw_bounds.max().x(),
    ];
    for junction in &junctions {
        let junction_y = center(junction.hit_bounds).y();
        for offset in [-2.0, -1.0, 0.0, 1.0, 2.0] {
            for x in x_samples {
                let position = point(x, junction_y + offset);
                let expected = expected_base_resize_visual(snapshot, position)
                    .expect("the paint-derived grid must stay inside a splitter receiver");
                for device in [PointerDeviceKind::Mouse, PointerDeviceKind::Touch] {
                    assert_eq!(
                        resolved_resize_visual(&session, snapshot, position, device),
                        Some(expected),
                        "wrong or missing paint receiver at {position:?} for {device:?}"
                    );
                }
            }
        }
    }

    let y = first_junction_center.y() - 12.0;
    for x in [center_x - 12.0, center_x + 12.0] {
        let position = point(x, y);
        assert_eq!(
            resolved_resize_visual(&session, snapshot, position, PointerDeviceKind::Touch),
            Some(junctions[0].visual_id),
            "the upper touch halo must belong to the nearer junction at {position:?}"
        );
    }
    for junction in &junctions {
        let y = center(junction.hit_bounds).y() + 12.0;
        for x in [center_x - 12.0, center_x + 12.0] {
            let position = point(x, y);
            assert!(snapshot.tab_bars.iter().any(|bar| bar.contains(position)));
            assert_eq!(
                resolved_resize_visual(&session, snapshot, position, PointerDeviceKind::Touch),
                None,
                "the expanded halo must preserve tab chrome at {position:?}"
            );
        }
    }

    for junction in &junctions {
        let junction_y = center(junction.hit_bounds).y();
        for y in [junction_y - 30.0, junction_y + 30.0] {
            let position = point(center_x, y);
            for device in [PointerDeviceKind::Mouse, PointerDeviceKind::Touch] {
                assert_eq!(
                    resolved_resize_visual(&session, snapshot, position, device),
                    Some(vertical.visual_id),
                    "the vertical arm must remain reachable at {position:?} for {device:?}"
                );
            }
        }
    }
}
