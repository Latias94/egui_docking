//! Ledger-only token models; gesture execution uses the public host regression.

use super::*;
use crate::interaction::{DragGeneration, DragSessionId, PointerContinuationToken};

fn sample(sequence: u64, pointer: u64, kind: PointerEdgeKind, millis: u64) -> PointerEdge {
    edge_for_pointer(
        sequence,
        pointer,
        kind,
        local_location(),
        Authority::Known(PointerCaptureOwner::None),
    )
    .with_metadata(PointerEventMetadata::new(
        PointerDeviceKind::Mouse,
        Duration::from_millis(millis),
    ))
}

fn journal(previous: u64, edges: Vec<PointerEdge>) -> PointerEdgeJournal {
    PointerEdgeJournal::new(
        PointerEdgeSequence::new(previous),
        edges.last().unwrap().sequence(),
        edges,
    )
    .unwrap()
}

fn token(owner: PointerStreamId) -> PointerContinuationToken {
    // This private model tests only stream admission, not whether a gesture
    // minted the token or its deadline action is eligible to execute.
    PointerContinuationToken {
        owner,
        session: crate::interaction::PointerContinuationSession::Drag(DragSessionId::new(
            WorkspaceEpoch::new(1),
            DragGeneration::new(1),
        )),
        generation: 1,
        deadline: Duration::from_millis(160),
        native_desktop: false,
    }
}

#[test]
fn continuation_requires_existing_exact_stream_and_rejects_suffix_atomically() {
    let mut ledger = PointerJournalLedger::new(domain(41));
    let provider = surface_local_provider(&mut ledger, 0);
    let lease = provider.lease();
    let initial = commit_candidate(
        &mut ledger,
        lease,
        journal(
            0,
            vec![sample(
                1,
                7,
                PointerEdgeKind::ButtonPressed(PointerButton::Primary),
                10,
            )],
        ),
    )
    .unwrap();
    let owner = initial.accepted_edges()[0].stream();
    for (pointer, claimed_owner) in [
        (8, owner),
        (
            7,
            PointerStreamId::new(lease, owner.pointer(), owner.incarnation() + 1),
        ),
    ] {
        let before = ledger.clone();
        let invalid = journal(
            1,
            vec![
                sample(2, 7, PointerEdgeKind::Moved, 30),
                sample(
                    3,
                    pointer,
                    PointerEdgeKind::Continuation(token(claimed_owner)),
                    40,
                ),
            ],
        );
        assert!(matches!(commit_candidate(&mut ledger, lease, invalid),
            Err(PointerJournalLedgerError::ContinuationStreamMismatch { sequence }) if sequence == PointerEdgeSequence::new(3)));
        assert_eq!(ledger, before);
    }
    let retry = commit_candidate(
        &mut ledger,
        lease,
        journal(
            1,
            vec![sample(
                2,
                7,
                PointerEdgeKind::Continuation(token(owner)),
                20,
            )],
        ),
    )
    .unwrap();
    assert_eq!(retry.accepted_edges()[0].stream(), owner);
    assert!(
        ledger
            .authority
            .pressed_buttons
            .contains_key(&(owner.pointer(), PointerButton::Primary))
    );
    assert_eq!(ledger.last_stream_incarnation.0, owner.incarnation());
}

#[test]
fn continuation_keeps_buttons_clock_and_stream_until_a_real_terminal_edge() {
    let mut ledger = PointerJournalLedger::new(domain(42));
    let provider = surface_local_provider(&mut ledger, 0);
    let lease = provider.lease();
    let initial = commit_candidate(
        &mut ledger,
        lease,
        journal(
            0,
            vec![sample(
                1,
                7,
                PointerEdgeKind::ButtonPressed(PointerButton::Primary),
                10,
            )],
        ),
    )
    .unwrap();
    let owner = initial.accepted_edges()[0].stream();
    for sequence in [2, 3] {
        let commit = commit_candidate(
            &mut ledger,
            lease,
            journal(
                sequence - 1,
                vec![sample(
                    sequence,
                    7,
                    PointerEdgeKind::Continuation(token(owner)),
                    10,
                )],
            ),
        )
        .unwrap();
        assert_eq!(commit.accepted_edges()[0].stream(), owner);
        assert!(
            ledger
                .authority
                .pressed_buttons
                .contains_key(&(owner.pointer(), PointerButton::Primary))
        );
    }
    let before = ledger.clone();
    assert!(matches!(
        commit_candidate(
            &mut ledger,
            lease,
            journal(
                3,
                vec![sample(4, 7, PointerEdgeKind::Continuation(token(owner)), 9),]
            )
        ),
        Err(PointerJournalLedgerError::NonMonotonicTime { .. })
    ));
    assert_eq!(ledger, before);
    commit_candidate(
        &mut ledger,
        lease,
        journal(
            3,
            vec![sample(
                4,
                7,
                PointerEdgeKind::ContactEnded(PointerButton::Primary),
                20,
            )],
        ),
    )
    .unwrap();
    let before = ledger.clone();
    assert!(matches!(
        commit_candidate(
            &mut ledger,
            lease,
            journal(
                4,
                vec![sample(
                    5,
                    7,
                    PointerEdgeKind::Continuation(token(owner)),
                    30
                ),]
            )
        ),
        Err(PointerJournalLedgerError::ContinuationStreamMismatch { .. })
    ));
    assert_eq!(ledger, before);
    assert!(ledger.active_streams.is_empty());
    assert!(ledger.authority.pressed_buttons.is_empty());
}

#[test]
fn capture_acquisition_cannot_create_or_replace_a_stream_and_retries_atomically() {
    let mut ledger = PointerJournalLedger::new(domain(43));
    let provider = surface_local_provider(&mut ledger, 0);
    let lease = provider.lease();
    let initial = commit_candidate(
        &mut ledger,
        lease,
        journal(
            0,
            vec![sample(
                1,
                7,
                PointerEdgeKind::ButtonPressed(PointerButton::Primary),
                10,
            )],
        ),
    )
    .unwrap();
    let owner = initial.accepted_edges()[0].stream();
    let before = ledger.clone();
    let invalid = journal(
        1,
        vec![
            sample(2, 7, PointerEdgeKind::Moved, 20),
            sample(3, 8, PointerEdgeKind::CaptureAcquired(token(owner)), 30),
        ],
    );
    assert!(matches!(commit_candidate(&mut ledger, lease, invalid),
        Err(PointerJournalLedgerError::ContinuationStreamMismatch { sequence })
            if sequence == PointerEdgeSequence::new(3)));
    assert_eq!(ledger, before);
    let retry = commit_candidate(
        &mut ledger,
        lease,
        journal(
            1,
            vec![sample(
                2,
                7,
                PointerEdgeKind::CaptureAcquired(token(owner)),
                20,
            )],
        ),
    )
    .unwrap();
    assert_eq!(retry.accepted_edges()[0].stream(), owner);
    assert_eq!(ledger.last_stream_incarnation.0, owner.incarnation());
    assert!(
        ledger
            .authority
            .pressed_buttons
            .contains_key(&(owner.pointer(), PointerButton::Primary))
    );
    commit_candidate(
        &mut ledger,
        lease,
        journal(
            2,
            vec![sample(
                3,
                7,
                PointerEdgeKind::ContactEnded(PointerButton::Primary),
                30,
            )],
        ),
    )
    .unwrap();
    let before = ledger.clone();
    assert!(matches!(
        commit_candidate(
            &mut ledger,
            lease,
            journal(
                3,
                vec![sample(
                    4,
                    7,
                    PointerEdgeKind::CaptureAcquired(token(owner)),
                    40
                )]
            ),
        ),
        Err(PointerJournalLedgerError::ContinuationStreamMismatch { .. })
    ));
    assert_eq!(ledger, before);
}
