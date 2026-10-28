//! Device-specific resize targets through public input and exact output settlement.

use std::time::Duration;

use dockspace::geometry::{LogicalPoint, LogicalRect, LogicalSize};
use dockspace::model::{
    DockspaceAxis, DockspaceContainedLayout, DockspaceLayout, DockspaceNode, DockspaceNodeView,
    DockspaceRootLayout, DockspaceSurfaceLayout, FloatingPresentationId, ItemId, RootId, SurfaceId,
    WorkspaceVersion,
};
use dockspace::policy::DockPolicy;
use dockspace::runtime::{
    ContainedResizeDirection, DockspaceHostFrame, DockspaceManipulationDelivery as Delivery,
    DockspaceManipulationPendingReason as PendingReason, DockspaceManipulationStatus as Status,
    DockspaceReceiverDescriptor, DockspaceReceiverRole as Role, DockspaceRuntimeErrorKind,
    DockspaceSession, DockspaceTouchEvent, HostFrameReport, HostInputOutcome, HostPointerInput,
    HostPointerRoute, PaintedSurfaceOutput, PointerDeviceKind as Device, PointerEventMetadata,
    PresentedDockReceiver, PresentedDockspaceSurface, SurfaceMeasurementAnswer as Answer,
    SurfaceMeasurementRequest as Question, SurfacePaintPlan,
    SurfacePaintProgramOperation as Operation, SurfacePaintProgramStep as Step,
    SurfacePointerButton as Button, SurfacePointerCapture, SurfacePointerEvent as Event,
    SurfacePointerId, SurfacePointerReceiverFacts, SurfacePresentationResult as Disposition,
    SurfaceUnavailableReason, TabStripControlMetric, TabStripControlMetrics,
    TabStripControlPlacement, TabStripMetrics, UniformSurfaceMetrics,
};
use dockspace_host_harness::{OwnedRecord, RecordingRenderer};

const SURFACE: SurfaceId = SurfaceId::new(1);
const ROOT: RootId = RootId::new(1);
const FLOATING: FloatingPresentationId = FloatingPresentationId::new(1);
const POINTER: SurfacePointerId = SurfacePointerId::new(1);

fn rect(x: f64, y: f64, width: f64, height: f64) -> LogicalRect {
    LogicalRect::new(x, y, width, height).unwrap()
}

fn point(x: f64, y: f64) -> LogicalPoint {
    LogicalPoint::new(x, y).unwrap()
}

fn center(bounds: LogicalRect) -> LogicalPoint {
    point(
        bounds.x() + bounds.width() * 0.5,
        bounds.y() + bounds.height() * 0.5,
    )
}

fn bounds() -> LogicalRect {
    rect(0.0, 0.0, 800.0, 600.0)
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

fn measure(frame: &mut DockspaceHostFrame<'_>, bounds: LogicalRect, controls: bool) {
    if !controls {
        frame
            .measure_surface(
                SURFACE,
                UniformSurfaceMetrics::new(bounds, LogicalSize::new(64.0, 48.0).unwrap(), 96.0)
                    .unwrap(),
            )
            .unwrap();
        return;
    }
    frame
        .measure_surface_with(SURFACE, |question| match question {
            Question::DockBounds { .. } | Question::PopupPlaneBounds { .. } => {
                Answer::Bounds(bounds)
            }
            Question::PaneMinimum { .. } => {
                Answer::PaneMinimum(LogicalSize::new(64.0, 48.0).unwrap())
            }
            Question::TabIntrinsic { .. } => Answer::TabIntrinsic(96.0),
            Question::TabStrip { .. } => Answer::TabStrip(
                TabStripMetrics::new(0.0, 0.0).unwrap().with_controls(
                    TabStripControlMetrics::new(0.0)
                        .unwrap()
                        .with_scroll_backward(
                            TabStripControlMetric::new(
                                18.0,
                                TabStripControlPlacement::OverlayLeading,
                            )
                            .unwrap(),
                        )
                        .with_scroll_forward(
                            TabStripControlMetric::new(
                                18.0,
                                TabStripControlPlacement::OverlayTrailing,
                            )
                            .unwrap(),
                        )
                        .with_tab_list_menu(
                            TabStripControlMetric::new(
                                20.0,
                                TabStripControlPlacement::ReservedTrailing,
                            )
                            .unwrap(),
                        ),
                ),
            ),
        })
        .unwrap();
}

#[derive(Default)]
struct Geometry {
    splitters: Vec<(DockspaceReceiverDescriptor, DockspaceAxis, LogicalRect)>,
    junctions: Vec<DockspaceReceiverDescriptor>,
    resizes: Vec<(
        FloatingPresentationId,
        ContainedResizeDirection,
        DockspaceReceiverDescriptor,
    )>,
    protected: Vec<(&'static str, LogicalPoint, Option<Role>)>,
}

fn geometry(plan: SurfacePaintPlan<'_>) -> Geometry {
    let mut geometry = Geometry::default();
    plan.visit_surface_program(|step| {
        match step {
            Step::RootBegin { root, .. } => {
                if let Some(contained) = root.contained() {
                    if let Some(title) = plan.receiver_for_contained_title(contained) {
                        geometry.protected.push((
                            "title",
                            title.center(),
                            Some(Role::ContainedTitle),
                        ));
                    }
                    if let Some(close) = plan.receiver_for_contained_close(contained) {
                        geometry.protected.push((
                            "title close",
                            close.center(),
                            Some(Role::ContainedClose),
                        ));
                    }
                    for resize in contained.resize() {
                        if let Some(receiver) =
                            plan.receiver_for_contained_resize(contained, resize)
                        {
                            geometry.resizes.push((
                                contained.floating(),
                                resize.direction(),
                                receiver,
                            ));
                        }
                    }
                }
            }
            Step::Operation { operation, .. } => match operation {
                Operation::Splitter(splitter) => {
                    geometry.splitters.push((
                        plan.receiver_for_splitter(splitter).unwrap(),
                        splitter.axis(),
                        splitter.draw_bounds(),
                    ));
                }
                Operation::SplitterJunction(junction) => {
                    geometry
                        .junctions
                        .push(plan.receiver_for_splitter_junction(junction).unwrap());
                }
                Operation::Tab(tab) => {
                    if let Some(close) = plan.receiver_for_tab_close(tab) {
                        geometry.protected.push((
                            "tab close",
                            close.center(),
                            Some(Role::TabClose),
                        ));
                    }
                }
                Operation::PresentationMenuAnchor(anchor) => {
                    if anchor.operable() {
                        geometry
                            .protected
                            .push(("menu", center(anchor.bounds()), None));
                    }
                }
                Operation::TabStripControl(control) if !control.enabled() => {
                    geometry.protected.push((
                        "disabled control",
                        center(control.hit_bounds()),
                        Some(Role::TabStripControl),
                    ));
                }
                _ => {}
            },
            _ => {}
        }
        Ok::<(), ()>(())
    })
    .unwrap();
    geometry
}

fn paint(session: &mut DockspaceSession) -> (Geometry, RecordingRenderer, PaintedSurfaceOutput) {
    let mut frame = session.begin_host_frame().unwrap();
    let plan = frame.paint_plan(SURFACE).unwrap().unwrap();
    let geometry = geometry(plan);
    let (recording, receipt) = RecordingRenderer::execute(plan).unwrap();
    frame.confirm_surface_painted(receipt).unwrap();
    let mut report = commit(frame);
    assert!(!report.workspace_changed());
    let mut outputs = report.take_painted_outputs();
    assert_eq!(outputs.len(), 1);
    let output = outputs.pop().unwrap();
    assert!(
        matches!(recording.records.last(), Some(OwnedRecord::SurfaceEnd(semantic))
        if output.matches_semantic_output(*semantic))
    );
    (geometry, recording, output)
}

fn inspect(session: &mut DockspaceSession) -> (Geometry, RecordingRenderer) {
    let mut frame = session.begin_host_frame().unwrap();
    let plan = frame.paint_plan(SURFACE).unwrap().unwrap();
    (geometry(plan), RecordingRenderer::execute(plan).unwrap().0)
}

fn present(
    session: &mut DockspaceSession,
    output: PaintedSurfaceOutput,
    result: Disposition,
) -> HostFrameReport {
    let artifact = output.presentation_artifact();
    let before = durable(session);
    session.report_surface_presentation(output, result).unwrap();
    assert_eq!(
        durable(session),
        before,
        "reporting alone cannot publish a resize"
    );
    let report = settle(session);
    let matching = report
        .surface_presentation_commits()
        .iter()
        .filter(|commit| commit.settles(artifact))
        .collect::<Vec<_>>();
    assert_eq!(
        matching.len(),
        1,
        "the exact recorded output must settle once"
    );
    if result == Disposition::Dropped {
        assert!(matching[0].retires(artifact));
    } else if !report.workspace_changed() {
        assert!(matching[0].presents(artifact));
    }
    report
}

fn fixture(surface: DockspaceSurfaceLayout, controls: bool) -> (DockspaceSession, Geometry) {
    let mut session = DockspaceSession::from_layout(
        DockspaceLayout::new([surface]).unwrap(),
        DockPolicy::default(),
    )
    .unwrap();
    assert_eq!(
        session.presentation_config().touch_resize_hit_extent(),
        44.0
    );
    session.enable_host_pointer().unwrap();
    let mut frame = session.begin_host_frame().unwrap();
    measure(&mut frame, bounds(), controls);
    commit(frame);
    let (geometry, _, output) = paint(&mut session);
    present(&mut session, output, Disposition::Presented);
    (session, geometry)
}

#[derive(Debug, PartialEq)]
struct Durable {
    version: WorkspaceVersion,
    weights: Vec<Vec<f32>>,
    contained: Vec<(FloatingPresentationId, LogicalRect)>,
}

fn durable(session: &DockspaceSession) -> Durable {
    fn collect(node: DockspaceNodeView<'_>, weights: &mut Vec<Vec<f32>>) {
        if let Some(split) = node.split() {
            weights.push(split.weights().collect());
            for child in split.children() {
                collect(child, weights);
            }
        }
    }
    let view = session.view();
    let mut weights = Vec::new();
    collect(view.root(ROOT).unwrap().content().unwrap(), &mut weights);
    Durable {
        version: session.version(),
        weights,
        contained: view
            .surface(SURFACE)
            .unwrap()
            .contained()
            .map(|record| (record.id(), record.rect()))
            .collect(),
    }
}

fn is_resize(role: Role) -> bool {
    matches!(
        role,
        Role::Splitter | Role::SplitterJunction | Role::ContainedResize
    )
}

fn touch_target(
    session: &DockspaceSession,
    point: LogicalPoint,
    expected: DockspaceReceiverDescriptor,
) -> PresentedDockReceiver {
    assert!(
        !expected.bounds().contains(point),
        "the sample must exercise touch-only geometry"
    );
    let touch = session
        .host_pointer_hit_for_device(SURFACE, point, Device::Touch)
        .expect("a touch target must resolve uniquely");
    let receiver = touch
        .drag_delivery()
        .expect("touch must resolve the resize lane");
    assert_eq!(
        receiver.descriptor(),
        expected,
        "touch retains the base descriptor rather than inventing union bounds"
    );
    assert!(is_resize(receiver.descriptor().role()));
    for device in [Device::Mouse, Device::Pen, Device::Unknown] {
        let hit = session
            .host_pointer_hit_for_device(SURFACE, point, device)
            .unwrap();
        assert!(
            hit.drag_delivery()
                .is_none_or(|receiver| !is_resize(receiver.descriptor().role())),
            "{device:?} must retain the original hit geometry at {point:?}"
        );
    }
    let old = session.host_pointer_hit(SURFACE, point).unwrap();
    let unknown = session
        .host_pointer_hit_for_device(SURFACE, point, Device::Unknown)
        .unwrap();
    assert_eq!(old.delivery(), unknown.delivery());
    assert_eq!(old.drag_delivery(), unknown.drag_delivery());
    receiver
}

fn edge(
    session: &mut DockspaceSession,
    surface: PresentedDockspaceSurface,
    event: Event,
    point: LogicalPoint,
    millis: u64,
    remeasure: bool,
) -> HostFrameReport {
    let hit = session.host_pointer_hit_for_device(SURFACE, point, Device::Touch);
    let click = hit.and_then(|hit| hit.delivery());
    let drag = hit.and_then(|hit| hit.drag_delivery());
    let facts = click.as_ref().map_or_else(
        || SurfacePointerReceiverFacts::unknown().with_no_delivery(&surface),
        SurfacePointerReceiverFacts::delivery,
    );
    let facts = drag.as_ref().map_or_else(
        || facts.with_no_drag_delivery(&surface),
        |receiver| facts.with_drag_delivery(receiver),
    );
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
                facts.with_clear_hover_lane(&surface),
            )
            .with_metadata(PointerEventMetadata::new(
                Device::Touch,
                Duration::from_millis(millis),
            )),
        )
        .unwrap();
    if remeasure {
        measure(&mut frame, bounds(), false);
    }
    commit(frame)
}

fn arm(session: &mut DockspaceSession, press: LogicalPoint) -> PresentedDockspaceSurface {
    let before = durable(session);
    let surface = session.presented_surface(SURFACE).unwrap();
    let pressed = edge(
        session,
        surface,
        Event::ButtonPressed(Button::Primary),
        press,
        0,
        false,
    );
    assert!(pressed.inputs().is_empty());
    assert_eq!(durable(session), before);
    let wake = session
        .pointer_continuation()
        .expect("an actual touch press must schedule its continuation");
    assert_eq!(wake.deadline(), Duration::from_millis(350));
    assert_eq!(wake.position(), press);
    let armed = edge(session, surface, wake.event(), press, 350, false);
    assert_eq!(
        armed.inputs(),
        &[HostInputOutcome::Touch(DockspaceTouchEvent::Armed {
            pointer: POINTER,
            surface: SURFACE
        })]
    );
    assert_eq!(durable(session), before);
    assert!(session.has_active_gesture());
    assert!(session.pointer_continuation().is_none());
    surface
}

#[test]
fn false_touch_drag_absence_rejects_atomically_and_corrected_press_retries_once() {
    let node = DockspaceNode::equal_split(
        DockspaceAxis::Horizontal,
        [
            DockspaceNode::tabs([ItemId::new(1)]),
            DockspaceNode::tabs([ItemId::new(2)]),
        ],
    )
    .unwrap();
    let (mut session, initial) = fixture(
        DockspaceSurfaceLayout::new(SURFACE, DockspaceRootLayout::new(ROOT, node)),
        false,
    );
    let descriptor = initial.splitters[0].0;
    let press = point(descriptor.center().x() + 21.0, 300.0);
    touch_target(&session, press, descriptor);
    let hit = session
        .host_pointer_hit_for_device(SURFACE, press, Device::Touch)
        .unwrap();
    let click = hit.delivery().expect("pane content owns the click lane");
    assert_eq!(click.descriptor().role(), Role::PaneBody);
    assert_eq!(hit.drag_delivery().unwrap().descriptor(), descriptor);
    let surface = hit.surface();
    let route = HostPointerRoute::new(surface, press);
    let before = durable(&session);
    assert!(!session.has_active_gesture());
    assert!(session.pointer_continuation().is_none());

    let mut rejected = session.begin_host_frame().unwrap();
    let error = rejected
        .submit_host_pointer(
            HostPointerInput::new(
                POINTER,
                Event::ButtonPressed(Button::Primary),
                route,
                route,
                SurfacePointerCapture::ProviderEndpoint,
                SurfacePointerReceiverFacts::delivery(&click).with_no_drag_delivery(&surface),
            )
            .with_metadata(PointerEventMetadata::new(Device::Touch, Duration::ZERO)),
        )
        .expect_err("Drag NoReceiver cannot conceal the expanded touch resize winner");
    assert_eq!(error.kind(), DockspaceRuntimeErrorKind::HostProtocol);
    drop(rejected);
    assert_eq!(durable(&session), before);
    assert!(!session.has_active_gesture());
    assert!(session.pointer_continuation().is_none());

    // The rejected frame cannot consume the runtime-assigned sequence. Retry
    // the same pointer, press position and time with the actual lane winners.
    let pressed = edge(
        &mut session,
        surface,
        Event::ButtonPressed(Button::Primary),
        press,
        0,
        false,
    );
    assert!(pressed.inputs().is_empty());
    assert!(!pressed.workspace_changed());
    assert_eq!(durable(&session), before);
    let wake = session
        .pointer_continuation()
        .expect("the corrected press must retain a real touch continuation");
    assert_eq!(wake.deadline(), Duration::from_millis(350));
    assert_eq!(wake.position(), press);
    for millis in [350, 351] {
        let report = edge(&mut session, surface, wake.event(), press, millis, false);
        if millis == 350 {
            assert_eq!(
                report.inputs(),
                &[HostInputOutcome::Touch(DockspaceTouchEvent::Armed {
                    pointer: POINTER,
                    surface: SURFACE,
                })]
            );
        } else {
            assert!(
                report.inputs().is_empty(),
                "the consumed token cannot arm twice"
            );
        }
        assert!(!report.workspace_changed());
        assert_eq!(durable(&session), before);
        assert!(session.has_active_gesture());
        assert!(session.pointer_continuation().is_none());
    }
}

#[test]
fn expanded_splitter_and_junction_start_from_touch_only_points_without_a_first_move_jump() {
    for junction in [false, true] {
        let left = if junction {
            DockspaceNode::equal_split(
                DockspaceAxis::Vertical,
                [
                    DockspaceNode::tabs([ItemId::new(1)]),
                    DockspaceNode::tabs([ItemId::new(2)]),
                ],
            )
            .unwrap()
        } else {
            DockspaceNode::tabs([ItemId::new(1)])
        };
        let node = DockspaceNode::equal_split(
            DockspaceAxis::Horizontal,
            [left, DockspaceNode::tabs([ItemId::new(3)])],
        )
        .unwrap();
        let (mut session, initial) = fixture(
            DockspaceSurfaceLayout::new(SURFACE, DockspaceRootLayout::new(ROOT, node)),
            false,
        );
        let (descriptor, press) = if junction {
            let descriptor = *initial
                .junctions
                .first()
                .expect("the T layout must expose one junction");
            (
                descriptor,
                point(
                    descriptor.center().x() + 21.0,
                    descriptor.center().y() + 21.0,
                ),
            )
        } else {
            let descriptor = initial.splitters[0].0;
            (descriptor, point(descriptor.center().x() + 21.0, 300.0))
        };
        touch_target(&session, press, descriptor);
        let before = durable(&session);
        let surface = arm(&mut session, press);
        let moved = point(press.x() + 28.0, press.y() + 24.0);
        let motion = edge(&mut session, surface, Event::Moved, moved, 360, true);
        assert!(!motion.workspace_changed());
        assert_eq!(durable(&session), before);
        let (proposal, _) = inspect(&mut session);
        for (descriptor, axis, old) in &initial.splitters {
            let (_, _, new) = proposal
                .splitters
                .iter()
                .find(|(candidate, _, _)| candidate.visual_id() == descriptor.visual_id())
                .unwrap();
            let (actual, expected) = match axis {
                DockspaceAxis::Horizontal => (new.x() - old.x(), 28.0),
                DockspaceAxis::Vertical => (new.y() - old.y(), 24.0),
            };
            // Split weights use f32; the projection retains the contact delta
            // within the same tolerance as the shared splitter regressions.
            assert!(
                (actual - expected).abs() < 1e-4,
                "junction={junction}: the first move must use contact delta, got {actual}"
            );
        }
        let released = edge(
            &mut session,
            surface,
            Event::ContactEnded(Button::Primary),
            moved,
            370,
            false,
        );
        assert!(
            released.workspace_changed(),
            "splitters retain their existing release contract"
        );
        let [terminal] = released.manipulation_feedback() else {
            panic!("one splitter terminal is required: {released:?}")
        };
        assert_eq!(
            terminal.status(),
            Status::Delivered(Delivery::SplitterResize)
        );
        let after = durable(&session);
        assert_eq!(after.weights.len(), before.weights.len());
        for (old, new) in before.weights.iter().zip(&after.weights) {
            assert_ne!(
                old, new,
                "each incident split must participate in the junction resize"
            );
        }
        assert!(!session.has_active_gesture());
        assert!(settle(&mut session).manipulation_feedback().is_empty());
        assert_eq!(durable(&session), after);
    }
}

#[test]
fn expanded_contained_edge_and_corner_wait_for_exact_output_and_preserve_the_press_offset() {
    for direction in [
        ContainedResizeDirection::East,
        ContainedResizeDirection::SouthEast,
    ] {
        for disposition in [Disposition::Presented, Disposition::Dropped] {
            let source = rect(240.0, 160.0, 320.0, 240.0);
            let layout = DockspaceSurfaceLayout::new(
                SURFACE,
                DockspaceRootLayout::new(ROOT, DockspaceNode::tabs([ItemId::new(1)])),
            )
            .with_contained(DockspaceContainedLayout::new(
                FLOATING,
                DockspaceRootLayout::new(RootId::new(2), DockspaceNode::tabs([ItemId::new(2)])),
                source,
            ));
            let (mut session, initial) = fixture(layout, false);
            let descriptor = initial
                .resizes
                .iter()
                .find(|(floating, candidate, _)| *floating == FLOATING && *candidate == direction)
                .unwrap()
                .2;
            let press = point(
                source.max().x() + 17.0,
                if direction == ContainedResizeDirection::East {
                    center(source).y()
                } else {
                    source.max().y() + 17.0
                },
            );
            touch_target(&session, press, descriptor);
            let before = durable(&session);
            let surface = arm(&mut session, press);
            let moved = point(press.x() + 24.0, press.y() + 16.0);
            let expected = rect(
                source.x(),
                source.y(),
                source.width() + 24.0,
                source.height()
                    + if direction == ContainedResizeDirection::East {
                        0.0
                    } else {
                        16.0
                    },
            );
            let motion = edge(&mut session, surface, Event::Moved, moved, 360, false);
            assert!(!motion.workspace_changed());
            assert_eq!(durable(&session), before);
            let (_, proposal) = inspect(&mut session);
            assert!(
                proposal
                    .records
                    .contains(&OwnedRecord::TransformPreview(FLOATING, expected)),
                "{direction:?}: the first move must preserve the outside press offset"
            );
            let released = edge(
                &mut session,
                surface,
                Event::ContactEnded(Button::Primary),
                moved,
                370,
                false,
            );
            assert!(!released.workspace_changed());
            assert_eq!(durable(&session), before);
            let [pending] = released.manipulation_feedback() else {
                panic!("one exact-output obligation is required: {released:?}")
            };
            assert_eq!(
                pending.status(),
                Status::Pending(PendingReason::AwaitingPresentation)
            );
            let (_, recording, output) = paint(&mut session);
            assert!(
                recording
                    .records
                    .contains(&OwnedRecord::TransformPreview(FLOATING, expected))
            );
            assert_eq!(durable(&session), before);
            let settled = present(&mut session, output, disposition);
            let [terminal] = settled.manipulation_feedback() else {
                panic!("one resize terminal is required: {settled:?}")
            };
            assert_eq!(terminal.manipulation_id(), pending.manipulation_id());
            if disposition == Disposition::Presented {
                assert!(settled.workspace_changed());
                assert_eq!(
                    terminal.status(),
                    Status::Delivered(Delivery::ContainedTransform)
                );
                assert_eq!(durable(&session).contained, [(FLOATING, expected)]);
            } else {
                assert!(!settled.workspace_changed());
                assert!(matches!(terminal.status(), Status::Cancelled(_)));
                assert_eq!(durable(&session), before);
            }
            assert!(!session.has_active_gesture());
            assert!(session.pointer_continuation().is_none());
            let after = durable(&session);
            assert!(settle(&mut session).manipulation_feedback().is_empty());
            assert_eq!(durable(&session), after);
        }
    }
}

#[test]
fn fully_clipped_base_west_resize_keeps_its_visible_touch_fragment_and_exact_delivery() {
    let layout = DockspaceSurfaceLayout::new(
        SURFACE,
        DockspaceRootLayout::new(ROOT, DockspaceNode::tabs([ItemId::new(1)])),
    )
    .with_contained(DockspaceContainedLayout::new(
        FLOATING,
        DockspaceRootLayout::new(RootId::new(2), DockspaceNode::tabs([ItemId::new(2)])),
        rect(-20.0, 160.0, 320.0, 240.0),
    ));
    let (mut session, _) = fixture(layout, false);
    let press = point(2.0, 300.0);
    let receiver = session
        .host_pointer_hit_for_device(SURFACE, press, Device::Touch)
        .unwrap()
        .drag_delivery()
        .expect("the visible West touch fragment must retain its semantic receiver");
    assert_eq!(receiver.descriptor().role(), Role::ContainedResize);
    assert_eq!(receiver.bounds().width(), 0.0);
    touch_target(&session, press, receiver.descriptor());
    let before = durable(&session);
    let surface = arm(&mut session, press);
    let moved = point(press.x() + 30.0, press.y());
    let expected = rect(10.0, 160.0, 290.0, 240.0);
    let motion = edge(&mut session, surface, Event::Moved, moved, 360, false);
    assert!(!motion.workspace_changed());
    assert_eq!(durable(&session), before);
    let (_, proposal) = inspect(&mut session);
    assert!(
        proposal
            .records
            .contains(&OwnedRecord::TransformPreview(FLOATING, expected)),
        "the first move must use the real contact delta despite an empty base hit"
    );
    let released = edge(
        &mut session,
        surface,
        Event::ContactEnded(Button::Primary),
        moved,
        370,
        false,
    );
    assert!(!released.workspace_changed());
    assert_eq!(durable(&session), before);
    let [pending] = released.manipulation_feedback() else {
        panic!("the West resize must await its exact output: {released:?}")
    };
    assert_eq!(
        pending.status(),
        Status::Pending(PendingReason::AwaitingPresentation)
    );
    let (_, recording, output) = paint(&mut session);
    assert!(
        recording
            .records
            .contains(&OwnedRecord::TransformPreview(FLOATING, expected))
    );
    assert_eq!(durable(&session), before);
    let settled = present(&mut session, output, Disposition::Presented);
    let [terminal] = settled.manipulation_feedback() else {
        panic!("the exact West resize output must deliver once: {settled:?}")
    };
    assert_eq!(terminal.manipulation_id(), pending.manipulation_id());
    assert_eq!(
        terminal.status(),
        Status::Delivered(Delivery::ContainedTransform)
    );
    assert!(settled.workspace_changed());
    assert_eq!(durable(&session).contained, [(FLOATING, expected)]);
    assert!(!session.has_active_gesture());
    assert!(session.pointer_continuation().is_none());
}

#[test]
fn expanded_targets_protect_title_close_menu_and_disabled_tab_controls() {
    let layout = DockspaceSurfaceLayout::new(
        SURFACE,
        DockspaceRootLayout::new(ROOT, DockspaceNode::tabs((1..13).map(ItemId::new))),
    )
    .with_contained(DockspaceContainedLayout::new(
        FLOATING,
        DockspaceRootLayout::new(
            RootId::new(2),
            DockspaceNode::tabs((20..25).map(ItemId::new)),
        ),
        rect(240.0, 160.0, 360.0, 280.0),
    ));
    let (session, geometry) = fixture(layout, true);
    for kind in [
        "title",
        "title close",
        "tab close",
        "menu",
        "disabled control",
    ] {
        assert!(
            geometry
                .protected
                .iter()
                .any(|(candidate, _, _)| *candidate == kind),
            "the fixture must expose {kind}"
        );
    }
    for (kind, point, expected) in geometry.protected {
        let hit = session
            .host_pointer_hit_for_device(SURFACE, point, Device::Touch)
            .expect("protected controls must keep an unambiguous query");
        let roles = [
            hit.delivery().map(|receiver| receiver.descriptor().role()),
            hit.drag_delivery()
                .map(|receiver| receiver.descriptor().role()),
        ];
        assert!(
            roles.into_iter().flatten().all(|role| !is_resize(role)),
            "{kind} must not be stolen by expanded resize geometry at {point:?}"
        );
        if let Some(expected) = expected {
            assert!(
                roles.contains(&Some(expected)),
                "{kind} must preserve its original receiver role: {roles:?}"
            );
        }
    }
}

#[test]
fn outside_targets_respect_front_occlusion_surface_clipping_and_stale_outputs() {
    let layout = DockspaceSurfaceLayout::new(
        SURFACE,
        DockspaceRootLayout::new(ROOT, DockspaceNode::tabs([ItemId::new(1)])),
    )
    .with_contained(DockspaceContainedLayout::new(
        FLOATING,
        DockspaceRootLayout::new(RootId::new(2), DockspaceNode::tabs([ItemId::new(2)])),
        rect(240.0, 160.0, 320.0, 240.0),
    ))
    .with_contained(DockspaceContainedLayout::new(
        FloatingPresentationId::new(2),
        DockspaceRootLayout::new(RootId::new(3), DockspaceNode::tabs([ItemId::new(3)])),
        rect(500.0, 340.0, 240.0, 200.0),
    ))
    .with_contained(DockspaceContainedLayout::new(
        FloatingPresentationId::new(3),
        DockspaceRootLayout::new(RootId::new(4), DockspaceNode::tabs([ItemId::new(4)])),
        rect(-4.0, 430.0, 180.0, 160.0),
    ));
    let (mut session, _) = fixture(layout, false);
    let covered = point(570.0, 410.0);
    let hit = session
        .host_pointer_hit_for_device(SURFACE, covered, Device::Touch)
        .unwrap();
    assert_eq!(
        hit.drag_delivery().unwrap().descriptor().role(),
        Role::PaneBody,
        "a rear resize halo cannot penetrate the front window's real content"
    );
    let clipped = point(12.0, 420.0);
    let hit = session
        .host_pointer_hit_for_device(SURFACE, clipped, Device::Touch)
        .unwrap();
    let stale = hit
        .drag_delivery()
        .expect("the visible portion of a clipped corner must remain reachable");
    assert_eq!(stale.descriptor().role(), Role::ContainedResize);
    touch_target(&session, clipped, stale.descriptor());
    for outside in [point(-1.0, 420.0), point(800.0, 420.0)] {
        assert!(
            session
                .host_pointer_hit_for_device(SURFACE, outside, Device::Touch)
                .is_none_or(|hit| hit.drag_delivery().is_none()),
            "touch geometry must remain surface-clipped"
        );
    }
    let before = durable(&session);
    let mut frame = session.begin_host_frame().unwrap();
    measure(&mut frame, rect(0.0, 0.0, 820.0, 600.0), false);
    commit(frame);
    let (_, _, output) = paint(&mut session);
    present(&mut session, output, Disposition::Presented);
    assert!(
        session
            .bind_presented_receiver(&stale.descriptor())
            .is_none(),
        "an old touch-only hit must not bind to a new output"
    );
    let current = session.presented_surface(SURFACE).unwrap();
    let route = HostPointerRoute::new(current, clipped);
    let input = HostPointerInput::new(
        POINTER,
        Event::ButtonPressed(Button::Primary),
        route,
        route,
        SurfacePointerCapture::ProviderEndpoint,
        SurfacePointerReceiverFacts::unknown().with_drag_delivery(&stale),
    )
    .with_metadata(PointerEventMetadata::new(Device::Touch, Duration::ZERO));
    let mut frame = session.begin_host_frame().unwrap();
    if frame.submit_host_pointer(input).is_ok() {
        assert!(!commit(frame).workspace_changed());
    } else {
        drop(frame);
    }
    assert_eq!(durable(&session), before);
    assert!(!session.has_active_gesture());
    assert!(session.pointer_continuation().is_none());
}
