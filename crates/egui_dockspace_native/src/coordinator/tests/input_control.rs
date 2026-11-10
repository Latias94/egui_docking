use dockspace::runtime::{HostInputOutcome, NativeWindowInputState};
use eframe::NativeViewportPointerPassthroughStatus;

use super::*;
use crate::input_control::NativePointerPassthroughRecord;

#[test]
fn applied_pointer_passthrough_callback_becomes_an_exact_snapshot_fact() {
    let mut native = coordinator();
    let (first, second) = register_roots(&mut native);
    let first_window = WindowId::from(11);
    let second_window = WindowId::from(22);
    native
        .bind_viewport(ViewportId::ROOT, first_window, first)
        .expect("the root route binds");
    let child = ViewportId::from_hash_of("input-control-child");
    native
        .bind_viewport(child, second_window, second)
        .expect("the child route binds");

    let record = NativePointerPassthroughRecord::for_test(
        pointer_passthrough_token(ViewportId::ROOT, true),
        ViewportId::ROOT,
        first_window,
        Some(first),
        true,
        NativeViewportPointerPassthroughStatus::Applied,
    );
    native
        .bridge
        .push_record(HostRecord::ViewportPointerPassthrough(record));

    assert!(
        native
            .reduce_callback_prefix(&egui::Context::default(), &mut test_budget())
            .expect("the exact input callback records")
            .is_some()
    );
    let mut callback_frame = native
        .begin_host_frame(|_| NativeReceiverAnswer::Unknown)
        .expect("the callback boundary begins");
    callback_frame
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .expect("the callback boundary settles every surface");
    callback_frame
        .commit()
        .expect("the callback boundary commits");
    assert_eq!(
        native.input_control.snapshot_fact(first),
        Some((NativeWindowInputState::PassThrough, None))
    );

    native
        .bridge
        .push_viewport_roster(live_roster([first, second]));
    assert!(
        native
            .reduce_callback_prefix(&egui::Context::default(), &mut test_budget())
            .expect("the complete roster records the input fact")
            .is_some()
    );
    let mut roster_frame = native
        .begin_host_frame(|_| NativeReceiverAnswer::Unknown)
        .expect("the roster boundary begins");
    roster_frame
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .expect("the roster boundary settles every surface");
    let report = roster_frame.commit().expect("the roster boundary commits");
    assert!(
        native
            .settle_host_frame_inputs(report.inputs())
            .expect("the exact native snapshot settles")
    );
    assert!(report.inputs().iter().any(|input| matches!(
        input,
        HostInputOutcome::NativePlatformSnapshotApplied { .. }
    )));
    assert_eq!(
        native.input_control.snapshot_fact(first),
        Some((NativeWindowInputState::PassThrough, None))
    );
}

#[test]
fn failed_pointer_passthrough_callback_does_not_invent_input_authority() {
    let mut native = coordinator();
    let (first, _) = register_roots(&mut native);
    let window = WindowId::from(11);
    native
        .bind_viewport(ViewportId::ROOT, window, first)
        .expect("the root route binds");
    let record = NativePointerPassthroughRecord::for_test(
        pointer_passthrough_token(ViewportId::ROOT, true),
        ViewportId::ROOT,
        window,
        Some(first),
        true,
        NativeViewportPointerPassthroughStatus::Failed,
    );
    native
        .bridge
        .push_record(HostRecord::ViewportPointerPassthrough(record));

    assert!(
        native
            .reduce_callback_prefix(&egui::Context::default(), &mut test_budget())
            .expect("the failed callback is consumed")
            .is_some()
    );
    assert_eq!(native.input_control.snapshot_fact(first), None);
}

#[test]
fn shared_root_sibling_drag_preserves_window_input_without_passthrough_effects() {
    use std::time::Duration;

    use dockspace::geometry::PhysicalPoint;
    use dockspace::runtime::{
        NativeDesktopPointerLocation, NativeDesktopPosition, NativeDragSourceInput,
        NativePointerButton, NativePointerEvent, NativePointerHover, NativePointerId,
        NativePointerOwner, NativeReceiverPurpose, NativeSurfacePlacement, NativeWindowHit,
        PointerDeviceKind, PointerEventMetadata,
    };

    fn finish(mut frame: NativeHostFrame<'_>) -> HostFrameReport {
        frame
            .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
            .unwrap();
        frame.commit().unwrap()
    }

    let layout = DockspaceLayout::new([FIRST_SURFACE, SECOND_SURFACE].map(|surface| {
        let node = if surface == FIRST_SURFACE {
            DockspaceNode::central_tabs([ItemId::new(1)])
        } else {
            DockspaceNode::tabs([ItemId::new(2)])
        };
        DockspaceSurfaceLayout::new(
            surface,
            DockspaceRootLayout::new(RootId::new(surface.get()), node),
        )
    }))
    .unwrap();
    let mut policy = DockPolicy::default();
    policy.set_allow_native_surfaces(true);
    let mut native = NativeCoordinator::new(
        DockspaceSession::from_layout(layout, policy).unwrap(),
        NativePointerRoster::Exact(Vec::new()),
    )
    .unwrap();
    native
        .register_native_root(FIRST_SURFACE, HostWindowToken::new(71))
        .unwrap();
    let registration = finish(
        native
            .begin_host_frame(|_| NativeReceiverAnswer::Unknown)
            .unwrap(),
    );
    let original = registration.native_bindings()[0];
    native
        .bind_viewport(ViewportId::ROOT, WindowId::from(71), original)
        .unwrap();
    let bounds = PhysicalRect::new(0.0, 0.0, 1400.0, 480.0).unwrap();
    let facts = ready_window_facts(0.0)
        .with_content_bounds(bounds)
        .with_outer_bounds(bounds);
    native
        .report_snapshot(
            [(original, facts)],
            NativeWorkAreaRoster::Unknown,
            capabilities_for_backend(eframe::NativeWindowingBackend::X11),
        )
        .unwrap();
    let placements = [FIRST_SURFACE, SECOND_SURFACE].map(|surface| {
        let x = if surface == FIRST_SURFACE { 0.0 } else { 720.0 };
        NativeSurfacePlacement::new(
            surface,
            LogicalSize::new(640.0, 480.0).unwrap(),
            PhysicalPoint::new(x, 0.0).unwrap(),
            ScaleFactor::new(1.0).unwrap(),
            PhysicalRect::new(x, 0.0, 640.0, 480.0).unwrap(),
        )
    });
    let mut composition = native
        .begin_host_frame(|_| NativeReceiverAnswer::Unknown)
        .unwrap();
    composition
        .frame
        .set_native_surface_placements(original.window(), &placements)
        .unwrap();
    finish(composition);
    let (_, artifacts) = paint_and_present_all_surfaces_with_artifacts(&mut native);
    let (source, artifact) = artifacts
        .into_iter()
        .find(|(binding, _)| binding.surface() == SECOND_SURFACE)
        .unwrap();
    assert_eq!(source.window(), original.window());
    assert_ne!(source, original);
    assert_eq!(native.viewport_binding(ViewportId::ROOT), Some(original));
    let mut inspection = native
        .begin_host_frame(|_| NativeReceiverAnswer::Unknown)
        .unwrap();
    let receiver = inspection
        .frame
        .paint_plan(SECOND_SURFACE)
        .unwrap()
        .unwrap()
        .tab_receiver(ItemId::new(2))
        .unwrap();
    finish(inspection);

    let start = receiver.center();
    let source_placement = placements[1];
    let point = |x: f64| {
        PhysicalPoint::new(
            source_placement.origin().x() + x,
            source_placement.origin().y() + start.y(),
        )
        .unwrap()
    };
    let version = native.session.version();
    let context = egui::Context::default();
    for (captured_at, event, position) in [
        (
            Duration::from_millis(10),
            NativePointerEvent::ButtonPressed(NativePointerButton::Primary),
            point(start.x()),
        ),
        (
            Duration::from_millis(20),
            NativePointerEvent::Moved,
            point(start.x() + 40.0),
        ),
    ] {
        native
            .record_pointer(
                NativePointerInput::new(
                    NativePointerId::new(101),
                    event,
                    NativeDesktopPointerLocation::new(
                        NativeDesktopPosition::Exact(position),
                        NativePointerHover::Dock(source.window()),
                        None,
                    ),
                    NativePointerOwner::Native(source.window()),
                    NativePointerOwner::Native(source.window()),
                )
                .with_metadata(PointerEventMetadata::new(
                    PointerDeviceKind::Mouse,
                    captured_at,
                )),
            )
            .unwrap();
        let frame = native
            .begin_host_frame(|query| match query {
                NativeReceiverQuery::WindowHit(query) => {
                    assert_eq!(query.window(), source.window());
                    assert!(source_placement.clip().contains(query.point()));
                    NativeReceiverAnswer::WindowHit(NativeWindowHit::Surface(artifact))
                }
                NativeReceiverQuery::Surface(query)
                    if matches!(query.purpose(), NativeReceiverPurpose::HoverHit) =>
                {
                    NativeReceiverAnswer::ClearHoverLane(query.presented_surface())
                }
                NativeReceiverQuery::Surface(query) => query
                    .bind_receiver(&receiver)
                    .map_or(NativeReceiverAnswer::Unknown, NativeReceiverAnswer::Dock),
            })
            .unwrap();
        let mut report = finish(frame);
        let effects = report.take_native_effects();
        assert!(effects.iter().all(|request| !matches!(
            request.operation(),
            NativeEffectOperation::SetPointerPassthrough { .. }
        )));
        native.accept_native_effects(&context, effects).unwrap();
    }
    assert!(native.session.has_active_gesture());
    assert!(!native.session.has_pending_native_input());
    let capture = native.session.native_pointer_capture_request().unwrap();
    assert_eq!(capture.source_binding(), source);
    assert_eq!(capture.source_input(), NativeDragSourceInput::Preserve);
    assert_eq!(native.session.version(), version);
    assert!(native.pointer_passthrough_command().is_none());
    assert!(!native.input_control.has_pending_work());
    let observation =
        native.with_native_input_observation(CompiledWindowObservation::new(original, facts));
    assert_eq!(observation.facts(), facts);
    assert_eq!(observation.input_acknowledgement(), None);
}
