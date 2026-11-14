use super::*;

use std::time::Duration;

use crate::input::ingress::BackendIngressPayload;
use crate::runtime::{PointerDeviceKind, PointerEventMetadata};

#[test]
fn native_event_metadata_survives_recorder_handoff_drop_and_exact_retry() {
    let (mut session, _) = native_root_session_with_profile(NativeHostProfile::ManagedDesktop);
    commit_managed_frame(&mut session);
    let metadata = PointerEventMetadata::new(PointerDeviceKind::Touch, Duration::from_millis(40));
    session
        .record_native_pointer(
            unknown_native_pointer_input(7, NativePointerEvent::Moved).with_metadata(metadata),
        )
        .unwrap();
    let committed = session.engine.backend_ingress_committed_through();
    let source_sequence = session.committed_source_sequence;
    let batch = session
        .native
        .as_ref()
        .unwrap()
        .recorder
        .batch_after(committed)
        .unwrap();
    let [record] = batch.records() else {
        panic!("the sample is one immutable record")
    };
    let BackendIngressPayload::PointerSegment(journal) = record.payload() else {
        panic!("the sample retains its pointer journal")
    };
    assert_eq!(journal.edges()[0].metadata(), metadata);
    let version = session.version();
    drop(
        session
            .begin_native_host_frame(|_| panic!("unknown position cannot query a receiver"))
            .unwrap(),
    );
    assert_eq!(session.version(), version);
    assert_eq!(
        session.engine.backend_ingress_committed_through(),
        committed
    );
    assert_eq!(session.committed_source_sequence, source_sequence);
    commit_managed_frame(&mut session);
    assert_ne!(
        session.engine.backend_ingress_committed_through(),
        committed
    );
    session
        .record_native_pointer(
            unknown_native_pointer_input(7, NativePointerEvent::StreamEnded)
                .with_metadata(metadata),
        )
        .unwrap();
    commit_managed_frame(&mut session);
    session
        .record_native_pointer(
            unknown_native_pointer_input(7, NativePointerEvent::Moved).with_metadata(
                PointerEventMetadata::new(PointerDeviceKind::Pen, Duration::from_millis(41)),
            ),
        )
        .unwrap();
    commit_managed_frame(&mut session);
}
