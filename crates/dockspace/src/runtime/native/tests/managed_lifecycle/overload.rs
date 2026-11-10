use super::*;
use crate::engine::{CoreHostFrameError, EngineInput};
use crate::geometry::LogicalPoint;
use crate::graph::Axis;
use crate::pointer_journal::AnyButtonDownAuthority;
use crate::runtime::{
    ContainedResizeDirection, DockspaceManipulationDelivery, DockspaceManipulationStatus,
    HostInputOutcome, NativeHostErrorKind, NativeInputBudget, NativeMonotonicClock,
};
use std::time::Duration;

#[derive(Debug, Clone, Copy)]
enum FiniteReset {
    Overload,
    DeviceRemoval,
    BindingRetired,
}

impl FiniteReset {
    const ALL: [Self; 2] = [Self::Overload, Self::DeviceRemoval];
    // Binding retirement preserves unknown and unrelated scroll streams.
    const RELEASE_RESETS: [Self; 3] = [Self::Overload, Self::DeviceRemoval, Self::BindingRetired];

    const fn recorded_delta(self) -> u64 {
        match self {
            Self::Overload => 0,
            Self::DeviceRemoval | Self::BindingRetired => 1,
        }
    }

    fn record_backend_fact(self, session: &mut DockspaceSession) -> Option<HostInputOutcome> {
        match self {
            Self::Overload => None,
            Self::DeviceRemoval => {
                session.report_native_input_device_removal().unwrap();
                Some(HostInputOutcome::NativeInputDeviceRemoval)
            }
            Self::BindingRetired => {
                let binding = session
                    .native
                    .as_ref()
                    .unwrap()
                    .current_binding(SURFACE)
                    .unwrap();
                session
                    .report_native_input_binding_retired(binding)
                    .unwrap();
                Some(HostInputOutcome::NativeInputBindingRetired { binding })
            }
        }
    }

    const fn gesture_cause(self) -> crate::interaction::InteractionCancelReason {
        match self {
            Self::Overload => crate::interaction::InteractionCancelReason::InputOverload,
            Self::DeviceRemoval | Self::BindingRetired => {
                crate::interaction::InteractionCancelReason::PointerStreamCancelled
            }
        }
    }
}

#[test]
fn overload_cancel_rejects_nonmanaged_hosts_without_poisoning_the_frame() {
    let (mut session, binding) = native_root_session();
    let version = session.version();
    let recorded = session.native_input_recorded_count();
    let mut frame = session
        .begin_host_frame()
        .expect("the observed frame begins");

    let error = frame
        .cancel_native_input_for_overload()
        .expect_err("an observed-root host cannot cancel managed input");
    assert_eq!(error.native_kind(), Some(NativeHostErrorKind::Unsupported));
    frame
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .expect("a rejected cancellation leaves presentation usable");
    let report = frame.commit().expect("the unchanged frame commits");

    assert!(report.inputs().is_empty());
    assert_eq!(session.version(), version);
    assert_eq!(session.native_input_recorded_count(), recorded);
    assert!(session.is_current_native_binding(binding));
}

#[test]
fn overload_cancel_rejects_pending_native_input_then_accepts_the_complete_prefix() {
    let (mut session, binding) =
        native_root_session_with_profile(NativeHostProfile::ManagedDesktop);
    let version = session.version();
    session
        .record_native_pointer_sequence(
            (0..257).map(|_| unknown_native_pointer_input(11, NativePointerEvent::Moved)),
        )
        .expect("independent native edges record before overload cancellation");
    let recorded = session.native_input_recorded_count();
    let pointer_through = session.native.as_ref().unwrap().recorder.pointer_through();
    let clock = NativeMonotonicClock::new(|| Duration::ZERO);
    let mut budget = NativeInputBudget::new(&clock);
    let mut prefix = session
        .poll_native_host_frame(&mut budget, |_| NativeReceiverAnswer::Unknown)
        .expect("the bounded prefix reduces")
        .expect("independent input can seal at the budget boundary");
    assert!(prefix.native_input_pending());

    let error = prefix
        .cancel_native_input_for_overload()
        .expect_err("cancellation cannot overtake an accepted native suffix");
    assert_eq!(
        error.native_kind(),
        Some(NativeHostErrorKind::OperationConflict)
    );
    prefix
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .expect("the rejected cancellation does not poison the prefix");
    assert!(
        prefix
            .commit()
            .expect("the prefix commits")
            .native_input_pending()
    );

    let mut terminal = session
        .begin_native_host_frame(|_| NativeReceiverAnswer::Unknown)
        .expect("the final accepted edge reduces before cancellation");
    assert!(!terminal.native_input_pending());
    let submitted = terminal
        .cancel_native_input_for_overload()
        .expect("the complete native prefix authorizes terminal cancellation");
    terminal
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .expect("terminal cancellation still permits presentation");
    let report = terminal.commit().expect("terminal cancellation commits");

    assert_eq!(
        report.submitted_action_outcome(submitted),
        Some(&crate::runtime::HostInputOutcome::NativeInputCancelledForOverload)
    );
    assert!(!report.native_input_pending());
    assert_eq!(session.native_input_recorded_count(), recorded);
    assert_eq!(
        session.native.as_ref().unwrap().recorder.pointer_through(),
        pointer_through
    );
    assert_eq!(session.version(), version);
    assert!(session.is_current_native_binding(binding));
    assert_eq!(
        session
            .engine
            .runtime_retention_manifest()
            .pointer()
            .active_stream_count(),
        0
    );
}

#[test]
fn overload_cancel_drop_preserves_the_active_gesture_and_allows_exact_retry() {
    let (mut session, binding, _) = pressed_session();
    let owner = session.engine.interaction().active_stream();
    let provider = session.engine.backend_ingress_provider();
    let version = session.version();
    let recorded = session.native_input_recorded_count();
    let pointer_through = session.native.as_ref().unwrap().recorder.pointer_through();
    let source_sequence = session.committed_source_sequence;
    let abandoned = {
        let mut frame = session
            .begin_native_host_frame(|_| NativeReceiverAnswer::Unknown)
            .expect("the speculative cancellation frame begins");
        frame
            .cancel_native_input_for_overload()
            .expect("cancellation stages privately before frame drop")
    };

    assert!(session.has_active_gesture());
    assert_eq!(session.engine.interaction().active_stream(), owner);
    assert_eq!(
        session.engine.pointer_button_authority(),
        AnyButtonDownAuthority::KnownDown
    );
    assert_eq!(session.committed_source_sequence, source_sequence);
    assert_eq!(session.native_input_recorded_count(), recorded);

    let mut retry = session
        .begin_native_host_frame(|_| NativeReceiverAnswer::Unknown)
        .expect("the same published state permits a cancellation retry");
    let submitted = retry
        .cancel_native_input_for_overload()
        .expect("the retry stages without replacing the provider");
    retry
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .expect("the successful cancellation retains presentation authority");
    let report = retry.commit().expect("the retry publishes atomically");

    assert!(report.submitted_action_outcome(abandoned).is_none());
    assert_eq!(
        report.submitted_action_outcome(submitted),
        Some(&HostInputOutcome::NativeInputCancelledForOverload)
    );
    assert!(!session.has_active_gesture());
    assert!(matches!(
        session.engine.pointer_button_authority(),
        AnyButtonDownAuthority::Unknown(_)
    ));
    assert_eq!(session.engine.backend_ingress_provider(), provider);
    assert_eq!(
        session.native.as_ref().unwrap().recorder.pointer_through(),
        pointer_through
    );
    assert_eq!(session.native_input_recorded_count(), recorded);
    assert_eq!(session.version(), version);
    assert!(session.is_current_native_binding(binding));
    let retained = session.engine.runtime_retention_manifest().pointer();
    assert_eq!(retained.active_provider_count(), 1);
    assert_eq!(retained.active_stream_count(), 0);
    assert_eq!(retained.retired_lease_guards(), 0);
}

fn pressed_session() -> (DockspaceSession, NativeSurfaceBinding, PresentedReceiver) {
    let (mut session, binding) = programmatic_tear_off_session();
    let receiver = presented_tab_receiver(&mut session, ITEM);
    let point = receiver.center();
    let desktop = PhysicalPoint::new(ROOT_ORIGIN_X + point.x(), ROOT_ORIGIN_Y + point.y())
        .expect("the source desktop point is finite");
    session
        .record_native_pointer(NativePointerInput::new(
            NativePointerId::new(1),
            NativePointerEvent::ButtonPressed(NativePointerButton::Primary),
            NativeDesktopPointerLocation::new(
                NativeDesktopPosition::Exact(desktop),
                NativePointerHover::Dock(binding.window()),
                None,
            ),
            NativePointerOwner::Native(binding.window()),
            NativePointerOwner::Native(binding.window()),
        ))
        .expect("the real managed press records");
    let mut press = session
        .begin_native_host_frame(with_window_hit(receiver, |query| {
            receiver_answer(query, receiver)
        }))
        .expect("the press resolves against its exact presented receiver");
    press
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .expect("the press retains its source output");
    press.commit().expect("the active gesture commits");
    assert!(session.has_active_gesture());
    assert_eq!(
        session.engine.pointer_button_authority(),
        AnyButtonDownAuthority::KnownDown
    );

    (session, binding, receiver)
}

fn native_input(
    binding: NativeSurfaceBinding,
    pointer: u64,
    event: NativePointerEvent,
    point: LogicalPoint,
) -> NativePointerInput {
    NativePointerInput::new(
        NativePointerId::new(pointer),
        event,
        NativeDesktopPointerLocation::new(
            NativeDesktopPosition::Exact(
                PhysicalPoint::new(ROOT_ORIGIN_X + point.x(), ROOT_ORIGIN_Y + point.y()).unwrap(),
            ),
            NativePointerHover::Dock(binding.window()),
            None,
        ),
        NativePointerOwner::Native(binding.window()),
        NativePointerOwner::Native(binding.window()),
    )
}

fn cancel_and_commit(session: &mut DockspaceSession) -> HostFrameReport {
    reset_and_commit(session, FiniteReset::Overload)
}

fn reset_and_commit(session: &mut DockspaceSession, reset: FiniteReset) -> HostFrameReport {
    let recorded = session.native_input_recorded_count();
    let through = session.native.as_ref().unwrap().recorder.pointer_through();
    let backend_outcome = reset.record_backend_fact(session);
    let mut frame = session
        .begin_native_host_frame(|_| NativeReceiverAnswer::Unknown)
        .expect("the finite input-reset frame begins");
    let submitted = match reset {
        FiniteReset::Overload => Some(frame.cancel_native_input_for_overload().unwrap()),
        FiniteReset::DeviceRemoval | FiniteReset::BindingRetired => None,
    };
    frame
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .unwrap();
    let report = frame.commit().expect("the finite input reset commits");
    if let Some(submitted) = submitted {
        assert_eq!(
            report.submitted_action_outcome(submitted),
            Some(&HostInputOutcome::NativeInputCancelledForOverload)
        );
    } else {
        let expected = backend_outcome.unwrap();
        assert_eq!(
            report
                .inputs()
                .iter()
                .filter(|input| *input == &expected)
                .count(),
            1,
            "reset={reset:?}"
        );
    }
    assert_eq!(
        session.native_input_recorded_count(),
        recorded + reset.recorded_delta(),
        "reset={reset:?}"
    );
    assert_eq!(
        session.native.as_ref().unwrap().recorder.pointer_through(),
        through,
        "a reset records no synthetic pointer edge: {reset:?}"
    );
    report
}

fn paint_and_commit(mut frame: DockspaceHostFrame<'_>) -> HostFrameReport {
    let receipt = execute_surface_program(&mut frame, SURFACE);
    frame.confirm_surface_painted(receipt).unwrap();
    frame
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .unwrap();
    frame.commit().expect("the real surface output commits")
}

#[test]
fn backend_reset_then_same_pointer_press_rebinds_the_presented_receiver_in_one_batch() {
    for reset in [FiniteReset::DeviceRemoval, FiniteReset::BindingRetired] {
        let (mut session, binding, receiver) = pressed_session();
        let previous = session.engine.interaction().active_stream().unwrap();
        let recorded = session.native_input_recorded_count();
        let pointer = session.native.as_ref().unwrap().recorder.pointer_through();
        let expected = reset.record_backend_fact(&mut session).unwrap();
        session
            .record_native_pointer(native_input(
                binding,
                1,
                NativePointerEvent::ButtonPressed(NativePointerButton::Primary),
                receiver.center(),
            ))
            .unwrap();
        let mut frame = session
            .begin_native_host_frame(with_window_hit(receiver, |query| {
                receiver_answer(query, receiver)
            }))
            .expect("reset and fresh press share the original exact presented receiver");
        frame
            .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
            .unwrap();
        let report = frame.commit().unwrap();
        assert_eq!(
            report
                .inputs()
                .iter()
                .filter(|outcome| *outcome == &expected)
                .count(),
            1,
            "reset={reset:?}"
        );
        assert!(report.manipulation_feedback().iter().any(|feedback| {
            feedback.status() == DockspaceManipulationStatus::Cancelled(reset.gesture_cause())
        }));
        let current = session.engine.interaction().active_stream().unwrap();
        assert_eq!(current.lease(), previous.lease());
        assert_eq!(current.pointer(), previous.pointer());
        assert!(current.incarnation() > previous.incarnation());
        assert_eq!(session.native_input_recorded_count(), recorded + 2);
        assert_eq!(
            session
                .native
                .as_ref()
                .unwrap()
                .recorder
                .pointer_through()
                .get(),
            pointer.get() + 1,
        );
    }
}

#[test]
fn overload_cancel_allows_a_held_pointer_to_press_again_with_a_fresh_stream() {
    let (mut session, binding, receiver) = pressed_session();
    let old = session.engine.interaction().active_stream().unwrap();
    let through = session.native.as_ref().unwrap().recorder.pointer_through();
    cancel_and_commit(&mut session);

    session
        .record_native_pointer(native_input(
            binding,
            1,
            NativePointerEvent::ButtonPressed(NativePointerButton::Primary),
            receiver.center(),
        ))
        .expect("a held pointer may start over without an invented release");
    let mut frame = session
        .begin_native_host_frame(with_window_hit(receiver, |query| {
            receiver_answer(query, receiver)
        }))
        .expect("the new press is not rejected as already held");
    frame
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .unwrap();
    frame.commit().unwrap();

    let new = session.engine.interaction().active_stream().unwrap();
    assert_eq!(new.lease(), old.lease());
    assert_eq!(new.pointer(), old.pointer());
    assert!(new.incarnation() > old.incarnation());
    assert_eq!(
        session
            .native
            .as_ref()
            .unwrap()
            .recorder
            .pointer_through()
            .get(),
        through.get() + 1
    );
    assert_eq!(
        session.engine.pointer_button_authority(),
        AnyButtonDownAuthority::KnownDown
    );
}

#[test]
fn overload_cancel_is_terminal_and_a_failed_commit_preserves_the_published_owner() {
    for append_pointer in [false, true] {
        let (mut session, _, _) = pressed_session();
        let owner = session.engine.interaction().active_stream();
        let provider = session.engine.pointer_provider().unwrap();
        let version = session.version();
        let sequence = session.committed_source_sequence;
        let through = session.native.as_ref().unwrap().recorder.pointer_through();
        let mut frame = session
            .begin_native_host_frame(|_| NativeReceiverAnswer::Unknown)
            .unwrap();
        frame.cancel_native_input_for_overload().unwrap();
        assert!(frame.select_item_current(SECOND_ITEM).is_err());
        assert!(frame.replace_policy(DockPolicy::default()).is_err());
        assert!(frame.cancel_native_input_for_overload().is_err());
        let error = if append_pointer {
            frame.frame.submit_pointer_journal(
                provider,
                crate::pointer_journal::PointerEdgeJournal::new(through, through, Vec::new())
                    .unwrap(),
            )
        } else {
            frame.frame.append_input(
                crate::runtime::host_frame::APPLICATION_INPUT_SOURCE,
                crate::ids::SourceSequence::new(sequence + 2),
                EngineInput::ValidateWorkspace,
            )
        }
        .expect_err("core rejects all later input independently of facade guards");
        assert_eq!(error, CoreHostFrameError::InputAfterOverloadCancellation);
        assert!(
            frame.commit().is_err(),
            "a poisoned candidate cannot publish cleanup"
        );
        assert_eq!(session.engine.interaction().active_stream(), owner);
        assert_eq!(
            session.engine.pointer_button_authority(),
            AnyButtonDownAuthority::KnownDown
        );
        assert_eq!(session.version(), version);
        assert_eq!(session.committed_source_sequence, sequence);
        assert_eq!(
            session.native.as_ref().unwrap().recorder.pointer_through(),
            through
        );
        cancel_and_commit(&mut session);
        assert!(!session.has_active_gesture());
    }
}

#[test]
fn overload_cancel_ends_gesture_and_owned_and_suppressed_scroll_idempotently() {
    for reset in FiniteReset::ALL {
        let mut builder = Workspace::builder();
        let tabs = builder.insert_node(Node::tabs((1..=16).map(ItemId::new)));
        builder.set_root(ROOT, RootRecord::new(tabs).with_central(tabs));
        builder.set_surface(SURFACE, SurfacePresentation::with_main(ROOT));
        let (mut session, binding) = managed_session_from_workspace(builder.build().unwrap());
        let tab = presented_tab_receiver(&mut session, ITEM);
        let scroll_receiver = {
            let mut frame = session
                .begin_native_host_frame(|_| NativeReceiverAnswer::Unknown)
                .unwrap();
            let plan = frame.paint_plan(SURFACE).unwrap().unwrap();
            plan.tab_bars()
                .find_map(|bar| plan.receiver_for_tab_strip_scroll(bar))
                .expect("the overflowing strip publishes a real scroll receiver")
        };
        let begin = |token| {
            NativePointerEvent::Scrolled(NativeScrollEvent::new(
                NativeScrollDeviceId::new(1),
                NativeScrollPhase::Begin {
                    sequence: NativeScrollSequenceId::new(token),
                    delta: None,
                },
                NativeScrollMomentum::Direct,
                NativeScrollModifiers::Exact {
                    shift: false,
                    control: false,
                    alt: false,
                    command: false,
                },
            ))
        };
        session
            .record_native_pointer_batch([
                native_input(
                    binding,
                    1,
                    NativePointerEvent::ButtonPressed(NativePointerButton::Primary),
                    tab.center(),
                ),
                native_input(binding, 2, begin(41), scroll_receiver.center()),
                unknown_native_pointer_input(3, begin(42)),
            ])
            .unwrap();
        let mut frame = session
            .begin_native_host_frame(with_window_hit(tab, |query| {
                let descriptor =
                    if matches!(query.purpose(), NativeReceiverPurpose::ScrollDelivery(_)) {
                        scroll_receiver
                    } else {
                        tab.descriptor
                    };
                receiver_answer(query, descriptor)
            }))
            .unwrap();
        frame
            .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
            .unwrap();
        frame.commit().unwrap();
        assert!(session.has_active_gesture());
        assert_eq!(
            session
                .engine
                .runtime_retention_manifest()
                .scroll()
                .active_sessions(),
            2
        );
        let version = session.version();
        let provider = session.engine.backend_ingress_provider();
        let recorded = session.native_input_recorded_count();
        let report = reset_and_commit(&mut session, reset);
        assert!(
            report
                .manipulation_feedback()
                .iter()
                .any(|feedback| feedback.status()
                    == DockspaceManipulationStatus::Cancelled(reset.gesture_cause()))
        );
        assert!(!session.has_active_gesture());
        assert!(matches!(
            session.engine.pointer_button_authority(),
            AnyButtonDownAuthority::Unknown(_)
        ));
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
            0
        );
        assert!(
            reset_and_commit(&mut session, reset)
                .manipulation_feedback()
                .is_empty()
        );
        assert_eq!(session.version(), version);
        assert_eq!(session.engine.backend_ingress_provider(), provider);
        assert_eq!(
            session.native_input_recorded_count(),
            recorded + 2 * reset.recorded_delta()
        );

        session
            .record_native_pointer(unknown_native_pointer_input(3, begin(43)))
            .unwrap();
        commit_managed_frame(&mut session);
        assert_eq!(
            session
                .engine
                .runtime_retention_manifest()
                .scroll()
                .active_sessions(),
            1
        );
        reset_and_commit(&mut session, reset);
        session
            .record_native_pointer(unknown_native_pointer_input(3, begin(43)))
            .unwrap();
        assert!(
            session
                .begin_native_host_frame(|_| NativeReceiverAnswer::Unknown)
                .is_err(),
            "neither reset may reset the provider's scroll token watermark: {reset:?}"
        );
    }
}

fn contained_resize_receiver(session: &mut DockspaceSession) -> PresentedReceiver {
    presented_receiver(session, |plan| {
        let contained = plan.contained().next().unwrap();
        let handle = contained
            .resize()
            .find(|handle| handle.direction() == ContainedResizeDirection::East)
            .unwrap();
        plan.receiver_for_contained_resize(contained, handle)
            .unwrap()
    })
}

#[test]
fn overload_cancel_restores_durable_contained_projection_without_changing_workspace() {
    let (mut session, binding) = programmatic_tear_off_session();
    let receiver = contained_resize_receiver(&mut session);
    let floating = FloatingPresentationId::new(1);
    let durable = session.view().contained(floating).unwrap().rect();
    let moved = LogicalPoint::new(receiver.center().x() + 32.0, receiver.center().y()).unwrap();
    session
        .record_native_pointer_batch([
            native_input(
                binding,
                1,
                NativePointerEvent::ButtonPressed(NativePointerButton::Primary),
                receiver.center(),
            ),
            native_input(binding, 1, NativePointerEvent::Moved, moved),
        ])
        .unwrap();
    let mut frame = session
        .begin_native_host_frame(with_window_hit(receiver, |query| {
            receiver_answer(query, receiver)
        }))
        .unwrap();
    assert_eq!(
        frame
            .paint_plan(SURFACE)
            .unwrap()
            .unwrap()
            .contained()
            .next()
            .unwrap()
            .outer_bounds()
            .width(),
        durable.width() + 32.0
    );
    let mut report = paint_and_commit(frame);
    present_surface_outputs(&mut session, &mut report);
    commit_managed_frame(&mut session);
    assert!(session.has_active_gesture());
    let workspace = session.engine.workspace().clone();
    let version = session.version();

    let mut frame = session
        .begin_native_host_frame(|_| NativeReceiverAnswer::Unknown)
        .unwrap();
    frame.cancel_native_input_for_overload().unwrap();
    let plan = frame.paint_plan(SURFACE).unwrap().unwrap();
    assert_eq!(plan.contained().next().unwrap().outer_bounds(), durable);
    assert!(!plan.has_manipulation_preview());
    paint_and_commit(frame);
    assert_eq!(session.engine.workspace(), &workspace);
    assert_eq!(session.version(), version);
    assert!(!session.has_active_gesture());
}

fn assert_delivered_release_survives_reset(
    mut session: DockspaceSession,
    mut release_report: HostFrameReport,
    delivery: DockspaceManipulationDelivery,
    reset: FiniteReset,
) {
    assert!(release_report.workspace_changed());
    assert!(release_report.take_painted_outputs().is_empty());
    assert!(!session.has_active_gesture());
    assert_eq!(session.engine.pending_release_preview(), None);
    let [feedback] = release_report.manipulation_feedback() else {
        panic!("a successful release must deliver one terminal: {release_report:?}");
    };
    assert_eq!(
        feedback.status(),
        DockspaceManipulationStatus::Delivered(delivery)
    );
    // A successor output is independent of the already committed edit.
    let (output, _) = paint_native_output(&mut session);
    let key = output.presentation_artifact().output().key();
    let version = session.version();
    let workspace = session.engine.workspace().clone();
    let recorded = session.native_input_recorded_count();
    let through = session.native.as_ref().unwrap().recorder.pointer_through();
    let backend_outcome = reset.record_backend_fact(&mut session);
    {
        let mut frame = session
            .begin_native_host_frame(|_| NativeReceiverAnswer::Unknown)
            .unwrap();
        if matches!(reset, FiniteReset::Overload) {
            frame.cancel_native_input_for_overload().unwrap();
        }
    }
    assert_eq!(
        session.native_input_recorded_count(),
        recorded + reset.recorded_delta()
    );
    assert_eq!(
        session.engine.workspace(),
        &workspace,
        "dropping the reset candidate preserves the accepted release: {reset:?}"
    );
    assert_eq!(session.version(), version);
    let report = match reset {
        FiniteReset::Overload => cancel_and_commit(&mut session),
        FiniteReset::DeviceRemoval | FiniteReset::BindingRetired => {
            let report = commit_managed_frame_report(&mut session);
            let expected = backend_outcome.unwrap();
            assert_eq!(
                report
                    .inputs()
                    .iter()
                    .filter(|input| *input == &expected)
                    .count(),
                1,
                "the dropped reset replays its original single backend record: {reset:?}"
            );
            report
        }
    };
    assert_eq!(
        session.native_input_recorded_count(),
        recorded + reset.recorded_delta()
    );
    assert_eq!(
        session.native.as_ref().unwrap().recorder.pointer_through(),
        through
    );
    assert!(
        report.manipulation_feedback().is_empty(),
        "accepted release is not a live gesture to cancel"
    );
    assert_eq!(session.engine.pending_release_preview(), None);
    assert!(
        session
            .engine
            .presentation_retention_manifest()
            .retains(key),
        "the successor output must survive input cancellation"
    );
    assert_eq!(session.engine.workspace(), &workspace);
    assert_eq!(session.version(), version);

    session
        .report_surface_presentation(output, SurfacePresentationResult::Dropped)
        .unwrap();
    let dropped = commit_managed_frame_report(&mut session);
    assert!(!dropped.workspace_changed());
    assert!(dropped.manipulation_feedback().is_empty());
    assert_eq!(session.engine.pending_release_preview(), None);
    assert!(
        !session
            .engine
            .presentation_retention_manifest()
            .retains(key)
    );
    assert_eq!(session.engine.workspace(), &workspace);
    assert_eq!(session.version(), version);
    assert!(
        reset_and_commit(&mut session, reset)
            .manipulation_feedback()
            .is_empty()
    );
    assert_eq!(session.engine.workspace(), &workspace);
    assert_eq!(session.version(), version);
}

#[test]
fn overload_cancel_preserves_delivered_contained_release_and_its_successor_output() {
    for reset in FiniteReset::RELEASE_RESETS {
        let (mut session, binding) = programmatic_tear_off_session();
        let receiver = contained_resize_receiver(&mut session);
        let floating = FloatingPresentationId::new(1);
        let original = session.view().contained(floating).unwrap().rect();
        let moved = LogicalPoint::new(receiver.center().x() + 32.0, receiver.center().y()).unwrap();
        session
            .record_native_pointer_batch([
                native_input(
                    binding,
                    1,
                    NativePointerEvent::ButtonPressed(NativePointerButton::Primary),
                    receiver.center(),
                ),
                native_input(binding, 1, NativePointerEvent::Moved, moved),
                native_input(
                    binding,
                    1,
                    NativePointerEvent::ButtonReleased(NativePointerButton::Primary),
                    moved,
                ),
            ])
            .unwrap();
        let mut frame = session
            .begin_native_host_frame(with_window_hit(receiver, |query| {
                receiver_answer(query, receiver)
            }))
            .unwrap();
        frame
            .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
            .unwrap();
        let report = frame.commit().unwrap();
        assert_eq!(
            session.view().contained(floating).unwrap().rect(),
            crate::geometry::LogicalRect::new(
                original.x(),
                original.y(),
                original.width() + 32.0,
                original.height()
            )
            .unwrap()
        );
        assert_delivered_release_survives_reset(
            session,
            report,
            DockspaceManipulationDelivery::ContainedTransform,
            reset,
        );
    }
}

#[test]
fn overload_cancel_preserves_a_release_accepted_earlier_in_the_same_frame() {
    let (mut session, binding) = programmatic_tear_off_session();
    let receiver = contained_resize_receiver(&mut session);
    let floating = FloatingPresentationId::new(1);
    let original = session.view().contained(floating).unwrap().rect();
    let moved = LogicalPoint::new(receiver.center().x() + 32.0, receiver.center().y()).unwrap();
    session
        .record_native_pointer_batch([
            native_input(
                binding,
                1,
                NativePointerEvent::ButtonPressed(NativePointerButton::Primary),
                receiver.center(),
            ),
            native_input(binding, 1, NativePointerEvent::Moved, moved),
            native_input(
                binding,
                1,
                NativePointerEvent::ButtonReleased(NativePointerButton::Primary),
                moved,
            ),
        ])
        .unwrap();
    let mut frame = session
        .begin_native_host_frame(with_window_hit(receiver, |query| {
            receiver_answer(query, receiver)
        }))
        .unwrap();
    let submitted = frame.cancel_native_input_for_overload().unwrap();
    frame
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .unwrap();
    let report = frame.commit().unwrap();
    assert_eq!(
        session.view().contained(floating).unwrap().rect(),
        crate::geometry::LogicalRect::new(
            original.x(),
            original.y(),
            original.width() + 32.0,
            original.height()
        )
        .unwrap()
    );
    assert_eq!(
        report.submitted_action_outcome(submitted),
        Some(&HostInputOutcome::NativeInputCancelledForOverload)
    );
    assert_delivered_release_survives_reset(
        session,
        report,
        DockspaceManipulationDelivery::ContainedTransform,
        FiniteReset::Overload,
    );
}

#[test]
fn overload_cancel_cannot_be_recorded_as_an_ordinary_backend_action() {
    let (mut session, _) = native_root_session_with_profile(NativeHostProfile::ManagedDesktop);
    let native = session.native.as_mut().unwrap();
    let provider = native.recorder.lease();
    let recorded = native.recorder.recorded_through();
    assert_eq!(
        native
            .recorder
            .record_semantic_input(EngineInput::CancelNativeInputForOverload { provider }),
        Err(crate::backend_ingress::BackendIngressError::SemanticInputIsHostControl)
    );
    assert_eq!(native.recorder.recorded_through(), recorded);
}

#[test]
fn overload_cancel_preserves_delivered_drag_release_and_its_successor_output() {
    for reset in FiniteReset::RELEASE_RESETS {
        let mut builder = Workspace::builder();
        let source_tabs = builder.insert_node(Node::tabs([ITEM, SECOND_ITEM]));
        let target_tabs = builder.insert_node(Node::tabs([ItemId::new(3)]));
        let split = builder
            .insert_node(Node::equal_split(Axis::Horizontal, [source_tabs, target_tabs]).unwrap());
        builder.set_root(ROOT, RootRecord::new(split).with_central(target_tabs));
        builder.set_surface(SURFACE, SurfacePresentation::with_main(ROOT));
        let (mut session, binding) = managed_session_from_workspace(builder.build().unwrap());
        let receiver = presented_tab_receiver(&mut session, ITEM);
        let target = {
            let mut frame = session
                .begin_native_host_frame(|_| NativeReceiverAnswer::Unknown)
                .unwrap();
            frame
                .paint_plan(SURFACE)
                .unwrap()
                .unwrap()
                .center_drop_receiver_for_item(ItemId::new(3))
                .unwrap()
        };
        session
            .record_native_pointer_batch([
                native_input(
                    binding,
                    1,
                    NativePointerEvent::ButtonPressed(NativePointerButton::Primary),
                    receiver.center(),
                ),
                native_input(binding, 1, NativePointerEvent::Moved, target.center()),
                native_input(
                    binding,
                    1,
                    NativePointerEvent::ButtonReleased(NativePointerButton::Primary),
                    target.center(),
                ),
            ])
            .unwrap();
        let mut frame = session
            .begin_native_host_frame(with_window_hit(receiver, |query| {
                receiver_answer(
                    query,
                    if matches!(query.purpose(), NativeReceiverPurpose::HoverHit) {
                        target
                    } else {
                        receiver.descriptor
                    },
                )
            }))
            .unwrap();
        frame
            .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
            .unwrap();
        let report = frame.commit().unwrap();
        assert_eq!(
            session.view().item(ITEM).unwrap().tabs().items(),
            &[ItemId::new(3), ITEM]
        );
        assert_eq!(
            session.view().item(SECOND_ITEM).unwrap().tabs().items(),
            &[SECOND_ITEM]
        );
        assert_delivered_release_survives_reset(
            session,
            report,
            DockspaceManipulationDelivery::Dock,
            reset,
        );
    }
}

#[test]
fn overload_cancel_preserves_an_accepted_native_create_lifecycle() {
    let PendingNativeCreate {
        mut session,
        root_binding,
        child_binding,
        placement,
        request,
    } = request_native_create();
    let workspace = session.engine.workspace().clone();
    let version = session.version();
    let mut report = cancel_and_commit(&mut session);
    assert!(report.take_native_effects().is_empty());
    assert!(report.native_bindings().contains(&child_binding));
    assert!(session.is_current_native_binding(root_binding));
    assert!(session.is_current_native_binding(child_binding));
    assert_eq!(session.engine.workspace(), &workspace);
    assert_eq!(session.version(), version);

    let Some(NativeEffectAcknowledgement::Create(created)) = request.accepted() else {
        panic!("the original accepted create still owns its acknowledgement");
    };
    session
        .report_managed_native_snapshot(
            [
                (root_binding, root_window_facts()),
                (
                    child_binding,
                    NativeWindowFacts::live()
                        .with_content_bounds(placement)
                        .with_outer_bounds(placement)
                        .with_native_scale_factor(native_scale())
                        .with_presentation_scale_factor(native_scale())
                        .with_input(NativeWindowInputState::ReceivesInput, None)
                        .with_presentation(NativeWindowPresentationState::Hidden)
                        .with_create_readiness(
                            crate::runtime::NativeCreateReadiness::Created,
                            Some(created),
                        ),
                ),
            ],
            NativeWorkAreaRoster::Exact(vec![work_area(1)]),
        )
        .unwrap();
    commit_managed_frame(&mut session);
    assert!(
        session
            .engine
            .viewport()
            .native_create_sagas()
            .any(|(_, saga)| {
                saga.binding() == child_binding.binding
                    && matches!(
                        saga.phase(),
                        crate::frame::NativeCreatePhase::AwaitingPrepared { .. }
                    )
            }),
        "overload cancellation must preserve the accepted native create"
    );
}

#[test]
fn overload_roster_resync_restores_authority_only_after_commit_and_real_release() {
    for still_held in [false, true] {
        let (mut session, binding, receiver) = pressed_session();
        let old_stream = session.engine.interaction().active_stream().unwrap();
        cancel_and_commit(&mut session);
        let provider = session.engine.backend_ingress_provider();
        let through = session.native.as_ref().unwrap().recorder.pointer_through();
        let version = session.version();
        assert!(matches!(
            session.engine.pointer_button_authority(),
            AnyButtonDownAuthority::Unknown(_)
        ));
        session
            .report_native_pointer_roster(NativePointerRoster::Unknown)
            .expect("unavailable facts must not consume the complete-roster recovery opportunity");
        commit_managed_frame(&mut session);
        assert!(matches!(
            session.engine.pointer_button_authority(),
            AnyButtonDownAuthority::Unknown(_)
        ));
        let roster = if still_held {
            NativePointerRoster::Exact(vec![NativePointerState::new(
                NativePointerId::new(1),
                [NativePointerButton::Primary],
                NativePointerOwner::Native(binding.window()),
            )])
        } else {
            NativePointerRoster::Exact(Vec::new())
        };
        session
            .report_native_pointer_roster(roster)
            .expect("the managed host reports its actual post-overload pointer state");
        assert!(matches!(
            session.engine.pointer_button_authority(),
            AnyButtonDownAuthority::Unknown(_)
        ));
        {
            let _uncommitted = session
                .begin_native_host_frame(|_| NativeReceiverAnswer::Unknown)
                .expect("the roster reduces into a private candidate");
        }
        assert!(matches!(
            session.engine.pointer_button_authority(),
            AnyButtonDownAuthority::Unknown(_)
        ));
        commit_managed_frame(&mut session);
        assert_eq!(
            session.engine.pointer_button_authority(),
            if still_held {
                AnyButtonDownAuthority::KnownDown
            } else {
                AnyButtonDownAuthority::KnownAllReleased
            },
            "a committed complete roster must restore exact button authority"
        );
        assert_eq!(session.engine.backend_ingress_provider(), provider);
        assert_eq!(
            session.native.as_ref().unwrap().recorder.pointer_through(),
            through
        );
        assert_eq!(session.version(), version);
        assert!(
            !session.has_active_gesture(),
            "roster facts must not revive the cancelled gesture"
        );

        if still_held {
            session
                .record_native_pointer(native_input(
                    binding,
                    1,
                    NativePointerEvent::ButtonReleased(NativePointerButton::Primary),
                    receiver.center(),
                ))
                .unwrap();
            let mut frame = session
                .begin_native_host_frame(|_| NativeReceiverAnswer::Unknown)
                .unwrap();
            frame
                .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
                .unwrap();
            frame.commit().unwrap();
            assert_eq!(
                session.engine.pointer_button_authority(),
                AnyButtonDownAuthority::KnownAllReleased
            );

            session
                .record_native_pointer(native_input(
                    binding,
                    1,
                    NativePointerEvent::ButtonPressed(NativePointerButton::Primary),
                    receiver.center(),
                ))
                .unwrap();
            let mut frame = session
                .begin_native_host_frame(with_window_hit(receiver, |query| {
                    receiver_answer(query, receiver)
                }))
                .unwrap();
            frame
                .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
                .unwrap();
            frame.commit().unwrap();
            let new_stream = session.engine.interaction().active_stream().unwrap();
            assert!(new_stream.incarnation() > old_stream.incarnation());
        }
    }
}

#[test]
fn overload_roster_resync_rejects_nonmanaged_and_pending_native_input() {
    let (mut observed, _) = native_root_session();
    let error = observed
        .report_native_pointer_roster(NativePointerRoster::Exact(Vec::new()))
        .unwrap_err();
    assert_eq!(error.native_kind(), Some(NativeHostErrorKind::Unsupported));

    let (mut session, _, _) = pressed_session();
    cancel_and_commit(&mut session);
    session
        .record_native_pointer(unknown_native_pointer_input(1, NativePointerEvent::Moved))
        .unwrap();
    let recorded = session.native_input_recorded_count();
    let error = session
        .report_native_pointer_roster(NativePointerRoster::Exact(Vec::new()))
        .unwrap_err();
    assert_eq!(
        error.native_kind(),
        Some(NativeHostErrorKind::OperationConflict)
    );
    assert_eq!(session.native_input_recorded_count(), recorded);
    assert!(matches!(
        session.engine.pointer_button_authority(),
        AnyButtonDownAuthority::Unknown(_)
    ));
}

#[test]
fn overload_roster_resync_recovers_after_later_press_and_release() {
    let (mut session, binding, receiver) = pressed_session();
    let provider = session.engine.backend_ingress_provider();
    cancel_and_commit(&mut session);
    session
        .record_native_pointer_batch([
            native_input(
                binding,
                1,
                NativePointerEvent::ButtonPressed(NativePointerButton::Primary),
                receiver.center(),
            ),
            native_input(
                binding,
                1,
                NativePointerEvent::ButtonReleased(NativePointerButton::Primary),
                receiver.center(),
            ),
        ])
        .unwrap();
    let mut frame = session
        .begin_native_host_frame(with_window_hit(receiver, |query| {
            receiver_answer(query, receiver)
        }))
        .unwrap();
    frame
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .unwrap();
    frame.commit().unwrap();
    assert!(!session.has_active_gesture());
    assert!(
        matches!(
            session.engine.pointer_button_authority(),
            AnyButtonDownAuthority::Unknown(_)
        ),
        "individual edges cannot prove the complete pointer roster"
    );
    let through = session.native.as_ref().unwrap().recorder.pointer_through();
    let version = session.version();

    session
        .report_native_pointer_roster(NativePointerRoster::Exact(Vec::new()))
        .expect("fresh complete facts may arrive only after the real release was observed");
    assert!(matches!(
        session.engine.pointer_button_authority(),
        AnyButtonDownAuthority::Unknown(_)
    ));
    {
        let _dropped = session
            .begin_native_host_frame(|_| NativeReceiverAnswer::Unknown)
            .expect("the late fresh roster reduces into a rollbackable candidate");
    }
    assert!(matches!(
        session.engine.pointer_button_authority(),
        AnyButtonDownAuthority::Unknown(_)
    ));
    commit_managed_frame(&mut session);
    assert_eq!(
        session.engine.pointer_button_authority(),
        AnyButtonDownAuthority::KnownAllReleased,
        "late complete roster must recover authority after post-cancel edges"
    );
    assert_eq!(session.engine.backend_ingress_provider(), provider);
    assert_eq!(
        session.native.as_ref().unwrap().recorder.pointer_through(),
        through
    );
    assert_eq!(session.version(), version);
}
