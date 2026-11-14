use super::*;
use crate::input::ingress::BackendIngressPayload;
use std::cell::Cell;
use std::num::NonZeroUsize;
use std::panic::{AssertUnwindSafe, catch_unwind};

const WORK_BYTES: usize = 1024 * 1024;

#[test]
fn bounded_pointer_transfer_stops_at_the_first_unadmittable_item_without_mutation() {
    for atomic_commit in [true, false] {
        let (mut session, _) = native_root_session_with_profile(NativeHostProfile::ManagedDesktop);
        session
            .record_native_pointer(unknown_native_pointer_input(10, NativePointerEvent::Moved))
            .unwrap();
        session
            .native
            .as_mut()
            .unwrap()
            .recorder
            .configure_native_transport_capacity(NonZeroUsize::new(2).unwrap(), WORK_BYTES)
            .unwrap();
        let before = format!("{session:?}");
        let owned_bytes = session.native_input_owned_capacity_bytes().unwrap();
        let recorder = &session.native.as_ref().unwrap().recorder;
        let backing = (
            recorder.retained_records().as_ptr(),
            recorder.record_buffer_capacity_bytes(),
        );
        let reads = Cell::new(0);
        let inputs = std::iter::from_fn(|| {
            let read = reads.get() + 1;
            reads.set(read);
            assert!(
                read <= 2,
                "capacity rejection must not read the caller's tail"
            );
            Some(unknown_native_pointer_input(11, NativePointerEvent::Moved))
        });

        let error = if atomic_commit {
            session.record_native_pointer_batch(inputs)
        } else {
            session.record_native_pointer_sequence(inputs)
        }
        .expect_err("the second item exceeds the one remaining record slot");

        assert_eq!(
            error.native_kind(),
            Some(NativeHostErrorKind::InputCapacity)
        );
        assert_eq!(reads.get(), 2);
        assert_eq!(format!("{session:?}"), before);
        assert_eq!(
            session.native_input_owned_capacity_bytes().unwrap(),
            owned_bytes
        );
        let recorder = &session.native.as_ref().unwrap().recorder;
        assert_eq!(
            (
                recorder.retained_records().as_ptr(),
                recorder.record_buffer_capacity_bytes(),
            ),
            backing
        );
        session
            .record_native_pointer(unknown_native_pointer_input(11, NativePointerEvent::Moved))
            .expect("rejection leaves the remaining slot available");
    }
}

#[test]
fn bounded_pointer_transfer_ignores_huge_size_hints_and_uses_the_captured_watermark() {
    struct HugeHint {
        remaining: u8,
    }

    impl Iterator for HugeHint {
        type Item = NativePointerInput;

        fn next(&mut self) -> Option<Self::Item> {
            self.remaining = self.remaining.checked_sub(1)?;
            Some(unknown_native_pointer_input(11, NativePointerEvent::Moved))
        }

        fn size_hint(&self) -> (usize, Option<usize>) {
            (usize::MAX, Some(usize::MAX))
        }
    }

    for atomic_commit in [true, false] {
        let (mut session, _) = native_root_session_with_profile(NativeHostProfile::ManagedDesktop);
        session
            .record_native_pointer(unknown_native_pointer_input(10, NativePointerEvent::Moved))
            .unwrap();
        session
            .native
            .as_mut()
            .unwrap()
            .recorder
            .configure_native_transport_capacity(NonZeroUsize::new(3).unwrap(), WORK_BYTES)
            .unwrap();
        let recorder = &session.native.as_ref().unwrap().recorder;
        let pointer_before = recorder.pointer_through();
        let recorded_before = recorder.recorded_through();
        let backing = recorder.record_buffer_capacity_bytes();
        let inputs = HugeHint { remaining: 2 };

        if atomic_commit {
            session.record_native_pointer_batch(inputs)
        } else {
            session.record_native_pointer_sequence(inputs)
        }
        .expect("two real items fit regardless of their caller-controlled size hint");

        let recorder = &session.native.as_ref().unwrap().recorder;
        assert_eq!(recorder.record_buffer_capacity_bytes(), backing);
        assert_eq!(recorder.retained_records().len(), 3);
        assert_eq!(recorder.recorded_through().get(), recorded_before.get() + 2);
        assert_eq!(recorder.pointer_through().get(), pointer_before.get() + 2);
        for (index, record) in recorder.retained_records()[1..].iter().enumerate() {
            let BackendIngressPayload::PointerSegment(journal) = record.payload() else {
                panic!("the transferred record must retain its prepared pointer journal");
            };
            let [edge] = journal.edges() else {
                panic!("each transferred journal must own exactly one pointer edge");
            };
            let previous = pointer_before.get() + index as u64;
            assert_eq!(journal.previous().get(), previous);
            assert_eq!(edge.sequence().get(), previous + 1);
            assert_eq!(journal.through(), edge.sequence());
            assert_eq!(record.is_commit_boundary(), !atomic_commit || index == 1);
        }
    }
}

#[test]
fn caller_iteration_panics_leave_committed_payloads_and_append_identity_untouched() {
    #[derive(Clone, Copy)]
    enum PanicAt {
        IntoIter,
        Next,
        Drop,
    }

    struct CallerInputs(PanicAt);

    struct CallerIterator {
        panic_at: PanicAt,
        yielded: bool,
    }

    impl IntoIterator for CallerInputs {
        type Item = NativePointerInput;
        type IntoIter = CallerIterator;

        fn into_iter(self) -> Self::IntoIter {
            assert!(
                !matches!(self.0, PanicAt::IntoIter),
                "the caller panics while constructing its iterator"
            );
            CallerIterator {
                panic_at: self.0,
                yielded: false,
            }
        }
    }

    impl Iterator for CallerIterator {
        type Item = NativePointerInput;

        fn next(&mut self) -> Option<Self::Item> {
            if !self.yielded {
                self.yielded = true;
                return Some(unknown_native_pointer_input(11, NativePointerEvent::Moved));
            }
            assert!(
                !matches!(self.panic_at, PanicAt::Next),
                "the caller panics after producing a valid prefix"
            );
            None
        }
    }

    impl Drop for CallerIterator {
        fn drop(&mut self) {
            assert!(
                !matches!(self.panic_at, PanicAt::Drop),
                "the caller panics while dropping its exhausted iterator"
            );
        }
    }

    for atomic_commit in [true, false] {
        for panic_at in [PanicAt::IntoIter, PanicAt::Next, PanicAt::Drop] {
            let (mut session, _) =
                native_root_session_with_profile(NativeHostProfile::ManagedDesktop);
            session
                .native
                .as_mut()
                .unwrap()
                .recorder
                .configure_native_transport_capacity(NonZeroUsize::new(4).unwrap(), WORK_BYTES)
                .unwrap();
            let recorder = &session.native.as_ref().unwrap().recorder;
            assert!(!recorder.retained_records().is_empty());
            assert_eq!(
                recorder.recorded_through(),
                session.engine.backend_ingress_committed_through()
            );
            let backing = (
                recorder.retained_records().as_ptr(),
                recorder.record_buffer_capacity_bytes(),
            );
            let owned_bytes = session.native_input_owned_capacity_bytes().unwrap();
            // Recorder Debug includes its non-rewinding append identity.
            let before = format!("{session:?}");

            let interrupted = catch_unwind(AssertUnwindSafe(|| {
                let inputs = CallerInputs(panic_at);
                if atomic_commit {
                    session.record_native_pointer_batch(inputs)
                } else {
                    session.record_native_pointer_sequence(inputs)
                }
            }));

            assert!(interrupted.is_err());
            assert_eq!(format!("{session:?}"), before);
            assert_eq!(
                session.native_input_owned_capacity_bytes().unwrap(),
                owned_bytes
            );
            let recorder = &session.native.as_ref().unwrap().recorder;
            assert_eq!(
                (
                    recorder.retained_records().as_ptr(),
                    recorder.record_buffer_capacity_bytes(),
                ),
                backing
            );
        }
    }
}

#[test]
fn prepared_pointer_peak_cannot_borrow_reclaim_credit_but_final_append_can() {
    for atomic_commit in [true, false] {
        let (mut session, _) = native_root_session_with_profile(NativeHostProfile::ManagedDesktop);
        for _ in 0..2 {
            session
                .record_native_pointer(unknown_native_pointer_input(11, NativePointerEvent::Moved))
                .unwrap();
        }
        commit_managed_frame(&mut session);
        let slots = NonZeroUsize::new(2).unwrap();
        let native = session.native.as_mut().unwrap();
        assert!(native.pending_replay.is_none());
        assert_eq!(native.recorder.retained_records().len(), slots.get());
        native
            .recorder
            .configure_native_transport_capacity(slots, WORK_BYTES)
            .unwrap();
        let final_required = native.recorder.native_transport_reserved_bytes().unwrap();
        native
            .recorder
            .configure_native_transport_capacity(slots, final_required)
            .unwrap();
        let recorded_before = native.recorder.recorded_through();
        let pointer_before = native.recorder.pointer_through();
        let backing = (
            native.recorder.retained_records().as_ptr(),
            native.recorder.record_buffer_capacity_bytes(),
        );
        assert_eq!(
            session.engine.backend_ingress_committed_through(),
            recorded_before
        );
        let owned_bytes = session.native_input_owned_capacity_bytes().unwrap();
        let before = format!("{session:?}");
        let inputs = || {
            [
                unknown_native_pointer_input(11, NativePointerEvent::Moved),
                unknown_native_pointer_input(11, NativePointerEvent::Moved),
            ]
        };

        let error = if atomic_commit {
            session.record_native_pointer_batch(inputs())
        } else {
            session.record_native_pointer_sequence(inputs())
        }
        .expect_err("prepared journals must coexist with the unreclaimed committed payloads");

        assert_eq!(
            error.native_kind(),
            Some(NativeHostErrorKind::InputCapacity)
        );
        assert_eq!(format!("{session:?}"), before);
        assert_eq!(
            session.native_input_owned_capacity_bytes().unwrap(),
            owned_bytes
        );
        session
            .native
            .as_mut()
            .unwrap()
            .recorder
            .configure_native_transport_capacity(slots, WORK_BYTES)
            .unwrap();
        if atomic_commit {
            session.record_native_pointer_batch(inputs())
        } else {
            session.record_native_pointer_sequence(inputs())
        }
        .expect("sufficient staging room permits safe committed-prefix reclamation");

        let recorder = &session.native.as_ref().unwrap().recorder;
        assert_eq!(recorder.retained_records().len(), slots.get());
        assert_eq!(
            (
                recorder.retained_records().as_ptr(),
                recorder.record_buffer_capacity_bytes(),
            ),
            backing
        );
        assert_eq!(
            recorder.native_transport_reserved_bytes(),
            Some(final_required)
        );
        assert_eq!(recorder.recorded_through().get(), recorded_before.get() + 2);
        assert_eq!(recorder.pointer_through().get(), pointer_before.get() + 2);
        assert_eq!(
            session.engine.backend_ingress_committed_through(),
            recorded_before
        );
        for (index, record) in recorder.retained_records().iter().enumerate() {
            assert!(record.ordinal() > recorded_before);
            let BackendIngressPayload::PointerSegment(journal) = record.payload() else {
                panic!("the replacement prefix must contain only transferred pointer journals");
            };
            let [edge] = journal.edges() else {
                panic!("each replacement record owns exactly one pointer edge");
            };
            assert_eq!(
                journal.previous().get(),
                pointer_before.get() + index as u64
            );
            assert_eq!(
                edge.sequence().get(),
                pointer_before.get() + index as u64 + 1
            );
            assert_eq!(journal.through(), edge.sequence());
            assert_eq!(record.is_commit_boundary(), !atomic_commit || index == 1);
        }
    }
}
