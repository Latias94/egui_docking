use super::*;
use dockspace::geometry::{LogicalRect, LogicalSize, ScaleFactor};
use dockspace::model::{
    DockspaceContainedLayout, DockspaceLayout, DockspaceNode, DockspaceRootLayout,
    DockspaceSurfaceLayout, FloatingPresentationId, ItemId, RootId, SurfaceId,
};
use dockspace::policy::DockPolicy;
use dockspace::runtime::{
    DockspaceHostFrame, DockspaceSession, HostFrameReport, HostWindowToken, HostWorkAreaToken,
    NativeCloseState, NativeInputStorageLimit, NativePointerRoster, NativeReceiverAnswer,
    NativeReportStorageLimit, NativeWindowFacts, NativeWindowInputState, NativeWindowPlacement,
    NativeWindowPresentationState, NativeWorkAreaFacts, NativeWorkAreaRoster,
    SurfacePresentationResult, SurfaceUnavailableReason, UniformSurfaceMetrics,
};
use std::num::NonZeroUsize;

const SURFACE: SurfaceId = SurfaceId::new(1);
const TEAR_OFF_ROOT: RootId = RootId::new(2);

struct CreateFixture {
    session: DockspaceSession,
    request: NativeEffectRequest,
}

fn complete(mut frame: DockspaceHostFrame<'_>) -> HostFrameReport {
    frame
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .unwrap();
    frame.commit().unwrap()
}

fn commit(session: &mut DockspaceSession) -> HostFrameReport {
    complete(
        session
            .begin_native_host_frame(|_| NativeReceiverAnswer::Unknown)
            .unwrap(),
    )
}

fn window_facts(bounds: PhysicalRect, state: NativeWindowPresentationState) -> NativeWindowFacts {
    let scale = ScaleFactor::new(1.0).unwrap();
    NativeWindowFacts::live()
        .with_content_bounds(bounds)
        .with_outer_bounds(bounds)
        .with_native_scale_factor(scale)
        .with_presentation_scale_factor(scale)
        .with_input(NativeWindowInputState::ReceivesInput, None)
        .with_presentation(state)
        .with_close(NativeCloseState::Clear, None)
}

fn root_facts() -> NativeWindowFacts {
    window_facts(
        PhysicalRect::new(0.0, 0.0, 640.0, 480.0).unwrap(),
        NativeWindowPresentationState::Visible,
    )
}

fn root_session() -> (DockspaceSession, NativeSurfaceBinding) {
    let layout = DockspaceLayout::new([DockspaceSurfaceLayout::new(
        SURFACE,
        DockspaceRootLayout::new(
            RootId::new(1),
            DockspaceNode::central_tabs([ItemId::new(1)]),
        ),
    )
    .with_contained(DockspaceContainedLayout::new(
        FloatingPresentationId::new(1),
        DockspaceRootLayout::new(TEAR_OFF_ROOT, DockspaceNode::central_tabs([ItemId::new(2)])),
        LogicalRect::new(40.0, 50.0, 360.0, 280.0).unwrap(),
    ))])
    .unwrap();
    let mut policy = DockPolicy::default();
    policy.set_allow_native_surfaces(true);
    let mut session = DockspaceSession::from_layout(layout, policy).unwrap();
    session
        .enable_managed_native_host_with_input_storage_limit(
            NativePointerRoster::Exact(Vec::new()),
            NativeInputStorageLimit::new(NonZeroUsize::new(32).unwrap(), 1 << 20)
                .with_report_storage_limit(NativeReportStorageLimit::new(
                    NonZeroUsize::new(8).unwrap(),
                    NonZeroUsize::new(1).unwrap(),
                    1 << 20,
                )),
        )
        .unwrap();
    session
        .configure_managed_native_capabilities(crate::capabilities::capabilities_for_backend(
            eframe::NativeWindowingBackend::X11,
        ))
        .unwrap();
    session
        .register_native_root(SURFACE, HostWindowToken::new(41))
        .unwrap();
    let root = commit(&mut session).native_bindings()[0];
    session
        .report_managed_native_snapshot(
            [(root, root_facts())],
            NativeWorkAreaRoster::Exact(vec![NativeWorkAreaFacts::new(
                HostWorkAreaToken::new(1),
                PhysicalRect::new(0.0, 0.0, 1920.0, 1080.0).unwrap(),
                ScaleFactor::new(1.0).unwrap(),
            )]),
        )
        .unwrap();
    commit(&mut session);
    let mut measured = session
        .begin_native_host_frame(|_| NativeReceiverAnswer::Unknown)
        .unwrap();
    measured
        .measure_surface(
            SURFACE,
            UniformSurfaceMetrics::new(
                LogicalRect::new(0.0, 0.0, 640.0, 480.0).unwrap(),
                LogicalSize::new(32.0, 24.0).unwrap(),
                80.0,
            )
            .unwrap(),
        )
        .unwrap();
    measured.commit().unwrap();
    let mut painted = session
        .begin_native_host_frame(|_| NativeReceiverAnswer::Unknown)
        .unwrap();
    let receipt = painted
        .paint_plan(SURFACE)
        .unwrap()
        .unwrap()
        .execute_surface_program(|_| Ok::<(), ()>(()))
        .unwrap();
    painted.confirm_surface_painted(receipt).unwrap();
    let mut report = complete(painted);
    let outputs = report.take_painted_outputs();
    assert_eq!(outputs.len(), 1);
    for output in outputs {
        session
            .report_surface_presentation(output, SurfacePresentationResult::Presented)
            .unwrap();
    }
    commit(&mut session);
    (session, root)
}

/// Mint real affine requests with one core report lease; no opaque effect or
/// visibility-command identity is reconstructed by the storage tests.
fn create_fixture() -> CreateFixture {
    let (mut session, _) = root_session();
    let mut frame = session
        .begin_native_host_frame(|_| NativeReceiverAnswer::Unknown)
        .unwrap();
    frame
        .move_root_to_new_window_current(
            TEAR_OFF_ROOT,
            NativeWindowPlacement::new(PhysicalRect::new(920.0, 120.0, 420.0, 320.0).unwrap()),
        )
        .unwrap();
    let mut report = complete(frame);
    let mut requests = report.take_native_effects();
    assert_eq!(
        requests.len(),
        1,
        "the real tear-off must emit one CreateWindow"
    );
    let request = requests.pop().unwrap();
    assert!(matches!(
        request.operation(),
        NativeEffectOperation::CreateWindow { .. }
    ));
    CreateFixture { session, request }
}

#[test]
fn one_effect_slot_returns_the_original_request_for_retry_without_growing_backing() {
    let first = create_fixture();
    let second = create_fixture();
    let mut effects = NativeEffectCoordinator::with_storage_limits(Some(1)).unwrap();
    let baseline = effects.owned_data_capacity_bytes();
    let pointers = effects.pending_viewports.as_ptr();
    let first_viewport = ViewportId::from_hash_of("first finite create");
    let second_viewport = ViewportId::from_hash_of("second finite create");
    let first_plan = effects.plan(first_viewport, &first.request).unwrap();
    let second_plan = effects.plan(second_viewport, &second.request).unwrap();
    let expected = second.request.operation().clone();
    let request_identity = format!("{:?}", second.request);
    effects.insert(first_plan, first.request).unwrap();
    let retry = effects.insert(second_plan, second.request).unwrap_err();
    assert_eq!(retry.operation(), &expected);
    assert_eq!(format!("{retry:?}"), request_identity);
    assert!(effects.plan(second_viewport, &retry).is_none());
    assert!(
        effects
            .take_for_output(first_viewport, second_plan.binding())
            .is_none()
    );
    let wrong_plan = NativeViewportEffectPlan {
        binding: second_plan.binding(),
        ..first_plan
    };
    assert!(effects.remove_unstarted(wrong_plan).is_none());
    assert!(
        effects
            .take_failure_result(NativeViewportCreateFailureRecord::new(
                first_viewport,
                second_plan.binding(),
                NativeViewportCreateFailureKind::WindowUnavailable,
            ))
            .is_none()
    );
    assert_eq!(effects.owned_data_capacity_bytes(), baseline);
    let first_request = effects
        .take_for_output(first_viewport, first_plan.binding())
        .unwrap();
    let _ = first_request.accepted();
    effects.insert(second_plan, retry).unwrap();
    let request = effects.remove_unstarted(second_plan).unwrap();
    assert_eq!(format!("{request:?}"), request_identity);
    let result = request.dispatch_failed(NativeDispatchFailure::WindowUnavailable);
    let mut session = second.session;
    session.report_native_effect_result(result).unwrap();
    commit(&mut session);
    assert!(!effects.has_pending_work());
    assert_eq!(effects.owned_data_capacity_bytes(), baseline);
    assert_eq!(effects.pending_viewports.as_ptr(), pointers);
}

#[test]
fn reservations_count_the_create_store_with_checked_arithmetic() {
    for effect_slots in [None, Some(0), Some(1), Some(2), Some(7)] {
        let effects = NativeEffectCoordinator::with_storage_limits(effect_slots).unwrap();
        let required = NativeEffectCoordinator::reservation_bytes(effect_slots).unwrap();
        assert_eq!(effects.owned_data_capacity_bytes(), required);
        assert_eq!(
            effects.pending_viewports.capacity(),
            effect_slots.unwrap_or(0)
        );
    }
    assert_eq!(
        NativeEffectCoordinator::reservation_bytes(Some(usize::MAX)),
        None
    );
    assert_eq!(
        NativeEffectCoordinator::with_storage_limits(Some(usize::MAX)).unwrap_err(),
        NativeHostProtocolError::NativeReportStorageUnavailable
    );
    let zero = NativeEffectCoordinator::with_storage_limits(Some(0)).unwrap();
    assert!(!zero.can_retain_effect());
}
