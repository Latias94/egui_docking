use super::*;

#[test]
fn locked_policy_rejects_mutation_requests_and_eligibility_fast_paths() {
    let surface = SurfaceId::new(1);
    let payload = DockPayloadPolicyFacts::new(
        DockPayloadKind::Root,
        [ItemId::new(1)],
        RootId::new(10),
        surface,
        DockPresentationMode::Contained,
        false,
    );
    let target = DockDropTargetFacts::new(surface, None, false);
    let mut policy = DockPolicy::new();
    policy.set_allow_native_surfaces(true);
    policy.set_close_capability(CloseCapability::DeferredAllowed);
    let unlocked = policy.snapshot(PolicyRevision::new(1));
    policy.set_arrangement_locked(true);
    assert!(policy.central_node().allows_tab_merge());
    assert!(policy.central_node().allows_edge_split());
    let locked = policy.snapshot(PolicyRevision::new(2));

    let mut requests = vec![
        DockPolicyRequest::TransformContained(DockContainedTransformPolicyRequest::new(
            payload.clone(),
            surface,
        )),
        DockPolicyRequest::Resize(DockResizePolicyRequest::new(
            Axis::Horizontal,
            Some(surface),
        )),
        DockPolicyRequest::ClosePane {
            item: ItemId::new(1),
            deferred: false,
        },
        DockPolicyRequest::ClosePane {
            item: ItemId::new(1),
            deferred: true,
        },
        DockPolicyRequest::CloseSurface { surface },
    ];
    for operation in [DockDropOperation::TabMerge, DockDropOperation::EdgeSplit] {
        requests.push(DockPolicyRequest::Drop(DockDropPolicyRequest::new(
            operation,
            payload.clone(),
            target,
        )));
        assert_eq!(
            unlocked.evaluate_drop_facts(operation, &payload, target),
            PolicyDecision::Allow
        );
        assert_eq!(
            locked.evaluate_drop_facts(operation, &payload, target),
            PolicyDecision::Reject(PolicyRejection::ArrangementLocked)
        );
    }
    for presentation in [
        DockPresentationTarget::Tiled(surface),
        DockPresentationTarget::Contained(surface),
        DockPresentationTarget::Native(SurfaceId::new(2)),
    ] {
        requests.push(DockPolicyRequest::Present(
            DockPresentationPolicyRequest::new(
                DockPayloadPolicyFacts::opened(DockPayloadKind::Item, [ItemId::new(2)]),
                presentation,
            ),
        ));
    }
    for request in requests {
        assert_eq!(
            unlocked.evaluate(&request),
            PolicyDecision::Allow,
            "{request:?}"
        );
        assert_eq!(
            locked.evaluate(&request),
            PolicyDecision::Reject(PolicyRejection::ArrangementLocked),
            "{request:?}"
        );
    }
    assert_eq!(
        unlocked.evaluate_drag_source_facts(&payload),
        PolicyDecision::Allow
    );
    assert_eq!(
        locked.evaluate_drag_source_facts(&payload),
        PolicyDecision::Reject(PolicyRejection::ArrangementLocked)
    );
    assert!(unlocked.splitter_resize_is_allowed(Axis::Horizontal, surface));
    assert!(!locked.splitter_resize_is_allowed(Axis::Horizontal, surface));
    for presentation in [TearOffPresentation::Contained, TearOffPresentation::Native] {
        assert_eq!(unlocked.check_tear_off(presentation), Ok(()));
        assert_eq!(
            locked.check_tear_off(presentation),
            Err(PolicyRejection::ArrangementLocked)
        );
    }
    assert_eq!(
        locked.tab_bar_declaration(surface, None),
        unlocked.tab_bar_declaration(surface, None),
    );
    assert_eq!(
        locked.evaluate(&DockPolicyRequest::RaiseContained(
            DockContainedTransformPolicyRequest::new(payload, surface)
        )),
        PolicyDecision::Allow
    );
}

#[test]
fn effective_close_capabilities_preserve_raw_rules_and_restore_after_unlock() {
    let immediate = ItemId::new(1);
    let disabled = ItemId::new(2);
    let inherited = ItemId::new(3);
    let mut policy = DockPolicy::new();
    policy.set_close_capability(CloseCapability::DeferredAllowed);
    for (item, capability) in [
        (immediate, Some(CloseCapability::Immediate)),
        (disabled, Some(CloseCapability::Disabled)),
        (inherited, None),
    ] {
        let mut rule = DockItemRule::new();
        rule.set_close_capability(capability);
        policy.set_item_rule(item, rule);
    }
    let original = policy.clone();
    let unlocked = policy.snapshot(PolicyRevision::new(1));
    assert!(!unlocked.arrangement_locked());
    assert_eq!(
        unlocked.pane_close_capability(immediate),
        CloseCapability::Immediate
    );
    assert_eq!(
        unlocked.pane_close_capability(disabled),
        CloseCapability::Disabled
    );
    assert_eq!(
        unlocked.pane_close_capability(inherited),
        CloseCapability::DeferredAllowed
    );

    policy.set_arrangement_locked(true);
    let locked = policy.snapshot(PolicyRevision::new(2));
    assert!(locked.arrangement_locked());
    assert_eq!(locked.item_rules(), unlocked.item_rules());
    assert_eq!(
        policy.close_capability(),
        CloseCapability::DeferredAllowed,
        "locking changes effective permissions without rewriting the declared default"
    );
    assert_eq!(
        policy
            .item_rule(immediate)
            .expect("the explicit immediate rule remains declared")
            .close_capability(),
        Some(CloseCapability::Immediate)
    );
    assert_eq!(
        policy
            .item_rule(inherited)
            .expect("the inheriting item rule remains declared")
            .close_capability(),
        None
    );
    for item in [immediate, disabled, inherited, ItemId::new(4)] {
        assert_eq!(
            locked.pane_close_capability(item),
            CloseCapability::Disabled
        );
        assert_eq!(
            locked.evaluate(&DockPolicyRequest::ClosePane {
                item,
                deferred: false
            }),
            PolicyDecision::Reject(PolicyRejection::ArrangementLocked)
        );
    }
    policy.set_arrangement_locked(false);
    assert_eq!(policy, original);
    let restored = policy.snapshot(PolicyRevision::new(3));
    assert_eq!(
        restored.pane_close_capability(inherited),
        CloseCapability::DeferredAllowed
    );
    assert_eq!(
        restored.evaluate(&DockPolicyRequest::ClosePane {
            item: disabled,
            deferred: false
        }),
        PolicyDecision::Reject(PolicyRejection::PaneCloseDisabled { item: disabled })
    );
    assert_eq!(
        restored.evaluate(&DockPolicyRequest::ClosePane {
            item: immediate,
            deferred: true
        }),
        PolicyDecision::Reject(PolicyRejection::DeferredPaneCloseDisabled { item: immediate })
    );
    assert_eq!(
        restored.evaluate(&DockPolicyRequest::ClosePane {
            item: inherited,
            deferred: true
        }),
        PolicyDecision::Allow
    );
    assert_eq!(
        unlocked.pane_close_capability(immediate),
        CloseCapability::Immediate
    );
    assert_eq!(
        locked.pane_close_capability(immediate),
        CloseCapability::Disabled
    );
}

#[test]
fn recovery_ignores_arrangement_lock_but_retains_configured_rules() {
    let item = ItemId::new(1);
    let request = DockPolicyRequest::RecoverSurface(DockSurfaceRecoveryPolicyRequest::new(
        SurfaceId::new(1),
        SurfaceId::new(2),
        BTreeMap::from([(
            RootId::new(10),
            DockSurfaceRecoveryRootFacts::new(DockPresentationMode::Native, [item]),
        )]),
    ));
    let mut policy = DockPolicy::new();
    policy.set_arrangement_locked(true);
    assert_eq!(
        policy.snapshot(PolicyRevision::new(1)).evaluate(&request),
        PolicyDecision::Allow
    );

    let mut rule = DockItemRule::new();
    rule.set_source_enabled(false);
    policy.set_item_rule(item, rule);
    let locked = policy.snapshot(PolicyRevision::new(2));
    policy.set_arrangement_locked(false);
    let unlocked = policy.snapshot(PolicyRevision::new(3));
    assert_eq!(
        unlocked.evaluate(&request),
        PolicyDecision::Reject(PolicyRejection::ItemSourceDisabled { item })
    );
    assert_eq!(locked.evaluate(&request), unlocked.evaluate(&request));
}

#[cfg(feature = "serde")]
#[test]
fn arrangement_lock_roundtrips_explicit_values_and_rejects_missing_field() {
    let mut policy = DockPolicy::new();
    policy.set_close_capability(CloseCapability::DeferredAllowed);
    for locked in [false, true] {
        policy.set_arrangement_locked(locked);
        let snapshot = policy.snapshot(PolicyRevision::new(7));
        let mut encoded = serde_json::to_value(&snapshot).expect("policy snapshot serializes");
        let decoded: DockPolicySnapshot =
            serde_json::from_value(encoded.clone()).expect("explicit lock value deserializes");
        assert_eq!(decoded, snapshot);
        assert_eq!(decoded.arrangement_locked(), locked);

        let removed = encoded["policy"]["rules"]
            .as_object_mut()
            .expect("policy rules are an object")
            .remove("arrangement_locked");
        assert_eq!(removed, Some(serde_json::Value::Bool(locked)));
        let error = serde_json::from_value::<DockPolicySnapshot>(encoded)
            .expect_err("an absent lock field must not silently unlock the arrangement");
        assert!(
            error
                .to_string()
                .contains("missing field `arrangement_locked`"),
            "{error}"
        );
    }
}
