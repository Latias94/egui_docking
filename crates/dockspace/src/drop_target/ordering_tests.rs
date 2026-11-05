use slotmap::SlotMap;

use super::*;

// Frozen pre-optimization comparator: keep both ordering rules independent
// from the production helpers so the differential oracle cannot drift with them.
fn reference_cmp(left: &DropTargetId, right: &DropTargetId) -> Ordering {
    fn variant_order(id: DropTargetId) -> u8 {
        match id {
            DropTargetId::TabGap { .. } => 0,
            DropTargetId::Center { .. } => 1,
            DropTargetId::InnerEdge { .. } => 2,
            DropTargetId::OuterEdge { .. } => 3,
            DropTargetId::SurfaceBackground { .. } => 4,
        }
    }

    fn edge_order(edge: Edge) -> u8 {
        match edge {
            Edge::Left => 0,
            Edge::Right => 1,
            Edge::Top => 2,
            Edge::Bottom => 3,
        }
    }

    variant_order(*left)
        .cmp(&variant_order(*right))
        .then_with(|| match (*left, *right) {
            (
                DropTargetId::TabGap {
                    surface: left_surface,
                    root: left_root,
                    tabs: left_tabs,
                    index: left_index,
                },
                DropTargetId::TabGap {
                    surface: right_surface,
                    root: right_root,
                    tabs: right_tabs,
                    index: right_index,
                },
            ) => left_surface
                .cmp(&right_surface)
                .then(left_root.cmp(&right_root))
                .then(left_tabs.cmp(&right_tabs))
                .then(left_index.cmp(&right_index)),
            (
                DropTargetId::Center {
                    surface: left_surface,
                    root: left_root,
                    tabs: left_tabs,
                },
                DropTargetId::Center {
                    surface: right_surface,
                    root: right_root,
                    tabs: right_tabs,
                },
            ) => left_surface
                .cmp(&right_surface)
                .then(left_root.cmp(&right_root))
                .then(left_tabs.cmp(&right_tabs)),
            (
                DropTargetId::InnerEdge {
                    surface: left_surface,
                    root: left_root,
                    node: left_node,
                    edge: left_edge,
                },
                DropTargetId::InnerEdge {
                    surface: right_surface,
                    root: right_root,
                    node: right_node,
                    edge: right_edge,
                },
            )
            | (
                DropTargetId::OuterEdge {
                    surface: left_surface,
                    root: left_root,
                    node: left_node,
                    edge: left_edge,
                },
                DropTargetId::OuterEdge {
                    surface: right_surface,
                    root: right_root,
                    node: right_node,
                    edge: right_edge,
                },
            ) => left_surface
                .cmp(&right_surface)
                .then(left_root.cmp(&right_root))
                .then(left_node.cmp(&right_node))
                .then(edge_order(left_edge).cmp(&edge_order(right_edge))),
            (
                DropTargetId::SurfaceBackground {
                    surface: left_surface,
                },
                DropTargetId::SurfaceBackground {
                    surface: right_surface,
                },
            ) => left_surface.cmp(&right_surface),
            _ => Ordering::Equal,
        })
}

fn mixed_targets() -> Vec<DropTargetId> {
    let mut nodes = SlotMap::<NodeId, ()>::with_key();
    let removed = nodes.insert(());
    assert_eq!(nodes.remove(removed), Some(()));
    let replacement = nodes.insert(());
    let other = nodes.insert(());
    assert_ne!(removed, replacement);
    let nodes = [NodeId::default(), removed, replacement, other];
    let mut targets = Vec::new();
    for surface in [0, 1, u64::MAX].map(SurfaceId::new) {
        targets.push(DropTargetId::SurfaceBackground { surface });
        for root in [0, u64::MAX].map(RootId::new) {
            for node in nodes {
                targets.push(DropTargetId::Center {
                    surface,
                    root,
                    tabs: node,
                });
                for index in [0, 1, usize::MAX] {
                    targets.push(DropTargetId::TabGap {
                        surface,
                        root,
                        tabs: node,
                        index,
                    });
                }
                for edge in [Edge::Left, Edge::Right, Edge::Top, Edge::Bottom] {
                    targets.push(DropTargetId::InnerEdge {
                        surface,
                        root,
                        node,
                        edge,
                    });
                    targets.push(DropTargetId::OuterEdge {
                        surface,
                        root,
                        node,
                        edge,
                    });
                }
            }
        }
    }
    targets
}

#[test]
fn drop_target_order_matches_frozen_comparator_and_equality_for_every_field() {
    let targets = mixed_targets();
    for left in &targets {
        for right in &targets {
            let expected = reference_cmp(left, right);
            assert_eq!(left.cmp(right), expected, "{left:?} versus {right:?}");
            assert_eq!(left.partial_cmp(right), Some(expected));
            assert_eq!(expected.is_eq(), left == right);
            assert_eq!(right.cmp(left), expected.reverse());
        }
    }
}

#[test]
fn mixed_target_sort_and_duplicate_collapse_match_frozen_order() {
    let mut actual = mixed_targets();
    actual.extend_from_within(..);
    actual.rotate_left(17);
    actual.reverse();
    let mut expected = actual.clone();
    expected.sort_unstable_by(reference_cmp);
    actual.sort_unstable();
    assert_eq!(actual, expected);
    actual.dedup();
    expected.dedup();
    assert_eq!(actual, expected);
    assert!(actual.windows(2).all(|pair| pair[0] < pair[1]));
}
