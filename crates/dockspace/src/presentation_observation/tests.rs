use super::*;
use crate::ids::WorkspaceEpoch;
use crate::viewport::{ViewportBinding, WindowIncarnation, WindowToken};

const SURFACE_A: SurfaceId = SurfaceId::new(1);
const SURFACE_B: SurfaceId = SurfaceId::new(2);

fn ledger() -> PresentationLedger {
    PresentationLedger::new(EngineAuthorityDomainId::new_for_test(71))
}

fn native_endpoint(window: u64) -> HostPresentationEndpoint {
    HostPresentationEndpoint::Native(ViewportBinding::new(
        EngineAuthorityDomainId::new_for_test(71),
        WorkspaceEpoch::new(1),
        SURFACE_A,
        WindowToken::new(window),
        WindowIncarnation::new(1),
    ))
}

fn settle_retiring_stream(
    ledger: &mut PresentationLedger,
    host: PresentationHostLease,
    retiring: HostPresentationOutput,
    active: HostPresentationOutput,
    generation: u64,
) {
    let scope = ledger
        .pending_stream_scope(host)
        .expect("complete pending scope");
    assert!(scope.contains(&retiring.stream()));
    assert!(scope.contains(&active.stream()));
    ledger
        .reduce_observation(
            host,
            &scope,
            HostPresentationObservation::Batch(vec![
                HostPresentationObservationEntry::new(
                    retiring.stream(),
                    HostPresentationStreamObservation::Captured {
                        generation: HostPresentationCaptureGeneration::new(generation),
                        progress: HostPresentationProgress::Retired {
                            settled_through: retiring.key(),
                            presented: Authority::Known(None),
                        },
                    },
                ),
                HostPresentationObservationEntry::new(
                    active.stream(),
                    HostPresentationStreamObservation::NoUpdate,
                ),
            ]),
        )
        .expect("retiring stream settles without promoting authority");
}

#[test]
fn repeated_emissions_have_distinct_core_keys_in_one_stream() {
    let mut ledger = ledger();
    let host = ledger.create_host().expect("host lease");
    let first = ledger
        .emit(
            host,
            SURFACE_A,
            HostPresentationEndpoint::Headless,
            HostPresentationOutputPayload::Bootstrap,
        )
        .expect("first emission");
    let second = ledger
        .emit(
            host,
            SURFACE_A,
            HostPresentationEndpoint::Headless,
            HostPresentationOutputPayload::Bootstrap,
        )
        .expect("second emission");

    assert_eq!(first.stream(), second.stream());
    assert_ne!(first.key(), second.key());
    assert_eq!(
        ledger.pending_stream_scope(host).expect("stream scope"),
        BTreeSet::from([first.stream()])
    );
}

#[test]
fn host_scope_isolated_from_other_hosts() {
    let mut ledger = ledger();
    let first_host = ledger.create_host().expect("first host lease");
    let second_host = ledger.create_host().expect("second host lease");
    let first = ledger
        .emit(
            first_host,
            SURFACE_A,
            HostPresentationEndpoint::Headless,
            HostPresentationOutputPayload::Bootstrap,
        )
        .expect("first emission");
    let second = ledger
        .emit(
            second_host,
            SURFACE_B,
            HostPresentationEndpoint::Headless,
            HostPresentationOutputPayload::Bootstrap,
        )
        .expect("second emission");

    assert_eq!(
        ledger
            .pending_stream_scope(first_host)
            .expect("first host scope"),
        BTreeSet::from([first.stream()])
    );
    assert_eq!(
        ledger
            .pending_stream_scope(second_host)
            .expect("second host scope"),
        BTreeSet::from([second.stream()])
    );
}

#[test]
fn batch_requires_the_complete_frozen_pending_scope() {
    let mut ledger = ledger();
    let host = ledger.create_host().expect("host lease");
    let first = ledger
        .emit(
            host,
            SURFACE_A,
            HostPresentationEndpoint::Headless,
            HostPresentationOutputPayload::Bootstrap,
        )
        .expect("first emission");
    let _second = ledger
        .emit(
            host,
            SURFACE_B,
            HostPresentationEndpoint::Headless,
            HostPresentationOutputPayload::Bootstrap,
        )
        .expect("second emission");
    let scope = ledger.pending_stream_scope(host).expect("stream scope");

    let error = ledger
        .reduce_observation(
            host,
            &scope,
            HostPresentationObservation::Batch(vec![HostPresentationObservationEntry::new(
                first.stream(),
                HostPresentationStreamObservation::NoUpdate,
            )]),
        )
        .expect_err("partial batch must be rejected");

    assert!(matches!(
        error,
        PresentationLedgerError::BatchMissingStreams { .. }
    ));
}

#[test]
fn superseded_stream_can_settle_without_regaining_promotion() {
    let mut ledger = ledger();
    let first_host = ledger.create_host().expect("first host lease");
    let second_host = ledger.create_host().expect("second host lease");
    let old = ledger
        .emit(
            first_host,
            SURFACE_A,
            HostPresentationEndpoint::Headless,
            HostPresentationOutputPayload::Bootstrap,
        )
        .expect("old emission");
    let _new = ledger
        .emit(
            second_host,
            SURFACE_A,
            HostPresentationEndpoint::Headless,
            HostPresentationOutputPayload::Bootstrap,
        )
        .expect("new emission");
    let scope = ledger
        .pending_stream_scope(first_host)
        .expect("old host scope");

    let reduction = ledger
        .reduce_observation(
            first_host,
            &scope,
            HostPresentationObservation::Batch(vec![HostPresentationObservationEntry::new(
                old.stream(),
                HostPresentationStreamObservation::Captured {
                    generation: HostPresentationCaptureGeneration::new(1),
                    progress: HostPresentationProgress::Retired {
                        settled_through: old.key(),
                        presented: Authority::Known(Some(old.key())),
                    },
                },
            )]),
        )
        .expect("old stream may settle");

    assert!(reduction.promotions().is_empty());
    assert!(matches!(
        reduction.outcomes(),
        [HostPresentationObservationOutcome::Retired {
            promotion_eligible: false,
            ..
        }]
    ));
    assert!(
        ledger
            .pending_stream_scope(first_host)
            .expect("old host scope")
            .is_empty()
    );
}

#[test]
fn superseded_host_cannot_emit_a_replacement_stream_for_the_same_surface() {
    for remove_surface in [false, true] {
        let mut ledger = ledger();
        let first_host = ledger.create_host().expect("first host lease");
        let second_host = ledger.create_host().expect("second host lease");
        let _old = ledger
            .emit(
                first_host,
                SURFACE_A,
                HostPresentationEndpoint::Headless,
                HostPresentationOutputPayload::Bootstrap,
            )
            .expect("old emission");
        if remove_surface {
            ledger
                .retire_absent_surface_streams(&BTreeSet::new())
                .expect("surface removal retires the first host's stream");
        }
        let current = ledger
            .emit(
                second_host,
                SURFACE_A,
                HostPresentationEndpoint::Headless,
                HostPresentationOutputPayload::Bootstrap,
            )
            .expect("current emission");

        let error = ledger
            .emit(
                first_host,
                SURFACE_A,
                HostPresentationEndpoint::Headless,
                HostPresentationOutputPayload::Bootstrap,
            )
            .expect_err("superseded host must not reclaim the surface");
        assert!(matches!(
            error,
            PresentationLedgerError::SupersededHostCannotEmit {
                host,
                surface: SURFACE_A,
                active_stream,
            } if host == first_host && active_stream == current.stream()
        ));

        let current_again = ledger
            .emit(
                second_host,
                SURFACE_A,
                HostPresentationEndpoint::Headless,
                HostPresentationOutputPayload::Bootstrap,
            )
            .expect("current host remains active");
        assert_eq!(current_again.stream(), current.stream());
    }
}

#[test]
fn same_host_may_advance_endpoint_but_cannot_reuse_a_retired_endpoint() {
    for first_endpoint in [HostPresentationEndpoint::Headless, native_endpoint(10)] {
        let mut ledger = ledger();
        let host = ledger.create_host().expect("host lease");
        let second_endpoint = native_endpoint(11);
        let first = ledger
            .emit(
                host,
                SURFACE_A,
                first_endpoint,
                HostPresentationOutputPayload::Bootstrap,
            )
            .expect("first endpoint emission");
        let successor = ledger
            .emit(
                host,
                SURFACE_A,
                second_endpoint,
                HostPresentationOutputPayload::Bootstrap,
            )
            .expect("same host may advance to a new endpoint");
        let successor_again = ledger
            .emit(
                host,
                SURFACE_A,
                second_endpoint,
                HostPresentationOutputPayload::Bootstrap,
            )
            .expect("active successor remains usable");
        assert_eq!(successor_again.stream(), successor.stream());

        let error = ledger
            .emit(
                host,
                SURFACE_A,
                first_endpoint,
                HostPresentationOutputPayload::Bootstrap,
            )
            .expect_err("retired endpoint must not regain authority");
        assert!(matches!(
            error,
            PresentationLedgerError::RetiredEndpointCannotEmit {
                surface: SURFACE_A,
                endpoint,
                stream,
            } if endpoint == first_endpoint && stream == first.stream()
        ));
    }
}

#[test]
fn removed_or_quiesced_native_endpoint_cannot_emit_before_reclamation() {
    for quiesce_endpoint in [false, true] {
        let mut ledger = ledger();
        let host = ledger.create_host().expect("host lease");
        let endpoint = native_endpoint(10);
        let removed = ledger
            .emit(
                host,
                SURFACE_A,
                endpoint,
                HostPresentationOutputPayload::Bootstrap,
            )
            .expect("native endpoint emits");
        if quiesce_endpoint {
            let HostPresentationEndpoint::Native(binding) = endpoint else {
                panic!("the fixture must use a native endpoint")
            };
            assert_eq!(
                ledger
                    .retire_native_binding_streams_for_quiescence(host, binding)
                    .expect("native endpoint quiescence retires its stream"),
                vec![removed.stream()]
            );
        } else {
            ledger
                .retire_absent_surface_streams(&BTreeSet::new())
                .expect("surface removal retires its native stream");
        }

        let error = ledger
            .emit(
                host,
                SURFACE_A,
                endpoint,
                HostPresentationOutputPayload::Bootstrap,
            )
            .expect_err("native endpoint reuse still requires exact reclamation");
        assert!(matches!(
            error,
            PresentationLedgerError::RetiredEndpointCannotEmit {
                surface: SURFACE_A,
                endpoint: rejected_endpoint,
                stream,
            } if rejected_endpoint == endpoint && stream == removed.stream()
        ));
    }
}

#[test]
fn removed_headless_stream_reclaims_without_disturbing_pending_successor() {
    let mut ledger = ledger();
    let host = ledger.create_host().expect("host lease");
    let removed = ledger
        .emit(
            host,
            SURFACE_A,
            HostPresentationEndpoint::Headless,
            HostPresentationOutputPayload::Bootstrap,
        )
        .expect("headless predecessor emits");
    ledger
        .retire_absent_surface_streams(&BTreeSet::new())
        .expect("surface removal retires its headless stream");
    let successor = ledger
        .emit(
            host,
            SURFACE_A,
            HostPresentationEndpoint::Headless,
            HostPresentationOutputPayload::Bootstrap,
        )
        .expect("same host may emit a fresh headless incarnation");
    assert_ne!(successor.stream(), removed.stream());
    assert_ne!(successor.key(), removed.key());
    assert_eq!(successor.key().ordinal_for_test(), 1);
    assert_eq!(removed.key().ordinal_for_test(), 1);
    assert!(matches!(
        ledger.prepare_stream_quiescence(host, removed.stream()),
        Err(PresentationLedgerError::StreamHasPendingOutputs { stream })
            if stream == removed.stream()
    ));

    settle_retiring_stream(&mut ledger, host, removed, successor, 1);
    let proof = ledger
        .prepare_stream_quiescence(host, removed.stream())
        .expect("a distinct active successor must not block exact predecessor quiescence");
    ledger
        .compact_quiesced_retiring_stream(proof, &BTreeSet::new())
        .expect("settled predecessor reclaims independently of its successor");
    assert!(!ledger.streams.contains_key(&removed.stream()));
    assert_eq!(
        ledger.hosts[&host].streams,
        BTreeSet::from([successor.stream()])
    );
    assert_eq!(
        ledger.active_surface_scope(SURFACE_A),
        Some((successor.stream(), host, HostPresentationEndpoint::Headless))
    );
    assert_eq!(
        ledger.pending_output_keys().collect::<Vec<_>>(),
        vec![successor.key()]
    );
}

#[test]
fn surface_removed_stream_cannot_quiesce_under_a_foreign_successor() {
    let mut ledger = ledger();
    let first_host = ledger.create_host().expect("first host lease");
    let second_host = ledger.create_host().expect("second host lease");
    let removed = ledger
        .emit(
            first_host,
            SURFACE_A,
            HostPresentationEndpoint::Headless,
            HostPresentationOutputPayload::Bootstrap,
        )
        .expect("predecessor emits");
    ledger
        .retire_absent_surface_streams(&BTreeSet::new())
        .expect("surface removal retires the predecessor");
    let current = ledger
        .emit(
            second_host,
            SURFACE_A,
            HostPresentationEndpoint::Headless,
            HostPresentationOutputPayload::Bootstrap,
        )
        .expect("foreign host owns the successor");
    let scope = ledger
        .pending_stream_scope(first_host)
        .expect("predecessor host pending scope");
    ledger
        .reduce_observation(
            first_host,
            &scope,
            HostPresentationObservation::Batch(vec![HostPresentationObservationEntry::new(
                removed.stream(),
                HostPresentationStreamObservation::Captured {
                    generation: HostPresentationCaptureGeneration::new(1),
                    progress: HostPresentationProgress::Retired {
                        settled_through: removed.key(),
                        presented: Authority::Known(Some(removed.key())),
                    },
                },
            )]),
        )
        .expect("removed predecessor settles under the foreign owner");

    let error = ledger
        .prepare_stream_quiescence(first_host, removed.stream())
        .expect_err("a foreign successor must prevent predecessor quiescence");
    assert!(matches!(
        error,
        PresentationLedgerError::SurfaceRemovedStreamRegainedOwnership {
            stream,
            surface: SURFACE_A,
        } if stream == removed.stream()
    ));
    let error = ledger
        .emit(
            first_host,
            SURFACE_A,
            HostPresentationEndpoint::Headless,
            HostPresentationOutputPayload::Bootstrap,
        )
        .expect_err("the superseded host must not reclaim the surface");
    assert!(matches!(
        error,
        PresentationLedgerError::SupersededHostCannotEmit {
            host,
            surface: SURFACE_A,
            active_stream,
        } if host == first_host && active_stream == current.stream()
    ));
}

#[test]
fn surface_removed_stream_quiescence_rechecks_foreign_owner_on_consumption() {
    let mut ledger = ledger();
    let first_host = ledger.create_host().expect("first host lease");
    let second_host = ledger.create_host().expect("second host lease");
    let removed = ledger
        .emit(
            first_host,
            SURFACE_A,
            HostPresentationEndpoint::Headless,
            HostPresentationOutputPayload::Bootstrap,
        )
        .expect("predecessor emits");
    ledger
        .retire_absent_surface_streams(&BTreeSet::new())
        .expect("surface removal retires the predecessor");
    let successor = ledger
        .emit(
            first_host,
            SURFACE_A,
            HostPresentationEndpoint::Headless,
            HostPresentationOutputPayload::Bootstrap,
        )
        .expect("same-host successor emits");
    settle_retiring_stream(&mut ledger, first_host, removed, successor, 1);
    let proof = ledger
        .prepare_stream_quiescence(first_host, removed.stream())
        .expect("same-host successor permits preparing predecessor quiescence");

    ledger
        .emit(
            second_host,
            SURFACE_A,
            HostPresentationEndpoint::Headless,
            HostPresentationOutputPayload::Bootstrap,
        )
        .expect("foreign host takes ownership after proof preparation");
    let before = ledger.clone();
    let error = ledger
        .compact_quiesced_retiring_stream(proof, &BTreeSet::new())
        .expect_err("consumption must recheck the current foreign owner");
    assert!(matches!(
        error,
        PresentationLedgerError::SurfaceRemovedStreamRegainedOwnership {
            stream,
            surface: SURFACE_A,
        } if stream == removed.stream()
    ));
    assert_eq!(ledger, before);
}

#[test]
fn tick_final_surface_removal_retires_and_reclaims_without_a_successor() {
    let mut ledger = ledger();
    let host = ledger.create_host().expect("host lease");
    let removed = ledger
        .emit(
            host,
            SURFACE_A,
            native_endpoint(10),
            HostPresentationOutputPayload::Bootstrap,
        )
        .expect("removed surface endpoint emits");
    let retained = ledger
        .emit(
            host,
            SURFACE_B,
            HostPresentationEndpoint::Headless,
            HostPresentationOutputPayload::Bootstrap,
        )
        .expect("retained surface endpoint emits");

    ledger
        .retire_absent_surface_streams(&BTreeSet::from([SURFACE_B]))
        .expect("the tick-final roster must retire only absent surfaces");
    assert!(!ledger.active_streams.contains_key(&SURFACE_A));
    assert_eq!(
        ledger.active_streams.get(&SURFACE_B),
        Some(&retained.stream())
    );
    assert!(ledger.streams.get(&removed.stream()).is_some_and(|state| {
        state.lifecycle
            == PresentationStreamLifecycle::Retiring(
                PresentationStreamRetirementCause::SurfaceRemoved,
            )
    }));
    assert!(matches!(
        ledger.prepare_stream_quiescence(host, removed.stream()),
        Err(PresentationLedgerError::StreamHasPendingOutputs { stream })
            if stream == removed.stream()
    ));

    let scope = ledger.pending_stream_scope(host).expect("pending scope");
    ledger
        .reduce_observation(
            host,
            &scope,
            HostPresentationObservation::Batch(vec![
                HostPresentationObservationEntry::new(
                    removed.stream(),
                    HostPresentationStreamObservation::Captured {
                        generation: HostPresentationCaptureGeneration::new(1),
                        progress: HostPresentationProgress::Retired {
                            settled_through: removed.key(),
                            presented: Authority::Known(None),
                        },
                    },
                ),
                HostPresentationObservationEntry::new(
                    retained.stream(),
                    HostPresentationStreamObservation::NoUpdate,
                ),
            ]),
        )
        .expect("removed surface output must settle without a successor");
    let proof = ledger
        .prepare_stream_quiescence(host, removed.stream())
        .expect("settled surface-removed stream must accept exact renderer quiescence");
    ledger
        .compact_quiesced_retiring_stream(proof, &BTreeSet::new())
        .expect("surface-removed stream must compact without a fabricated successor");
    assert!(!ledger.streams.contains_key(&removed.stream()));
    assert!(ledger.streams.contains_key(&retained.stream()));
}

#[test]
fn quiescence_reclaims_only_a_settled_same_host_retiring_stream_and_late_results_fail_closed() {
    let mut ledger = ledger();
    let host = ledger.create_host().expect("host lease");
    let retiring = ledger
        .emit(
            host,
            SURFACE_A,
            native_endpoint(10),
            HostPresentationOutputPayload::Bootstrap,
        )
        .expect("first endpoint emits");
    let active = ledger
        .emit(
            host,
            SURFACE_A,
            native_endpoint(11),
            HostPresentationOutputPayload::Bootstrap,
        )
        .expect("successor endpoint emits");

    assert!(matches!(
        ledger.prepare_stream_quiescence(host, retiring.stream()),
        Err(PresentationLedgerError::StreamHasPendingOutputs { stream }) if stream == retiring.stream()
    ));

    settle_retiring_stream(&mut ledger, host, retiring, active, 1);
    let retained_before = crate::retention::PresentationRetentionManifest::from_resources(
        ledger.pending_output_keys(),
        ledger.retained_stream_ids(),
    );
    assert!(retained_before.retains_stream(retiring.stream()));
    let proof = ledger
        .prepare_stream_quiescence(host, retiring.stream())
        .expect("settled retiring stream may be externally quiesced");
    assert!(matches!(
        ledger.compact_quiesced_retiring_stream(proof, &BTreeSet::from([retiring.stream()])),
        Err(PresentationLedgerError::StreamQuiescenceStillRetained { stream }) if stream == retiring.stream()
    ));

    let proof = ledger
        .prepare_stream_quiescence(host, retiring.stream())
        .expect("failed compaction consumes only its proof");
    ledger
        .compact_quiesced_retiring_stream(proof, &BTreeSet::new())
        .expect("quiesced stream is reclaimed after every core reference disappears");
    assert!(!ledger.streams.contains_key(&retiring.stream()));
    assert!(
        ledger
            .hosts
            .get(&host)
            .is_some_and(|state| !state.streams.contains(&retiring.stream()))
    );
    assert_eq!(
        ledger.retained_stream_ids().collect::<Vec<_>>(),
        vec![active.stream()]
    );
    let retained_after = crate::retention::PresentationRetentionManifest::from_resources(
        ledger.pending_output_keys(),
        ledger.retained_stream_ids(),
    );
    assert!(!retained_after.retains_stream(retiring.stream()));

    let error = ledger
        .reduce_observation(
            host,
            &BTreeSet::new(),
            HostPresentationObservation::Batch(vec![HostPresentationObservationEntry::new(
                retiring.stream(),
                HostPresentationStreamObservation::NoUpdate,
            )]),
        )
        .expect_err("late results for a quiesced stream remain outside the frozen scope");
    assert!(matches!(
        error,
        PresentationLedgerError::BatchExtraStreams { extra } if extra == vec![retiring.stream()]
    ));
}

#[test]
fn ten_thousand_same_host_native_endpoint_rotations_reclaim_quiesced_streams() {
    let mut ledger = ledger();
    let host = ledger.create_host().expect("host lease");
    let mut retiring = ledger
        .emit(
            host,
            SURFACE_A,
            native_endpoint(10),
            HostPresentationOutputPayload::Bootstrap,
        )
        .expect("initial endpoint emits");

    for generation in 1..=10_000 {
        let endpoint = if generation % 2 == 0 {
            native_endpoint(10)
        } else {
            native_endpoint(11)
        };
        let active = ledger
            .emit(
                host,
                SURFACE_A,
                endpoint,
                HostPresentationOutputPayload::Bootstrap,
            )
            .expect("quiesced endpoints may rotate without retaining old stream state");
        settle_retiring_stream(&mut ledger, host, retiring, active, generation);
        let proof = ledger
            .prepare_stream_quiescence(host, retiring.stream())
            .expect("settled old endpoint is externally quiescent");
        ledger
            .compact_quiesced_retiring_stream(proof, &BTreeSet::new())
            .expect("quiesced old endpoint stream is compacted");
        retiring = active;
    }

    let diagnostics = ledger.diagnostics();
    assert_eq!(diagnostics.live_hosts(), 1);
    assert_eq!(diagnostics.retained_host_states(), 1);
    assert_eq!(diagnostics.active_streams(), 1);
    assert_eq!(diagnostics.retiring_streams(), 0);
    assert_eq!(diagnostics.retained_stream_states(), 1);
    assert_eq!(diagnostics.pending_streams(), 1);
    assert_eq!(
        ledger.hosts[&host].streams,
        BTreeSet::from([retiring.stream()])
    );
}

#[test]
fn host_retirement_terminates_its_complete_roster_without_releasing_a_successor() {
    let mut ledger = ledger();
    let retired_host = ledger.create_host().expect("retiring host lease");
    let current_host = ledger.create_host().expect("current host lease");
    let first_endpoint = HostPresentationEndpoint::Headless;
    let second_endpoint = HostPresentationEndpoint::Native(ViewportBinding::new(
        EngineAuthorityDomainId::new_for_test(71),
        WorkspaceEpoch::new(1),
        SURFACE_A,
        WindowToken::new(10),
        WindowIncarnation::new(1),
    ));
    let first = ledger
        .emit(
            retired_host,
            SURFACE_A,
            first_endpoint,
            HostPresentationOutputPayload::Bootstrap,
        )
        .expect("first retiring-host stream emits");
    let second = ledger
        .emit(
            retired_host,
            SURFACE_A,
            second_endpoint,
            HostPresentationOutputPayload::Bootstrap,
        )
        .expect("second endpoint creates another stream");
    let sibling = ledger
        .emit(
            retired_host,
            SURFACE_B,
            HostPresentationEndpoint::Headless,
            HostPresentationOutputPayload::Bootstrap,
        )
        .expect("sibling surface stream emits");
    let successor = ledger
        .emit(
            current_host,
            SURFACE_A,
            HostPresentationEndpoint::Headless,
            HostPresentationOutputPayload::Bootstrap,
        )
        .expect("successor host takes surface ownership");

    let retirement = ledger
        .retire_host(
            retired_host,
            ReducerTickId::new(9),
            PresentationHostRetirementReason::RuntimeDestroyed,
        )
        .expect("host retirement succeeds");
    assert_eq!(
        retirement.streams(),
        &BTreeSet::from([first.stream(), second.stream(), sibling.stream()])
    );
    assert_eq!(
        retirement.retired_outputs(),
        &BTreeSet::from([first.key(), second.key(), sibling.key()])
    );
    assert_eq!(retirement.retired_output_count(), 3);
    assert_eq!(
        retirement.released_active_surfaces(),
        &BTreeSet::from([SURFACE_B])
    );
    assert_eq!(
        ledger.active_streams.get(&SURFACE_A),
        Some(&successor.stream())
    );
    assert!(!ledger.active_streams.contains_key(&SURFACE_B));
    assert!(retirement.streams().iter().all(|stream| {
        ledger.streams.get(stream).is_some_and(|state| {
            state.lifecycle == PresentationStreamLifecycle::Terminated && state.pending.is_empty()
        })
    }));
    let diagnostics = ledger.diagnostics();
    assert_eq!(diagnostics.live_hosts(), 1);
    assert_eq!(diagnostics.retired_hosts(), 1);
    assert_eq!(diagnostics.active_streams(), 1);
    assert_eq!(diagnostics.retiring_streams(), 0);
    assert_eq!(diagnostics.terminated_streams(), 3);
    assert_eq!(diagnostics.active_surface_owners(), 1);
    assert_eq!(diagnostics.pending_outputs(), 1);

    assert!(matches!(
        ledger.validate_lease(retired_host),
        Err(PresentationLedgerError::HostRetired {
            host,
            retired_at,
            reason: PresentationHostRetirementReason::RuntimeDestroyed,
        }) if host == retired_host && retired_at == ReducerTickId::new(9)
    ));
    assert!(matches!(
        ledger.emit(
            retired_host,
            SURFACE_B,
            HostPresentationEndpoint::Headless,
            HostPresentationOutputPayload::Bootstrap,
        ),
        Err(PresentationLedgerError::HostRetired { host, .. }) if host == retired_host
    ));
}

#[test]
fn ten_thousand_retired_hosts_compact_to_one_interval_without_reusing_identity() {
    let mut ledger = ledger();

    for value in 1..=10_000 {
        let host = ledger
            .create_host()
            .expect("host identity remains available");
        ledger
            .emit(
                host,
                SURFACE_A,
                HostPresentationEndpoint::Headless,
                HostPresentationOutputPayload::Bootstrap,
            )
            .expect("one stream emits before retirement");
        let retirement = ledger
            .retire_host(
                host,
                ReducerTickId::new(value),
                PresentationHostRetirementReason::ExplicitShutdown,
            )
            .expect("host retirement remains available");
        assert_eq!(retirement.streams().len(), 1);
        assert!(
            ledger
                .compact_retired_host(host)
                .expect("terminal stream roster is compactable")
        );
        assert!(matches!(
            ledger.validate_lease(host),
            Err(PresentationLedgerError::HostRetiredCompacted { host: retired })
                if retired == host
        ));
    }

    let diagnostics = ledger.diagnostics();
    assert_eq!(ledger.host_frontier(), 10_000);
    assert_eq!(diagnostics.retained_host_states(), 0);
    assert_eq!(diagnostics.retained_stream_states(), 0);
    assert_eq!(diagnostics.compacted_retired_hosts(), 10_000);
    assert_eq!(diagnostics.compacted_retirement_ranges(), 1);
    assert_eq!(diagnostics.pending_outputs(), 0);

    let successor = ledger
        .create_host()
        .expect("allocation continues beyond compacted identities");
    assert_eq!(successor.serial.0, 10_001);
}

#[test]
fn one_live_host_is_an_exact_hole_in_compacted_retirement_ranges() {
    let mut ledger = ledger();
    let live = ledger.create_host().expect("live host identity");

    for value in 2..=10_000 {
        let host = ledger.create_host().expect("retiring host identity");
        ledger
            .retire_host(
                host,
                ReducerTickId::new(value),
                PresentationHostRetirementReason::ExplicitShutdown,
            )
            .expect("host retirement");
        assert!(ledger.compact_retired_host(host).expect("compaction"));
    }

    let with_hole = ledger.diagnostics();
    assert_eq!(with_hole.live_hosts(), 1);
    assert_eq!(with_hole.retained_host_states(), 1);
    assert_eq!(with_hole.compacted_retired_hosts(), 9_999);
    assert_eq!(with_hole.compacted_retirement_ranges(), 1);
    ledger
        .validate_lease(live)
        .expect("the serial hole remains a live authority");

    ledger
        .retire_host(
            live,
            ReducerTickId::new(10_001),
            PresentationHostRetirementReason::ExplicitShutdown,
        )
        .expect("live hole retires");
    assert!(ledger.compact_retired_host(live).expect("hole compacts"));
    let merged = ledger.diagnostics();
    assert_eq!(merged.retained_host_states(), 0);
    assert_eq!(merged.compacted_retired_hosts(), 10_000);
    assert_eq!(merged.compacted_retirement_ranges(), 1);
}
