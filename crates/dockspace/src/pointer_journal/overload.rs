use super::*;

fn journal(previous: u64, pointer: u64, kind: PointerEdgeKind) -> PointerEdgeJournal {
    let sequence = PointerEdgeSequence::new(previous + 1);
    PointerEdgeJournal::new(
        PointerEdgeSequence::new(previous),
        sequence,
        vec![PointerEdge::new(
            sequence,
            PointerId::new(pointer),
            kind,
            PointerEdgeLocation::Desktop {
                route: DesktopRouteFact::unknown(
                    Authority::Unknown(AuthorityUnavailableReason::NotReported),
                    AuthorityUnavailableReason::NotReported,
                ),
            },
            Authority::Known(PointerCaptureOwner::Foreign),
        )],
    )
    .unwrap()
}

#[test]
fn overload_revokes_capture_and_old_candidates_without_resetting_identity_or_watermark() {
    let mut ledger = PointerJournalLedger::new(EngineAuthorityDomainId::new_for_test(701));
    let lease = ledger
        .create_provider(
            PointerProviderScope::DesktopGlobal,
            PointerEdgeSequence::new(0),
        )
        .unwrap();
    let mut old_streams = Vec::new();
    for (previous, pointer) in [(0, 1), (1, 2)] {
        let prepared = ledger
            .prepare_candidate(
                lease,
                journal(
                    previous,
                    pointer,
                    PointerEdgeKind::ButtonPressed(PointerButton::Primary),
                ),
            )
            .unwrap();
        old_streams.push(ledger.commit_prepared(prepared).unwrap().accepted_edges()[0].stream());
    }
    assert_eq!(ledger.button_authority(), AnyButtonDownAuthority::KnownDown);
    let prepared_before_cancel = ledger
        .prepare_candidate(lease, journal(2, 1, PointerEdgeKind::Moved))
        .unwrap();
    let old_counter = ledger.last_stream_incarnation;

    ledger.cancel_live_streams_for_reset(lease).unwrap();
    assert_eq!(ledger.active_lease(), Some(lease));
    assert_eq!(
        ledger.retained_committed_through(lease).unwrap(),
        PointerEdgeSequence::new(2)
    );
    assert!(ledger.active_streams.is_empty());
    assert!(ledger.authority.pressed_buttons.is_empty());
    assert!(ledger.authority.captures.is_empty());
    assert!(matches!(
        ledger.button_authority(),
        AnyButtonDownAuthority::Unknown(_)
    ));
    assert_eq!(ledger.last_stream_incarnation, old_counter);
    assert!(ledger.retired.is_empty());
    assert!(matches!(
        ledger.commit_prepared(prepared_before_cancel),
        Err(PointerJournalLedgerError::PreparedJournalStale { .. })
    ));

    ledger.cancel_live_streams_for_reset(lease).unwrap();
    let next = ledger
        .prepare_candidate(
            lease,
            journal(2, 1, PointerEdgeKind::ButtonPressed(PointerButton::Primary)),
        )
        .unwrap();
    let committed = ledger.commit_prepared(next).unwrap();
    let stream = committed.accepted_edges()[0].stream();
    assert!(
        old_streams
            .iter()
            .all(|old| stream.incarnation() > old.incarnation())
    );
    assert_eq!(stream.lease(), lease);
    assert_eq!(stream.pointer(), PointerId::new(1));
    assert_eq!(
        ledger.retained_committed_through(lease).unwrap(),
        PointerEdgeSequence::new(3)
    );
}

#[test]
fn overload_rejects_a_foreign_or_retired_lease_without_clearing_live_state() {
    let domain = EngineAuthorityDomainId::new_for_test(702);
    let mut ledger = PointerJournalLedger::new(domain);
    let lease = ledger
        .create_provider(
            PointerProviderScope::DesktopGlobal,
            PointerEdgeSequence::new(0),
        )
        .unwrap();
    let prepared = ledger
        .prepare_candidate(
            lease,
            journal(0, 1, PointerEdgeKind::ButtonPressed(PointerButton::Primary)),
        )
        .unwrap();
    ledger.commit_prepared(prepared).unwrap();
    let before = ledger.clone();
    let foreign = PointerInputLease::new(
        EngineAuthorityDomainId::new_for_test(703),
        lease.incarnation(),
        PointerProviderScope::DesktopGlobal,
    );
    assert!(ledger.cancel_live_streams_for_reset(foreign).is_err());
    assert_eq!(ledger, before);

    ledger.retire_provider(lease).unwrap();
    let successor = ledger
        .create_provider(
            PointerProviderScope::DesktopGlobal,
            PointerEdgeSequence::new(1),
        )
        .unwrap();
    let prepared = ledger
        .prepare_candidate(
            successor,
            journal(1, 1, PointerEdgeKind::ButtonPressed(PointerButton::Primary)),
        )
        .unwrap();
    ledger.commit_prepared(prepared).unwrap();
    let before = ledger.clone();
    assert!(ledger.cancel_live_streams_for_reset(lease).is_err());
    assert_eq!(ledger, before);
}

#[test]
fn overload_roster_resync_recovers_complete_authority_without_fabricating_release() {
    for still_held in [false, true] {
        let mut ledger = PointerJournalLedger::new(EngineAuthorityDomainId::new_for_test(704));
        let lease = ledger
            .create_provider(
                PointerProviderScope::DesktopGlobal,
                PointerEdgeSequence::new(0),
            )
            .unwrap();
        let prepared = ledger
            .prepare_candidate(
                lease,
                journal(0, 1, PointerEdgeKind::ButtonPressed(PointerButton::Primary)),
            )
            .unwrap();
        let old = ledger.commit_prepared(prepared).unwrap().accepted_edges()[0].stream();
        ledger.cancel_live_streams_for_reset(lease).unwrap();
        assert!(matches!(
            ledger.button_authority(),
            AnyButtonDownAuthority::Unknown(_)
        ));

        let through = PointerEdgeSequence::new(1);
        let pointers = if still_held {
            vec![
                PointerStateObservation::new(
                    PointerId::new(1),
                    vec![PointerButton::Primary],
                    Authority::Known(PointerCaptureOwner::Foreign),
                )
                .unwrap(),
            ]
        } else {
            Vec::new()
        };
        let checkpoint = PointerAuthorityCheckpoint::known(through, pointers).unwrap();
        let resync = PointerEdgeJournal::new(through, through, Vec::new())
            .unwrap()
            .with_authority_checkpoint(checkpoint)
            .unwrap();
        let prepared = ledger
            .prepare_candidate(lease, resync)
            .expect("the exact post-overload boundary must accept a complete observed roster");
        ledger.commit_prepared(prepared).unwrap();
        assert_eq!(ledger.active_lease(), Some(lease));
        assert_eq!(ledger.retained_committed_through(lease).unwrap(), through);
        assert_eq!(ledger.retention_manifest().active_stream_count(), 0);
        assert_eq!(
            ledger.button_authority(),
            if still_held {
                AnyButtonDownAuthority::KnownDown
            } else {
                AnyButtonDownAuthority::KnownAllReleased
            },
            "only the actual complete roster may prove every button released"
        );

        if still_held {
            let prepared = ledger
                .prepare_candidate(
                    lease,
                    journal(
                        1,
                        1,
                        PointerEdgeKind::ButtonReleased(PointerButton::Primary),
                    ),
                )
                .unwrap();
            let committed = ledger.commit_prepared(prepared).unwrap();
            assert!(committed.accepted_edges()[0].stream().incarnation() > old.incarnation());
            assert_eq!(
                ledger.button_authority(),
                AnyButtonDownAuthority::KnownAllReleased,
                "a real release restores all-released authority after full roster resync"
            );
        }
    }
}

#[test]
fn overload_roster_resync_accepts_fresh_watermark_after_press_release_but_rejects_stale_and_foreign()
 {
    let mut ledger = PointerJournalLedger::new(EngineAuthorityDomainId::new_for_test(705));
    let lease = ledger
        .create_provider(
            PointerProviderScope::DesktopGlobal,
            PointerEdgeSequence::new(0),
        )
        .unwrap();
    let prepared = ledger
        .prepare_candidate(
            lease,
            journal(0, 1, PointerEdgeKind::ButtonPressed(PointerButton::Primary)),
        )
        .unwrap();
    ledger.commit_prepared(prepared).unwrap();
    ledger.cancel_live_streams_for_reset(lease).unwrap();
    for (previous, kind) in [
        (1, PointerEdgeKind::ButtonPressed(PointerButton::Primary)),
        (2, PointerEdgeKind::ButtonReleased(PointerButton::Primary)),
    ] {
        let prepared = ledger
            .prepare_candidate(lease, journal(previous, 1, kind))
            .unwrap();
        ledger.commit_prepared(prepared).unwrap();
    }
    assert!(matches!(
        ledger.button_authority(),
        AnyButtonDownAuthority::Unknown(_)
    ));
    let checkpoint = |watermark| {
        let through = PointerEdgeSequence::new(watermark);
        PointerEdgeJournal::new(through, through, Vec::new())
            .unwrap()
            .with_authority_checkpoint(
                PointerAuthorityCheckpoint::known(through, Vec::new()).unwrap(),
            )
            .unwrap()
    };
    let before = ledger.clone();
    assert!(matches!(
        ledger.prepare_candidate(lease, checkpoint(1)),
        Err(PointerJournalLedgerError::JournalReplay { .. })
    ));
    let foreign = PointerInputLease::new(
        EngineAuthorityDomainId::new_for_test(706),
        lease.incarnation(),
        PointerProviderScope::DesktopGlobal,
    );
    assert!(matches!(
        ledger.prepare_candidate(foreign, checkpoint(3)),
        Err(PointerJournalLedgerError::ForeignLease { .. })
    ));
    assert_eq!(ledger, before);

    let prepared = ledger
        .prepare_candidate(lease, checkpoint(3))
        .expect("a fresh complete roster must recover authority after intervening press/release");
    assert!(matches!(
        ledger.button_authority(),
        AnyButtonDownAuthority::Unknown(_)
    ));
    ledger.commit_prepared(prepared).unwrap();
    assert_eq!(
        ledger.button_authority(),
        AnyButtonDownAuthority::KnownAllReleased
    );
    assert_eq!(
        ledger.retained_committed_through(lease).unwrap(),
        PointerEdgeSequence::new(3)
    );
}
