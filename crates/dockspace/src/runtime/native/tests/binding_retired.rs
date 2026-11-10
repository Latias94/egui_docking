use super::*;
use crate::model::{DockspaceLayout, DockspaceNode, DockspaceRootLayout, DockspaceSurfaceLayout};
use crate::pointer_journal::AnyButtonDownAuthority;
use crate::runtime::HostInputOutcome;
use std::num::NonZeroUsize;

fn two_roots() -> (DockspaceSession, [NativeSurfaceBinding; 2]) {
    let layout = DockspaceLayout::new([1, 2].map(|identity| {
        DockspaceSurfaceLayout::new(
            SurfaceId::new(identity),
            DockspaceRootLayout::new(
                RootId::new(identity),
                DockspaceNode::central_tabs([ItemId::new(identity)]),
            ),
        )
    }))
    .unwrap();
    let mut session = DockspaceSession::from_layout(layout, DockPolicy::default()).unwrap();
    session
        .enable_managed_native_host(NativePointerRoster::Exact(Vec::new()))
        .unwrap();
    session
        .configure_managed_native_capabilities(full_managed_capabilities())
        .unwrap();
    for identity in [1, 2] {
        session
            .register_native_root(SurfaceId::new(identity), HostWindowToken::new(identity))
            .unwrap();
    }
    let report = complete(
        session
            .begin_native_host_frame(|_| NativeReceiverAnswer::Unknown)
            .unwrap(),
    );
    let bindings = report.native_bindings().try_into().unwrap();
    (session, bindings)
}

fn complete(mut frame: crate::runtime::DockspaceHostFrame<'_>) -> HostFrameReport {
    frame
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .unwrap();
    frame.commit().unwrap()
}

fn input(
    binding: NativeSurfaceBinding,
    pointer: u64,
    event: NativePointerEvent,
) -> NativePointerInput {
    NativePointerInput::new(
        NativePointerId::new(pointer),
        event,
        NativeDesktopPointerLocation::new(
            NativeDesktopPosition::Unknown,
            NativePointerHover::Unknown,
            None,
        ),
        NativePointerOwner::Native(binding.window()),
        NativePointerOwner::Unknown,
    )
}

#[test]
fn one_bounded_binding_fact_ends_five_streams_after_retry_and_preserves_another_binding() {
    let (mut session, [first, other]) = two_roots();
    for pointer in 1..=6 {
        session
            .record_native_pointer(input(
                if pointer == 6 { other } else { first },
                pointer,
                NativePointerEvent::ButtonPressed(NativePointerButton::Primary),
            ))
            .unwrap();
        commit_managed_frame(&mut session);
    }
    session.prepare_native_pointer_capacity().unwrap();
    session
        .native
        .as_mut()
        .unwrap()
        .recorder
        .configure_native_transport_capacity(NonZeroUsize::new(1).unwrap(), 1 << 20)
        .unwrap();
    let recorded = session.native_input_recorded_count();
    let pointer = session.native.as_ref().unwrap().recorder.pointer_through();
    let backend = session.engine.backend_ingress_committed_through();
    let version = session.version();
    session.report_native_input_binding_retired(first).unwrap();
    assert_eq!(session.native_input_recorded_count(), recorded + 1);
    assert_eq!(
        session.native.as_ref().unwrap().recorder.pointer_through(),
        pointer
    );
    assert_eq!(
        session
            .report_native_input_binding_retired(first)
            .unwrap_err()
            .native_kind(),
        Some(NativeHostErrorKind::InputCapacity)
    );
    assert_eq!(session.native_input_recorded_count(), recorded + 1);
    drop(
        session
            .begin_native_host_frame(|_| NativeReceiverAnswer::Unknown)
            .unwrap(),
    );
    assert_eq!(session.engine.backend_ingress_committed_through(), backend);
    assert_eq!(
        session
            .engine
            .runtime_retention_manifest()
            .pointer()
            .active_stream_count(),
        6
    );
    assert_eq!(session.version(), version);

    let report = complete(
        session
            .begin_native_host_frame(|_| NativeReceiverAnswer::Unknown)
            .unwrap(),
    );
    assert_eq!(
        report.inputs(),
        [HostInputOutcome::NativeInputBindingRetired { binding: first }]
    );
    assert_eq!(
        session
            .engine
            .runtime_retention_manifest()
            .pointer()
            .active_stream_count(),
        1
    );
    assert_eq!(
        session.engine.pointer_button_authority(),
        AnyButtonDownAuthority::KnownDown
    );
    assert!(session.is_current_native_binding(first));
    assert!(session.is_current_native_binding(other));
    session
        .record_native_pointer(input(
            other,
            6,
            NativePointerEvent::ButtonReleased(NativePointerButton::Primary),
        ))
        .unwrap();
    commit_managed_frame(&mut session);
    assert_eq!(
        session.engine.pointer_button_authority(),
        AnyButtonDownAuthority::KnownAllReleased
    );
}

#[test]
fn binding_retirement_closes_suppressed_scroll_provider_tails_without_ending_another_sequence() {
    let (mut session, [first, other]) = two_roots();
    let event = |identity, phase| {
        NativePointerEvent::Scrolled(NativeScrollEvent::new(
            NativeScrollDeviceId::new(identity),
            phase,
            NativeScrollMomentum::Direct,
            NativeScrollModifiers::Exact {
                shift: false,
                control: false,
                alt: false,
                command: false,
            },
        ))
    };
    for (binding, identity) in [(first, 1), (other, 2)] {
        session
            .record_native_pointer(input(
                binding,
                identity,
                event(
                    identity,
                    NativeScrollPhase::Begin {
                        sequence: NativeScrollSequenceId::new(identity),
                        delta: None,
                    },
                ),
            ))
            .unwrap();
    }
    commit_managed_frame(&mut session);
    assert_eq!(
        session
            .engine
            .runtime_retention_manifest()
            .scroll()
            .active_sessions(),
        2
    );
    session.report_native_input_binding_retired(first).unwrap();
    session
        .record_native_pointer(input(
            other,
            2,
            event(
                2,
                NativeScrollPhase::End {
                    sequence: NativeScrollSequenceId::new(2),
                    delta: None,
                },
            ),
        ))
        .unwrap();
    let report = complete(
        session
            .begin_native_host_frame(|_| NativeReceiverAnswer::Unknown)
            .unwrap(),
    );
    assert_eq!(
        report.inputs(),
        [HostInputOutcome::NativeInputBindingRetired { binding: first }]
    );
    assert_eq!(
        session
            .engine
            .runtime_retention_manifest()
            .scroll()
            .active_sessions(),
        0
    );
    assert_eq!(
        session
            .engine
            .runtime_retention_manifest()
            .scroll()
            .sequence_watermark_guards(),
        1
    );
    assert_eq!(
        session
            .engine
            .runtime_retention_manifest()
            .pointer()
            .active_stream_count(),
        2
    );
}

#[test]
fn binding_retirement_rejects_a_foreign_provider_before_recording() {
    let (mut session, _) = two_roots();
    let (_, [foreign, _]) = two_roots();
    let before = session.native_input_recorded_count();
    let error = session
        .report_native_input_binding_retired(foreign)
        .unwrap_err();
    assert_eq!(error.native_kind(), Some(NativeHostErrorKind::StaleBinding));
    assert_eq!(session.native_input_recorded_count(), before);
    let (mut observed, binding) = native_root_session();
    assert_eq!(
        observed
            .report_native_input_binding_retired(binding)
            .unwrap_err()
            .native_kind(),
        Some(NativeHostErrorKind::Unsupported)
    );
}
