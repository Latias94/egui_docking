use crate::geometry::{LogicalPoint, LogicalRect, LogicalSize};
use crate::model::{
    DockspaceAxis, DockspaceContainedLayout, DockspaceLayout, DockspaceNode, DockspaceRootLayout,
    DockspaceSurfaceLayout, FloatingPresentationId, ItemId, RootId, SurfaceId,
};
use crate::policy::DockPolicy;
use crate::presentation::compiler::{reset_surface_compilation_count, surface_compilation_count};
use crate::runtime::{
    ContainedResizeDirection, DockspaceHostFrame, DockspaceReceiverDescriptor, DockspaceSession,
    DockspaceVisualId, HostPointerInput, HostPointerRoute, PaintedSurfaceOutput,
    SurfacePaintDisposition, SurfacePaintProgramOperation as Operation,
    SurfacePaintProgramStep as Step, SurfacePointerButton, SurfacePointerCancelReason,
    SurfacePointerCapture, SurfacePointerEvent as Event, SurfacePointerId,
    SurfacePointerReceiverFacts, SurfacePresentationResult, SurfaceUnavailableReason,
    UniformSurfaceMetrics,
};

mod authority_tests;

mod layout_checkpoint;

mod measurement_tests;

mod selection_cost_tests;

const SURFACE: SurfaceId = SurfaceId::new(1);
const MAIN_ROOT: RootId = RootId::new(1);
const CONTAINED_ROOT: RootId = RootId::new(2);
const FLOATING: FloatingPresentationId = FloatingPresentationId::new(1);
const LEFT: ItemId = ItemId::new(1);
const RIGHT: ItemId = ItemId::new(2);
const MOVE_COUNT: u32 = 64;

fn rect(x: f64, y: f64, width: f64, height: f64) -> LogicalRect {
    LogicalRect::new(x, y, width, height).unwrap()
}

fn initial_rect() -> LogicalRect {
    rect(200.0, 140.0, 400.0, 300.0)
}

fn offset(point: LogicalPoint, dx: f64, dy: f64) -> LogicalPoint {
    LogicalPoint::new(point.x() + dx, point.y() + dy).unwrap()
}

struct Painted {
    outer: LogicalRect,
    title: DockspaceReceiverDescriptor,
    resize: DockspaceReceiverDescriptor,
    panes: Vec<(DockspaceVisualId, LogicalRect)>,
}

impl Painted {
    fn assert_projection(&self, before: &Self, expected: LogicalRect) {
        assert_eq!(self.outer, expected);
        let dx = expected.x() - before.outer.x();
        let dy = expected.y() - before.outer.y();
        let dw = (expected.width() - before.outer.width()) * 0.5;
        let dh = expected.height() - before.outer.height();
        assert_eq!(self.panes.len(), 2);
        for (index, ((visual, bounds), (old_visual, old_bounds))) in
            self.panes.iter().zip(&before.panes).enumerate()
        {
            assert_eq!(visual, old_visual);
            let pane_dx = if index == 0 { dx } else { dx + dw };
            assert_eq!(
                *bounds,
                rect(
                    old_bounds.x() + pane_dx,
                    old_bounds.y() + dy,
                    old_bounds.width() + dw,
                    old_bounds.height() + dh,
                ),
                "the complete split content, not just its shell, must use the final batch point"
            );
        }
    }
}

fn paint_frame(
    mut frame: DockspaceHostFrame<'_>,
    pane_count: usize,
) -> (Painted, PaintedSurfaceOutput) {
    let plan = frame.paint_plan(SURFACE).unwrap().unwrap();
    let semantic = plan.semantic_output();
    let mut outer = None;
    let mut title = None;
    let mut resize = None;
    let mut panes = Vec::new();
    let mut total_panes = 0;
    plan.visit_surface_program(|step| {
        match step {
            Step::RootBegin { root, .. } if root.root() == CONTAINED_ROOT => {
                let contained = root.contained().unwrap();
                assert_eq!(contained.floating(), FLOATING);
                assert!(outer.replace(contained.outer_bounds()).is_none());
                title = plan.receiver_for_contained_title(contained);
                let handle = contained
                    .resize()
                    .find(|handle| handle.direction() == ContainedResizeDirection::SouthEast)
                    .unwrap();
                resize = plan.receiver_for_contained_resize(contained, handle);
            }
            Step::Operation {
                operation: Operation::Pane(pane),
                paint,
                ..
            } => {
                total_panes += 1;
                if pane.root() == CONTAINED_ROOT {
                    assert_eq!(paint, SurfacePaintDisposition::Visible);
                    assert_eq!(pane.selected(), Some([LEFT, RIGHT][panes.len()]));
                    panes.push((pane.visual_id(), pane.content_bounds()));
                }
            }
            _ => {}
        }
        Ok::<(), ()>(())
    })
    .unwrap();
    assert_eq!(total_panes, pane_count);
    assert_eq!(panes.len(), 2);
    let receipt = plan.execute_surface_program(|_| Ok::<(), ()>(())).unwrap();

    for _ in 0..8 {
        assert_eq!(
            frame
                .paint_plan(SURFACE)
                .unwrap()
                .unwrap()
                .semantic_output(),
            semantic,
            "read-only lookups must keep the exact compiled output"
        );
    }
    frame.confirm_surface_painted(receipt).unwrap();
    frame
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .unwrap();
    let mut report = frame.commit().unwrap();
    assert!(!report.workspace_changed());
    let mut outputs = report.take_painted_outputs();
    assert_eq!(outputs.len(), 1);
    let output = outputs.pop().unwrap();
    assert_eq!(output.surface(), SURFACE);
    assert!(output.matches_semantic_output(semantic));
    (
        Painted {
            outer: outer.unwrap(),
            title: title.unwrap(),
            resize: resize.unwrap(),
            panes,
        },
        output,
    )
}

fn present(session: &mut DockspaceSession, output: PaintedSurfaceOutput) {
    let artifact = output.presentation_artifact();
    session
        .report_surface_presentation(output, SurfacePresentationResult::Presented)
        .unwrap();
    let mut frame = session.begin_host_frame().unwrap();
    frame
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .unwrap();
    let report = frame.commit().unwrap();
    assert!(!report.workspace_changed());
    let [commit] = report.surface_presentation_commits() else {
        panic!("the exact executed output must settle once");
    };
    assert!(commit.settles(artifact));
    assert!(commit.presents(artifact));
}

fn fixture(pane_count: usize) -> (DockspaceSession, Painted) {
    let background = DockspaceNode::equal_split(
        DockspaceAxis::Horizontal,
        (3..=pane_count).map(|index| {
            let item = ItemId::new(u64::try_from(index).unwrap());
            if index == 3 {
                DockspaceNode::central_tabs([item])
            } else {
                DockspaceNode::tabs([item])
            }
        }),
    )
    .unwrap();
    let contained = DockspaceNode::equal_split(
        DockspaceAxis::Horizontal,
        [DockspaceNode::tabs([LEFT]), DockspaceNode::tabs([RIGHT])],
    )
    .unwrap();
    let layout = DockspaceLayout::new([DockspaceSurfaceLayout::new(
        SURFACE,
        DockspaceRootLayout::new(MAIN_ROOT, background),
    )
    .with_contained(DockspaceContainedLayout::new(
        FLOATING,
        DockspaceRootLayout::new(CONTAINED_ROOT, contained),
        initial_rect(),
    ))])
    .unwrap();
    let mut session = DockspaceSession::from_layout(layout, DockPolicy::default()).unwrap();
    session.enable_host_pointer().unwrap();
    let mut frame = session.begin_host_frame().unwrap();
    #[cfg(feature = "motion")]
    frame
        .set_motion(crate::motion::MotionFrame {
            now: std::time::Duration::ZERO,
            settings: crate::motion::MotionSettings::default(),
            preference: crate::motion::MotionPreference::Full,
        })
        .unwrap();
    // Keep every background pane above its minimum at all three fixture sizes.
    frame
        .measure_surface(
            SURFACE,
            UniformSurfaceMetrics::new(
                rect(0.0, 0.0, 131_072.0, 720.0),
                LogicalSize::new(64.0, 48.0).unwrap(),
                96.0,
            )
            .unwrap(),
        )
        .unwrap();
    frame.commit().unwrap();
    let (painted, output) = paint_frame(session.begin_host_frame().unwrap(), pane_count);
    assert_eq!(painted.outer, initial_rect());
    present(&mut session, output);
    (session, painted)
}

fn press(session: &mut DockspaceSession, descriptor: DockspaceReceiverDescriptor) -> LogicalPoint {
    let receiver = session.bind_presented_receiver(&descriptor).unwrap();
    let surface = session.presented_surface(SURFACE).unwrap();
    let position = receiver.center();
    let route = HostPointerRoute::new(surface, position);
    let mut frame = session.begin_host_frame().unwrap();
    frame
        .submit_host_pointer_batch([HostPointerInput::new(
            SurfacePointerId::new(1),
            Event::ButtonPressed(SurfacePointerButton::Primary),
            route,
            route,
            SurfacePointerCapture::ProviderEndpoint,
            SurfacePointerReceiverFacts::delivery(&receiver).with_clear_hover_lane(&surface),
        )])
        .unwrap();
    frame
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .unwrap();
    assert!(!frame.commit().unwrap().workspace_changed());
    position
}

#[test]
fn host_local_move_batches_compile_once_and_reuse_the_plan_at_every_workspace_size() {
    for pane_count in [16, 128, 1_024] {
        for resize in [false, true] {
            let (mut session, before) = fixture(pane_count);
            let version = session.version();
            let start = press(
                &mut session,
                if resize { before.resize } else { before.title },
            );
            let surface = session.presented_surface(SURFACE).unwrap();
            reset_surface_compilation_count();
            let mut frame = session.begin_host_frame().unwrap();
            frame
                .submit_host_pointer_batch((1..=MOVE_COUNT).map(|index| {
                    let route = HostPointerRoute::new(
                        surface,
                        offset(start, f64::from(index), f64::from(index) * 0.5),
                    );
                    HostPointerInput::new(
                        SurfacePointerId::new(1),
                        Event::Moved,
                        route,
                        route,
                        SurfacePointerCapture::ProviderEndpoint,
                        SurfacePointerReceiverFacts::unknown().with_clear_hover_lane(&surface),
                    )
                }))
                .unwrap();
            assert_eq!(
                surface_compilation_count(),
                0,
                "{pane_count} panes, resize={resize}: input edges must not compile"
            );
            let (painted, output) = paint_frame(frame, pane_count);
            let dx = f64::from(MOVE_COUNT);
            let dy = dx * 0.5;
            let expected = if resize {
                rect(200.0, 140.0, 400.0 + dx, 300.0 + dy)
            } else {
                rect(200.0 + dx, 140.0 + dy, 400.0, 300.0)
            };
            painted.assert_projection(&before, expected);
            assert_eq!(
                surface_compilation_count(),
                1,
                "{pane_count} panes, resize={resize}: one batch needs exactly one compilation"
            );
            assert!(session.has_active_gesture());
            assert_eq!(session.version(), version);
            assert_eq!(
                session.view().contained(FLOATING).unwrap().rect(),
                initial_rect()
            );
            present(&mut session, output);
            assert_eq!(surface_compilation_count(), 1);

            for _ in 0..3 {
                let (idle, output) = paint_frame(session.begin_host_frame().unwrap(), pane_count);
                idle.assert_projection(&before, expected);
                present(&mut session, output);
                assert_eq!(
                    surface_compilation_count(),
                    1,
                    "{pane_count} panes, resize={resize}: unchanged idle/paint frames must reuse layout"
                );
                assert!(session.has_active_gesture());
                assert_eq!(session.version(), version);
                assert_eq!(
                    session.view().contained(FLOATING).unwrap().rect(),
                    initial_rect()
                );
            }
        }
    }
}

#[test]
fn host_local_cancel_restores_durable_split_once_at_input_close_then_reuses_it() {
    for pane_count in [16, 128, 1_024] {
        let (mut session, before) = fixture(pane_count);
        let version = session.version();
        let start = press(&mut session, before.resize);
        let moved = offset(start, 64.0, 32.0);
        let surface = session.presented_surface(SURFACE).unwrap();
        let route = HostPointerRoute::new(surface, moved);
        let mut frame = session.begin_host_frame().unwrap();
        frame
            .submit_host_pointer_batch([HostPointerInput::new(
                SurfacePointerId::new(1),
                Event::Moved,
                route,
                route,
                SurfacePointerCapture::ProviderEndpoint,
                SurfacePointerReceiverFacts::unknown(),
            )])
            .unwrap();
        let (projected, output) = paint_frame(frame, pane_count);
        projected.assert_projection(&before, rect(200.0, 140.0, 464.0, 332.0));
        present(&mut session, output);
        assert!(session.has_active_gesture());

        let surface = session.presented_surface(SURFACE).unwrap();
        let route = HostPointerRoute::new(surface, moved);
        reset_surface_compilation_count();
        let mut frame = session.begin_host_frame().unwrap();
        frame
            .submit_host_pointer_batch([HostPointerInput::new(
                SurfacePointerId::new(1),
                Event::StreamCancelled(SurfacePointerCancelReason::ExplicitPlatformCancellation),
                route,
                route,
                SurfacePointerCapture::ProviderEndpoint,
                SurfacePointerReceiverFacts::unknown(),
            )])
            .unwrap();
        assert_eq!(surface_compilation_count(), 0);
        frame
            .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
            .unwrap();
        assert_eq!(
            surface_compilation_count(),
            1,
            "input close restores layout"
        );
        assert!(!frame.commit().unwrap().workspace_changed());
        assert!(!session.has_active_gesture());

        for _ in 0..3 {
            let (restored, output) = paint_frame(session.begin_host_frame().unwrap(), pane_count);
            restored.assert_projection(&before, initial_rect());
            present(&mut session, output);
            assert_eq!(
                surface_compilation_count(),
                1,
                "{pane_count} panes: restore compiles once"
            );
            assert_eq!(session.version(), version);
            assert_eq!(
                session.view().contained(FLOATING).unwrap().rect(),
                initial_rect()
            );
        }
    }
}
