//! Real completed-pass hits keep independently occluded click and drag lanes coherent.

use dockspace::geometry::{LogicalRect, LogicalSize, PhysicalPoint, PhysicalRect, ScaleFactor};
use dockspace::model::{
    DockspaceAxis, DockspaceLayout, DockspaceNode, DockspaceRootLayout, DockspaceSurfaceLayout,
    RootId,
};
use dockspace::runtime::{
    DockspaceManipulationDelivery, DockspaceManipulationStatus, DockspaceReceiverRole,
    DockspaceSession, HostWindowToken, NativeDesktopPointerLocation, NativeDesktopPosition,
    NativePointerButton, NativePointerEvent, NativePointerHover, NativePointerId,
    NativePointerInput, NativePointerOwner, NativePointerRoster, NativeWindowFacts,
    NativeWindowInputState, NativeWindowPresentationState, NativeWorkAreaRoster,
    SurfacePaintProgramOperation, SurfacePaintProgramStep, SurfacePresentationResult,
    SurfaceUnavailableReason, UniformSurfaceMetrics,
};
use egui_dockspace::DockStyle;

use super::tests::TestPanes;
use super::*;
use crate::host_frame::NativeHostFrame;
use crate::mailbox::NativeHostBridge;
use crate::viewport_map::NativeViewportMap;

const SURFACE: SurfaceId = SurfaceId::new(1);

struct SplitterFixture {
    session: DockspaceSession,
    binding: NativeSurfaceBinding,
    context: egui::Context,
    store: NativeReceiverStore,
    zoom: f32,
}

impl SplitterFixture {
    fn new(axis: DockspaceAxis, zoom: f32) -> Self {
        let node = DockspaceNode::equal_split(
            axis,
            [
                DockspaceNode::central_tabs([ItemId::new(1)]),
                DockspaceNode::tabs([ItemId::new(2)]),
            ],
        )
        .unwrap();
        let layout = DockspaceLayout::new([DockspaceSurfaceLayout::new(
            SURFACE,
            DockspaceRootLayout::new(RootId::new(1), node),
        )])
        .unwrap();
        let mut session = DockspaceSession::from_layout(layout, Default::default()).unwrap();
        session
            .enable_managed_native_host(NativePointerRoster::Exact(Vec::new()))
            .unwrap();
        session
            .configure_managed_native_capabilities(crate::capabilities::capabilities_for_backend(
                eframe::NativeWindowingBackend::Windows,
            ))
            .unwrap();
        session
            .register_native_root(SURFACE, HostWindowToken::new(1))
            .unwrap();
        let binding = idle(&mut session).native_bindings()[0];
        let bounds = PhysicalRect::new(0.0, 0.0, 800.0, 600.0).unwrap();
        session
            .report_managed_native_snapshot(
                [(
                    binding,
                    NativeWindowFacts::live()
                        .with_content_bounds(bounds)
                        .with_outer_bounds(bounds)
                        .with_native_scale_factor(ScaleFactor::new(1.0).unwrap())
                        .with_presentation_scale_factor(ScaleFactor::new(f64::from(zoom)).unwrap())
                        .with_input(NativeWindowInputState::ReceivesInput, None)
                        .with_presentation(NativeWindowPresentationState::Visible),
                )],
                NativeWorkAreaRoster::Unknown,
            )
            .unwrap();
        idle(&mut session);
        let context = egui::Context::default();
        context.options_mut(|options| options.zoom_factor = zoom);
        Self {
            session,
            binding,
            context,
            store: NativeReceiverStore::default(),
            zoom,
        }
    }

    fn publish(&mut self) -> (LogicalRect, LogicalRect) {
        let screen =
            egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(800.0, 600.0) / self.zoom);
        let mut painted = None;
        let mut output = None;
        let mut splitter = None;
        let bridge = Arc::new(NativeHostBridge::new(NativeViewportMap::default()));
        self.context
            .run_ui(
                egui::RawInput {
                    screen_rect: Some(screen),
                    ..Default::default()
                },
                |ui| {
                    let mut frame = NativeHostFrame::new(
                        self.session
                            .begin_native_host_frame(|_| NativeReceiverAnswer::Unknown)
                            .unwrap(),
                        Arc::clone(&bridge),
                        &mut self.store,
                        None,
                    );
                    frame
                        .frame
                        .measure_surface(
                            SURFACE,
                            UniformSurfaceMetrics::new(
                                LogicalRect::new(
                                    0.0,
                                    0.0,
                                    f64::from(screen.width()),
                                    f64::from(screen.height()),
                                )
                                .unwrap(),
                                LogicalSize::new(32.0, 24.0).unwrap(),
                                80.0,
                            )
                            .unwrap(),
                        )
                        .unwrap();
                    frame
                        .frame
                        .paint_plan(SURFACE)
                        .unwrap()
                        .unwrap()
                        .visit_surface_program(|step| {
                            if let SurfacePaintProgramStep::Operation {
                                operation: SurfacePaintProgramOperation::Splitter(record),
                                ..
                            } = step
                            {
                                assert!(record.operable());
                                assert!(
                                    splitter
                                        .replace((record.hit_bounds(), record.draw_bounds()))
                                        .is_none()
                                );
                            }
                            Ok::<_, ()>(())
                        })
                        .unwrap();
                    let mut paint = frame
                        .paint_surface(
                            egui::Id::unique("native-splitter-lanes"),
                            SURFACE,
                            ui,
                            &mut TestPanes,
                            &DockStyle::default(),
                        )
                        .unwrap();
                    assert!(paint.confirm_into(&mut frame.frame).unwrap());
                    frame
                        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
                        .unwrap();
                    output = frame.commit().unwrap().take_painted_outputs().pop();
                    painted = Some(paint);
                },
            )
            .drop_without_applying_deltas();
        let paint = painted.unwrap();
        let output = output.unwrap();
        super::test_support::present_completed_output(
            &mut self.store,
            &self.context,
            self.zoom,
            self.binding,
            &output,
            &paint,
        )
        .unwrap();
        self.session
            .report_surface_presentation(output, SurfacePresentationResult::Presented)
            .unwrap();
        let report = idle(&mut self.session);
        self.store
            .commit_presentations(report.surface_presentation_commits());
        assert_eq!(self.store.presented[&SURFACE].len(), 1);
        splitter.unwrap()
    }

    fn pointer(
        &mut self,
        point: LogicalPoint,
        event: NativePointerEvent,
        trace: &str,
    ) -> (
        dockspace::runtime::HostFrameReport,
        [Option<NativeReceiverAnswer>; 2],
    ) {
        let physical = PhysicalPoint::new(
            point.x() * f64::from(self.zoom),
            point.y() * f64::from(self.zoom),
        )
        .unwrap();
        self.session
            .record_native_pointer(NativePointerInput::new(
                NativePointerId::new(1),
                event,
                NativeDesktopPointerLocation::new(
                    NativeDesktopPosition::Exact(physical),
                    NativePointerHover::Dock(self.binding.window()),
                    None,
                ),
                NativePointerOwner::Native(self.binding.window()),
                NativePointerOwner::Native(self.binding.window()),
            ))
            .unwrap();
        let mut answers = [None; 2];
        let mut frame = self
            .session
            .begin_native_host_frame(|query| {
                let answer = self.store.resolve(query);
                if let NativeReceiverQuery::Surface(query) = query {
                    match query.purpose() {
                        NativeReceiverPurpose::ClickDelivery => answers[0] = Some(answer),
                        NativeReceiverPurpose::DragDelivery => answers[1] = Some(answer),
                        _ => {}
                    }
                }
                answer
            })
            .unwrap_or_else(|error| {
                panic!("{trace}, point={point:?}, event={event:?}, answers={answers:?}: {error:?}")
            });
        frame
            .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
            .unwrap();
        (frame.commit().unwrap(), answers)
    }
}

fn idle(session: &mut DockspaceSession) -> dockspace::runtime::HostFrameReport {
    let mut frame = session
        .begin_native_host_frame(|_| NativeReceiverAnswer::Unknown)
        .unwrap();
    frame
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .unwrap();
    frame.commit().unwrap()
}

#[test]
fn real_splitter_hit_padding_keeps_click_occlusion_separate_from_resize_delivery() {
    for axis in [DockspaceAxis::Horizontal, DockspaceAxis::Vertical] {
        for zoom in [1.0_f32, 1.2] {
            for side in [0.25, 0.75] {
                let trace = format!("splitter-padding axis={axis:?}, zoom={zoom}, side={side}");
                let mut fixture = SplitterFixture::new(axis, zoom);
                let (hit, draw) = fixture.publish();
                let point = match axis {
                    DockspaceAxis::Horizontal => LogicalPoint::new(
                        hit.x() + hit.width() * side,
                        hit.y() + hit.height() * 0.75,
                    )
                    .unwrap(),
                    DockspaceAxis::Vertical => {
                        LogicalPoint::new(hit.x() + 40.0, hit.y() + hit.height() * side).unwrap()
                    }
                };
                assert!(
                    hit.contains(point) && !draw.contains(point),
                    "{trace}: {hit:?}, {draw:?}, {point:?}"
                );
                let before_version = fixture.session.version();
                let (_, answers) = fixture.pointer(
                    point,
                    NativePointerEvent::ButtonPressed(NativePointerButton::Primary),
                    &trace,
                );
                assert!(
                    matches!(answers[0], Some(NativeReceiverAnswer::Blocked(_))),
                    "{trace}: the drag-only widget occludes the underlying pane's click lane: {answers:?}"
                );
                assert!(
                    matches!(answers[1], Some(NativeReceiverAnswer::Dock(receiver)) if receiver.descriptor().role() == DockspaceReceiverRole::Splitter),
                    "{trace}: {answers:?}"
                );
                assert!(fixture.session.has_active_gesture(), "{trace}");
                let moved = match axis {
                    DockspaceAxis::Horizontal => {
                        LogicalPoint::new(point.x() + 24.0, point.y()).unwrap()
                    }
                    DockspaceAxis::Vertical => {
                        LogicalPoint::new(point.x(), point.y() + 24.0).unwrap()
                    }
                };
                fixture.pointer(moved, NativePointerEvent::Moved, &trace);
                let (released, _) = fixture.pointer(
                    moved,
                    NativePointerEvent::ButtonReleased(NativePointerButton::Primary),
                    &trace,
                );
                assert!(released.workspace_changed(), "{trace}: {released:?}");
                assert!(
                    released
                        .manipulation_feedback()
                        .iter()
                        .any(|feedback| feedback.status()
                            == DockspaceManipulationStatus::Delivered(
                                DockspaceManipulationDelivery::SplitterResize
                            )),
                    "{trace}: {released:?}"
                );
                assert!(!fixture.session.has_active_gesture(), "{trace}");
                assert!(!fixture.session.has_pending_presentation(), "{trace}");
                assert_ne!(fixture.session.version(), before_version, "{trace}");
                let view = fixture.session.view();
                let split = view
                    .root(RootId::new(1))
                    .unwrap()
                    .content()
                    .unwrap()
                    .split()
                    .unwrap();
                let items = split
                    .children()
                    .flat_map(|child| child.tabs().unwrap().items().iter().copied())
                    .collect::<Vec<_>>();
                assert_eq!(items, [ItemId::new(1), ItemId::new(2)], "{trace}");
                let (_, moved_draw) = fixture.publish();
                assert_ne!(
                    draw, moved_draw,
                    "{trace}: the resize must actually move the divider"
                );
            }
        }
    }
}
