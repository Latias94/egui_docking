//! Touch delivery activates its source when the ordinary release update commits.

use std::time::Duration;

use dockspace::geometry::{LogicalPoint, LogicalRect, LogicalSize};
use dockspace::model::{
    DockspaceContainedLayout, DockspaceLayout, DockspaceNode, DockspaceRootLayout,
    DockspaceSurfaceLayout, FloatingPresentationId, ItemId, RootId, SurfaceId, WorkspaceVersion,
};
use dockspace::policy::DockPolicy;
use dockspace::runtime::{
    ContainedResizeDirection, DockspaceHostFrame, DockspaceManipulationDelivery as Delivery,
    DockspaceManipulationStatus as Status, DockspacePaneFocusRequest, DockspaceReceiverDescriptor,
    DockspaceReceiverRole, DockspaceSession, DockspaceTouchEvent, HostFrameReport,
    HostInputOutcome, HostPointerInput, HostPointerRoute, PaintedSurfaceOutput, PointerDeviceKind,
    PointerEventMetadata, SurfacePaintProgramOperation as Operation,
    SurfacePaintProgramStep as Step, SurfacePointerButton as Button, SurfacePointerCapture,
    SurfacePointerEvent as Event, SurfacePointerId, SurfacePointerReceiverFacts,
    SurfacePresentationResult as Disposition, SurfaceUnavailableReason, UniformSurfaceMetrics,
};
use dockspace_host_harness::{OwnedRecord, RecordingRenderer};

const SURFACE: SurfaceId = SurfaceId::new(1);
const FIRST: ItemId = ItemId::new(1);
const INACTIVE: ItemId = ItemId::new(2);
const THIRD: ItemId = ItemId::new(3);
const REAR: FloatingPresentationId = FloatingPresentationId::new(1);
const FRONT: FloatingPresentationId = FloatingPresentationId::new(2);
const POINTER: SurfacePointerId = SurfacePointerId::new(1);

fn rect(x: f64, y: f64, width: f64, height: f64) -> LogicalRect {
    LogicalRect::new(x, y, width, height).unwrap()
}

#[derive(Debug, PartialEq)]
struct DurableState {
    version: WorkspaceVersion,
    tabs: Vec<ItemId>,
    selected: Option<ItemId>,
    contained: Vec<(FloatingPresentationId, LogicalRect)>,
}

fn durable(session: &DockspaceSession) -> DurableState {
    let view = session.view();
    let tabs = view.item(FIRST).unwrap().tabs();
    DurableState {
        version: session.version(),
        tabs: tabs.items().to_vec(),
        selected: tabs.selected(),
        contained: view
            .surface(SURFACE)
            .unwrap()
            .contained()
            .map(|contained| (contained.id(), contained.rect()))
            .collect(),
    }
}

fn commit(mut frame: DockspaceHostFrame<'_>) -> HostFrameReport {
    frame
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .unwrap();
    frame.commit().unwrap()
}

fn settle(session: &mut DockspaceSession) -> HostFrameReport {
    commit(session.begin_host_frame().unwrap())
}

fn measure(session: &mut DockspaceSession) {
    let mut frame = session.begin_host_frame().unwrap();
    frame
        .measure_surface(
            SURFACE,
            UniformSurfaceMetrics::new(
                rect(0.0, 0.0, 960.0, 720.0),
                LogicalSize::new(64.0, 48.0).unwrap(),
                96.0,
            )
            .unwrap(),
        )
        .unwrap();
    assert!(!commit(frame).workspace_changed());
}

struct Recorded {
    renderer: RecordingRenderer,
    tab: Option<DockspaceReceiverDescriptor>,
    before_first_tab: Option<LogicalPoint>,
    rear_resize: Option<DockspaceReceiverDescriptor>,
    focus: Option<DockspacePaneFocusRequest>,
}

fn metrics() -> UniformSurfaceMetrics {
    UniformSurfaceMetrics::new(
        rect(0.0, 0.0, 960.0, 720.0),
        LogicalSize::new(64.0, 48.0).unwrap(),
        96.0,
    )
    .unwrap()
}

fn paint(session: &mut DockspaceSession) -> (Recorded, PaintedSurfaceOutput) {
    let mut frame = session.begin_host_frame().unwrap();
    if frame.paint_plan(SURFACE).unwrap().is_none() {
        frame.measure_surface(SURFACE, metrics()).unwrap();
    }
    let plan = frame.paint_plan(SURFACE).unwrap().unwrap();
    let tab = plan.tab_receiver(INACTIVE);
    let focus = plan.pane_focus_request();
    let mut before_first_tab = None;
    let mut rear_resize = None;
    plan.visit_surface_program(|step| {
        match step {
            Step::Operation {
                operation: Operation::Tab(tab),
                ..
            } if tab.item() == FIRST => {
                let bounds = tab.visible_bounds();
                // The leading half of the first tab is the same-group gap at index zero.
                before_first_tab = Some(
                    LogicalPoint::new(
                        bounds.x() + bounds.width() * 0.25,
                        bounds.y() + bounds.height() * 0.5,
                    )
                    .unwrap(),
                );
            }
            Step::RootBegin { root, .. } => {
                if let Some(contained) = root.contained()
                    && contained.floating() == REAR
                {
                    let handle = contained
                        .resize()
                        .find(|handle| handle.direction() == ContainedResizeDirection::SouthEast)
                        .unwrap();
                    rear_resize = plan.receiver_for_contained_resize(contained, handle);
                }
            }
            _ => {}
        }
        Ok::<(), ()>(())
    })
    .unwrap();
    let (renderer, receipt) = RecordingRenderer::execute(plan).unwrap();
    frame.confirm_surface_painted(receipt).unwrap();
    let mut report = commit(frame);
    assert!(!report.workspace_changed());
    let mut outputs = report.take_painted_outputs();
    assert_eq!(outputs.len(), 1);
    let output = outputs.pop().unwrap();
    assert!(matches!(
        renderer.records.last(),
        Some(OwnedRecord::SurfaceEnd(semantic)) if output.matches_semantic_output(*semantic)
    ));
    (
        Recorded {
            renderer,
            tab,
            before_first_tab,
            rear_resize,
            focus,
        },
        output,
    )
}

fn present(
    session: &mut DockspaceSession,
    output: PaintedSurfaceOutput,
    disposition: Disposition,
) -> HostFrameReport {
    let artifact = output.presentation_artifact();
    let before = durable(session);
    session
        .report_surface_presentation(output, disposition)
        .unwrap();
    assert_eq!(
        durable(session),
        before,
        "a host report alone cannot deliver"
    );
    let report = settle(session);
    let matching = report
        .surface_presentation_commits()
        .iter()
        .filter(|commit| commit.settles(artifact))
        .collect::<Vec<_>>();
    assert_eq!(matching.len(), 1, "the exact recorded output must settle");
    if disposition == Disposition::Presented {
        if !report.workspace_changed() {
            assert!(matching[0].presents(artifact));
        } else {
            assert!(matching[0].settles(artifact));
        }
    } else {
        assert!(matching[0].retires(artifact));
    }
    report
}

fn fixture(source_in_rear: bool) -> (DockspaceSession, Recorded) {
    let tabs = || DockspaceNode::tabs([FIRST, INACTIVE, THIRD]);
    let separate = || DockspaceNode::tabs([ItemId::new(4)]);
    let layout = DockspaceLayout::new([DockspaceSurfaceLayout::new(
        SURFACE,
        DockspaceRootLayout::new(
            RootId::new(1),
            if source_in_rear { separate() } else { tabs() },
        ),
    )
    .with_contained(DockspaceContainedLayout::new(
        REAR,
        DockspaceRootLayout::new(
            RootId::new(2),
            if source_in_rear { tabs() } else { separate() },
        ),
        if source_in_rear {
            rect(180.0, 180.0, 480.0, 300.0)
        } else {
            rect(180.0, 180.0, 300.0, 240.0)
        },
    ))
    .with_contained(DockspaceContainedLayout::new(
        FRONT,
        DockspaceRootLayout::new(RootId::new(3), DockspaceNode::tabs([ItemId::new(5)])),
        if source_in_rear {
            // Overlap the rear body while leaving its full tab strip exposed.
            rect(100.0, 360.0, 240.0, 180.0)
        } else {
            rect(100.0, 120.0, 240.0, 180.0)
        },
    ))])
    .unwrap();
    let mut session = DockspaceSession::from_layout(layout, DockPolicy::default()).unwrap();
    session.enable_host_pointer().unwrap();
    assert_eq!(
        session.presentation_config().touch_long_press_delay(),
        Duration::from_millis(350)
    );
    assert_eq!(
        session.presentation_config().touch_press_slop_distance(),
        8.0
    );
    measure(&mut session);
    let (recorded, output) = paint(&mut session);
    assert!(recorded.focus.is_none());
    assert!(!present(&mut session, output, Disposition::Presented).workspace_changed());
    (session, recorded)
}

fn edge(
    session: &mut DockspaceSession,
    event: Event,
    point: LogicalPoint,
    millis: u64,
) -> HostFrameReport {
    let hit = session.host_pointer_hit(SURFACE, point).unwrap();
    let surface = hit.surface();
    let click = hit.delivery();
    let drag = hit.drag_delivery();
    let hover = hit.hover();
    let facts = match click.as_ref() {
        Some(receiver) => SurfacePointerReceiverFacts::delivery(receiver),
        None => SurfacePointerReceiverFacts::unknown().with_empty_delivery_lane(&surface),
    };
    let facts = match drag.as_ref() {
        Some(receiver) => facts.with_drag_delivery(receiver),
        None => facts.with_empty_drag_delivery_lane(&surface),
    };
    let facts = match hover.as_ref() {
        Some(receiver) => facts.with_hover(receiver),
        None => facts.with_empty_hover_lane(&surface),
    };
    let route = HostPointerRoute::new(surface, point);
    let mut frame = session.begin_host_frame().unwrap();
    frame
        .submit_host_pointer(
            HostPointerInput::new(
                POINTER,
                event,
                route,
                route,
                SurfacePointerCapture::ProviderEndpoint,
                facts,
            )
            .with_metadata(PointerEventMetadata::new(
                PointerDeviceKind::Touch,
                Duration::from_millis(millis),
            )),
        )
        .unwrap();
    commit(frame)
}

fn arm_touch(session: &mut DockspaceSession, point: LogicalPoint) {
    let before = durable(session);
    let pressed = edge(session, Event::ButtonPressed(Button::Primary), point, 0);
    assert!(!pressed.workspace_changed());
    assert_eq!(durable(session), before);
    assert!(session.has_active_gesture());
    let wake = session
        .pointer_continuation()
        .expect("the touch press must schedule its real long-press continuation");
    assert_eq!(wake.pointer(), POINTER);
    assert_eq!(wake.surface(), SURFACE);
    assert_eq!(wake.position(), point);
    assert_eq!(wake.deadline(), Duration::from_millis(350));
    let armed = edge(session, wake.event(), wake.position(), 350);
    assert_eq!(
        armed.inputs(),
        &[HostInputOutcome::Touch(DockspaceTouchEvent::Armed {
            pointer: POINTER,
            surface: SURFACE,
        })]
    );
    assert!(!armed.workspace_changed());
    assert_eq!(durable(session), before);
    assert!(session.has_active_gesture());
    assert!(session.pointer_continuation().is_none());
}

#[test]
fn inactive_tab_reorder_selects_and_requests_focus_at_release_commit() {
    for (source_in_rear, disposition) in [
        (false, Disposition::Presented),
        (false, Disposition::Dropped),
        (true, Disposition::Presented),
        (true, Disposition::Dropped),
    ] {
        let (mut session, initial) = fixture(source_in_rear);
        assert_eq!(
            session.view().item(INACTIVE).unwrap().contained(),
            source_in_rear.then_some(REAR)
        );
        let source = initial.tab.unwrap();
        assert!(
            initial
                .renderer
                .records
                .contains(&OwnedRecord::Receiver(source))
        );
        assert!(session.bind_presented_receiver(&source).is_some());
        let target = initial.before_first_tab.unwrap();
        assert!((source.center().x() - target.x()).abs() > 8.0);
        assert_eq!(
            session
                .host_pointer_hit(SURFACE, target)
                .unwrap()
                .hover()
                .unwrap()
                .descriptor()
                .role(),
            DockspaceReceiverRole::DropTarget
        );
        let before = durable(&session);
        assert_eq!(before.tabs, [FIRST, INACTIVE, THIRD]);
        assert_eq!(before.selected, Some(FIRST));
        assert_eq!(
            before
                .contained
                .iter()
                .map(|(id, _)| *id)
                .collect::<Vec<_>>(),
            [REAR, FRONT]
        );
        arm_touch(&mut session, source.center());
        let moved = edge(&mut session, Event::Moved, target, 360);
        assert!(!moved.workspace_changed());
        assert_eq!(durable(&session), before);
        let released = edge(
            &mut session,
            Event::ContactEnded(Button::Primary),
            target,
            370,
        );
        assert!(released.workspace_changed());
        assert_eq!(released.before(), before.version);
        assert_eq!(released.after(), session.version());
        assert!(!session.has_active_gesture());
        let [terminal] = released.manipulation_feedback() else {
            panic!("release must deliver the reorder once: {released:?}");
        };
        assert_eq!(terminal.status(), Status::Delivered(Delivery::Dock));
        let after = durable(&session);
        assert_eq!(after.tabs, [INACTIVE, FIRST, THIRD]);
        assert_eq!(after.selected, Some(INACTIVE));
        if source_in_rear {
            assert_eq!(
                after.contained,
                [
                    (FRONT, before.contained[1].1),
                    (REAR, before.contained[0].1)
                ]
            );
        } else {
            assert_eq!(after.contained, before.contained);
        }
        assert_ne!(after.version, before.version);
        let (recorded, output) = paint(&mut session);
        let focus = recorded
            .focus
            .expect("delivered inactive tab requests focus immediately in the successor output");
        assert_eq!(focus.surface(), SURFACE);
        assert_eq!(focus.item(), Some(INACTIVE));
        assert!(recorded.renderer.records.iter().all(|record| !matches!(
            record,
            OwnedRecord::Preview(_) | OwnedRecord::TransformPreview(..)
        )));
        let settled = present(&mut session, output, disposition);
        assert!(!settled.workspace_changed());
        assert!(settled.manipulation_feedback().is_empty());
        assert_eq!(durable(&session), after);
        let (_, output) = paint(&mut session);
        let later = present(&mut session, output, Disposition::Presented);
        assert!(later.manipulation_feedback().is_empty());
        assert_eq!(durable(&session), after);
    }
}

#[test]
fn rear_contained_touch_resize_raises_at_release_commit() {
    for disposition in [Disposition::Presented, Disposition::Dropped] {
        let (mut session, initial) = fixture(false);
        let resize = initial
            .rear_resize
            .expect("the rear resize handle remains exposed");
        assert!(session.bind_presented_receiver(&resize).is_some());
        assert_eq!(
            session
                .host_pointer_hit(SURFACE, resize.center())
                .unwrap()
                .drag_delivery()
                .unwrap()
                .descriptor(),
            resize
        );
        let before = durable(&session);
        assert_eq!(
            before
                .contained
                .iter()
                .map(|(id, _)| *id)
                .collect::<Vec<_>>(),
            [REAR, FRONT]
        );
        let source_rect = before.contained[0].1;
        let target =
            LogicalPoint::new(resize.center().x() + 40.0, resize.center().y() + 24.0).unwrap();
        let expected = rect(
            source_rect.x(),
            source_rect.y(),
            source_rect.width() + 40.0,
            source_rect.height() + 24.0,
        );
        arm_touch(&mut session, resize.center());
        let moved = edge(&mut session, Event::Moved, target, 360);
        assert!(!moved.workspace_changed());
        assert_eq!(durable(&session), before);
        let released = edge(
            &mut session,
            Event::ContactEnded(Button::Primary),
            target,
            370,
        );
        assert!(released.workspace_changed());
        assert_eq!(released.before(), before.version);
        assert_eq!(released.after(), session.version());
        assert!(!session.has_active_gesture());
        let [terminal] = released.manipulation_feedback() else {
            panic!("resize release must deliver once: {released:?}");
        };
        assert_eq!(
            terminal.status(),
            Status::Delivered(Delivery::ContainedTransform)
        );
        let after = durable(&session);
        assert_eq!(
            after.contained,
            [(FRONT, before.contained[1].1), (REAR, expected)]
        );
        assert_eq!(after.tabs, before.tabs);
        assert_eq!(after.selected, before.selected);
        assert_ne!(after.version, before.version);
        let (recorded, output) = paint(&mut session);
        assert!(recorded.renderer.records.iter().all(|record| !matches!(
            record,
            OwnedRecord::Preview(_) | OwnedRecord::TransformPreview(..)
        )));
        let settled = present(&mut session, output, disposition);
        assert!(!settled.workspace_changed());
        assert!(settled.manipulation_feedback().is_empty());
        assert_eq!(durable(&session), after);
        let (_, output) = paint(&mut session);
        let later = present(&mut session, output, Disposition::Presented);
        assert!(later.manipulation_feedback().is_empty());
        assert_eq!(durable(&session), after);
    }
}

#[test]
fn cross_root_touch_dock_does_not_require_unrelated_destination_raise_permission() {
    let (mut session, _) = fixture(false);
    let mut policy = DockPolicy::default();
    policy.set_allow_contained_transform(false);
    let mut frame = session.begin_host_frame().unwrap();
    frame.replace_policy(policy).unwrap();
    commit(frame);
    measure(&mut session);
    let (initial, output) = paint(&mut session);
    present(&mut session, output, Disposition::Presented);
    let source = initial.tab.unwrap();
    let destination = ItemId::new(4);
    let target = {
        let mut frame = session.begin_host_frame().unwrap();
        frame
            .paint_plan(SURFACE)
            .unwrap()
            .unwrap()
            .center_drop_receiver_for_item(destination)
            .unwrap()
            .center()
    };
    let before = durable(&session);
    let target_root = session.view().item(destination).unwrap().root();
    assert_ne!(target_root, session.view().item(INACTIVE).unwrap().root());
    assert_eq!(
        session.view().item(destination).unwrap().contained(),
        Some(REAR)
    );
    arm_touch(&mut session, source.center());
    assert!(!edge(&mut session, Event::Moved, target, 360).workspace_changed());
    let release = edge(
        &mut session,
        Event::ContactEnded(Button::Primary),
        target,
        370,
    );
    assert!(release.workspace_changed());
    assert!(
        matches!(release.manipulation_feedback(), [terminal] if terminal.status() == Status::Delivered(Delivery::Dock))
    );
    let delivered_version = session.version();
    let (_, output) = paint(&mut session);
    let settled = present(&mut session, output, Disposition::Presented);
    assert!(!settled.workspace_changed());
    assert!(settled.manipulation_feedback().is_empty());
    assert_eq!(session.version(), delivered_version);
    let after = durable(&session);
    assert_eq!(after.contained, before.contained);
    assert_eq!(session.view().item(INACTIVE).unwrap().root(), target_root);
    assert!(session.view().item(INACTIVE).unwrap().is_selected());
    assert!(!session.has_active_gesture());
    assert!(session.pointer_continuation().is_none());
}
