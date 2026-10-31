use super::*;
use dockspace::geometry::{LogicalRect, LogicalSize, PhysicalPoint, PhysicalRect, ScaleFactor};
use dockspace::model::{
    DockspaceLayout, DockspaceNode, DockspaceRootLayout, DockspaceSurfaceLayout, ItemId, RootId,
    SurfaceId,
};
use dockspace::policy::DockPolicy;
use dockspace::runtime::{
    DockspaceHostFrame, DockspaceSession, HostFrameReport, HostWindowToken, HostWorkAreaToken,
    NativeCloseState, NativeDesktopPointerLocation, NativeDesktopPosition, NativeEffectOperation,
    NativeInputStorageLimit, NativePointerButton, NativePointerEvent, NativePointerHover,
    NativePointerId, NativePointerInput, NativePointerOwner, NativePointerRoster,
    NativeReceiverAnswer, NativeReportStorageLimit, NativeWindowFacts,
    NativeWindowPresentationState, NativeWorkAreaFacts, NativeWorkAreaRoster,
    SurfacePresentationResult, SurfaceUnavailableReason, UniformSurfaceMetrics,
};
use std::num::NonZeroUsize;

const SURFACE: SurfaceId = SurfaceId::new(41);
const ITEM: ItemId = ItemId::new(61);
const RECOVERY_SURFACE: SurfaceId = SurfaceId::new(42);

struct InputFixture {
    session: DockspaceSession,
    binding: NativeSurfaceBinding,
    recovery: NativeSurfaceBinding,
    request: NativeEffectRequest,
}

fn layout() -> DockspaceLayout {
    DockspaceLayout::new([DockspaceSurfaceLayout::new(
        SURFACE,
        DockspaceRootLayout::new(RootId::new(51), DockspaceNode::central_tabs([ITEM])),
    )])
    .unwrap()
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

fn registered_session(effect_slots: usize) -> (DockspaceSession, NativeSurfaceBinding) {
    let mut session = native_session(layout(), effect_slots);
    session
        .register_native_root(SURFACE, HostWindowToken::new(71))
        .unwrap();
    let binding = commit(&mut session).native_bindings()[0];
    (session, binding)
}

fn native_session(layout: DockspaceLayout, effect_slots: usize) -> DockspaceSession {
    let mut policy = DockPolicy::default();
    policy.set_allow_native_surfaces(true);
    let mut session = DockspaceSession::from_layout(layout, policy).unwrap();
    session
        .enable_managed_native_host_with_input_storage_limit(
            dockspace::runtime::NativeCreateStrategy::Staged,
            NativePointerRoster::Exact(Vec::new()),
            NativeInputStorageLimit::new(NonZeroUsize::new(64).unwrap(), 4 << 20)
                .with_report_storage_limit(NativeReportStorageLimit::new(
                    NonZeroUsize::new(16).unwrap(),
                    NonZeroUsize::new(effect_slots).unwrap(),
                    4 << 20,
                )),
        )
        .unwrap();
    session
        .configure_managed_native_capabilities(crate::capabilities::capabilities_for_backend(
            eframe::NativeWindowingBackend::X11,
        ))
        .unwrap();
    session
}

fn window_facts(
    state: NativeWindowInputState,
    acknowledgement: Option<NativeInputEffectAcknowledgement>,
) -> NativeWindowFacts {
    let bounds = PhysicalRect::new(0.0, 0.0, 640.0, 360.0).unwrap();
    let scale = ScaleFactor::new(1.0).unwrap();
    NativeWindowFacts::live()
        .with_content_bounds(bounds)
        .with_outer_bounds(bounds)
        .with_native_scale_factor(scale)
        .with_presentation_scale_factor(scale)
        .with_input(state, acknowledgement)
        .with_presentation(NativeWindowPresentationState::Visible, None)
        .with_close(NativeCloseState::Clear, None)
}

fn only_input_request(report: &mut HostFrameReport, enabled: bool) -> NativeEffectRequest {
    let mut requests = report.take_native_effects();
    assert_eq!(requests.len(), 1);
    let request = requests.pop().unwrap();
    assert!(matches!(request.operation(),
        NativeEffectOperation::SetPointerPassthrough { enabled: actual, .. } if *actual == enabled
    ));
    request
}

/// Requests and their report leases come from a presented core drag. Command
/// identities below come from eframe's preparation API, without enqueueing work.
fn input_fixture(effect_slots: usize) -> InputFixture {
    let layout = DockspaceLayout::new([
        DockspaceSurfaceLayout::new(
            SURFACE,
            DockspaceRootLayout::new(RootId::new(51), DockspaceNode::tabs([ITEM])),
        ),
        DockspaceSurfaceLayout::new(
            RECOVERY_SURFACE,
            DockspaceRootLayout::new(
                RootId::new(52),
                DockspaceNode::central_tabs([ItemId::new(62)]),
            ),
        ),
    ])
    .unwrap();
    let mut session = native_session(layout, effect_slots);
    session
        .register_native_root(RECOVERY_SURFACE, HostWindowToken::new(72))
        .unwrap();
    let recovery = commit(&mut session).native_bindings()[0];
    session
        .register_owned_native_child(SURFACE, HostWindowToken::new(71), RECOVERY_SURFACE)
        .unwrap();
    let binding = *commit(&mut session)
        .native_bindings()
        .iter()
        .find(|binding| binding.surface() == SURFACE)
        .unwrap();
    session
        .report_managed_native_snapshot(
            [
                (
                    binding,
                    window_facts(NativeWindowInputState::ReceivesInput, None),
                ),
                (
                    recovery,
                    window_facts(NativeWindowInputState::ReceivesInput, None),
                ),
            ],
            NativeWorkAreaRoster::Exact(vec![NativeWorkAreaFacts::new(
                HostWorkAreaToken::new(1),
                PhysicalRect::new(0.0, 0.0, 1920.0, 1040.0).unwrap(),
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
                LogicalRect::new(0.0, 0.0, 640.0, 360.0).unwrap(),
                LogicalSize::new(0.0, 0.0).unwrap(),
                80.0,
            )
            .unwrap(),
        )
        .unwrap();
    complete(measured);
    let mut painted = session
        .begin_native_host_frame(|_| NativeReceiverAnswer::Unknown)
        .unwrap();
    let plan = painted.paint_plan(SURFACE).unwrap().unwrap();
    let receiver = plan.tab_receiver(ITEM).unwrap();
    let receipt = plan.execute_surface_program(|_| Ok::<(), ()>(())).unwrap();
    painted.confirm_surface_painted(receipt).unwrap();
    let mut report = complete(painted);
    let mut outputs = report.take_painted_outputs();
    assert_eq!(outputs.len(), 1);
    session
        .report_surface_presentation(outputs.pop().unwrap(), SurfacePresentationResult::Presented)
        .unwrap();
    commit(&mut session);

    let center = receiver.center();
    let work_area = session.native_work_area(HostWorkAreaToken::new(1)).unwrap();
    for (event, point, hover, area) in [
        (
            NativePointerEvent::ButtonPressed(NativePointerButton::Primary),
            PhysicalPoint::new(center.x(), center.y()).unwrap(),
            NativePointerHover::Dock(binding),
            None,
        ),
        (
            NativePointerEvent::Moved,
            PhysicalPoint::new(1000.0, 700.0).unwrap(),
            NativePointerHover::OutsideAll,
            Some(work_area),
        ),
    ] {
        session
            .record_native_pointer(NativePointerInput::new(
                NativePointerId::new(1),
                event,
                NativeDesktopPointerLocation::new(NativeDesktopPosition::Exact(point), hover, area),
                NativePointerOwner::Native(binding),
                NativePointerOwner::Native(binding),
            ))
            .unwrap();
    }
    let mut report = complete(
        session
            .begin_native_host_frame(|query| {
                query
                    .bind_receiver(&receiver)
                    .map_or(NativeReceiverAnswer::Unknown, NativeReceiverAnswer::Dock)
            })
            .unwrap(),
    );
    let request = only_input_request(&mut report, true);
    assert_eq!(request.operation().binding(), binding);
    InputFixture {
        session,
        binding,
        recovery,
        request,
    }
}

fn restore_request(session: &mut DockspaceSession) -> NativeEffectRequest {
    session.report_native_input_device_removal().unwrap();
    only_input_request(&mut commit(session), false)
}

fn retain(control: &mut NativeInputControl, request: NativeEffectRequest, route: u64) {
    let (binding, enabled) = match request.operation() {
        NativeEffectOperation::SetPointerPassthrough { binding, enabled } => (*binding, *enabled),
        operation => panic!("unexpected input operation: {operation:?}"),
    };
    control
        .retain(
            ViewportId::from_hash_of(route),
            WindowId::from(route),
            binding,
            enabled,
            request,
        )
        .unwrap();
}

fn unowned_record(binding: NativeSurfaceBinding, enabled: bool) -> NativePointerPassthroughRecord {
    NativePointerPassthroughRecord::for_test(
        eframe::prepare_native_viewport_pointer_passthrough(ViewportId::ROOT, enabled).token(),
        ViewportId::ROOT,
        WindowId::from(71),
        Some(binding),
        enabled,
        NativeViewportPointerPassthroughStatus::Applied,
    )
}

fn dispatch(control: &mut NativeInputControl) -> NativePointerPassthroughRecord {
    let command = control.command_to_dispatch().unwrap();
    let token =
        eframe::prepare_native_viewport_pointer_passthrough(command.viewport(), command.enabled())
            .token();
    let window = control.pending.front().unwrap().window;
    assert!(control.mark_dispatched(command, token));
    NativePointerPassthroughRecord::for_test(
        token,
        command.viewport(),
        window,
        Some(command.binding()),
        command.enabled(),
        NativeViewportPointerPassthroughStatus::Applied,
    )
}

#[test]
fn one_effect_lease_allows_one_accepted_head_and_one_successor_without_growth() {
    let InputFixture {
        mut session,
        binding,
        recovery,
        request,
    } = input_fixture(1);
    let mut control = NativeInputControl::with_storage_limits(Some(1), Some(1)).unwrap();
    let baseline = control.owned_data_capacity_bytes();
    let observation_backing = control.observations.as_ptr();
    assert_eq!(control.pending.capacity(), 2);
    retain(&mut control, request, 71);
    let applied = dispatch(&mut control);
    assert!(control.accept_applied(applied).unwrap());
    assert!(matches!(
        control.pending.front().unwrap().state,
        PendingNativeInputState::AwaitingSnapshot
    ));
    let Some((state, Some(acknowledgement))) = control.snapshot_fact(binding) else {
        panic!("an accepted head must retain its exact acknowledgement");
    };

    // The same core has only one report slot. This successor can be emitted
    // because accepted() released that slot while the first queue entry stays.
    let restore = restore_request(&mut session);
    retain(&mut control, restore, 71);
    assert_eq!(control.pending.len(), 2);
    assert!(control.command_to_dispatch().is_none());
    assert!(!control.observe_unowned_applied(applied).unwrap());
    assert_eq!(
        control.snapshot_fact(binding),
        Some((state, Some(acknowledgement)))
    );
    assert_eq!(control.owned_data_capacity_bytes(), baseline);

    session
        .report_managed_native_snapshot(
            [
                (binding, window_facts(state, Some(acknowledgement))),
                (
                    recovery,
                    window_facts(NativeWindowInputState::ReceivesInput, None),
                ),
            ],
            NativeWorkAreaRoster::Unknown,
        )
        .unwrap();
    commit(&mut session);
    assert!(control.settle_snapshot(Some(binding)));
    let restore = control.command_to_dispatch().unwrap();
    assert!(!restore.enabled());
    let restored = dispatch(&mut control);
    assert!(control.accept_applied(restored).unwrap());
    assert_eq!(control.pending.len(), 1);
    assert_eq!(control.pending.capacity(), 2);
    assert_eq!(control.observations.as_ptr(), observation_backing);
    assert_eq!(control.owned_data_capacity_bytes(), baseline);
    control.retire_binding_into(binding, |_| {
        panic!("accepted requests cannot emit another result")
    });
    assert!(!control.has_pending_work());
    assert!(!control.references_binding(binding));
    assert_eq!(control.owned_data_capacity_bytes(), baseline);
}

#[test]
fn full_pending_backing_returns_the_original_affine_request_for_retry() {
    let InputFixture {
        mut session,
        binding,
        request,
        ..
    } = input_fixture(1);
    let third = input_fixture(1);
    let mut control = NativeInputControl::with_storage_limits(Some(2), Some(1)).unwrap();
    let baseline = control.owned_data_capacity_bytes();
    retain(&mut control, request, 1);
    let applied = dispatch(&mut control);
    assert!(control.accept_applied(applied).unwrap());
    retain(&mut control, restore_request(&mut session), 1);
    let identity = format!("{:?}", third.request);
    let operation = third.request.operation().clone();
    let request = control
        .retain(
            ViewportId::from_hash_of(3_u64),
            WindowId::from(3),
            third.binding,
            true,
            third.request,
        )
        .unwrap_err();
    assert_eq!(format!("{request:?}"), identity);
    assert_eq!(request.operation(), &operation);
    assert_eq!(control.pending.len(), 2);
    assert_eq!(control.owned_data_capacity_bytes(), baseline);
    let mut results = Vec::with_capacity(1);
    control.retire_binding_into(binding, |result| results.push(result));
    assert_eq!(results.len(), 1, "the accepted head cannot terminate twice");
    assert!(!control.references_binding(binding));
    control.retire_binding_into(binding, |_| panic!("repeated retirement must be inert"));
    retain(&mut control, request, 3);
    assert_eq!(control.pending.len(), 1);
    let result = results.pop().unwrap();
    session.report_native_effect_result(result).unwrap();
    commit(&mut session);
    assert_eq!(control.owned_data_capacity_bytes(), baseline);
}

#[test]
fn full_observation_backing_preserves_the_current_callback_and_request_until_retry() {
    let (_old_session, old) = registered_session(1);
    let fixture = input_fixture(1);
    assert_eq!(old.surface(), fixture.binding.surface());
    assert_eq!(old.window_token(), fixture.binding.window_token());
    assert_ne!(old, fixture.binding);
    let mut control = NativeInputControl::with_storage_limits(Some(1), Some(1)).unwrap();
    let baseline = control.owned_data_capacity_bytes();
    let backing = control.observations.as_ptr();
    let old_record = unowned_record(old, false);
    assert!(control.observe_unowned_applied(old_record).unwrap());
    let identity = format!("{:?}", fixture.request);
    retain(&mut control, fixture.request, 71);
    let applied = dispatch(&mut control);
    let stale_token =
        eframe::prepare_native_viewport_pointer_passthrough(applied.viewport, true).token();
    for wrong in [
        NativePointerPassthroughRecord {
            token: stale_token,
            ..applied
        },
        NativePointerPassthroughRecord {
            viewport: ViewportId::ROOT,
            ..applied
        },
        NativePointerPassthroughRecord {
            window: WindowId::from(72),
            ..applied
        },
        NativePointerPassthroughRecord {
            enabled: false,
            ..applied
        },
        NativePointerPassthroughRecord {
            status: NativeViewportPointerPassthroughStatus::Failed,
            ..applied
        },
        NativePointerPassthroughRecord {
            binding: Some(old),
            ..applied
        },
    ] {
        assert!(!control.accept_applied(wrong).unwrap());
    }
    assert!(matches!(
        control.classify(applied),
        NativeInputResultDisposition::Current { .. }
    ));
    assert_eq!(
        control.accept_applied(applied),
        Err(NativeHostProtocolError::NativeReportStorageUnavailable)
    );
    let PendingNativeInputState::AwaitingCallback { request, token } =
        &control.pending.front().unwrap().state
    else {
        panic!("a refused observation must retain its dispatched request");
    };
    assert_eq!(format!("{request:?}"), identity);
    assert_eq!(*token, applied.token());
    assert_eq!(control.pending.len(), 1);
    assert_eq!(control.snapshot_fact(fixture.binding), None);
    assert_eq!(
        control.snapshot_fact(old),
        Some((NativeWindowInputState::ReceivesInput, None))
    );
    assert_eq!(control.observations.as_ptr(), backing);
    assert_eq!(control.owned_data_capacity_bytes(), baseline);

    control.retire_binding_into(old, |_| {
        panic!("an unowned fact has no request to terminate")
    });
    assert!(control.accept_applied(applied).unwrap());
    assert!(!control.accept_applied(applied).unwrap());
    assert!(!control.settle_snapshot(Some(old)));
    assert!(matches!(
        control.snapshot_fact(fixture.binding),
        Some((NativeWindowInputState::PassThrough, Some(_)))
    ));
    assert_eq!(control.observations.as_ptr(), backing);
    assert_eq!(control.owned_data_capacity_bytes(), baseline);
}

#[test]
fn retired_incarnation_keeps_its_slot_and_cannot_remove_or_overwrite_the_successor() {
    let (mut session, old) = registered_session(1);
    let mut control = NativeInputControl::with_storage_limits(Some(1), None).unwrap();
    assert!(
        control
            .observe_unowned_applied(unowned_record(old, false))
            .unwrap()
    );
    let baseline = control.owned_data_capacity_bytes();
    let backing = control.observations.as_ptr();
    let reset = session.prepare_layout_reset(layout()).unwrap();
    let new = complete(
        session
            .begin_native_layout_reset_frame(&reset, |_| NativeReceiverAnswer::Unknown)
            .unwrap(),
    )
    .native_bindings()[0];
    assert_ne!(old, new);
    assert_eq!(old.surface(), new.surface());
    assert_eq!(old.window_token(), new.window_token());
    let incoming = unowned_record(new, true);
    assert_eq!(
        control.observe_unowned_applied(incoming),
        Err(NativeHostProtocolError::NativeReportStorageUnavailable)
    );
    assert_eq!(control.snapshot_fact(new), None);
    assert!(control.references_binding(old));
    control.retire_binding_into(new, |_| panic!("the successor has no owner yet"));
    assert!(control.references_binding(old));
    control.retire_binding_into(old, |_| panic!("an unowned fact has no terminal result"));
    assert!(control.observe_unowned_applied(incoming).unwrap());
    control.retire_binding_into(old, |_| panic!("repeated old retirement must be inert"));
    assert_eq!(
        control.observe_unowned_applied(unowned_record(old, false)),
        Err(NativeHostProtocolError::NativeReportStorageUnavailable)
    );
    assert_eq!(
        control.snapshot_fact(new),
        Some((NativeWindowInputState::PassThrough, None))
    );
    assert!(
        control
            .observe_unowned_applied(unowned_record(new, false))
            .unwrap()
    );
    assert_eq!(
        control.snapshot_fact(new),
        Some((NativeWindowInputState::ReceivesInput, None))
    );
    assert!(!control.references_binding(old));
    assert_eq!(control.observations.as_ptr(), backing);
    assert_eq!(control.owned_data_capacity_bytes(), baseline);
}

#[test]
fn retirement_rotates_original_backing_preserves_survivors_and_emits_each_result_once() {
    let InputFixture {
        mut session,
        binding,
        request,
        ..
    } = input_fixture(2);
    let first_survivor = input_fixture(1);
    let second_survivor = input_fixture(1);
    let mut control = NativeInputControl::with_storage_limits(Some(3), Some(3)).unwrap();
    let baseline = control.owned_data_capacity_bytes();
    let capacity = control.pending.capacity();
    let observation_backing = control.observations.as_ptr();
    retain(&mut control, request, 1);
    let applied = dispatch(&mut control);
    retain(&mut control, first_survivor.request, 2);
    let restore = restore_request(&mut session);
    retain(&mut control, restore, 1);
    retain(&mut control, second_survivor.request, 3);
    assert_eq!(control.pending.len(), 4);
    let survivors = [first_survivor.binding, second_survivor.binding];
    let mut results = Vec::with_capacity(2);
    let result_backing = results.as_ptr();
    control.retire_binding_into(binding, |result| results.push(result));
    assert_eq!(results.len(), 2);
    assert_eq!(results[0].binding(), binding);
    assert_eq!(results[1].binding(), binding);
    // The public result deliberately hides the core ledger. Its Debug view
    // distinguishes the two terminal categories without minting test authority.
    assert!(format!("{:?}", results[0]).contains("Indeterminate(AcknowledgementLost)"));
    assert!(format!("{:?}", results[1]).contains("DispatchFailed(ProviderStopped)"));
    assert!(
        control
            .pending
            .iter()
            .map(|pending| pending.binding)
            .eq(survivors)
    );
    assert!(!control.references_binding(binding));
    assert_eq!(
        control.classify(applied),
        NativeInputResultDisposition::Unrelated
    );
    control.retire_binding_into(binding, |_| {
        panic!("retirement cannot emit a second terminal")
    });
    assert!(
        control
            .pending
            .iter()
            .map(|pending| pending.binding)
            .eq(survivors)
    );
    assert_eq!(control.pending.capacity(), capacity);
    assert_eq!(control.observations.as_ptr(), observation_backing);
    assert_eq!(control.owned_data_capacity_bytes(), baseline);
    assert_eq!(results.as_ptr(), result_backing);
    assert_eq!(results.capacity(), 2);
    for result in results {
        session.report_native_effect_result(result).unwrap();
    }
    commit(&mut session);
}

#[test]
fn predispatch_failure_is_adapter_rejection_and_cannot_consume_a_dispatched_owner() {
    let fixture = input_fixture(1);
    let mut control = NativeInputControl::with_storage_limits(Some(1), Some(1)).unwrap();
    let baseline = control.owned_data_capacity_bytes();
    retain(&mut control, fixture.request, 71);
    let command = control.command_to_dispatch().unwrap();
    assert!(
        control
            .fail_queued_dispatch(NativePointerPassthroughCommand {
                enabled: false,
                ..command
            })
            .is_none()
    );
    let result = control.fail_queued_dispatch(command).unwrap();
    assert!(format!("{result:?}").contains("DispatchFailed(AdapterRejected)"));
    assert!(control.fail_queued_dispatch(command).is_none());
    assert!(!control.has_pending_work());
    assert_eq!(control.owned_data_capacity_bytes(), baseline);
    let mut session = fixture.session;
    session.report_native_effect_result(result).unwrap();
    commit(&mut session);

    let dispatched = input_fixture(1);
    retain(&mut control, dispatched.request, 71);
    let command = control.command_to_dispatch().unwrap();
    let record = dispatch(&mut control);
    assert!(control.fail_queued_dispatch(command).is_none());
    assert!(matches!(
        control.classify(record),
        NativeInputResultDisposition::Current { .. }
    ));
    assert!(control.accept_applied(record).unwrap());
    assert!(control.fail_queued_dispatch(command).is_none());
    assert_eq!(control.owned_data_capacity_bytes(), baseline);
}

#[test]
fn selected_dimensions_match_actual_capacity_and_overflow_leaves_existing_owners_intact() {
    for bindings in [None, Some(0), Some(1), Some(7)] {
        for effects in [None, Some(0), Some(1), Some(7)] {
            let control = NativeInputControl::with_storage_limits(bindings, effects).unwrap();
            assert_eq!(control.observations.capacity(), bindings.unwrap_or(0));
            assert_eq!(
                control.pending.capacity(),
                effects.map_or(0, |slots| slots + 1)
            );
            let required = NativeInputControl::reservation_bytes(bindings, effects).unwrap();
            assert_eq!(control.owned_data_capacity_bytes(), required);
        }
    }
    let fixture = input_fixture(1);
    let mut control = NativeInputControl::with_storage_limits(Some(1), Some(1)).unwrap();
    retain(&mut control, fixture.request, 71);
    let baseline = control.owned_data_capacity_bytes();
    let command = control.command_to_dispatch();
    for (bindings, effects) in [
        (Some(usize::MAX), None),
        (None, Some(usize::MAX)),
        (Some(1), Some(usize::MAX - 1)),
        (
            Some(usize::MAX / size_of::<NativeInputObservation>()),
            Some(1),
        ),
    ] {
        assert_eq!(
            NativeInputControl::reservation_bytes(bindings, effects),
            None
        );
        assert_eq!(
            NativeInputControl::with_storage_limits(bindings, effects).unwrap_err(),
            NativeHostProtocolError::NativeReportStorageUnavailable
        );
        assert_eq!(control.binding_slots, Some(1));
        assert_eq!(control.pending_slots, Some(2));
        assert_eq!(control.command_to_dispatch(), command);
        assert_eq!(control.owned_data_capacity_bytes(), baseline);
    }
}

#[test]
fn one_byte_short_shared_allowance_rejects_without_replacing_the_previous_configuration() {
    use crate::mailbox::NativeHostBridge;
    use crate::pointer_event::NativePointerTranslator;

    let control = NativeInputControl::with_storage_limits(Some(1), Some(1)).unwrap();
    let reserved = NativeInputControl::reservation_bytes(Some(1), Some(1)).unwrap();
    assert_eq!(control.owned_data_capacity_bytes(), reserved);
    let bridge = NativeHostBridge::new(NativeViewportMap::default());
    let baseline = bridge.owned_input_capacity_bytes(None)
        + NativePointerTranslator::default().reserved_translation_capacity_bytes();
    assert!(bridge.configure_input_storage_allowance(usize::MAX, reserved));
    assert!(!bridge.configure_input_storage_allowance(baseline + reserved - 1, reserved));
    // This still requires output headroom. It succeeds only if rejection kept
    // the previous allowance instead of installing the undersized candidate.
    assert!(bridge.configure_snapshot_storage_allowance(0, 1, 1));
    assert!(bridge.configure_input_storage_allowance(baseline + reserved, reserved));
    assert!(!bridge.configure_input_storage_allowance(baseline + reserved - 1, reserved));
    assert_eq!(control.binding_slots, Some(1));
    assert_eq!(control.pending_slots, Some(2));
    assert_eq!(control.owned_data_capacity_bytes(), reserved);
    assert_eq!(
        bridge.owned_input_capacity_bytes(None)
            + NativePointerTranslator::default().reserved_translation_capacity_bytes(),
        baseline
    );
}
