use dockspace::external_item_key::{
    ExternalItemKeyMap, ExternalItemKeyMapError, ExternalItemKeyReconcileError,
};
use dockspace::ids::ItemId;

#[test]
fn lexical_predecessor_insertions_never_renumber_existing_items() {
    let mut mapping = ExternalItemKeyMap::new();
    let middle = mapping.ensure("panes/middle").expect("first key must fit");
    let predecessor = mapping
        .ensure("panes/alpha")
        .expect("lexical predecessor must fit");

    assert_eq!(middle, ItemId::new(1));
    assert_eq!(predecessor, ItemId::new(2));
    assert_eq!(mapping.item_id("panes/middle"), Some(middle));
    assert_eq!(mapping.external_key(middle), Some("panes/middle"));
}

#[test]
fn workspace_removal_and_reopen_keep_the_append_only_identity() {
    let mut mapping = ExternalItemKeyMap::new();
    let opened = mapping.ensure("panes/log").expect("first key must fit");
    let unrelated = mapping
        .ensure("panes/editor")
        .expect("unrelated key must fit");

    let reopened = mapping
        .ensure("panes/log")
        .expect("a key absent from the live workspace keeps its sidecar mapping");
    assert_eq!(opened, ItemId::new(1));
    assert_eq!(unrelated, ItemId::new(2));
    assert_eq!(reopened, opened);
    assert_eq!(
        mapping
            .ensure("panes/search")
            .expect("the next new key advances monotonically"),
        ItemId::new(3)
    );
}

#[test]
fn later_topology_and_iteration_order_cannot_change_registered_identities() {
    let mut mapping = ExternalItemKeyMap::new();
    mapping
        .ensure_all(["panes/zeta", "panes/alpha", "panes/middle"])
        .expect("explicit registration transaction must fit");
    let registered = mapping.clone();

    mapping
        .ensure_all(["panes/middle", "panes/alpha", "panes/zeta", "panes/alpha"])
        .expect("a reordered live traversal only resolves existing keys");

    assert_eq!(mapping, registered);
    assert_eq!(mapping.item_id("panes/zeta"), Some(ItemId::new(1)));
    assert_eq!(mapping.item_id("panes/alpha"), Some(ItemId::new(2)));
    assert_eq!(mapping.item_id("panes/middle"), Some(ItemId::new(3)));
}

#[test]
fn invalid_batch_is_atomic_and_does_not_consume_an_identity() {
    let mut mapping = ExternalItemKeyMap::new();
    assert_eq!(
        mapping.ensure_all(["panes/valid", ""]),
        Err(ExternalItemKeyMapError::EmptyExternalKey)
    );
    assert!(mapping.is_empty());
    assert_eq!(
        mapping
            .ensure("panes/valid")
            .expect("failed batch must leave first identity available"),
        ItemId::new(1)
    );
}

#[test]
fn reconciliation_preserves_newer_tombstones_and_the_monotonic_frontier() {
    let mut active = ExternalItemKeyMap::new();
    active
        .ensure_all(["panes/primary", "panes/issued-after-snapshot"])
        .expect("active identities must allocate");
    let mut restored = ExternalItemKeyMap::new();
    restored
        .ensure("panes/primary")
        .expect("older restored identity must allocate");

    let mut reconciled = active
        .reconciled_with(&restored)
        .expect("compatible identity histories must reconcile");

    assert_eq!(
        reconciled.item_id("panes/issued-after-snapshot"),
        Some(ItemId::new(2))
    );
    assert_eq!(
        reconciled
            .ensure("panes/allocated-after-restore")
            .expect("reconciled frontier must remain allocatable"),
        ItemId::new(3)
    );
}

#[test]
fn reconciliation_rejects_key_and_item_conflicts_without_mutating_either_input() {
    let mut active = ExternalItemKeyMap::new();
    active
        .ensure("panes/a")
        .expect("active identity must allocate");

    let mut key_conflict = ExternalItemKeyMap::new();
    key_conflict
        .ensure_all(["panes/prefix", "panes/a"])
        .expect("incoming key-conflict fixture must allocate");
    let active_before = active.clone();
    let incoming_before = key_conflict.clone();
    assert_eq!(
        active.reconciled_with(&key_conflict),
        Err(ExternalItemKeyReconcileError::ExternalKeyConflict {
            external_key: "panes/a".to_owned(),
            active_item: ItemId::new(1),
            incoming_item: ItemId::new(2),
        })
    );
    assert_eq!(active, active_before);
    assert_eq!(key_conflict, incoming_before);

    let mut item_conflict = ExternalItemKeyMap::new();
    item_conflict
        .ensure("panes/b")
        .expect("incoming item-conflict fixture must allocate");
    let incoming_before = item_conflict.clone();
    assert_eq!(
        active.reconciled_with(&item_conflict),
        Err(ExternalItemKeyReconcileError::ItemIdConflict {
            item: ItemId::new(1),
            active_external_key: "panes/a".to_owned(),
            incoming_external_key: "panes/b".to_owned(),
        })
    );
    assert_eq!(active, active_before);
    assert_eq!(item_conflict, incoming_before);
}

#[cfg(feature = "serde")]
mod persistence {
    use dockspace::external_item_key::{
        EXTERNAL_ITEM_KEY_SNAPSHOT_VERSION, ExternalItemKeyMap, ExternalItemKeyMapError,
        ExternalItemKeyReconcileError, ExternalItemKeyRestoreError, ExternalItemKeySnapshot,
        ExternalItemKeySnapshotEnvelope, SnapshotExternalItemKeyEntry,
    };
    use dockspace::ids::ItemId;

    fn snapshot(
        entries: impl IntoIterator<Item = (&'static str, u64)>,
        next_item_id: Option<u64>,
    ) -> ExternalItemKeySnapshot {
        ExternalItemKeySnapshot {
            entries: entries
                .into_iter()
                .map(|(external_key, item_id)| SnapshotExternalItemKeyEntry {
                    external_key: external_key.to_owned(),
                    item_id,
                })
                .collect(),
            next_item_id,
        }
    }

    #[test]
    fn restart_round_trip_preserves_assignments_and_reopen_identity() {
        let mut original = ExternalItemKeyMap::new();
        original
            .ensure_all(["panes/search", "panes/editor", "panes/terminal"])
            .expect("initial batch must fit");
        let search = original
            .item_id("panes/search")
            .expect("search mapping must exist");

        let encoded = serde_json::to_string_pretty(&ExternalItemKeySnapshot::capture(&original))
            .expect("sidecar must serialize");
        let mut restored = serde_json::from_str::<ExternalItemKeySnapshotEnvelope>(&encoded)
            .expect("sidecar must decode")
            .into_mapping()
            .expect("sidecar must validate");
        assert_eq!(restored, original);

        let reopened = restored
            .ensure("panes/search")
            .expect("restored key must remain allocated");
        assert_eq!(reopened, search);
        assert_eq!(
            restored
                .ensure("panes/problems")
                .expect("restored frontier must allocate a new key"),
            ItemId::new(4)
        );
    }

    #[test]
    fn restore_rejects_zero_duplicate_and_non_monotonic_assignments() {
        assert_eq!(
            snapshot([("panes/a", 0)], Some(1))
                .restore()
                .expect_err("zero item identity must fail"),
            ExternalItemKeyRestoreError::ZeroItemId {
                external_key: "panes/a".to_owned(),
            }
        );
        assert_eq!(
            snapshot([("panes/a", 1), ("panes/a", 2)], Some(3))
                .restore()
                .expect_err("duplicate key must fail"),
            ExternalItemKeyRestoreError::DuplicateExternalKey {
                external_key: "panes/a".to_owned(),
            }
        );
        assert_eq!(
            snapshot([("panes/a", 7), ("panes/b", 7)], Some(8))
                .restore()
                .expect_err("duplicate item identity must fail"),
            ExternalItemKeyRestoreError::DuplicateItemId {
                item_id: 7,
                first_external_key: "panes/a".to_owned(),
                second_external_key: "panes/b".to_owned(),
            }
        );
        assert_eq!(
            snapshot([("panes/a", 7)], Some(7))
                .restore()
                .expect_err("frontier cannot reuse a live identity"),
            ExternalItemKeyRestoreError::FrontierNotBeyondAssigned {
                next_item_id: 7,
                highest_item_id: 7,
            }
        );
        assert_eq!(
            snapshot([], None)
                .restore()
                .expect_err("an exhausted frontier must prove the final identity was consumed"),
            ExternalItemKeyRestoreError::ExhaustedFrontierWithoutMaximumAssignment
        );
        assert_eq!(
            snapshot([], Some(0))
                .restore()
                .expect_err("zero frontier must fail"),
            ExternalItemKeyRestoreError::ZeroNextItemId
        );
        assert_eq!(
            snapshot([("", 1)], Some(2))
                .restore()
                .expect_err("empty external key must fail"),
            ExternalItemKeyRestoreError::EmptyExternalKey
        );
    }

    #[test]
    fn failed_overflow_batch_does_not_advance_the_frontier() {
        let mut mapping = snapshot([], Some(u64::MAX))
            .restore()
            .expect("maximum frontier is valid before allocation");
        assert_eq!(
            mapping.ensure_all(["panes/a", "panes/b"]),
            Err(ExternalItemKeyMapError::ItemIdSpaceExhausted {
                requested_new_keys: 2,
            })
        );
        assert!(mapping.is_empty());

        assert_eq!(
            mapping
                .ensure("panes/a")
                .expect("failed batch must preserve the final identity"),
            ItemId::new(u64::MAX)
        );
        assert_eq!(
            mapping.ensure("panes/b"),
            Err(ExternalItemKeyMapError::ItemIdSpaceExhausted {
                requested_new_keys: 1,
            })
        );

        let encoded = serde_json::to_string(&ExternalItemKeySnapshot::capture(&mapping))
            .expect("exhausted frontier must serialize");
        assert!(encoded.contains(r#""next_item_id":null"#));
        let mut restarted = serde_json::from_str::<ExternalItemKeySnapshotEnvelope>(&encoded)
            .expect("exhausted sidecar must decode")
            .into_mapping()
            .expect("exhausted sidecar must validate");
        assert_eq!(restarted, mapping);
        assert!(matches!(
            restarted.ensure("panes/c"),
            Err(ExternalItemKeyMapError::ItemIdSpaceExhausted { .. })
        ));
    }

    #[test]
    fn reconciliation_preserves_exhaustion_from_either_identity_history() {
        let mut active = ExternalItemKeyMap::new();
        active
            .ensure("panes/primary")
            .expect("active identity must allocate");
        let exhausted = snapshot([("panes/primary", 1), ("panes/final", u64::MAX)], None)
            .restore()
            .expect("exhausted incoming fixture must validate");

        for mut reconciled in [
            active
                .reconciled_with(&exhausted)
                .expect("incoming exhaustion must reconcile"),
            exhausted
                .reconciled_with(&active)
                .expect("active exhaustion must reconcile"),
        ] {
            assert_eq!(reconciled.item_id("panes/primary"), Some(ItemId::new(1)));
            assert_eq!(
                reconciled.item_id("panes/final"),
                Some(ItemId::new(u64::MAX))
            );
            assert_eq!(
                reconciled.ensure("panes/never-reusable"),
                Err(ExternalItemKeyMapError::ItemIdSpaceExhausted {
                    requested_new_keys: 1,
                })
            );
        }
    }

    #[test]
    fn reconciliation_rejects_assignments_that_fill_an_opaque_consumed_gap() {
        let opaque = snapshot([("panes/primary", 1)], Some(3))
            .restore()
            .expect("opaque-gap fixture must validate");
        let explicit = snapshot([("panes/primary", 1), ("panes/secondary", 2)], Some(3))
            .restore()
            .expect("explicit fixture must validate");
        let opaque_before = opaque.clone();
        let explicit_before = explicit.clone();

        assert_eq!(
            opaque.reconciled_with(&explicit),
            Err(ExternalItemKeyReconcileError::ActiveHistoryOpaqueConflict {
                item: ItemId::new(2),
                incoming_external_key: "panes/secondary".to_owned(),
            })
        );
        assert_eq!(
            explicit.reconciled_with(&opaque),
            Err(
                ExternalItemKeyReconcileError::IncomingHistoryOpaqueConflict {
                    item: ItemId::new(2),
                    active_external_key: "panes/secondary".to_owned(),
                }
            )
        );
        assert_eq!(opaque, opaque_before);
        assert_eq!(explicit, explicit_before);
    }

    #[test]
    fn supported_wire_schema_rejects_unknown_duplicate_and_malformed_fields() {
        for invalid in [
            r#"[1,{"entries":[],"next_item_id":1,"future":true}]"#,
            r#"[1,{"entries":[{"external_key":"panes/a","item_id":1,"future":true}],"next_item_id":2}]"#,
            r#"[1,{"entries":[],"entries":[],"next_item_id":1}]"#,
            r#"[1,{"entries":[{"external_key":"panes/a","external_key":"panes/b","item_id":1}],"next_item_id":2}]"#,
            r#"[1,{"entries":[]}]"#,
            r#"[1,{"entries":[],"next_item_id":1},{}]"#,
        ] {
            assert!(
                serde_json::from_str::<ExternalItemKeySnapshotEnvelope>(invalid).is_err(),
                "malformed supported sidecar must fail: {invalid}"
            );
        }
    }

    #[test]
    fn unsupported_version_is_reported_before_future_payload_shape() {
        let envelope = serde_json::from_str::<ExternalItemKeySnapshotEnvelope>(
            r#"[9,{"future":{"shape":[1,2,3]}}]"#,
        )
        .expect("future payload must be skipped by the version-first envelope");
        assert_eq!(envelope.version(), 9);
        assert_eq!(
            envelope
                .into_mapping()
                .expect_err("future version must not restore"),
            ExternalItemKeyRestoreError::UnsupportedVersion {
                found: 9,
                supported: EXTERNAL_ITEM_KEY_SNAPSHOT_VERSION,
            }
        );
    }
}
