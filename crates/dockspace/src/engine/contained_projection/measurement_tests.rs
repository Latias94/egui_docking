use super::*;

#[test]
fn measured_source_keeps_motion_without_authorizing_blocked_or_unknown_targets() {
    for blocked in [false, true] {
        let (mut session, before) = fixture(16);
        let durable = session.workspace().clone();
        let start = press(&mut session, before.title);
        let surface = session.presented_surface(SURFACE).unwrap();
        let moved = offset(start, 64.0, 32.0);
        let route = HostPointerRoute::new(surface, moved);
        let facts = if blocked {
            SurfacePointerReceiverFacts::unknown().with_blocked_hover(&surface)
        } else {
            SurfacePointerReceiverFacts::unknown()
        };
        let mut frame = session.begin_host_frame().unwrap();
        frame
            .submit_host_pointer_batch([HostPointerInput::new(
                SurfacePointerId::new(1),
                Event::Moved,
                route,
                route,
                SurfacePointerCapture::ProviderEndpoint,
                facts,
            )])
            .unwrap();
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
        assert!(session.has_active_gesture());
        let (projected, output) = paint_frame(session.begin_host_frame().unwrap(), 16);
        projected.assert_projection(&before, rect(264.0, 172.0, 400.0, 300.0));
        present(&mut session, output);
        assert_eq!(session.workspace(), &durable);

        let surface = session.presented_surface(SURFACE).unwrap();
        let route = HostPointerRoute::new(surface, moved);
        let facts = if blocked {
            SurfacePointerReceiverFacts::unknown().with_blocked_hover(&surface)
        } else {
            SurfacePointerReceiverFacts::unknown()
        };
        let mut frame = session.begin_host_frame().unwrap();
        frame
            .submit_host_pointer_batch([HostPointerInput::new(
                SurfacePointerId::new(1),
                Event::ButtonReleased(SurfacePointerButton::Primary),
                route,
                route,
                SurfacePointerCapture::ProviderEndpoint,
                facts,
            )])
            .unwrap();
        frame
            .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
            .unwrap();
        assert!(!frame.commit().unwrap().workspace_changed());
        assert!(!session.has_active_gesture());
        assert_eq!(session.workspace(), &durable);
        let (restored, output) = paint_frame(session.begin_host_frame().unwrap(), 16);
        restored.assert_projection(&before, initial_rect());
        present(&mut session, output);
        assert_eq!(session.workspace(), &durable);
    }
}

#[test]
fn measuring_another_surface_still_refreshes_the_active_contained_source() {
    let (original, _) = fixture(16);
    let mut workspace = original.workspace().clone();
    let other_surface = SurfaceId::new(99);
    let other_root = RootId::new(99);
    let tabs = workspace.insert_runtime_node(crate::graph::Node::tabs([ItemId::new(9_999)]));
    workspace
        .roots_mut()
        .insert(other_root, crate::graph::RootRecord::new(tabs));
    workspace.surfaces_mut().insert(
        other_surface,
        crate::graph::SurfacePresentation::with_main(other_root),
    );
    let mut session =
        DockspaceSession::from_workspace_for_test(workspace, DockPolicy::default()).unwrap();
    session.enable_host_pointer().unwrap();
    let metrics = UniformSurfaceMetrics::new(
        rect(0.0, 0.0, 131_072.0, 720.0),
        LogicalSize::new(64.0, 48.0).unwrap(),
        96.0,
    )
    .unwrap();
    let mut frame = session.begin_host_frame().unwrap();
    frame.measure_surface(SURFACE, metrics).unwrap();
    frame.measure_surface(other_surface, metrics).unwrap();
    frame.commit().unwrap();
    let (before, output) = paint_frame(session.begin_host_frame().unwrap(), 16);
    present(&mut session, output);
    let start = press(&mut session, before.resize);
    let surface = session.presented_surface(SURFACE).unwrap();
    let moved = offset(start, 40.0, 24.0);
    let route = HostPointerRoute::new(surface, moved);
    reset_surface_compilation_count();
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
    frame.measure_surface(other_surface, metrics).unwrap();
    assert_eq!(
        surface_compilation_count(),
        2,
        "fresh measurement of B must not suppress the required cached projection of A"
    );
    let (projected, output) = paint_frame(frame, 16);
    projected.assert_projection(&before, rect(200.0, 140.0, 440.0, 324.0));
    present(&mut session, output);
    assert_eq!(surface_compilation_count(), 2);
    assert!(session.has_active_gesture());
    assert_eq!(
        session.view().contained(FLOATING).unwrap().rect(),
        initial_rect()
    );
}

#[test]
fn same_frame_measurement_after_contained_move_or_resize_compiles_once_and_retains_projection() {
    for pane_count in [16, 128, 1_024] {
        for resize in [false, true] {
            let (mut session, before) = fixture(pane_count);
            let durable = session.workspace().clone();
            let version = session.version();
            let start = press(
                &mut session,
                if resize { before.resize } else { before.title },
            );
            let surface = session.presented_surface(SURFACE).unwrap();
            let metrics = UniformSurfaceMetrics::new(
                rect(0.0, 0.0, 131_072.0, 720.0),
                LogicalSize::new(64.0, 48.0).unwrap(),
                96.0,
            )
            .unwrap();
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
            frame.measure_surface(SURFACE, metrics).unwrap();
            // Measurement answers this surface, so painting must wait until the next frame.
            let mut report = frame.commit().unwrap();
            assert!(!report.workspace_changed());
            assert!(report.take_painted_outputs().is_empty());
            assert_eq!(
                surface_compilation_count(),
                1,
                "{pane_count} panes, resize={resize}: same-frame measurement and commit must compile once, not refresh the cache before measuring"
            );
            assert!(session.has_active_gesture());
            assert_eq!(session.version(), version);
            assert_eq!(session.workspace(), &durable);

            let dx = f64::from(MOVE_COUNT);
            let dy = dx * 0.5;
            let expected = if resize {
                rect(200.0, 140.0, 400.0 + dx, 300.0 + dy)
            } else {
                rect(200.0 + dx, 140.0 + dy, 400.0, 300.0)
            };
            for paint_index in 0..4 {
                let (painted, output) =
                    paint_frame(session.begin_host_frame().unwrap(), pane_count);
                painted.assert_projection(&before, expected);
                present(&mut session, output);
                assert_eq!(
                    surface_compilation_count(),
                    1,
                    "{pane_count} panes, resize={resize}, paint={paint_index}: next-frame and repeated idle paints must reuse the measured projection"
                );
                assert!(session.has_active_gesture());
                assert_eq!(session.version(), version);
                assert_eq!(session.workspace(), &durable);
            }
        }
    }
}
