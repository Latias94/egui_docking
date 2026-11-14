//! Device and clock facts share the existing journal publication boundary.

use super::*;

fn sample(
    sequence: u64,
    pointer: u64,
    kind: PointerEdgeKind,
    device: PointerDeviceKind,
    millis: u64,
) -> PointerEdge {
    edge_for_pointer(
        sequence,
        pointer,
        kind,
        local_location(),
        Authority::Known(PointerCaptureOwner::None),
    )
    .with_metadata(PointerEventMetadata::new(
        device,
        Duration::from_millis(millis),
    ))
}

fn batch(previous: u64, edges: Vec<PointerEdge>) -> PointerEdgeJournal {
    let through = edges.last().map_or(previous, |edge| edge.sequence().get());
    PointerEdgeJournal::new(
        PointerEdgeSequence::new(previous),
        PointerEdgeSequence::new(through),
        edges,
    )
    .unwrap()
}

#[test]
fn metadata_time_is_provider_ordered_and_bad_suffix_preserves_every_ledger_frontier() {
    let mut ledger = PointerJournalLedger::new(domain(1));
    let provider = surface_local_provider(&mut ledger, 0);
    let lease = provider.lease();
    commit_candidate(
        &mut ledger,
        lease,
        batch(
            0,
            vec![sample(
                1,
                1,
                PointerEdgeKind::Moved,
                PointerDeviceKind::Mouse,
                10,
            )],
        ),
    )
    .unwrap();
    let before = ledger.clone();
    let invalid = batch(
        1,
        vec![
            sample(2, 1, PointerEdgeKind::Moved, PointerDeviceKind::Mouse, 30),
            sample(3, 2, PointerEdgeKind::Moved, PointerDeviceKind::Touch, 20),
        ],
    );
    assert!(
        matches!(commit_candidate(&mut ledger, lease, invalid), Err(PointerJournalLedgerError::NonMonotonicTime { sequence, .. }) if sequence == PointerEdgeSequence::new(3))
    );
    assert_eq!(ledger, before);
    let valid = batch(
        1,
        vec![
            sample(2, 1, PointerEdgeKind::Moved, PointerDeviceKind::Mouse, 20),
            sample(3, 2, PointerEdgeKind::Moved, PointerDeviceKind::Touch, 20),
        ],
    );
    let commit = commit_candidate(&mut ledger, lease, valid).unwrap();
    assert_eq!(
        commit.journal().edges()[1].metadata().device(),
        PointerDeviceKind::Touch
    );
    assert_eq!(commit.accepted_edges()[1].stream().incarnation(), 2);
}

#[test]
fn metadata_unknown_does_not_reset_clock_or_reclassify_a_live_stream() {
    let mut ledger = PointerJournalLedger::new(domain(2));
    let provider = surface_local_provider(&mut ledger, 0);
    let lease = provider.lease();
    let initial = commit_candidate(
        &mut ledger,
        lease,
        batch(
            0,
            vec![sample(
                1,
                7,
                PointerEdgeKind::Moved,
                PointerDeviceKind::Touch,
                50,
            )],
        ),
    )
    .unwrap();
    let original_stream = initial.accepted_edges()[0].stream();
    let unknown = edge_for_pointer(
        2,
        7,
        PointerEdgeKind::Moved,
        local_location(),
        Authority::Known(PointerCaptureOwner::None),
    );
    let committed = commit_candidate(&mut ledger, lease, batch(1, vec![unknown])).unwrap();
    assert_eq!(
        committed.journal().edges()[0].metadata(),
        PointerEventMetadata::unknown()
    );
    assert_eq!(committed.accepted_edges()[0].stream(), original_stream);
    let before = ledger.clone();
    assert!(matches!(
        commit_candidate(
            &mut ledger,
            lease,
            batch(
                2,
                vec![sample(
                    3,
                    8,
                    PointerEdgeKind::Moved,
                    PointerDeviceKind::Pen,
                    49
                )]
            )
        ),
        Err(PointerJournalLedgerError::NonMonotonicTime { .. })
    ));
    assert_eq!(ledger, before);
    assert!(matches!(
        commit_candidate(
            &mut ledger,
            lease,
            batch(
                2,
                vec![sample(
                    3,
                    7,
                    PointerEdgeKind::Moved,
                    PointerDeviceKind::Mouse,
                    50
                )]
            )
        ),
        Err(PointerJournalLedgerError::DeviceKindChanged { .. })
    ));
    assert_eq!(ledger, before);
    let ended = commit_candidate(
        &mut ledger,
        lease,
        batch(
            2,
            vec![sample(
                3,
                7,
                PointerEdgeKind::StreamEnded,
                PointerDeviceKind::Touch,
                50,
            )],
        ),
    )
    .unwrap();
    assert_eq!(ended.accepted_edges()[0].stream(), original_stream);
    let successor = commit_candidate(
        &mut ledger,
        lease,
        batch(
            3,
            vec![sample(
                4,
                7,
                PointerEdgeKind::Moved,
                PointerDeviceKind::Mouse,
                51,
            )],
        ),
    )
    .unwrap();
    assert_ne!(successor.accepted_edges()[0].stream(), original_stream);
}

#[test]
fn metadata_overload_keeps_provider_clock_and_only_new_provider_can_reset_it() {
    let mut ledger = PointerJournalLedger::new(domain(3));
    let provider = surface_local_provider(&mut ledger, 0);
    let lease = provider.lease();
    commit_candidate(
        &mut ledger,
        lease,
        batch(
            0,
            vec![sample(
                1,
                1,
                PointerEdgeKind::Moved,
                PointerDeviceKind::Mouse,
                100,
            )],
        ),
    )
    .unwrap();
    ledger.cancel_live_streams_for_reset(lease).unwrap();
    let before = ledger.clone();
    assert!(matches!(
        commit_candidate(
            &mut ledger,
            lease,
            batch(
                1,
                vec![sample(
                    2,
                    1,
                    PointerEdgeKind::Moved,
                    PointerDeviceKind::Touch,
                    99
                )]
            )
        ),
        Err(PointerJournalLedgerError::NonMonotonicTime { .. })
    ));
    assert_eq!(ledger, before);
    commit_candidate(
        &mut ledger,
        lease,
        batch(
            1,
            vec![sample(
                2,
                1,
                PointerEdgeKind::Moved,
                PointerDeviceKind::Touch,
                100,
            )],
        ),
    )
    .unwrap();
    ledger.retire_provider(lease).unwrap();
    let successor = surface_local_provider(&mut ledger, 0);
    assert_ne!(successor.lease(), lease);
    commit_candidate(
        &mut ledger,
        successor.lease(),
        batch(
            0,
            vec![sample(
                1,
                1,
                PointerEdgeKind::Moved,
                PointerDeviceKind::Pen,
                0,
            )],
        ),
    )
    .unwrap();
}

#[test]
fn metadata_device_observation_without_time_does_not_invent_a_timestamp() {
    let mut ledger = PointerJournalLedger::new(domain(4));
    let provider = surface_local_provider(&mut ledger, 0);
    let lease = provider.lease();
    let metadata = PointerEventMetadata::untimed(PointerDeviceKind::Touch);
    let input = edge_for_pointer(
        1,
        1,
        PointerEdgeKind::Moved,
        local_location(),
        Authority::Known(PointerCaptureOwner::None),
    )
    .with_metadata(metadata);
    let accepted = commit_candidate(&mut ledger, lease, batch(0, vec![input])).unwrap();
    assert_eq!(accepted.journal().edges()[0].metadata(), metadata);
    assert_eq!(metadata.observed_at(), None);
    let later = sample(2, 1, PointerEdgeKind::Moved, PointerDeviceKind::Touch, 0);
    commit_candidate(&mut ledger, lease, batch(1, vec![later])).unwrap();
}
