use dockspace::graph::Axis;
use dockspace::ids::{ItemId, RootId, SurfaceId};
use dockspace::policy::{
    CloseCapability, DockClassId, DockDropOperation, DockDropPolicyRequest, DockDropTargetFacts,
    DockItemRule, DockPayloadKind, DockPayloadPolicyFacts, DockPolicy, DockPolicyRequest,
    DockPresentationMode, DockPresentationPolicyRequest, DockPresentationTarget,
    DockResizePolicyRequest, DockSourceRule, DockSurfaceRule, DockTabBarPolicyRequest,
    DockTargetRule, DockTargetRuleKey, PolicyDecision, PolicyRejection, PolicyRevision,
    PolicyRuleScope, TabBarInteraction, TabBarPolicy, TabBarVisibility,
};

const SOURCE_ROOT: RootId = RootId::new(1);
const SOURCE_SURFACE: SurfaceId = SurfaceId::new(10);
const TARGET_SURFACE: SurfaceId = SurfaceId::new(20);
const SOURCE_ITEM: ItemId = ItemId::new(100);
const PEER_ITEM: ItemId = ItemId::new(101);
const TARGET: DockTargetRuleKey = DockTargetRuleKey::Root(RootId::new(2));

fn payload(
    items: impl IntoIterator<Item = ItemId>,
    undocks_source: bool,
) -> DockPayloadPolicyFacts {
    DockPayloadPolicyFacts::new(
        DockPayloadKind::TabGroup,
        items,
        SOURCE_ROOT,
        SOURCE_SURFACE,
        DockPresentationMode::Tiled,
        undocks_source,
    )
}

fn drop_request(payload: DockPayloadPolicyFacts) -> DockPolicyRequest {
    DockPolicyRequest::Drop(DockDropPolicyRequest::new(
        DockDropOperation::TabMerge,
        payload,
        DockDropTargetFacts::new(TARGET_SURFACE, Some(TARGET), false),
    ))
}

#[test]
fn snapshot_freezes_rules_under_the_caller_owned_revision() {
    let mut policy = DockPolicy::default();
    let snapshot = policy.snapshot(PolicyRevision::new(7));

    policy.set_allow_tab_merge(false);

    assert_eq!(snapshot.revision(), PolicyRevision::new(7));
    assert_eq!(
        snapshot.evaluate(&drop_request(payload([SOURCE_ITEM], false))),
        PolicyDecision::Allow
    );
    assert_eq!(
        policy
            .snapshot(PolicyRevision::new(8))
            .evaluate(&drop_request(payload([SOURCE_ITEM], false))),
        PolicyDecision::Reject(PolicyRejection::TabMergeDisabled)
    );
    assert_eq!(
        PolicyRevision::new(u64::MAX).checked_next(),
        None,
        "the policy module must not invent a wrapping or global revision clock"
    );
}

#[test]
fn evaluator_applies_item_source_target_and_surface_rules_without_fallthrough() {
    let editor = DockClassId::new(1);
    let inspector = DockClassId::new(2);
    let mut policy = DockPolicy::default();

    let mut source_item = DockItemRule::default();
    source_item.set_dock_class(Some(editor));
    let _ = policy.set_item_rule(SOURCE_ITEM, source_item);

    let mut peer_item = DockItemRule::default();
    peer_item.set_dock_class(Some(inspector));
    let _ = policy.set_item_rule(PEER_ITEM, peer_item);

    let mut target = DockTargetRule::default();
    target.set_accepted_classes([editor], false);
    let _ = policy.set_target_rule(TARGET, target);

    let snapshot = policy.snapshot(PolicyRevision::new(1));
    assert_eq!(
        snapshot.evaluate(&drop_request(payload([SOURCE_ITEM], false))),
        PolicyDecision::Allow
    );
    assert_eq!(
        snapshot.evaluate(&drop_request(payload([SOURCE_ITEM, PEER_ITEM], false))),
        PolicyDecision::Reject(PolicyRejection::DockClassRejected {
            item: PEER_ITEM,
            class: Some(inspector),
            scope: PolicyRuleScope::Target(TARGET),
        })
    );

    let mut source = DockSourceRule::default();
    source.set_allow_undocking(false);
    let _ = policy.set_source_rule(SOURCE_ROOT, source);
    let snapshot = policy.snapshot(PolicyRevision::new(2));
    assert_eq!(
        snapshot.evaluate(&drop_request(payload([SOURCE_ITEM], true))),
        PolicyDecision::Reject(PolicyRejection::SourceUndockingDisabled { root: SOURCE_ROOT })
    );

    let mut surface = DockSurfaceRule::default();
    surface.set_accepted_classes([inspector], false);
    let _ = policy.set_surface_rule(TARGET_SURFACE, surface);
    let snapshot = policy.snapshot(PolicyRevision::new(3));
    assert_eq!(
        snapshot.evaluate(&drop_request(payload([SOURCE_ITEM], false))),
        PolicyDecision::Reject(PolicyRejection::DockClassRejected {
            item: SOURCE_ITEM,
            class: Some(editor),
            scope: PolicyRuleScope::Surface(TARGET_SURFACE),
        }),
        "the exact surface rejection remains the sole result; policy never tries another target"
    );
}

#[test]
fn axis_tab_close_and_presentation_permissions_are_independent() {
    let mut policy = DockPolicy::default();
    policy.set_allow_resize_axis(Axis::Vertical, false);
    policy.set_close_capability(CloseCapability::DeferredAllowed);
    policy.set_allow_native_surfaces(true);

    let mut item = DockItemRule::default();
    item.set_close_capability(Some(CloseCapability::Immediate));
    let _ = policy.set_item_rule(SOURCE_ITEM, item);

    let mut target = DockTargetRule::default();
    target.set_tab_bar(TabBarPolicy::new(
        TabBarVisibility::Visible,
        TabBarInteraction::Disabled,
    ));
    let _ = policy.set_target_rule(TARGET, target);

    let mut surface = DockSurfaceRule::default();
    surface.set_resize_axis(Axis::Horizontal, false);
    surface.set_allowed_presentations([DockPresentationMode::Tiled]);
    surface.set_close_enabled(false);
    let _ = policy.set_surface_rule(TARGET_SURFACE, surface);

    let snapshot = policy.snapshot(PolicyRevision::new(9));
    assert_eq!(
        snapshot.evaluate(&DockPolicyRequest::Resize(DockResizePolicyRequest::new(
            Axis::Vertical,
            Some(TARGET_SURFACE),
        ))),
        PolicyDecision::Reject(PolicyRejection::ResizeAxisDisabled {
            axis: Axis::Vertical,
        })
    );
    assert_eq!(
        snapshot.evaluate(&DockPolicyRequest::Resize(DockResizePolicyRequest::new(
            Axis::Horizontal,
            Some(TARGET_SURFACE),
        ))),
        PolicyDecision::Reject(PolicyRejection::SurfaceResizeAxisDisabled {
            surface: TARGET_SURFACE,
            axis: Axis::Horizontal,
        })
    );
    assert_eq!(
        snapshot.evaluate(&DockPolicyRequest::InteractWithTabBar(
            DockTabBarPolicyRequest::new(TARGET_SURFACE, Some(TARGET)),
        )),
        PolicyDecision::Reject(PolicyRejection::TabBarInteractionDisabled)
    );
    assert_eq!(
        snapshot.evaluate(&DockPolicyRequest::ClosePane {
            item: SOURCE_ITEM,
            deferred: true,
        }),
        PolicyDecision::Reject(PolicyRejection::DeferredPaneCloseDisabled { item: SOURCE_ITEM })
    );
    assert_eq!(
        snapshot.evaluate(&DockPolicyRequest::ClosePane {
            item: PEER_ITEM,
            deferred: true,
        }),
        PolicyDecision::Allow
    );
    assert_eq!(
        snapshot.evaluate(&DockPolicyRequest::CloseSurface {
            surface: TARGET_SURFACE,
        }),
        PolicyDecision::Reject(PolicyRejection::SurfaceCloseDisabled {
            surface: TARGET_SURFACE,
        })
    );
    assert_eq!(
        snapshot.evaluate(&DockPolicyRequest::Present(
            DockPresentationPolicyRequest::new(
                payload([SOURCE_ITEM], true),
                DockPresentationTarget::Contained(TARGET_SURFACE),
            ),
        )),
        PolicyDecision::Reject(PolicyRejection::SurfacePresentationModeRejected {
            surface: TARGET_SURFACE,
            mode: DockPresentationMode::Contained,
        })
    );
    assert_eq!(
        snapshot.evaluate(&DockPolicyRequest::Present(
            DockPresentationPolicyRequest::new(
                payload([SOURCE_ITEM], true),
                DockPresentationTarget::Native(SurfaceId::new(30)),
            ),
        )),
        PolicyDecision::Allow,
        "native permission is independent of one contained target surface"
    );
}

#[cfg(feature = "serde")]
#[test]
fn snapshot_is_a_serializable_value_without_runtime_callbacks() {
    let mut policy = DockPolicy::default();
    let mut item = DockItemRule::default();
    item.set_dock_class(Some(DockClassId::new(44)));
    let _ = policy.set_item_rule(SOURCE_ITEM, item);
    let snapshot = policy.snapshot(PolicyRevision::new(12));

    let encoded = serde_json::to_string(&snapshot).expect("policy snapshot must serialize");
    let decoded = serde_json::from_str(&encoded).expect("policy snapshot must deserialize");

    assert_eq!(snapshot, decoded);
}
