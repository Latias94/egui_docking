//! Nested edge contact through public rendering, pointer and semantic resize APIs.

use std::collections::BTreeMap;

use dockspace::geometry::{LogicalPoint, LogicalRect, LogicalSize};
use dockspace::model::{
    DockspaceAxis, DockspaceLayout, DockspaceNode, DockspaceNodeView, DockspaceRootLayout,
    DockspaceSurfaceLayout, DockspaceView, ItemId, RootId, SurfaceId, WorkspaceVersion,
};
use dockspace::policy::DockPolicy;
use dockspace::runtime::{
    DockPresentationConfig, DockspaceHostFrame, DockspaceManipulationDelivery as Delivery,
    DockspaceManipulationStatus as Status, DockspaceReceiverDescriptor, DockspaceSession,
    DockspaceVisualId, HostFrameReport, HostPointerInput, HostPointerRoute, PaintedSurfaceOutput,
    PresentedDockReceiver, PresentedDockspaceSurface, SurfaceMeasurementAnswer as Answer,
    SurfaceMeasurementRequest as Question, SurfacePaintPlan,
    SurfacePaintProgramOperation as Operation, SurfacePaintProgramStep as Step,
    SurfacePointerButton as Button, SurfacePointerCapture, SurfacePointerEvent as Event,
    SurfacePointerId, SurfacePointerReceiverFacts, SurfacePresentationResult as Disposition,
    SurfaceSplitterAdjustment, SurfaceUnavailableReason, TabStripMetrics,
};
use dockspace_host_harness::{OwnedRecord, RecordingRenderer};

const SURFACE: SurfaceId = SurfaceId::new(1);
const ROOT: RootId = RootId::new(1);
const POINTER: SurfacePointerId = SurfacePointerId::new(1);
const A: ItemId = ItemId::new(1);
const B: ItemId = ItemId::new(2);
const C: ItemId = ItemId::new(3);
const D: ItemId = ItemId::new(4);
const E: ItemId = ItemId::new(5);
const F: ItemId = ItemId::new(6);
const GAP: f64 = 2.0;
const TAB_BAR: f64 = 28.0;
// Durable split weights use f32, as in the existing public splitter regressions.
const EPSILON: f64 = 1e-4;

fn point(x: f64, y: f64) -> LogicalPoint {
    LogicalPoint::new(x, y).unwrap()
}

fn rect(x: f64, y: f64, width: f64, height: f64) -> LogicalRect {
    LogicalRect::new(x, y, width, height).unwrap()
}

fn close(actual: f64, expected: f64) {
    assert!(
        (actual - expected).abs() < EPSILON,
        "expected {expected}, got {actual}"
    );
}

fn same_rect(actual: LogicalRect, expected: LogicalRect) {
    close(actual.x(), expected.x());
    close(actual.y(), expected.y());
    close(actual.width(), expected.width());
    close(actual.height(), expected.height());
}

#[derive(Debug)]
struct Pane {
    bounds: LogicalRect,
    content: LogicalRect,
}

#[derive(Debug)]
struct Splitter {
    visual: DockspaceVisualId,
    receiver: DockspaceReceiverDescriptor,
    axis: DockspaceAxis,
    draw: LogicalRect,
    minimums: (f64, f64),
}

#[derive(Debug, Default)]
struct Geometry {
    panes: BTreeMap<ItemId, Pane>,
    splitters: Vec<Splitter>,
    junctions: Vec<(DockspaceReceiverDescriptor, usize)>,
}

impl Geometry {
    fn read(plan: SurfacePaintPlan<'_>) -> Self {
        let mut geometry = Self::default();
        plan.visit_surface_program(|step| {
            if let Step::Operation { operation, .. } = step {
                match operation {
                    Operation::Pane(pane) => {
                        assert_eq!(pane.root(), ROOT);
                        assert!(
                            geometry
                                .panes
                                .insert(
                                    pane.selected().expect("each fixture pane has one item"),
                                    Pane {
                                        bounds: pane.bounds(),
                                        content: pane.content_bounds(),
                                    },
                                )
                                .is_none()
                        );
                    }
                    Operation::Splitter(splitter) => geometry.splitters.push(Splitter {
                        // Semantic actions use the paint record's identity;
                        // pointer delivery uses its separate receiver identity.
                        visual: splitter.visual_id(),
                        receiver: plan.receiver_for_splitter(splitter).unwrap(),
                        axis: splitter.axis(),
                        draw: splitter.draw_bounds(),
                        minimums: splitter.minimum_extents(),
                    }),
                    Operation::SplitterJunction(junction) => geometry.junctions.push((
                        plan.receiver_for_splitter_junction(junction).unwrap(),
                        junction.arm_count(),
                    )),
                    _ => {}
                }
            }
            Ok::<(), ()>(())
        })
        .unwrap();
        geometry
    }

    fn splitter_at(&self, axis: DockspaceAxis, position: LogicalPoint) -> &Splitter {
        let mut matches = self
            .splitters
            .iter()
            .filter(|splitter| splitter.axis == axis && splitter.draw.contains(position));
        let splitter = matches
            .next()
            .expect("the draw position selects a splitter");
        assert!(matches.next().is_none(), "the draw position must be unique");
        assert!(splitter.receiver.bounds().contains(position));
        splitter
    }

    fn four_arm_junction(&self) -> DockspaceReceiverDescriptor {
        let mut matches = self.junctions.iter().filter(|(receiver, arms)| {
            *arms == 4 && receiver.bounds().contains(point(515.0, 257.0))
        });
        let receiver = matches
            .next()
            .expect("the central crossing must expose its four-arm junction")
            .0;
        assert!(matches.next().is_none(), "the junction must be unique");
        receiver
    }

    fn assert_panes(&self, expected: &[(ItemId, LogicalRect)]) {
        assert_eq!(self.panes.len(), expected.len());
        for &(item, bounds) in expected {
            let pane = &self.panes[&item];
            same_rect(pane.bounds, bounds);
            same_rect(
                pane.content,
                rect(
                    bounds.x(),
                    bounds.y() + TAB_BAR,
                    bounds.width(),
                    bounds.height() - TAB_BAR,
                ),
            );
        }
    }
}

fn weights(view: DockspaceView<'_>) -> Vec<Vec<u32>> {
    fn collect(node: DockspaceNodeView<'_>, result: &mut Vec<Vec<u32>>) {
        if let Some(split) = node.split() {
            result.push(split.weights().map(f32::to_bits).collect());
            for child in split.children() {
                collect(child, result);
            }
        }
    }
    let mut result = Vec::new();
    collect(view.root(ROOT).unwrap().content().unwrap(), &mut result);
    result
}

#[derive(Debug, PartialEq)]
struct Durable {
    version: WorkspaceVersion,
    weights: Vec<Vec<u32>>,
}

fn durable(session: &DockspaceSession) -> Durable {
    Durable {
        version: session.version(),
        weights: weights(session.view()),
    }
}

fn changed_splits(before: &[Vec<u32>], after: &[Vec<u32>], expected: &[bool]) {
    assert_eq!(before.len(), expected.len());
    assert_eq!(after.len(), expected.len());
    for (index, ((before, after), expected)) in before.iter().zip(after).zip(expected).enumerate() {
        assert_eq!(before != after, *expected, "split {index}");
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

fn measure(
    frame: &mut DockspaceHostFrame<'_>,
    bounds: LogicalRect,
    minimums: &[(ItemId, f64, f64)],
) {
    frame
        .measure_surface_with(SURFACE, |question| match question {
            Question::DockBounds { .. } | Question::PopupPlaneBounds { .. } => {
                Answer::Bounds(bounds)
            }
            Question::PaneMinimum { item, .. } => {
                let (_, width, height) = minimums
                    .iter()
                    .copied()
                    .find(|(candidate, _, _)| Some(*candidate) == item)
                    .unwrap_or((A, 64.0, 48.0));
                Answer::PaneMinimum(LogicalSize::new(width, height).unwrap())
            }
            Question::TabIntrinsic { .. } => Answer::TabIntrinsic(56.0),
            Question::TabStrip { .. } => Answer::TabStrip(TabStripMetrics::new(0.0, 0.0).unwrap()),
        })
        .unwrap();
}

fn paint(session: &mut DockspaceSession) -> (Geometry, PaintedSurfaceOutput) {
    let mut frame = session.begin_host_frame().unwrap();
    let plan = frame.paint_plan(SURFACE).unwrap().unwrap();
    let geometry = Geometry::read(plan);
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
    (geometry, output)
}

fn report_presentation(
    session: &mut DockspaceSession,
    output: PaintedSurfaceOutput,
    expected_retained: bool,
) {
    let before = durable(session);
    let artifact = output.presentation_artifact();
    session
        .report_surface_presentation(output, Disposition::Presented)
        .unwrap();
    assert_eq!(durable(session), before);
    let report = settle(session);
    assert!(!report.workspace_changed());
    assert!(report.manipulation_feedback().is_empty());
    assert_eq!(durable(session), before);
    let matching = report
        .surface_presentation_commits()
        .iter()
        .filter(|commit| commit.settles(artifact))
        .collect::<Vec<_>>();
    assert_eq!(matching.len(), 1, "the exact renderer output settles once");
    assert_eq!(matching[0].presents(artifact), expected_retained);
    assert_eq!(matching[0].retires(artifact), !expected_retained);
}

fn present(session: &mut DockspaceSession, output: PaintedSurfaceOutput) {
    report_presentation(session, output, true);
}

struct Fixture {
    session: DockspaceSession,
    bounds: LogicalRect,
    minimums: Vec<(ItemId, f64, f64)>,
}

fn fixture(
    node: DockspaceNode,
    bounds: LogicalRect,
    minimums: &[(ItemId, f64, f64)],
    keyboard_step: f64,
) -> (Fixture, Geometry) {
    let config = DockPresentationConfig::builder()
        .splitter_thickness(GAP)
        .tab_bar_height(TAB_BAR)
        .splitter_keyboard_step(keyboard_step)
        .minimum_pane_size(64.0, 48.0)
        .build()
        .unwrap();
    let layout = DockspaceLayout::new([DockspaceSurfaceLayout::new(
        SURFACE,
        DockspaceRootLayout::new(ROOT, node),
    )])
    .unwrap();
    let mut session = DockspaceSession::from_layout_with_presentation_config(
        layout,
        DockPolicy::default(),
        config,
    )
    .unwrap();
    session.enable_host_pointer().unwrap();
    let before = durable(&session);
    let mut frame = session.begin_host_frame().unwrap();
    measure(&mut frame, bounds, minimums);
    assert!(!commit(frame).workspace_changed());
    let (geometry, output) = paint(&mut session);
    present(&mut session, output);
    assert_eq!(durable(&session), before, "projection preserves intent");
    (
        Fixture {
            session,
            bounds,
            minimums: minimums.to_vec(),
        },
        geometry,
    )
}

fn tabs(item: ItemId, central: ItemId) -> DockspaceNode {
    if item == central {
        DockspaceNode::central_tabs([item])
    } else {
        DockspaceNode::tabs([item])
    }
}

fn nested_pair(central: ItemId) -> (Fixture, Geometry) {
    let lower = DockspaceNode::equal_split(
        DockspaceAxis::Horizontal,
        [tabs(C, central), tabs(D, central)],
    )
    .unwrap();
    let right =
        DockspaceNode::equal_split(DockspaceAxis::Vertical, [tabs(B, central), lower]).unwrap();
    let root =
        DockspaceNode::equal_split(DockspaceAxis::Horizontal, [tabs(A, central), right]).unwrap();
    fixture(root, rect(0.0, 0.0, 1030.0, 514.0), &[], 64.0)
}

fn nested_minimum() -> (Fixture, Geometry) {
    let lower = DockspaceNode::split(
        DockspaceAxis::Horizontal,
        [(tabs(C, C), 0.25), (tabs(D, C), 0.25), (tabs(E, C), 0.5)],
    )
    .unwrap();
    let right = DockspaceNode::equal_split(DockspaceAxis::Vertical, [tabs(B, C), lower]).unwrap();
    let root = DockspaceNode::equal_split(DockspaceAxis::Horizontal, [tabs(A, C), right]).unwrap();
    fixture(
        root,
        rect(0.0, 0.0, 1034.0, 514.0),
        &[(C, 96.0, 48.0), (B, 400.0, 48.0)],
        1000.0,
    )
}

fn nested_junction() -> (Fixture, Geometry) {
    let lower =
        DockspaceNode::equal_split(DockspaceAxis::Horizontal, [tabs(C, C), tabs(D, C)]).unwrap();
    let left =
        DockspaceNode::split(DockspaceAxis::Vertical, [(tabs(A, C), 0.25), (lower, 0.75)]).unwrap();
    let right =
        DockspaceNode::equal_split(DockspaceAxis::Vertical, [tabs(E, C), tabs(F, C)]).unwrap();
    let root = DockspaceNode::equal_split(DockspaceAxis::Horizontal, [left, right]).unwrap();
    fixture(
        root,
        rect(0.0, 0.0, 1030.0, 514.0),
        &[(A, 64.0, 228.0)],
        64.0,
    )
}

// Every edge takes its click and drag facts from the actual presented output.
struct PointerSample {
    surface: PresentedDockspaceSurface,
    position: LogicalPoint,
    click: Option<PresentedDockReceiver>,
    drag: Option<PresentedDockReceiver>,
}

impl PointerSample {
    fn at(session: &DockspaceSession, position: LogicalPoint) -> Self {
        let surface = session.presented_surface(SURFACE).unwrap();
        let hit = session.host_pointer_hit(SURFACE, position);
        if let Some(hit) = hit {
            assert_eq!(hit.surface(), surface);
        }
        Self {
            surface,
            position,
            click: hit.and_then(|hit| hit.delivery()),
            drag: hit.and_then(|hit| hit.drag_delivery()),
        }
    }

    fn input(&self, event: Event) -> HostPointerInput<'_> {
        let facts = self.click.as_ref().map_or_else(
            || SurfacePointerReceiverFacts::unknown().with_no_delivery(&self.surface),
            SurfacePointerReceiverFacts::delivery,
        );
        let facts = self.drag.as_ref().map_or_else(
            || facts.with_no_drag_delivery(&self.surface),
            |receiver| facts.with_drag_delivery(receiver),
        );
        let route = HostPointerRoute::new(self.surface, self.position);
        HostPointerInput::new(
            POINTER,
            event,
            route,
            route,
            if matches!(event, Event::ButtonReleased(Button::Primary)) {
                SurfacePointerCapture::None
            } else {
                SurfacePointerCapture::ProviderEndpoint
            },
            facts.with_clear_hover_lane(&self.surface),
        )
    }
}

fn edge(fixture: &mut Fixture, event: Event, position: LogicalPoint) -> HostFrameReport {
    let sample = PointerSample::at(&fixture.session, position);
    let mut frame = fixture.session.begin_host_frame().unwrap();
    frame.submit_host_pointer(sample.input(event)).unwrap();
    measure(&mut frame, fixture.bounds, &fixture.minimums);
    commit(frame)
}

fn press(fixture: &mut Fixture, position: LogicalPoint, target: DockspaceReceiverDescriptor) {
    let sample = PointerSample::at(&fixture.session, position);
    assert_eq!(sample.drag.unwrap().descriptor(), target);
    assert!(fixture.session.bind_presented_receiver(&target).is_some());
    let before = durable(&fixture.session);
    let report = edge(fixture, Event::ButtonPressed(Button::Primary), position);
    assert!(!report.workspace_changed());
    assert_eq!(durable(&fixture.session), before);
    assert!(fixture.session.has_active_gesture());
}

fn assert_delivered(report: &HostFrameReport, before: WorkspaceVersion) {
    assert!(report.workspace_changed());
    assert_eq!(report.before(), before);
    assert_ne!(report.after(), before);
    let [terminal] = report.manipulation_feedback() else {
        panic!("one atomic splitter delivery is required: {report:?}")
    };
    assert_eq!(
        terminal.status(),
        Status::Delivered(Delivery::SplitterResize)
    );
}

#[derive(Debug, Clone, Copy)]
enum OutputTiming {
    Immediate,
    Deferred,
}

fn pointer_resize(
    fixture: &mut Fixture,
    start: LogicalPoint,
    target: DockspaceReceiverDescriptor,
    delta: LogicalPoint,
    timing: OutputTiming,
    assert_geometry: impl Fn(&Geometry),
) -> Durable {
    let before = durable(&fixture.session);
    press(fixture, start, target);
    let moved = point(start.x() + delta.x(), start.y() + delta.y());
    let report = edge(fixture, Event::Moved, moved);
    assert!(!report.workspace_changed());
    assert_eq!(durable(&fixture.session), before);
    let (proposal, output) = paint(&mut fixture.session);
    assert_geometry(&proposal);
    let pending = match timing {
        OutputTiming::Immediate => {
            present(&mut fixture.session, output);
            None
        }
        OutputTiming::Deferred => Some(output),
    };
    assert_eq!(durable(&fixture.session), before);
    let released = edge(fixture, Event::ButtonReleased(Button::Primary), moved);
    assert_delivered(&released, before.version);
    assert_eq!(released.after(), fixture.session.version());
    assert!(!fixture.session.has_active_gesture());
    let after = durable(&fixture.session);
    let (geometry, output) = paint(&mut fixture.session);
    assert_geometry(&geometry);
    if let Some(pending) = pending {
        // The accepted resize makes this exact proposal the durable geometry.
        // Its late observation can retain the displayed snapshot without replaying delivery.
        report_presentation(&mut fixture.session, pending, true);
    }
    present(&mut fixture.session, output);
    assert_eq!(durable(&fixture.session), after);
    assert!(!settle(&mut fixture.session).workspace_changed());
    after
}

fn semantic_resize(fixture: &mut Fixture, target: DockspaceVisualId) -> HostFrameReport {
    let mut frame = fixture.session.begin_host_frame().unwrap();
    let plan = frame.paint_plan(SURFACE).unwrap().unwrap();
    let action = plan
        .prepare_splitter_adjustment(target, SurfaceSplitterAdjustment::Increment)
        .expect("the exact public splitter supports a keyboard adjustment");
    frame.submit_surface_action(action).unwrap();
    measure(&mut frame, fixture.bounds, &fixture.minimums);
    commit(frame)
}

#[test]
fn semantic_endpoints_and_accessible_range_include_nested_touching_minimums() {
    for adjustment in [
        SurfaceSplitterAdjustment::Minimum,
        SurfaceSplitterAdjustment::Maximum,
    ] {
        let (mut fixture, initial) = nested_minimum();
        let target = initial
            .splitter_at(DockspaceAxis::Horizontal, point(517.0, 128.0))
            .visual;
        let before = fixture.session.version();
        let mut frame = fixture.session.begin_host_frame().unwrap();
        let plan = frame.paint_plan(SURFACE).unwrap().unwrap();
        let range = plan.splitter_resize_range(target).unwrap();
        close(range.value(), 0.5);
        // C's touching edge reaches its 96-point minimum after only 32 points.
        // B's own 400-point minimum would incorrectly allow 116 points.
        close(range.maximum(), 548.0 / 1032.0);
        assert!(range.minimum() > 0.0 && range.minimum() < range.value());
        let expected = match adjustment {
            SurfaceSplitterAdjustment::Minimum => range.minimum(),
            SurfaceSplitterAdjustment::Maximum => range.maximum(),
            _ => unreachable!(),
        };
        let action = plan
            .prepare_splitter_adjustment(target, adjustment)
            .unwrap();
        frame.submit_surface_action(action).unwrap();
        measure(&mut frame, fixture.bounds, &fixture.minimums);
        let report = commit(frame);
        assert!(report.workspace_changed());
        assert_eq!(report.before(), before);
        let (geometry, output) = paint(&mut fixture.session);
        present(&mut fixture.session, output);
        let left = geometry.panes[&A].bounds.width();
        let right = geometry.panes[&B].bounds.width();
        close(left / (left + right), expected);
        if adjustment == SurfaceSplitterAdjustment::Maximum {
            assert_minimum_resized(&geometry);
        }
    }
}

#[test]
fn junction_endpoints_resize_one_axis_atomically_and_preserve_its_perpendicular_axis() {
    for axis in [DockspaceAxis::Horizontal, DockspaceAxis::Vertical] {
        for adjustment in [
            SurfaceSplitterAdjustment::Minimum,
            SurfaceSplitterAdjustment::Maximum,
        ] {
            let (mut fixture, initial) = nested_junction();
            let before = fixture.session.version();
            let target = initial.four_arm_junction();
            let mut frame = fixture.session.begin_host_frame().unwrap();
            let plan = frame.paint_plan(SURFACE).unwrap().unwrap();
            let mut found = None;
            plan.visit_surface_program(|step| {
                if let Step::Operation {
                    operation: Operation::SplitterJunction(junction),
                    ..
                } = step
                    && junction.arm_count() == 4
                    && junction.hit_bounds().contains(target.center())
                {
                    found = Some(junction);
                }
                Ok::<(), ()>(())
            })
            .unwrap();
            let junction = found.unwrap();
            let action = plan
                .prepare_splitter_junction_adjustment(junction.visual_id(), axis, adjustment)
                .unwrap();
            frame.submit_surface_action(action).unwrap();
            measure(&mut frame, fixture.bounds, &fixture.minimums);
            let report = commit(frame);
            // A already starts at its vertical minimum, including tab chrome.
            let expected_change =
                axis != DockspaceAxis::Vertical || adjustment != SurfaceSplitterAdjustment::Minimum;
            assert_eq!(
                report.workspace_changed(),
                expected_change,
                "{axis:?} {adjustment:?}"
            );
            assert_eq!(report.before(), before);
            let (geometry, output) = paint(&mut fixture.session);
            present(&mut fixture.session, output);
            for (item, pane) in &initial.panes {
                let after = geometry.panes[item].bounds;
                match axis {
                    DockspaceAxis::Horizontal => {
                        close(after.y(), pane.bounds.y());
                        close(after.height(), pane.bounds.height());
                    }
                    DockspaceAxis::Vertical => {
                        close(after.x(), pane.bounds.x());
                        close(after.width(), pane.bounds.width());
                    }
                }
            }
        }
    }
}

fn assert_pair_initial(geometry: &Geometry) {
    assert_eq!(geometry.splitters.len(), 3);
    geometry.assert_panes(&[
        (A, rect(0.0, 0.0, 514.0, 514.0)),
        (B, rect(516.0, 0.0, 514.0, 256.0)),
        (C, rect(516.0, 258.0, 256.0, 256.0)),
        (D, rect(774.0, 258.0, 256.0, 256.0)),
    ]);
}

fn assert_pair_resized(geometry: &Geometry) {
    geometry.assert_panes(&[
        (A, rect(0.0, 0.0, 578.0, 514.0)),
        (B, rect(580.0, 0.0, 450.0, 256.0)),
        (C, rect(580.0, 258.0, 192.0, 256.0)),
        (D, rect(774.0, 258.0, 256.0, 256.0)),
    ]);
}

fn assert_minimum_resized(geometry: &Geometry) {
    geometry.assert_panes(&[
        (A, rect(0.0, 0.0, 548.0, 514.0)),
        (B, rect(550.0, 0.0, 484.0, 256.0)),
        (C, rect(550.0, 258.0, 96.0, 256.0)),
        (D, rect(648.0, 258.0, 128.0, 256.0)),
        (E, rect(778.0, 258.0, 256.0, 256.0)),
    ]);
    close(geometry.panes[&A].bounds.width() - 516.0, 32.0);
    close(516.0 - geometry.panes[&B].bounds.width(), 32.0);
    close(128.0 - geometry.panes[&C].bounds.width(), 32.0);
}

#[test]
fn nested_touching_edge_pointer_and_semantic_adjustments_preserve_the_far_pane() {
    for central in [C, D] {
        for timing in [OutputTiming::Immediate, OutputTiming::Deferred] {
            let (mut pointer, initial) = nested_pair(central);
            assert_pair_initial(&initial);
            let before = durable(&pointer.session);
            let start = point(515.0, 128.0);
            let target = initial
                .splitter_at(DockspaceAxis::Horizontal, start)
                .receiver;
            let after = pointer_resize(
                &mut pointer,
                start,
                target,
                point(64.0, 0.0),
                timing,
                assert_pair_resized,
            );
            changed_splits(&before.weights, &after.weights, &[true, false, true]);

            let (mut semantic, initial) = nested_pair(central);
            assert_pair_initial(&initial);
            assert_eq!(durable(&semantic.session), before);
            let target = initial.splitter_at(DockspaceAxis::Horizontal, start).visual;
            let report = semantic_resize(&mut semantic, target);
            assert!(report.workspace_changed());
            let (geometry, output) = paint(&mut semantic.session);
            assert_pair_resized(&geometry);
            present(&mut semantic.session, output);
            assert_eq!(
                durable(&semantic.session),
                after,
                "central={central:?}, output={timing:?}: the configured 64-point semantic step must match pointer weights bit for bit"
            );
        }
    }
}

#[test]
fn nested_touching_minimum_clamps_the_shared_delta_once_before_weight_reconstruction() {
    for timing in [OutputTiming::Immediate, OutputTiming::Deferred] {
        let (mut fixture, initial) = nested_minimum();
        assert_eq!(initial.splitters.len(), 4);
        initial.assert_panes(&[
            (A, rect(0.0, 0.0, 516.0, 514.0)),
            (B, rect(518.0, 0.0, 516.0, 256.0)),
            (C, rect(518.0, 258.0, 128.0, 256.0)),
            (D, rect(648.0, 258.0, 128.0, 256.0)),
            (E, rect(778.0, 258.0, 256.0, 256.0)),
        ]);
        let start = point(517.0, 128.0);
        let outer = initial.splitter_at(DockspaceAxis::Horizontal, start);
        close(outer.minimums.1, 400.0);
        let before = durable(&fixture.session);
        let after = pointer_resize(
            &mut fixture,
            start,
            outer.receiver,
            point(1000.0, 0.0),
            timing,
            assert_minimum_resized,
        );
        changed_splits(&before.weights, &after.weights, &[true, false, true]);

        let (mut semantic, initial) = nested_minimum();
        assert_eq!(durable(&semantic.session), before);
        let target = initial.splitter_at(DockspaceAxis::Horizontal, start).visual;
        let report = semantic_resize(&mut semantic, target);
        assert!(report.workspace_changed());
        let (geometry, output) = paint(&mut semantic.session);
        assert_minimum_resized(&geometry);
        present(&mut semantic.session, output);
        assert_eq!(
            durable(&semantic.session),
            after,
            "output={timing:?}: the configured 1000-point semantic step must share the nested minimum clamp"
        );
    }
}

fn assert_junction_initial(geometry: &Geometry) {
    assert_eq!(geometry.splitters.len(), 4);
    geometry.four_arm_junction();
    geometry.assert_panes(&[
        (A, rect(0.0, 0.0, 514.0, 256.0)),
        (C, rect(0.0, 258.0, 256.0, 256.0)),
        (D, rect(258.0, 258.0, 256.0, 256.0)),
        (E, rect(516.0, 0.0, 514.0, 256.0)),
        (F, rect(516.0, 258.0, 514.0, 256.0)),
    ]);
    close(geometry.panes[&A].content.height(), 228.0);
}

fn assert_junction_resized(geometry: &Geometry) {
    assert_eq!(geometry.splitters.len(), 4);
    geometry.assert_panes(&[
        (A, rect(0.0, 0.0, 578.0, 304.0)),
        (C, rect(0.0, 306.0, 256.0, 208.0)),
        (D, rect(258.0, 306.0, 320.0, 208.0)),
        (E, rect(580.0, 0.0, 450.0, 304.0)),
        (F, rect(580.0, 306.0, 450.0, 208.0)),
    ]);
}

fn single_split_resize_versions() -> (WorkspaceVersion, WorkspaceVersion) {
    let (mut fixture, initial) = nested_junction();
    assert_junction_initial(&initial);
    let before = durable(&fixture.session);
    let target = initial
        .splitter_at(DockspaceAxis::Horizontal, point(257.0, 386.0))
        .visual;
    let report = semantic_resize(&mut fixture, target);
    assert!(report.workspace_changed());
    let after = durable(&fixture.session);
    changed_splits(
        &before.weights,
        &after.weights,
        &[false, false, true, false],
    );
    assert_eq!(report.before(), before.version);
    assert_eq!(report.after(), after.version);
    assert_ne!(before.version, after.version);
    // WorkspaceVersion is opaque: use one real single-split command as the
    // version reference, without constructing or decoding private counters.
    (before.version, after.version)
}

#[test]
fn four_arm_junction_commits_all_four_splits_in_one_workspace_revision() {
    let (initial_version, one_resize_version) = single_split_resize_versions();
    for timing in [OutputTiming::Immediate, OutputTiming::Deferred] {
        let (mut fixture, initial) = nested_junction();
        assert_junction_initial(&initial);
        let before = durable(&fixture.session);
        assert_eq!(before.version, initial_version);
        assert_eq!(before.weights[1], [0.25_f32.to_bits(), 0.75_f32.to_bits()]);
        let target = initial.four_arm_junction();
        let after = pointer_resize(
            &mut fixture,
            target.center(),
            target,
            point(64.0, 48.0),
            timing,
            assert_junction_resized,
        );
        changed_splits(&before.weights, &after.weights, &[true; 4]);
        assert_eq!(
            after.version, one_resize_version,
            "all four split updates must advance exactly as one single-split command"
        );
    }
}

#[test]
fn four_arm_junction_failed_or_dropped_candidate_rolls_back_every_split_and_retries_once() {
    let (initial_version, one_resize_version) = single_split_resize_versions();
    for fail_commit in [false, true] {
        let (mut fixture, initial) = nested_junction();
        assert_junction_initial(&initial);
        let before = durable(&fixture.session);
        assert_eq!(before.version, initial_version);
        let target = initial.four_arm_junction();
        let start = PointerSample::at(&fixture.session, target.center());
        assert_eq!(start.drag.unwrap().descriptor(), target);
        let end = PointerSample::at(
            &fixture.session,
            point(target.center().x() + 64.0, target.center().y() + 48.0),
        );
        let mut frame = fixture.session.begin_host_frame().unwrap();
        frame
            .submit_host_pointer_batch([
                start.input(Event::ButtonPressed(Button::Primary)),
                end.input(Event::Moved),
                end.input(Event::ButtonReleased(Button::Primary)),
            ])
            .unwrap();
        measure(&mut frame, fixture.bounds, &fixture.minimums);
        assert_eq!(frame.version(), one_resize_version);
        let candidate_weights = weights(frame.view());
        changed_splits(&before.weights, &candidate_weights, &[true; 4]);
        // A complete release batch need not leave a Ready paint candidate.
        // The durable candidate view still exposes every tentative split update.
        if fail_commit {
            assert!(
                frame
                    .defer_surface(SURFACE, SurfaceUnavailableReason::Deferred)
                    .is_err()
            );
            assert!(frame.commit().is_err());
        } else {
            drop(frame);
        }
        assert_eq!(durable(&fixture.session), before);
        assert!(!fixture.session.has_active_gesture());
        assert_eq!(
            fixture.session.presented_surface(SURFACE),
            Some(start.surface)
        );

        let mut retry = fixture.session.begin_host_frame().unwrap();
        retry
            .submit_host_pointer_batch([
                start.input(Event::ButtonPressed(Button::Primary)),
                end.input(Event::Moved),
                end.input(Event::ButtonReleased(Button::Primary)),
            ])
            .expect("the exact uncommitted pointer sequence must remain retryable");
        measure(&mut retry, fixture.bounds, &fixture.minimums);
        assert_eq!(retry.version(), one_resize_version);
        changed_splits(&before.weights, &weights(retry.view()), &[true; 4]);
        assert_eq!(weights(retry.view()), candidate_weights);
        let report = commit(retry);
        assert_delivered(&report, before.version);
        assert_eq!(report.after(), one_resize_version);
        let after = durable(&fixture.session);
        changed_splits(&before.weights, &after.weights, &[true; 4]);
        // Measure the published resize in a fresh frame before rendering it.
        let mut measured = fixture.session.begin_host_frame().unwrap();
        measure(&mut measured, fixture.bounds, &fixture.minimums);
        assert!(!commit(measured).workspace_changed());
        assert_eq!(durable(&fixture.session), after);
        let (geometry, output) = paint(&mut fixture.session);
        assert_junction_resized(&geometry);
        present(&mut fixture.session, output);
        assert_eq!(durable(&fixture.session), after);
        assert!(!fixture.session.has_active_gesture());
        let settled = settle(&mut fixture.session);
        assert!(!settled.workspace_changed());
        assert!(settled.manipulation_feedback().is_empty());
        assert_eq!(durable(&fixture.session), after);
    }
}

#[test]
fn four_arm_junction_return_to_origin_restores_original_weight_bits_without_a_commit() {
    for timing in [OutputTiming::Immediate, OutputTiming::Deferred] {
        let (mut fixture, initial) = nested_junction();
        assert_junction_initial(&initial);
        let before = durable(&fixture.session);
        assert_eq!(before.weights[1], [0.25_f32.to_bits(), 0.75_f32.to_bits()]);
        let target = initial.four_arm_junction();
        let start = target.center();
        press(&mut fixture, start, target);
        let moved = point(start.x() + 64.0, start.y() + 48.0);
        assert!(!edge(&mut fixture, Event::Moved, moved).workspace_changed());
        let (resized_geometry, output) = paint(&mut fixture.session);
        assert_junction_resized(&resized_geometry);
        let pending = match timing {
            OutputTiming::Immediate => {
                present(&mut fixture.session, output);
                None
            }
            OutputTiming::Deferred => Some(output),
        };
        assert_eq!(durable(&fixture.session), before);
        let returned = edge(&mut fixture, Event::Moved, start);
        assert!(!returned.workspace_changed());
        assert_eq!(durable(&fixture.session), before);
        let (geometry, returned_output) = paint(&mut fixture.session);
        assert_junction_initial(&geometry);
        let released = edge(&mut fixture, Event::ButtonReleased(Button::Primary), start);
        assert!(!released.workspace_changed());
        assert_eq!(released.before(), before.version);
        assert_eq!(released.after(), before.version);
        let [terminal] = released.manipulation_feedback() else {
            panic!("the unchanged junction must still finish once: {released:?}")
        };
        assert_eq!(
            terminal.status(),
            Status::Delivered(Delivery::SplitterResize)
        );
        assert!(!fixture.session.has_active_gesture());
        assert_eq!(durable(&fixture.session), before);
        if let Some(pending) = pending {
            report_presentation(&mut fixture.session, pending, false);
            for splitter in &resized_geometry.splitters {
                assert!(
                    fixture
                        .session
                        .bind_presented_receiver(&splitter.receiver)
                        .is_none(),
                    "an unpresented resize projection cannot authorize a new gesture after returning to the original geometry"
                );
            }
        }
        present(&mut fixture.session, returned_output);
        let (geometry, output) = paint(&mut fixture.session);
        assert_junction_initial(&geometry);
        present(&mut fixture.session, output);
        assert_eq!(durable(&fixture.session), before);
        assert!(!settle(&mut fixture.session).workspace_changed());
    }
}

#[test]
fn four_arm_junction_semantic_axis_adjustments_share_delta_in_one_workspace_revision() {
    let (initial_version, one_resize_version) = single_split_resize_versions();
    for axis in [DockspaceAxis::Horizontal, DockspaceAxis::Vertical] {
        let (mut fixture, initial) = nested_junction();
        assert_junction_initial(&initial);
        close(
            fixture
                .session
                .presentation_config()
                .splitter_keyboard_step(),
            64.0,
        );
        let before = durable(&fixture.session);
        assert_eq!(before.version, initial_version);
        let target = initial.four_arm_junction();
        let mut frame = fixture.session.begin_host_frame().unwrap();
        let plan = frame.paint_plan(SURFACE).unwrap().unwrap();
        let mut action = None;
        plan.visit_surface_program(|step| {
            if let Step::Operation {
                operation: Operation::SplitterJunction(junction),
                ..
            } = step
                && junction.arm_count() == 4
                && junction.hit_bounds().contains(target.center())
            {
                assert!(junction.has_axis(axis));
                let prepared = plan
                    .prepare_splitter_junction_adjustment(
                        junction.visual_id(),
                        axis,
                        SurfaceSplitterAdjustment::Increment,
                    )
                    .expect("the exact junction supports an adjustment on this axis");
                assert!(action.replace(prepared).is_none(), "the junction is unique");
            }
            Ok::<(), ()>(())
        })
        .unwrap();
        frame
            .submit_surface_action(action.expect("the program contains the selected junction"))
            .unwrap();
        measure(&mut frame, fixture.bounds, &fixture.minimums);
        assert_eq!(frame.version(), one_resize_version);
        let report = commit(frame);
        assert!(report.workspace_changed());
        assert_eq!(report.before(), before.version);
        assert_eq!(report.after(), one_resize_version);
        let after = durable(&fixture.session);
        assert_eq!(after.version, one_resize_version);
        let (geometry, output) = paint(&mut fixture.session);
        assert_eq!(geometry.splitters.len(), 4);
        let boundary_positions = match axis {
            DockspaceAxis::Horizontal => {
                changed_splits(&before.weights, &after.weights, &[true, false, true, false]);
                geometry.assert_panes(&[
                    (A, rect(0.0, 0.0, 578.0, 256.0)),
                    (C, rect(0.0, 258.0, 256.0, 256.0)),
                    (D, rect(258.0, 258.0, 320.0, 256.0)),
                    (E, rect(580.0, 0.0, 450.0, 256.0)),
                    (F, rect(580.0, 258.0, 450.0, 256.0)),
                ]);
                vec![point(515.0, 128.0)]
            }
            DockspaceAxis::Vertical => {
                changed_splits(&before.weights, &after.weights, &[false, true, false, true]);
                geometry.assert_panes(&[
                    (A, rect(0.0, 0.0, 514.0, 320.0)),
                    (C, rect(0.0, 322.0, 256.0, 192.0)),
                    (D, rect(258.0, 322.0, 256.0, 192.0)),
                    (E, rect(516.0, 0.0, 514.0, 320.0)),
                    (F, rect(516.0, 322.0, 514.0, 192.0)),
                ]);
                vec![point(128.0, 257.0), point(772.0, 257.0)]
            }
        };
        for position in boundary_positions {
            let old = initial.splitter_at(axis, position);
            let new = geometry
                .splitters
                .iter()
                .find(|splitter| splitter.visual == old.visual)
                .expect("the adjusted boundary retains its public visual identity");
            close(
                match axis {
                    DockspaceAxis::Horizontal => new.draw.x() - old.draw.x(),
                    DockspaceAxis::Vertical => new.draw.y() - old.draw.y(),
                },
                64.0,
            );
        }
        assert!(!fixture.session.has_active_gesture());
        present(&mut fixture.session, output);
        assert_eq!(durable(&fixture.session), after);
        let settled = settle(&mut fixture.session);
        assert!(!settled.workspace_changed());
        assert_eq!(durable(&fixture.session), after);
    }
}
