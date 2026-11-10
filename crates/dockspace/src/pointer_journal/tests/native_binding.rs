use super::*;

fn sample(
    sequence: u64,
    pointer: u64,
    kind: PointerEdgeKind,
    delivery: ViewportBinding,
    capture: Authority<PointerCaptureOwner>,
) -> PointerEdge {
    PointerEdge::new_with_delivery(
        PointerEdgeSequence::new(sequence),
        PointerId::new(pointer),
        kind,
        PointerEdgeLocation::Desktop {
            route: DesktopRouteFact::unknown(
                Authority::Unknown(AuthorityUnavailableReason::NotReported),
                AuthorityUnavailableReason::NotReported,
            ),
        },
        Authority::Known(PointerEventDeliveryOwner::Native(delivery.window())),
        capture,
    )
}

#[test]
fn native_binding_retirement_uses_held_origins_and_retained_capture_without_idle_delivery() {
    let domain = domain(815);
    let first = binding(domain, 1, 1);
    let successor = binding_with_incarnation(domain, 1, 1, 2);
    let other = binding(domain, 2, 2);
    let mut ledger = PointerJournalLedger::new(domain);
    let lease = ledger
        .create_provider(
            PointerProviderScope::DesktopGlobal,
            PointerEdgeSequence::new(0),
        )
        .unwrap();
    let unknown = Authority::Unknown(AuthorityUnavailableReason::NotReported);
    let none = Authority::Known(PointerCaptureOwner::None);
    let edges = vec![
        sample(
            1,
            1,
            PointerEdgeKind::ButtonPressed(PointerButton::Primary),
            first,
            none,
        ),
        sample(2, 1, PointerEdgeKind::Moved, other, unknown),
        sample(
            3,
            2,
            PointerEdgeKind::Moved,
            other,
            Authority::Known(PointerCaptureOwner::Native(first.window())),
        ),
        sample(4, 2, PointerEdgeKind::Moved, other, unknown),
        sample(5, 3, PointerEdgeKind::Moved, first, none),
        sample(
            6,
            4,
            PointerEdgeKind::ButtonPressed(PointerButton::Primary),
            other,
            none,
        ),
        sample(
            7,
            5,
            PointerEdgeKind::Moved,
            other,
            Authority::Known(PointerCaptureOwner::Native(first.window())),
        ),
        sample(8, 5, PointerEdgeKind::CaptureChanged, other, none),
    ];
    let journal = PointerEdgeJournal::new(
        PointerEdgeSequence::new(0),
        PointerEdgeSequence::new(8),
        edges,
    )
    .unwrap();
    let prepared = ledger.prepare_candidate(lease, journal).unwrap();
    ledger.commit_prepared(prepared).unwrap();
    let first_stream = ledger.active_streams[&PointerId::new(1)].id;
    let capture_stream = ledger.active_streams[&PointerId::new(2)].id;
    let surviving = [3, 4, 5].map(|pointer| ledger.active_streams[&PointerId::new(pointer)]);
    assert_eq!(
        ledger.authority.captures[&PointerId::new(2)].observed,
        unknown
    );
    assert!(
        ledger
            .retire_native_binding_input(lease, successor.window())
            .unwrap()
            .is_empty()
    );
    assert_eq!(ledger.active_streams.len(), 5);

    assert_eq!(
        ledger
            .retire_native_binding_input(lease, first.window())
            .unwrap(),
        [first_stream, capture_stream],
    );
    assert_eq!(ledger.active_streams.len(), 3);
    for stream in surviving {
        assert_eq!(ledger.active_streams[&stream.id.pointer()], stream);
    }
    assert_eq!(ledger.button_authority(), AnyButtonDownAuthority::KnownDown);
    assert_eq!(ledger.retained_committed_through(lease).unwrap().get(), 8);
    assert!(
        ledger
            .retire_native_binding_input(lease, first.window())
            .unwrap()
            .is_empty()
    );

    let release = PointerEdgeJournal::new(
        PointerEdgeSequence::new(8),
        PointerEdgeSequence::new(9),
        vec![sample(
            9,
            4,
            PointerEdgeKind::ButtonReleased(PointerButton::Primary),
            other,
            none,
        )],
    )
    .unwrap();
    let prepared = ledger.prepare_candidate(lease, release).unwrap();
    let committed = ledger.commit_prepared(prepared).unwrap();
    assert_eq!(committed.accepted_edges()[0].stream(), surviving[1].id);
}

#[test]
fn native_binding_retirement_clears_checkpoint_dependencies_and_invalidates_old_preparation() {
    let domain = domain(816);
    let first = binding(domain, 1, 1);
    let other = binding(domain, 2, 2);
    let mut ledger = PointerJournalLedger::new(domain);
    let lease = ledger
        .create_provider(
            PointerProviderScope::DesktopGlobal,
            PointerEdgeSequence::new(0),
        )
        .unwrap();
    let checkpoint = PointerAuthorityCheckpoint::known(
        PointerEdgeSequence::new(0),
        vec![
            PointerStateObservation::new(
                PointerId::new(1),
                vec![PointerButton::Primary],
                Authority::Known(PointerCaptureOwner::Native(first.window())),
            )
            .unwrap(),
            PointerStateObservation::new(
                PointerId::new(2),
                vec![PointerButton::Secondary],
                Authority::Known(PointerCaptureOwner::Native(other.window())),
            )
            .unwrap(),
        ],
    )
    .unwrap();
    let journal = PointerEdgeJournal::new(
        PointerEdgeSequence::new(0),
        PointerEdgeSequence::new(0),
        Vec::new(),
    )
    .unwrap()
    .with_authority_checkpoint(checkpoint)
    .unwrap();
    let prepared = ledger.prepare_candidate(lease, journal).unwrap();
    ledger.commit_prepared(prepared).unwrap();
    let old_checkpoint = ledger.last_checkpoint.clone().unwrap();
    let pending = ledger
        .prepare_candidate(
            lease,
            PointerEdgeJournal::new(
                PointerEdgeSequence::new(0),
                PointerEdgeSequence::new(1),
                vec![sample(
                    1,
                    1,
                    PointerEdgeKind::Moved,
                    first,
                    Authority::Known(PointerCaptureOwner::Native(first.window())),
                )],
            )
            .unwrap(),
        )
        .unwrap();

    assert!(
        ledger
            .retire_native_binding_input(lease, first.window())
            .unwrap()
            .is_empty()
    );
    assert!(!ledger.authority.captures.contains_key(&PointerId::new(1)));
    assert_eq!(ledger.authority.captures.len(), 1);
    assert_eq!(ledger.authority.pressed_buttons.len(), 1);
    assert_eq!(ledger.button_authority(), AnyButtonDownAuthority::KnownDown);
    assert!(matches!(
        ledger.commit_prepared(pending),
        Err(PointerJournalLedgerError::PreparedJournalStale { .. })
    ));
    let obsolete = PointerEdgeJournal::new(
        PointerEdgeSequence::new(0),
        PointerEdgeSequence::new(0),
        Vec::new(),
    )
    .unwrap()
    .with_authority_checkpoint(old_checkpoint)
    .unwrap();
    let obsolete = ledger.prepare_candidate(lease, obsolete).unwrap();
    assert!(matches!(
        ledger.commit_prepared(obsolete),
        Err(PointerJournalLedgerError::AuthorityCheckpointAfterPointerEdges { .. })
    ));
    assert!(
        ledger
            .retire_native_binding_input(lease, other.window())
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        ledger.button_authority(),
        AnyButtonDownAuthority::KnownAllReleased
    );
}

#[test]
fn checkpoint_capture_does_not_invent_a_held_button_origin_after_capture_moves() {
    let domain = domain(817);
    let first = binding(domain, 1, 1);
    let other = binding(domain, 2, 2);
    let mut ledger = PointerJournalLedger::new(domain);
    let lease = ledger
        .create_provider(
            PointerProviderScope::DesktopGlobal,
            PointerEdgeSequence::new(0),
        )
        .unwrap();
    let checkpoint = PointerAuthorityCheckpoint::known(
        PointerEdgeSequence::new(0),
        vec![
            PointerStateObservation::new(
                PointerId::new(1),
                vec![PointerButton::Primary],
                Authority::Known(PointerCaptureOwner::Native(first.window())),
            )
            .unwrap(),
        ],
    )
    .unwrap();
    let journal = PointerEdgeJournal::new(
        PointerEdgeSequence::new(0),
        PointerEdgeSequence::new(1),
        vec![sample(
            1,
            1,
            PointerEdgeKind::CaptureChanged,
            other,
            Authority::Known(PointerCaptureOwner::Native(other.window())),
        )],
    )
    .unwrap()
    .with_authority_checkpoint(checkpoint)
    .unwrap();
    let prepared = ledger.prepare_candidate(lease, journal).unwrap();
    let committed = ledger.commit_prepared(prepared).unwrap();
    let stream = committed.accepted_edges()[0].stream();
    assert!(
        ledger
            .retire_native_binding_input(lease, first.window())
            .unwrap()
            .is_empty()
    );
    assert_eq!(ledger.active_streams[&PointerId::new(1)].id, stream);
    assert_eq!(ledger.button_authority(), AnyButtonDownAuthority::KnownDown);
    assert_eq!(
        ledger
            .retire_native_binding_input(lease, other.window())
            .unwrap(),
        [stream]
    );
    assert_eq!(
        ledger.button_authority(),
        AnyButtonDownAuthority::KnownAllReleased
    );
}
