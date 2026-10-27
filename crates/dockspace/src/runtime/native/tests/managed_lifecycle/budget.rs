use super::*;
use crate::runtime::{NativeInputBudget, NativeMonotonicClock};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

#[test]
fn receiver_resolution_time_seals_current_record_without_starting_its_successor() {
    let (mut session, binding) = programmatic_tear_off_session();
    let receiver = presented_tab_receiver(&mut session, ITEM);
    let point = receiver.center();
    let position = PhysicalPoint::new(ROOT_ORIGIN_X + point.x(), ROOT_ORIGIN_Y + point.y())
        .expect("the source desktop point must be finite");
    let pointer = NativePointerId::new(0x701);
    let input = |event| {
        NativePointerInput::new(
            pointer,
            event,
            NativeDesktopPointerLocation::new(
                NativeDesktopPosition::Exact(position),
                NativePointerHover::Dock(binding),
                None,
            ),
            NativePointerOwner::Native(binding),
            NativePointerOwner::Native(binding),
        )
    };
    let before = session.engine.backend_ingress_committed_through();
    session
        .record_native_pointer_sequence([
            input(NativePointerEvent::ButtonPressed(
                NativePointerButton::Primary,
            )),
            input(NativePointerEvent::ButtonReleased(
                NativePointerButton::Primary,
            )),
        ])
        .expect("the complete press/release sequence must record");

    let nanos = Arc::new(AtomicU64::new(0));
    let clock_source = Arc::clone(&nanos);
    let clock = NativeMonotonicClock::new(move || {
        Duration::from_nanos(clock_source.load(Ordering::SeqCst))
    });
    let mut budget = NativeInputBudget::new(&clock);
    let mut queries = 0;
    let mut pressed = session
        .poll_native_host_frame(&mut budget, |query| {
            queries += 1;
            nanos.store(3_000_000, Ordering::SeqCst);
            receiver_answer(query, receiver)
        })
        .expect("the current record must finish after its resolver crosses the deadline")
        .expect("a completed independent press can seal");
    assert!(queries > 0, "the real receiver resolver must consume time");
    assert!(budget.is_exhausted());
    assert_eq!(budget.remaining_records(), 255);
    assert_eq!(budget.elapsed(), Duration::from_millis(3));
    assert!(pressed.native_input_pending());
    pressed
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .expect("the press prefix must settle its surface roster");
    let report = pressed
        .commit()
        .expect("the exact press prefix must commit");
    assert!(report.native_input_pending());
    assert_eq!(
        session.engine.backend_ingress_committed_through().get(),
        before.get() + 1
    );
    assert_eq!(
        session
            .native
            .as_ref()
            .unwrap()
            .recorder
            .batch_after(session.engine.backend_ingress_committed_through())
            .expect("the unstarted release must remain recorder-owned")
            .len(),
        1,
    );

    let mut next_budget = NativeInputBudget::new(&clock);
    let mut released = session
        .poll_native_host_frame(&mut next_budget, |query| receiver_answer(query, receiver))
        .expect("the exact release must continue in a new scheduling turn")
        .expect("the release fits the fresh budget");
    assert!(!released.native_input_pending());
    released
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .expect("the release must settle its surface roster");
    assert!(
        !released
            .commit()
            .expect("the release must commit")
            .native_input_pending()
    );
    assert_eq!(
        session.engine.backend_ingress_committed_through().get(),
        before.get() + 2
    );
}
