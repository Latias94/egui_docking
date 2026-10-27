use super::*;

#[test]
fn pointer_batch_and_sequence_have_distinct_commit_boundaries() {
    for atomic_commit in [true, false] {
        let (mut session, _) = native_root_session_with_profile(NativeHostProfile::ManagedDesktop);
        let recorded_before = session
            .native
            .as_ref()
            .expect("the managed host exists")
            .recorder
            .recorded_through();
        let inputs = [
            unknown_native_pointer_input(11, NativePointerEvent::Moved),
            unknown_native_pointer_input(12, NativePointerEvent::Moved),
        ];

        if atomic_commit {
            session.record_native_pointer_batch(inputs)
        } else {
            session.record_native_pointer_sequence(inputs)
        }
        .expect("both admission modes accept valid pointer edges");

        let batch = session
            .native
            .as_ref()
            .expect("the managed host exists")
            .recorder
            .batch_after(recorded_before)
            .expect("the admitted records freeze");
        let boundaries = batch
            .records()
            .iter()
            .map(crate::backend_ingress::BackendIngressRecord::is_commit_boundary)
            .collect::<Vec<_>>();
        assert_eq!(boundaries, vec![!atomic_commit, true]);
    }
}

#[test]
fn pointer_batch_and_sequence_reject_a_suffix_without_changing_recorded_input() {
    for atomic_commit in [true, false] {
        let (mut session, _) = native_root_session_with_profile(NativeHostProfile::ManagedDesktop);
        session
            .record_native_pointer(unknown_native_pointer_input(10, NativePointerEvent::Moved))
            .expect("a preceding record establishes an independent commit boundary");
        let native = session.native.as_ref().expect("the managed host exists");
        let recorded_before = native.recorder.recorded_through();
        let pointer_before = native.recorder.pointer_through();
        let batch_before = native
            .recorder
            .pending_batch()
            .expect("the existing input freezes before admission");
        let inputs = [
            unknown_native_pointer_input(11, NativePointerEvent::Moved),
            unknown_native_pointer_input(
                12,
                NativePointerEvent::Scrolled(NativeScrollEvent::new(
                    NativeScrollDeviceId::new(5),
                    NativeScrollPhase::Discrete {
                        delta: NativeScrollDelta::Lines {
                            x: f64::NAN,
                            y: 1.0,
                        },
                    },
                    NativeScrollMomentum::Direct,
                    NativeScrollModifiers::Unknown,
                )),
            ),
        ];

        let error = if atomic_commit {
            session.record_native_pointer_batch(inputs)
        } else {
            session.record_native_pointer_sequence(inputs)
        }
        .expect_err("an invalid suffix rejects the entire admission");

        assert_eq!(error.native_kind(), Some(NativeHostErrorKind::InvalidFacts));
        let native = session.native.as_ref().expect("the managed host exists");
        assert_eq!(native.recorder.recorded_through(), recorded_before);
        assert_eq!(native.recorder.pointer_through(), pointer_before);
        assert_eq!(
            native
                .recorder
                .pending_batch()
                .expect("the unchanged input freezes after rejection"),
            batch_before
        );
    }
}

#[test]
fn pointer_batch_and_sequence_do_not_append_when_the_iterator_panics() {
    use std::panic::{AssertUnwindSafe, catch_unwind};

    for atomic_commit in [true, false] {
        let (mut session, _) = native_root_session_with_profile(NativeHostProfile::ManagedDesktop);
        let native = session.native.as_ref().expect("the managed host exists");
        let recorded_before = native.recorder.recorded_through();
        let pointer_before = native.recorder.pointer_through();
        let batch_before = native
            .recorder
            .pending_batch()
            .expect("the existing input freezes before iteration");
        let first = unknown_native_pointer_input(11, NativePointerEvent::Moved);
        let inputs = std::iter::once(first).chain(std::iter::from_fn(|| {
            panic!("the caller iterator panics before producing its suffix");
        }));

        let interrupted = catch_unwind(AssertUnwindSafe(|| {
            if atomic_commit {
                session.record_native_pointer_batch(inputs)
            } else {
                session.record_native_pointer_sequence(inputs)
            }
        }));

        assert!(interrupted.is_err());
        let native = session.native.as_ref().expect("the managed host exists");
        assert_eq!(native.recorder.recorded_through(), recorded_before);
        assert_eq!(native.recorder.pointer_through(), pointer_before);
        assert_eq!(
            native
                .recorder
                .pending_batch()
                .expect("the unchanged input freezes after the iterator panic"),
            batch_before
        );
    }
}
