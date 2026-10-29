use super::*;
use dockspace::geometry::{LogicalRect, LogicalSize, ScaleFactor};
use dockspace::model::{
    DockspaceContainedLayout, DockspaceLayout, DockspaceNode, DockspaceRootLayout,
    DockspaceSurfaceLayout, FloatingPresentationId, ItemId, RootId, SurfaceId,
};
use dockspace::policy::DockPolicy;
use dockspace::runtime::{
    DockspaceHostFrame, DockspaceSession, HostFrameReport, HostWindowToken, HostWorkAreaToken,
    NativeCloseState, NativeEffectAcknowledgement, NativeInputStorageLimit, NativePointerRoster,
    NativeReceiverAnswer, NativeReportStorageLimit, NativeWindowFacts, NativeWindowInputState,
    NativeWindowPlacement, NativeWindowPresentationState, NativeWorkAreaFacts,
    NativeWorkAreaRoster, SurfacePresentationResult, SurfaceUnavailableReason,
    UniformSurfaceMetrics,
};
use std::num::NonZeroUsize;

const SURFACE: SurfaceId = SurfaceId::new(1);
const TEAR_OFF_ROOT: RootId = RootId::new(2);

struct CreateFixture {
    session: DockspaceSession,
    root: NativeSurfaceBinding,
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
        .with_presentation(state, None)
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
    let (mut session, root) = root_session();
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
    CreateFixture {
        session,
        root,
        request,
    }
}

fn show_fixture() -> (DockspaceSession, NativeSurfaceBinding, NativeEffectRequest) {
    let CreateFixture {
        mut session,
        root,
        request,
    } = create_fixture();
    let (binding, placement) = match request.operation() {
        NativeEffectOperation::CreateWindow {
            binding, placement, ..
        } => (*binding, *placement),
        operation => panic!("unexpected fixture operation: {operation:?}"),
    };
    let Some(NativeEffectAcknowledgement::Presentation(acknowledgement)) = request.accepted()
    else {
        panic!("the real create request must carry a presentation acknowledgement");
    };
    session
        .report_managed_native_snapshot(
            [
                (root, root_facts()),
                (
                    binding,
                    window_facts(placement, NativeWindowPresentationState::Hidden)
                        .with_presentation(
                            NativeWindowPresentationState::Hidden,
                            Some(acknowledgement),
                        ),
                ),
            ],
            NativeWorkAreaRoster::Unknown,
        )
        .unwrap();
    commit(&mut session);
    let mut staging = session
        .begin_native_host_frame(|_| NativeReceiverAnswer::Unknown)
        .unwrap();
    let paint = staging
        .native_staging_paints()
        .find(|paint| paint.binding() == binding)
        .unwrap();
    staging.confirm_native_staging_painted(paint).unwrap();
    let mut report = complete(staging);
    let mut outputs = report.take_painted_native_staging_outputs();
    assert_eq!(outputs.len(), 1);
    session
        .report_native_staging_presentation(
            outputs.pop().unwrap(),
            SurfacePresentationResult::Presented,
        )
        .unwrap();
    let mut report = commit(&mut session);
    let mut requests = report.take_native_effects();
    assert_eq!(requests.len(), 1);
    let request = requests.pop().unwrap();
    assert!(
        matches!(request.operation(), NativeEffectOperation::ShowWindow { binding: exact } if *exact == binding)
    );
    (session, binding, request)
}

#[test]
fn one_effect_slot_returns_the_original_request_for_retry_without_growing_backing() {
    let first = create_fixture();
    let second = create_fixture();
    let mut effects = NativeEffectCoordinator::with_storage_limits(Some(1), Some(0)).unwrap();
    let baseline = effects.owned_data_capacity_bytes();
    let pointers = (
        effects.pending_viewports.as_ptr(),
        effects.pending_shows.as_ptr(),
    );
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
    assert_eq!(
        (
            effects.pending_viewports.as_ptr(),
            effects.pending_shows.as_ptr()
        ),
        pointers
    );
}

#[test]
fn create_and_show_share_one_slot_and_only_exact_visibility_can_settle_it() {
    let create = create_fixture();
    let (_session, binding, show) = show_fixture();
    let mut effects = NativeEffectCoordinator::with_storage_limits(Some(1), Some(0)).unwrap();
    let baseline = effects.owned_data_capacity_bytes();
    let create_viewport = ViewportId::from_hash_of("held create");
    let show_viewport = ViewportId::from_hash_of("held show");
    let window = WindowId::from(17);
    let token = eframe::prepare_native_viewport_visibility(show_viewport, true).token();
    let plan = effects.plan(create_viewport, &create.request).unwrap();
    effects.insert(plan, create.request).unwrap();
    let identity = format!("{show:?}");
    assert!(!effects.preflight_can_retain_show(binding));
    let show = effects
        .retain_show(binding, show_viewport, window, token, show)
        .unwrap_err();
    assert_eq!(format!("{show:?}"), identity);
    let create_request = effects
        .take_for_output(create_viewport, plan.binding())
        .unwrap();
    assert!(effects.preflight_can_retain_show(binding));
    let show = effects
        .retain_show(plan.binding(), show_viewport, window, token, show)
        .unwrap_err();
    assert_eq!(format!("{show:?}"), identity);
    assert!(!effects.has_pending_show(plan.binding()));
    effects
        .retain_show(binding, show_viewport, window, token, show)
        .unwrap();
    assert!(effects.plan(create_viewport, &create_request).is_none());
    let stale_token = eframe::prepare_native_viewport_visibility(show_viewport, true).token();
    assert_ne!(token, stale_token);
    let record = |token, viewport, window, binding, visible| {
        NativeViewportVisibilityRecord::for_test(
            token,
            viewport,
            window,
            binding,
            visible,
            eframe::NativeViewportVisibilityStatus::Dispatched,
        )
    };
    for wrong in [
        record(stale_token, show_viewport, window, binding, true),
        record(token, create_viewport, window, binding, true),
        record(token, show_viewport, WindowId::from(18), binding, true),
        record(token, show_viewport, window, plan.binding(), true),
        record(token, show_viewport, window, binding, false),
    ] {
        assert!(effects.take_show(wrong).is_none());
        assert!(effects.has_pending_show(binding));
        assert_eq!(effects.show_token(binding), Some(token));
        assert_eq!(effects.owned_data_capacity_bytes(), baseline);
    }
    effects.remove_show(plan.binding());
    assert!(effects.has_pending_show(binding));
    let show = effects
        .take_show(record(token, show_viewport, window, binding, true))
        .unwrap();
    assert_eq!(format!("{show:?}"), identity);
    assert!(
        effects
            .take_show(record(token, show_viewport, window, binding, true))
            .is_none()
    );
    effects.insert(plan, create_request).unwrap();
    let _ = effects.remove_unstarted(plan).unwrap().accepted();
    let _ = show.accepted();
    assert!(!effects.has_pending_work());
    assert_eq!(effects.owned_data_capacity_bytes(), baseline);
}

#[test]
fn unlimited_create_and_show_preflight_reserve_backing_before_retaining_requests() {
    let create = create_fixture();
    let (_session, binding, show) = show_fixture();
    let mut effects = NativeEffectCoordinator::default();
    let create_viewport = ViewportId::from_hash_of("preflight create");
    let show_viewport = ViewportId::from_hash_of("preflight show");
    assert_eq!(effects.pending_viewports.capacity(), 0);
    let plan = effects.plan(create_viewport, &create.request).unwrap();
    let create_backing = effects.pending_viewports.as_ptr();
    let create_capacity = effects.pending_viewports.capacity();
    let prepared_bytes = effects.owned_data_capacity_bytes();
    assert!(create_capacity > 0);
    effects.insert(plan, create.request).unwrap();
    assert_eq!(effects.pending_viewports.as_ptr(), create_backing);
    assert_eq!(effects.pending_viewports.capacity(), create_capacity);
    assert_eq!(effects.owned_data_capacity_bytes(), prepared_bytes);

    assert_eq!(effects.pending_shows.capacity(), 0);
    assert!(effects.preflight_can_retain_show(binding));
    let show_backing = effects.pending_shows.as_ptr();
    let show_capacity = effects.pending_shows.capacity();
    let prepared_bytes = effects.owned_data_capacity_bytes();
    assert!(show_capacity > 0);
    let token = eframe::prepare_native_viewport_visibility(show_viewport, true).token();
    effects
        .retain_show(binding, show_viewport, WindowId::from(19), token, show)
        .unwrap();
    assert!(!effects.preflight_can_retain_show(binding));
    assert_eq!(effects.pending_shows.as_ptr(), show_backing);
    assert_eq!(effects.pending_shows.capacity(), show_capacity);
    assert_eq!(effects.owned_data_capacity_bytes(), prepared_bytes);
}

#[test]
fn independent_command_bound_returns_the_original_command_and_drain_keeps_order_and_backing() {
    let (_session, binding) = root_session();
    let mut effects = NativeEffectCoordinator::with_storage_limits(Some(0), Some(2)).unwrap();
    let baseline = effects.owned_data_capacity_bytes();
    let backing = effects.pending_commands.as_ptr();
    let first = ViewportId::from_hash_of("command first");
    let second = ViewportId::from_hash_of("command second");
    assert!(effects.can_queue_command());
    effects
        .queue_command(first, binding, NativeViewportCommand::Close)
        .unwrap();
    effects
        .queue_command(second, binding, NativeViewportCommand::Focus)
        .unwrap();
    assert!(!effects.can_queue_command());
    let rejected = effects
        .queue_command(ViewportId::ROOT, binding, NativeViewportCommand::Close)
        .unwrap_err();
    assert_eq!(rejected, NativeViewportCommand::Close);
    assert_eq!(effects.owned_data_capacity_bytes(), baseline);
    assert_eq!(
        effects.drain_commands().collect::<Vec<_>>(),
        [
            (first, ViewportCommand::Close),
            (second, ViewportCommand::Focus),
        ]
    );
    assert_eq!(effects.pending_commands.as_ptr(), backing);
    assert_eq!(effects.owned_data_capacity_bytes(), baseline);
    effects
        .queue_command(ViewportId::ROOT, binding, rejected)
        .unwrap();
    assert_eq!(
        effects.drain_commands().collect::<Vec<_>>(),
        [(ViewportId::ROOT, ViewportCommand::Close)]
    );
    assert_eq!(effects.owned_data_capacity_bytes(), baseline);
    effects
        .queue_command(first, binding, NativeViewportCommand::Focus)
        .unwrap();
    effects
        .queue_command(second, binding, NativeViewportCommand::Close)
        .unwrap();
    let mut drain = effects.drain_commands();
    assert_eq!(drain.next(), Some((first, ViewportCommand::Focus)));
    drop(drain);
    assert!(!effects.has_pending_work());
    assert!(effects.can_queue_command());
    assert_eq!(effects.pending_commands.as_ptr(), backing);
    assert_eq!(effects.owned_data_capacity_bytes(), baseline);
}

#[test]
fn reservations_count_both_effect_stores_and_independent_commands_with_checked_arithmetic() {
    for (effect_slots, command_slots) in [
        (None, None),
        (Some(0), Some(0)),
        (Some(1), Some(1)),
        (Some(2), Some(7)),
        (Some(7), Some(1)),
        (Some(3), None),
        (None, Some(5)),
    ] {
        let effects =
            NativeEffectCoordinator::with_storage_limits(effect_slots, command_slots).unwrap();
        let required =
            NativeEffectCoordinator::reservation_bytes(effect_slots, command_slots).unwrap();
        assert_eq!(effects.owned_data_capacity_bytes(), required);
        assert_eq!(
            effects.pending_viewports.capacity(),
            effect_slots.unwrap_or(0)
        );
        assert_eq!(effects.pending_shows.capacity(), effect_slots.unwrap_or(0));
        assert_eq!(
            effects.pending_commands.capacity(),
            command_slots.unwrap_or(0)
        );
    }
    for limits in [(Some(usize::MAX), Some(1)), (Some(1), Some(usize::MAX))] {
        assert_eq!(
            NativeEffectCoordinator::reservation_bytes(limits.0, limits.1),
            None
        );
        assert_eq!(
            NativeEffectCoordinator::with_storage_limits(limits.0, limits.1).unwrap_err(),
            NativeHostProtocolError::NativeReportStorageUnavailable
        );
    }
    let mut zero = NativeEffectCoordinator::with_storage_limits(Some(0), Some(0)).unwrap();
    assert!(!zero.can_queue_command());
    assert!(!zero.can_retain_effect());
}
