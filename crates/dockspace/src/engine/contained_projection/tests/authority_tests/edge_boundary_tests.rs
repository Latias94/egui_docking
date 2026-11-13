use super::*;

use crate::runtime::{SurfaceWork, SurfaceWorkDeferred, TabGroupDragRegionKind};

struct BoundaryPaint {
    outer: LogicalRect,
    title: LogicalRect,
    title_receiver: Option<DockspaceReceiverDescriptor>,
    group_receiver: Option<DockspaceReceiverDescriptor>,
    panes: Vec<(ItemId, SurfacePaintDisposition)>,
    root_paint: SurfacePaintDisposition,
    projected_root: Option<LogicalRect>,
}

fn measure_bounds(session: &mut DockspaceSession, bounds: LogicalRect) {
    let mut frame = session.begin_host_frame().unwrap();
    frame
        .measure_surface(
            SURFACE,
            UniformSurfaceMetrics::new(bounds, LogicalSize::new(64.0, 48.0).unwrap(), 96.0)
                .unwrap(),
        )
        .unwrap();
    frame.commit().unwrap();
}

fn paint_boundary(mut frame: DockspaceHostFrame<'_>) -> (BoundaryPaint, PaintedSurfaceOutput) {
    let plan = frame.paint_plan(SURFACE).unwrap().unwrap();
    let mut contained = None;
    let mut group_receiver = None;
    let mut panes = Vec::new();
    let mut projected_root = None;
    plan.visit_surface_program(|step| {
        match step {
            Step::RootBegin { root, paint } if root.root() == CONTAINED_ROOT => {
                let record = root.contained().unwrap();
                contained = Some((
                    record.outer_bounds(),
                    record.title_bounds(),
                    plan.receiver_for_contained_title(record),
                    paint,
                ));
            }
            Step::Operation {
                root,
                operation: Operation::Pane(pane),
                paint,
            } if root.root() == CONTAINED_ROOT => {
                panes.push((pane.selected().unwrap(), paint));
            }
            Step::Operation {
                root,
                operation: Operation::TabBar(bar),
                ..
            } if root.root() == CONTAINED_ROOT && group_receiver.is_none() => {
                group_receiver = bar
                    .group_drag_region(TabGroupDragRegionKind::LeadingGrip)
                    .and_then(|region| plan.receiver_for_tab_group_drag_region(region));
            }
            Step::DragDecoration(decoration) => {
                if decoration.source_is_projected() {
                    projected_root = decoration.projected_root_bounds();
                }
            }
            _ => {}
        }
        Ok::<(), ()>(())
    })
    .unwrap();
    let receipt = plan.execute_surface_program(|_| Ok::<(), ()>(())).unwrap();
    let (outer, title, title_receiver, root_paint) = contained.unwrap();
    frame.confirm_surface_painted(receipt).unwrap();
    frame
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .unwrap();
    let mut report = frame.commit().unwrap();
    let mut outputs = report.take_painted_outputs();
    assert_eq!(outputs.len(), 1);
    (
        BoundaryPaint {
            outer,
            title,
            title_receiver,
            group_receiver,
            panes,
            root_paint,
            projected_root,
        },
        outputs.pop().unwrap(),
    )
}

fn fixture_at_boundary(
    source: LogicalRect,
    bounds: LogicalRect,
) -> (DockspaceSession, BoundaryPaint) {
    let (engine, _) = core_fixture_at(source);
    let mut session = DockspaceSession::from_workspace_for_test(
        engine.workspace().clone(),
        DockPolicy::default(),
    )
    .unwrap();
    session.enable_host_pointer().unwrap();
    measure_bounds(&mut session, bounds);
    let (painted, output) = paint_boundary(session.begin_host_frame().unwrap());
    present(&mut session, output);
    (session, painted)
}

fn submit_boundary_edge(
    session: &mut DockspaceSession,
    bounds: LogicalRect,
    position: LogicalPoint,
    event: Event,
) -> (BoundaryPaint, PaintedSurfaceOutput) {
    let surface = session.presented_surface(SURFACE).unwrap();
    let route = HostPointerRoute::new(surface, position);
    let mut frame = session.begin_host_frame().unwrap();
    frame
        .submit_host_pointer_batch([HostPointerInput::new(
            SurfacePointerId::new(1),
            event,
            route,
            route,
            SurfacePointerCapture::ProviderEndpoint,
            SurfacePointerReceiverFacts::unknown().with_clear_hover_lane(&surface),
        )])
        .unwrap();
    // A successful release commits durable geometry and can request new
    // measurements. Match the native adapter's Unmeasured admission path.
    if matches!(
        frame.surface_work(SURFACE).unwrap(),
        SurfaceWork::Deferred(SurfaceWorkDeferred::Unmeasured)
    ) {
        frame
            .measure_surface(
                SURFACE,
                UniformSurfaceMetrics::new(bounds, LogicalSize::new(64.0, 48.0).unwrap(), 96.0)
                    .unwrap(),
            )
            .unwrap();
    }
    paint_boundary(frame)
}

fn assert_visible_content(painted: &BoundaryPaint) {
    assert_eq!(painted.root_paint, SurfacePaintDisposition::Visible);
    assert_eq!(
        painted.panes,
        [
            (LEFT, SurfacePaintDisposition::Visible),
            (RIGHT, SurfacePaintDisposition::Visible),
        ],
        "a live complete-root projection must keep both real pane bodies visible"
    );
}

#[test]
fn contained_title_drag_recovers_full_chrome_inside_toolbar_bounds() {
    for top in [0.0_f64, 94.0, 124.0] {
        let bounds = rect(0.0, top, 1_280.0, 726.0);
        let source = rect(850.0, top - 14.0, 360.0, 410.0);
        let (mut session, clipped) = fixture_at_boundary(source, bounds);
        assert_eq!(session.view().contained(FLOATING).unwrap().rect(), source);
        assert_eq!(clipped.outer.y(), top);
        assert!(clipped.title.height() < session.presentation_config().floating_title_height());
        assert_visible_content(&clipped);

        let start = press(&mut session, clipped.title_receiver.unwrap());
        let moved = offset(start, -40.0, -80.0);
        let expected = rect(810.0, top, source.width(), source.height());
        let (projected, output) = submit_boundary_edge(&mut session, bounds, moved, Event::Moved);
        assert!(session.has_active_gesture());
        assert_eq!(projected.projected_root, Some(expected));
        assert_eq!(projected.outer, expected);
        assert_eq!(
            projected.title.height(),
            session.presentation_config().floating_title_height()
        );
        assert_visible_content(&projected);
        assert_eq!(session.view().contained(FLOATING).unwrap().rect(), source);
        present(&mut session, output);

        let (released, output) = submit_boundary_edge(
            &mut session,
            bounds,
            moved,
            Event::ButtonReleased(SurfacePointerButton::Primary),
        );
        assert!(!session.has_active_gesture());
        assert_eq!(session.view().contained(FLOATING).unwrap().rect(), expected);
        assert_eq!(released.outer, expected);
        assert_visible_content(&released);
        present(&mut session, output);
    }
}

#[test]
fn fractional_contained_title_translation_projects_and_commits_its_canonical_rect() {
    let bounds = rect(0.0, 70.0, 1_280.0, 750.0);
    let source = rect(
        71.890_571_594_238_28,
        296.400_008_916_854_86,
        246.163_301_463_831_52 - 71.890_571_594_238_28,
        820.0 - 296.400_008_916_854_86,
    );
    let expected = rect(
        source.x() + 24.0,
        source.y() - 24.0,
        source.width(),
        source.height(),
    );
    let (mut session, before) = fixture_at_boundary(source, bounds);
    let start = press(&mut session, before.title_receiver.unwrap());
    let moved = offset(start, 24.0, -24.0);

    let (projected, output) = submit_boundary_edge(&mut session, bounds, moved, Event::Moved);
    assert!(session.has_active_gesture());
    assert_eq!(projected.projected_root, Some(expected));
    assert_eq!(projected.outer, expected);
    assert_visible_content(&projected);
    assert_eq!(session.view().contained(FLOATING).unwrap().rect(), source);
    present(&mut session, output);

    let (released, output) = submit_boundary_edge(
        &mut session,
        bounds,
        moved,
        Event::ButtonReleased(SurfacePointerButton::Primary),
    );
    assert!(!session.has_active_gesture());
    let durable = session.view().contained(FLOATING).unwrap().rect();
    assert_eq!(durable, expected);
    assert_ne!(durable, source);
    for (extent, original) in [
        (durable.width(), source.width()),
        (durable.height(), source.height()),
    ] {
        assert!(extent.is_finite());
        assert!((extent - original).abs() < 1e-9);
    }
    assert_eq!(released.outer, expected);
    assert_visible_content(&released);
    present(&mut session, output);
}

#[test]
fn oversized_contained_title_drag_preserves_size_instead_of_forcing_full_title_visibility() {
    let top = 124.0;
    let bounds = rect(0.0, top, 1_280.0, 726.0);
    let source = rect(0.0, top, 1_500.0, 900.0);
    let (mut session, before) = fixture_at_boundary(source, bounds);
    let start = press(&mut session, before.title_receiver.unwrap());
    let moved = offset(start, -120.0, -64.0);
    let expected = rect(-120.0, top - 64.0, source.width(), source.height());
    let (projected, output) = submit_boundary_edge(&mut session, bounds, moved, Event::Moved);
    assert_eq!(projected.projected_root, Some(expected));
    assert_eq!(projected.outer, bounds);
    assert_eq!(projected.title.height(), 0.0);
    assert_visible_content(&projected);
    present(&mut session, output);

    let (released, output) = submit_boundary_edge(
        &mut session,
        bounds,
        moved,
        Event::ButtonReleased(SurfacePointerButton::Primary),
    );
    assert!(!session.has_active_gesture());
    assert_eq!(session.view().contained(FLOATING).unwrap().rect(), expected);
    assert_eq!(released.outer, bounds);
    assert_visible_content(&released);
    present(&mut session, output);
}

#[test]
fn cancelling_a_tab_group_drag_at_toolbar_bounds_restores_its_real_pane_content() {
    for top in [94.0, 124.0] {
        let source = rect(850.0, top - 14.0, 360.0, 410.0);
        let (mut session, before) = fixture_at_boundary(source, rect(0.0, top, 1_280.0, 726.0));
        let durable = session.workspace().clone();
        let start = press(&mut session, before.group_receiver.unwrap());
        let moved = offset(start, -80.0, 30.0);
        let bounds = rect(0.0, top, 1_280.0, 726.0);
        let (dragging, output) = submit_boundary_edge(&mut session, bounds, moved, Event::Moved);
        assert!(session.has_active_gesture());
        assert_eq!(dragging.root_paint, SurfacePaintDisposition::Visible);
        assert_eq!(dragging.projected_root, None);
        assert_eq!(
            dragging.panes,
            [
                (LEFT, SurfacePaintDisposition::SourceGap),
                (RIGHT, SurfacePaintDisposition::Visible),
            ],
            "a partial tabs-group drag may omit its own pane while retaining the floating frame"
        );
        present(&mut session, output);

        let (restored, output) = submit_boundary_edge(
            &mut session,
            bounds,
            moved,
            Event::StreamCancelled(SurfacePointerCancelReason::ExplicitPlatformCancellation),
        );
        assert!(!session.has_active_gesture());
        assert_eq!(session.workspace(), &durable);
        assert_eq!(restored.outer, before.outer);
        assert_eq!(restored.title, before.title);
        assert_visible_content(&restored);
        present(&mut session, output);

        let (idle, output) = paint_boundary(session.begin_host_frame().unwrap());
        assert_visible_content(&idle);
        assert_eq!(session.workspace(), &durable);
        present(&mut session, output);
    }
}
