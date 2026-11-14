#![cfg(feature = "motion")]

use std::collections::BTreeMap;
use std::time::Duration;

use dockspace::geometry::{LogicalPoint, LogicalRect, LogicalSize};
use dockspace::model::{
    DockAnchor, DockEdge, DockFraction, DockPlacement, DockspaceAxis, DockspaceContainedLayout,
    DockspaceLayout, DockspaceNode, DockspaceRootLayout, DockspaceSurfaceLayout,
    FloatingPresentationId, ItemId, RootId, SurfaceId,
};
use dockspace::motion::{MotionFrame, MotionPreference, MotionSettings};
use dockspace::policy::DockPolicy;
use dockspace::runtime::{
    DockspaceManipulationStatus, DockspaceReceiverDescriptor, DockspaceSession, HostFrameReport,
    HostPointerInput, HostPointerRoute, PaintedSurfaceOutput, PointerDeviceKind,
    PointerEventMetadata, SurfacePaintProgramOperation as Operation,
    SurfacePaintProgramStep as Step, SurfacePointerButton, SurfacePointerCapture,
    SurfacePointerEvent, SurfacePointerId, SurfacePointerReceiverFacts, SurfacePresentationResult,
    SurfaceUnavailableReason, UniformSurfaceMetrics,
};

const SURFACE: SurfaceId = SurfaceId::new(1);
const ROOT: RootId = RootId::new(1);
const A: ItemId = ItemId::new(1);
const B: ItemId = ItemId::new(2);
const C: ItemId = ItemId::new(3);

fn metrics() -> UniformSurfaceMetrics {
    UniformSurfaceMetrics::new(
        LogicalRect::new(0.0, 0.0, 960.0, 600.0).unwrap(),
        LogicalSize::new(40.0, 30.0).unwrap(),
        80.0,
    )
    .unwrap()
}

fn motion(now: u64) -> MotionFrame {
    MotionFrame {
        now: Duration::from_millis(now),
        settings: MotionSettings::default(),
        preference: MotionPreference::Full,
    }
}

struct Sample {
    panes: BTreeMap<ItemId, LogicalRect>,
    receivers: BTreeMap<ItemId, DockspaceReceiverDescriptor>,
    partition: bool,
    splitters: usize,
    splitter_receivers: Vec<(DockspaceReceiverDescriptor, LogicalRect)>,
    tab_offsets: Vec<f64>,
}

fn paint(session: &mut DockspaceSession, motion: MotionFrame) -> (Sample, PaintedSurfaceOutput) {
    let mut frame = session.begin_host_frame().unwrap();
    frame.set_motion(motion).unwrap();
    frame.measure_surface(SURFACE, metrics()).unwrap();
    let plan = frame.paint_plan(SURFACE).unwrap().unwrap();
    let mut sample = Sample {
        panes: BTreeMap::new(),
        receivers: BTreeMap::new(),
        partition: plan.partition_available(ROOT),
        splitters: 0,
        splitter_receivers: Vec::new(),
        tab_offsets: Vec::new(),
    };
    plan.visit_surface_program(|step| {
        match step {
            Step::Operation {
                operation: Operation::Pane(pane),
                ..
            } => {
                if let Some(item) = pane.selected() {
                    sample.panes.insert(item, pane.bounds());
                    sample
                        .receivers
                        .insert(item, plan.receiver_for_pane(pane).unwrap());
                }
            }
            Step::Operation {
                operation: Operation::TabBar(bar),
                ..
            } => {
                sample.tab_offsets.push(bar.scroll_offset());
            }
            Step::Operation {
                operation: Operation::Splitter(splitter),
                ..
            } => {
                let receiver = plan.receiver_for_splitter(splitter).unwrap();
                sample
                    .splitter_receivers
                    .push((receiver, splitter.draw_bounds()));
                sample.splitters += 1;
            }
            _ => {}
        }
        Ok::<(), ()>(())
    })
    .unwrap();
    let receipt = plan.execute_surface_program(|_| Ok::<(), ()>(())).unwrap();
    frame.confirm_surface_painted(receipt).unwrap();
    frame
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .unwrap();
    let mut report = frame.commit().unwrap();
    (sample, report.take_painted_outputs().pop().unwrap())
}

fn settle(session: &mut DockspaceSession) {
    let mut frame = session.begin_host_frame().unwrap();
    frame
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .unwrap();
    frame.commit().unwrap();
}

fn present(session: &mut DockspaceSession, output: PaintedSurfaceOutput) {
    session
        .report_surface_presentation(output, SurfacePresentationResult::Presented)
        .unwrap();
    settle(session);
}

fn fixture() -> (DockspaceSession, Sample) {
    let content = DockspaceNode::equal_split(
        DockspaceAxis::Horizontal,
        [A, B, C].map(|item| DockspaceNode::tabs([item])),
    )
    .unwrap();
    let layout = DockspaceLayout::new([DockspaceSurfaceLayout::new(
        SURFACE,
        DockspaceRootLayout::new(ROOT, content),
    )])
    .unwrap();
    let mut session = DockspaceSession::from_layout(layout, DockPolicy::default()).unwrap();
    let (before, output) = paint(&mut session, motion(0));
    present(&mut session, output);
    (session, before)
}

fn dock(session: &mut DockspaceSession, item: ItemId, after: ItemId) {
    let mut frame = session.begin_host_frame().unwrap();
    frame
        .dock_item_current(item, DockPlacement::After(after))
        .unwrap();
    frame
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .unwrap();
    assert!(frame.commit().unwrap().workspace_changed());
}

#[test]
fn independent_host_uses_core_sampled_geometry_and_restores_structural_receivers() {
    let (mut session, before) = fixture();
    dock(&mut session, C, A);
    let (start, output) = paint(&mut session, motion(1000));
    assert_eq!(start.panes[&B], before.panes[&B]);
    assert!(!start.partition);
    assert_eq!(start.splitters, 0);
    present(&mut session, output);
    let (middle, output) = paint(&mut session, motion(1050));
    assert_ne!(middle.panes[&B], before.panes[&B]);
    assert!(!middle.partition);
    assert!(session.surface_motion_is_active(SURFACE));
    present(&mut session, output);
    let (end, output) = paint(&mut session, motion(6000));
    assert!(end.partition);
    assert_eq!(end.splitters, 1);
    assert!(!session.surface_motion_is_active(SURFACE));
    present(&mut session, output);
    assert!(
        session
            .bind_presented_receiver(&end.receivers[&B])
            .is_some()
    );
}

const PREVIEW: ItemId = ItemId::new(20);
const INSPECTOR: ItemId = ItemId::new(21);
const HISTORY: ItemId = ItemId::new(4);
const FLOATING: FloatingPresentationId = FloatingPresentationId::new(1);
const FLOATING_ROOT: RootId = RootId::new(2);
const POINTER: SurfacePointerId = SurfacePointerId::new(1);

fn nested_editor() -> DockspaceSession {
    let center = DockspaceNode::split(
        DockspaceAxis::Vertical,
        [
            (DockspaceNode::central_tabs([A, HISTORY]), 0.74),
            (DockspaceNode::tabs([B]), 0.26),
        ],
    )
    .unwrap();
    let main = DockspaceNode::split(
        DockspaceAxis::Horizontal,
        [(DockspaceNode::tabs([C]), 0.20), (center, 0.80)],
    )
    .unwrap();
    let layout = DockspaceLayout::new([DockspaceSurfaceLayout::new(
        SURFACE,
        DockspaceRootLayout::new(ROOT, main),
    )
    .with_contained(DockspaceContainedLayout::new(
        FLOATING,
        DockspaceRootLayout::new(
            FLOATING_ROOT,
            DockspaceNode::central_tabs_with_selection([PREVIEW, INSPECTOR], Some(INSPECTOR))
                .unwrap(),
        ),
        LogicalRect::new(580.0, 80.0, 300.0, 320.0).unwrap(),
    ))])
    .unwrap();
    let mut session = DockspaceSession::from_layout(layout, DockPolicy::default()).unwrap();
    session.enable_host_pointer().unwrap();
    let (_, output) = paint(&mut session, motion(0));
    present(&mut session, output);
    session
}

fn pointer_edge(
    session: &mut DockspaceSession,
    event: SurfacePointerEvent,
    point: LogicalPoint,
    now: u64,
    trace: &str,
) -> HostFrameReport {
    let hit = session
        .host_pointer_hit(SURFACE, point)
        .unwrap_or_else(|| panic!("{trace}: the painted point has no presented surface"));
    let surface = hit.surface();
    let delivery = hit.delivery();
    let drag_delivery = hit.drag_delivery();
    let mut facts = delivery.as_ref().map_or_else(
        || SurfacePointerReceiverFacts::unknown().with_clear_hover_lane(&surface),
        SurfacePointerReceiverFacts::delivery,
    );
    if let Some(receiver) = drag_delivery.as_ref() {
        facts = facts.with_drag_delivery(receiver);
    }
    let capture = if matches!(
        event,
        SurfacePointerEvent::ButtonReleased(SurfacePointerButton::Primary)
    ) {
        SurfacePointerCapture::None
    } else {
        SurfacePointerCapture::ProviderEndpoint
    };
    let mut frame = session.begin_host_frame().expect(trace);
    frame.set_motion(motion(now)).expect(trace);
    frame
        .submit_host_pointer(
            HostPointerInput::new(
                POINTER,
                event,
                HostPointerRoute::new(surface, point),
                HostPointerRoute::new(surface, point),
                capture,
                facts,
            )
            .with_metadata(PointerEventMetadata::new(
                PointerDeviceKind::Mouse,
                Duration::from_millis(now),
            )),
        )
        .expect(trace);
    frame
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .expect(trace);
    frame.commit().expect(trace)
}

fn assert_drained_editor(session: &DockspaceSession, trace: &str) {
    let view = session.view();
    assert!(view.contained(FLOATING).is_none(), "{trace}");
    assert!(view.root(FLOATING_ROOT).is_none(), "{trace}");
    assert_eq!(
        view.surface(SURFACE).unwrap().contained_count(),
        0,
        "{trace}"
    );
    for item in [A, B, C, HISTORY, PREVIEW, INSPECTOR] {
        let location = view.item(item).expect(trace);
        assert_eq!(location.root(), ROOT, "{trace}: {item:?}");
        assert_eq!(location.surface(), SURFACE, "{trace}: {item:?}");
    }
}

#[test]
fn drained_contained_central_tabs_keep_nested_resize_usable_across_motion_presentations() {
    for edge in [None, Some(DockEdge::Left)] {
        for interrupt in [false, true] {
            if edge.is_none() && interrupt {
                continue;
            }
            let trace = format!("edge={edge:?}, interrupt={interrupt}");
            let mut session = nested_editor();
            for (step, (item, target)) in [(PREVIEW, A), (INSPECTOR, B)].into_iter().enumerate() {
                let now = u64::try_from(step).unwrap() * 16;
                let placement =
                    edge.map_or(DockPlacement::Center(DockAnchor::Item(target)), |edge| {
                        DockPlacement::InnerEdge {
                            anchor: DockAnchor::Item(target),
                            edge,
                            fraction: DockFraction::new(0.35).unwrap(),
                        }
                    });
                let mut frame = session.begin_host_frame().expect(&trace);
                frame.set_motion(motion(now)).expect(&trace);
                frame.dock_item_current(item, placement).expect(&trace);
                frame
                    .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
                    .expect(&trace);
                assert!(frame.commit().expect(&trace).workspace_changed(), "{trace}");
                let (_, output) = paint(&mut session, motion(now));
                present(&mut session, output);
            }
            assert_drained_editor(&session, &trace);
            let (moving, output) = paint(&mut session, motion(32));
            let animating = session.surface_motion_is_active(SURFACE);
            assert_eq!(animating, edge.is_some(), "{trace}");
            present(&mut session, output);

            if interrupt {
                // Animated layouts intentionally expose no structural splitter.
                // A real attempt at the visible pane boundary must still end cleanly.
                assert!(moving.splitter_receivers.is_empty(), "{trace}");
                let outline = moving.panes[&C];
                let start = LogicalPoint::new(
                    outline.max().x() + 1.0,
                    outline.y() + outline.height() * 0.5,
                )
                .unwrap();
                let end = LogicalPoint::new(start.x() + 20.0, start.y()).unwrap();
                let version = session.version();
                for (now, event, point) in [
                    (
                        33,
                        SurfacePointerEvent::ButtonPressed(SurfacePointerButton::Primary),
                        start,
                    ),
                    (34, SurfacePointerEvent::Moved, end),
                    (
                        35,
                        SurfacePointerEvent::ButtonReleased(SurfacePointerButton::Primary),
                        end,
                    ),
                ] {
                    let report = pointer_edge(&mut session, event, point, now, &trace);
                    assert!(!report.workspace_changed(), "{trace}: {report:?}");
                    let (_, output) = paint(&mut session, motion(now));
                    present(&mut session, output);
                }
                assert_eq!(session.version(), version, "{trace}");
                assert!(!session.has_active_gesture(), "{trace}");
            }

            let settled_at = if animating { 1000 } else { 40 };
            let (settled, output) = paint(&mut session, motion(settled_at));
            present(&mut session, output);
            assert!(!session.surface_motion_is_active(SURFACE), "{trace}");
            assert!(settled.splitters >= 2, "{trace}");
            for index in 0..settled.splitters {
                let now = settled_at + 100 + u64::try_from(index).unwrap() * 64;
                let (before, output) = paint(&mut session, motion(now));
                present(&mut session, output);
                let (descriptor, bounds) = before.splitter_receivers[index];
                let start = descriptor.center();
                let horizontal = bounds.height() > bounds.width();
                let end = LogicalPoint::new(
                    start.x() + if horizontal { 24.0 } else { 0.0 },
                    start.y() + if horizontal { 0.0 } else { 24.0 },
                )
                .unwrap();
                for (offset, event, point) in [
                    (
                        1,
                        SurfacePointerEvent::ButtonPressed(SurfacePointerButton::Primary),
                        start,
                    ),
                    (17, SurfacePointerEvent::Moved, end),
                    (
                        33,
                        SurfacePointerEvent::ButtonReleased(SurfacePointerButton::Primary),
                        end,
                    ),
                ] {
                    let report = pointer_edge(&mut session, event, point, now + offset, &trace);
                    assert!(
                        !report
                            .manipulation_feedback()
                            .iter()
                            .any(|feedback| matches!(
                                feedback.status(),
                                DockspaceManipulationStatus::Cancelled(_)
                                    | DockspaceManipulationStatus::Rejected(_)
                            )),
                        "{trace}, splitter={index}: {report:?}"
                    );
                    let (_, output) = paint(&mut session, motion(now + offset));
                    assert!(!session.surface_motion_is_active(SURFACE), "{trace}");
                    present(&mut session, output);
                }
                let (after, output) = paint(&mut session, motion(now + 40));
                present(&mut session, output);
                assert_ne!(before.panes, after.panes, "{trace}, splitter={index}");
                assert!(!session.has_active_gesture(), "{trace}, splitter={index}");
                assert!(!session.has_pending_presentation(), "{trace}");
                assert_drained_editor(&session, &trace);
            }
        }
    }
}
