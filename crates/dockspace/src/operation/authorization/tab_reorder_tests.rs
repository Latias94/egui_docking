use super::*;
use crate::graph::SurfacePresentation;
use crate::policy::{DockItemRule, DockPolicy, PolicyRejection, PolicyRevision};

const ROOT: RootId = RootId::new(1);
const OTHER_ROOT: RootId = RootId::new(2);
const BARRIER: ItemId = ItemId::new(3);

fn fixture() -> (Workspace, DockPolicySnapshot, NodeId, NodeId) {
    let mut builder = Workspace::builder();
    let tabs = builder.insert_node(Node::tabs((1..=5).map(ItemId::new)));
    let other = builder.insert_node(Node::tabs([ItemId::new(6)]));
    builder.set_root(ROOT, RootRecord::new(tabs));
    builder.set_root(OTHER_ROOT, RootRecord::new(other));
    builder.set_surface(SurfaceId::new(1), SurfacePresentation::with_main(ROOT));
    builder.set_surface(
        SurfaceId::new(2),
        SurfacePresentation::with_main(OTHER_ROOT),
    );
    let mut policy = DockPolicy::default();
    let mut rule = DockItemRule::new();
    rule.set_allow_tab_reorder(false);
    policy.set_item_rule(BARRIER, rule);
    (
        builder.build().unwrap(),
        policy.snapshot(PolicyRevision::default()),
        tabs,
        other,
    )
}

#[test]
fn locked_tabs_and_crossing_moves_share_one_rule_for_commands_and_drop_eligibility() {
    let (workspace, policy, tabs, _) = fixture();
    let original = workspace.clone();
    for from in 0..5 {
        for insertion_index in 0..=5 {
            let item = ItemId::new(u64::try_from(from + 1).unwrap());
            let source = workspace.capture_item_source(ROOT, tabs, item).unwrap();
            let target = DockTarget::TabGap {
                target: workspace.capture_tab_target(ROOT, tabs).unwrap(),
                index: insertion_index,
            };
            let payload = MovePayload::Item(source.clone());
            // A tab stays on its original side of the locked tab; the locked
            // tab itself permits only the two gaps which keep its index.
            let allowed = match from {
                0 | 1 => insertion_index <= 2,
                2 => (2..=3).contains(&insertion_index),
                _ => insertion_index >= 3,
            };
            let eligibility = DropCommandEligibility::new_validated(&workspace, &policy, &payload)
                .unwrap()
                .check_validated_target(&target);
            assert_eq!(
                eligibility.is_ok(),
                allowed,
                "from={from}, gap={insertion_index}"
            );
            for command in [
                WorkspaceCommand::Reorder {
                    source,
                    insertion_index,
                },
                WorkspaceCommand::Move { payload, target },
            ] {
                let result = prepare_transaction(&workspace, &policy, &[command]);
                assert_eq!(
                    result.is_ok(),
                    allowed,
                    "from={from}, gap={insertion_index}"
                );
                if !allowed {
                    assert!(matches!(
                        result.unwrap_err(),
                        TransactionError::Command {
                            source: CommandError::Policy(PolicyRejection::TabReorderDisabled {
                                item: BARRIER
                            }),
                            ..
                        }
                    ));
                }
            }
        }
    }
    assert_eq!(
        workspace, original,
        "preparing or rejecting a command never publishes it"
    );
}

#[test]
fn reorder_constraints_allow_undocking_and_new_items_before_the_barrier() {
    let (workspace, policy, tabs, other) = fixture();
    let moved = WorkspaceCommand::Move {
        payload: MovePayload::Item(workspace.capture_item_source(ROOT, tabs, BARRIER).unwrap()),
        target: DockTarget::Center(workspace.capture_tab_target(OTHER_ROOT, other).unwrap()),
    };
    let (candidate, report) = prepare_transaction(&workspace, &policy, &[moved])
        .unwrap()
        .into_parts();
    assert!(report.changed());
    assert_eq!(
        candidate
            .capture_item_source_by_id(BARRIER)
            .unwrap()
            .unwrap()
            .root(),
        OTHER_ROOT
    );

    let opened = WorkspaceCommand::Open {
        item: ItemId::new(7),
        target: DockTarget::TabGap {
            target: workspace.capture_tab_target(ROOT, tabs).unwrap(),
            index: 0,
        },
    };
    let (candidate, report) = prepare_transaction(&workspace, &policy, &[opened])
        .unwrap()
        .into_parts();
    assert!(report.changed());
    let Node::Tabs { items, .. } = candidate.node(tabs).unwrap() else {
        unreachable!()
    };
    assert_eq!(
        items,
        &vec![7, 1, 2, 3, 4, 5]
            .into_iter()
            .map(ItemId::new)
            .collect::<Vec<_>>()
    );
}
