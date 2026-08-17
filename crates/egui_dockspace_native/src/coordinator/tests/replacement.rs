use dockspace::geometry::{LogicalRect, LogicalSize, PhysicalRect, ScaleFactor};
use dockspace::runtime::{
    NativeEffectOperation, NativeWindowInputState, NativeWindowPresentationState,
    UniformSurfaceMetrics,
};

use super::*;

fn exact_live_window(x: f64) -> NativeWindowFacts {
    let content = PhysicalRect::new(x, 0.0, 900.0, 700.0).expect("content bounds validate");
    let outer = PhysicalRect::new(x - 8.0, -30.0, 916.0, 738.0).expect("outer bounds validate");
    let scale = ScaleFactor::new(1.0).expect("scale validates");
    NativeWindowFacts::live()
        .with_content_bounds(content)
        .with_outer_bounds(outer)
        .with_native_scale_factor(scale)
        .with_presentation_scale_factor(scale)
        .with_input(NativeWindowInputState::ReceivesInput, None)
        .with_presentation(NativeWindowPresentationState::Visible, None)
        .with_close(NativeCloseState::Clear, None)
}

fn measure_ready_surfaces(native: &mut NativeCoordinator) {
    let metrics = UniformSurfaceMetrics::new(
        LogicalRect::new(0.0, 0.0, 900.0, 700.0).expect("surface bounds validate"),
        LogicalSize::new(32.0, 24.0).expect("pane minimum validates"),
        80.0,
    )
    .expect("uniform measurements validate");
    let mut frame = native
        .begin_host_frame(|_| NativeReceiverAnswer::Unknown)
        .expect("the exact live snapshot begins a host frame");
    frame
        .frame
        .measure_surface(FIRST_SURFACE, metrics)
        .expect("the recovery host becomes ready");
    frame
        .frame
        .measure_surface(SECOND_SURFACE, metrics)
        .expect("the child surface becomes ready");
    frame.commit().expect("the ready surfaces commit");
}

fn request_replacement_on_existing_viewport() -> (
    NativeCoordinator,
    ViewportId,
    NativeSurfaceBinding,
    NativeSurfaceBinding,
) {
    let mut native = coordinator();
    let (root, predecessor) = register_root_and_child(&mut native);
    let child_viewport = ViewportId::from_hash_of("replacement-owned-child");
    native
        .bind_viewport(ViewportId::ROOT, WindowId::from(11), root)
        .expect("the root viewport binds");
    native
        .bind_viewport(child_viewport, WindowId::from(22), predecessor)
        .expect("the child viewport binds");

    native
        .report_snapshot(
            [
                (root, exact_live_window(0.0)),
                (predecessor, exact_live_window(1200.0)),
            ],
            NativeWorkAreaRoster::Unknown,
        )
        .expect("the exact live roster records");
    measure_ready_surfaces(&mut native);

    let destroyed = {
        let mut viewports = native
            .viewports
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        NativeWindowEventRecord::for_test_ingress(
            14,
            WindowId::from(22),
            None,
            WindowEvent::Destroyed,
            &mut viewports,
        )
    };
    native
        .bridge
        .push_record(HostRecord::WindowEvent(destroyed));
    native.bridge.push_viewport_roster(live_roster([root]));
    assert!(
        native
            .reduce_callback_head()
            .expect("the destroyed callback reduces")
    );
    let mut destroyed_frame = native
        .begin_host_frame(|_| NativeReceiverAnswer::Unknown)
        .expect("the destroyed callback boundary begins");
    destroyed_frame
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .expect("the destroyed callback settles every surface");
    destroyed_frame
        .commit()
        .expect("the destroyed callback commits");

    assert!(
        native
            .reduce_callback_head()
            .expect("the complete roster publishes the destroyed tombstone")
    );
    let mut frame = native
        .begin_host_frame(|_| NativeReceiverAnswer::Unknown)
        .expect("the destroyed-child recovery frame begins");
    frame
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .expect("the recovery frame settles every surface");
    let mut report = frame.commit().expect("the recovery frame commits");
    assert!(native.settle_host_frame_inputs(report.inputs()));
    let effects = report.take_native_effects();
    let successor = match effects.as_slice() {
        [request] => match request.operation() {
            NativeEffectOperation::RequestReplacement {
                binding,
                placement: _,
                role: _,
            } => *binding,
            operation => panic!("expected one replacement request, got {operation:?}"),
        },
        requests => panic!("expected one replacement request, got {requests:?}"),
    };
    assert_ne!(successor, predecessor);
    assert_eq!(successor.surface(), predecessor.surface());

    native
        .accept_native_effects(effects)
        .expect("the replacement request is retained by the native coordinator");

    (native, child_viewport, predecessor, successor)
}

#[test]
fn destroyed_owned_child_retains_replacement_on_the_existing_viewport() {
    let (mut native, child_viewport, predecessor, successor) =
        request_replacement_on_existing_viewport();

    assert_eq!(native.viewport_binding(child_viewport), Some(successor));
    assert!(
        native
            .deferred_viewport_specs()
            .iter()
            .any(|spec| spec.viewport() == child_viewport && spec.binding() == successor),
        "the successor must reuse the exact logical child viewport"
    );
    assert!(
        native
            .prepare_committed_retirements()
            .expect("replacement must not leave a stale route retirement")
            .is_none()
    );
    assert!(
        native
            .try_report_retirement_quiescence()
            .expect("the replaced predecessor becomes quiescent")
    );
    assert!(!native.session.recognizes_native_binding(predecessor));
    assert!(native.session.is_current_native_binding(successor));
}

#[test]
fn shutdown_cancels_unadmitted_replacement_without_retiring_its_predecessor() {
    let (mut native, child_viewport, predecessor, successor) =
        request_replacement_on_existing_viewport();

    assert!(native.session.recognizes_native_binding(predecessor));
    assert!(native.quarantine_after_fatal().is_empty());

    assert_eq!(native.viewport_binding(child_viewport), None);
    assert!(
        native
            .deferred_viewport_specs()
            .iter()
            .all(|spec| spec.binding() != successor)
    );
    assert!(!native.effects.references_binding(successor));
    assert!(!native.bridge.references_binding(successor));
    assert!(
        native.session.recognizes_native_binding(predecessor),
        "cancelling the successor must preserve predecessor retirement ownership"
    );

    assert!(
        native
            .try_report_retirement_quiescence()
            .expect("the preserved predecessor becomes quiescent")
    );
    assert!(!native.session.recognizes_native_binding(predecessor));

    let advance = native.advance_shutdown_boundary();
    assert!(advance.commands.is_empty());
    assert!(advance.abandoned_outputs.is_empty());
    assert!(!native.session.is_current_native_binding(successor));
}
