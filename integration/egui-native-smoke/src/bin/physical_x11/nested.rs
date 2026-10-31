//! Fixed three-leaf fixture and exact durable-model checks for nested X11 targets.

use dockspace::model::{DockspaceNodeView, DockspaceSplitView};

use super::super::assertions::NestedPaneCase;
use super::{
    CHILD_ITEM, DockspaceAxis, DockspaceLayout, DockspaceNode, DockspaceRootLayout,
    DockspaceSurfaceLayout, DockspaceView, ItemId, MAIN_ITEM, MAIN_ROOT, ROOT_SURFACE,
    RedockLayout,
};

pub(super) const SIBLING_ITEM: ItemId = ItemId::new(3);
const OUTER_WEIGHTS: [f32; 2] = [0.25, 0.75];
const INNER_WEIGHTS: [f32; 2] = [0.375, 0.625];

pub(super) fn smoke_layout() -> DockspaceLayout {
    layout_with_target(
        DockspaceNode::central_tabs([CHILD_ITEM]),
        DockspaceNode::tabs([MAIN_ITEM]),
    )
}

fn layout_with_target(source: DockspaceNode, target: DockspaceNode) -> DockspaceLayout {
    let inner = DockspaceNode::split(
        DockspaceAxis::Vertical,
        [
            (DockspaceNode::tabs([SIBLING_ITEM]), INNER_WEIGHTS[0]),
            (target, INNER_WEIGHTS[1]),
        ],
    )
    .expect("the fixed nested vertical split validates");
    let outer = DockspaceNode::split(
        DockspaceAxis::Horizontal,
        [(source, OUTER_WEIGHTS[0]), (inner, OUTER_WEIGHTS[1])],
    )
    .expect("the fixed nested horizontal split validates");
    DockspaceLayout::new([DockspaceSurfaceLayout::new(
        ROOT_SURFACE,
        DockspaceRootLayout::new(MAIN_ROOT, outer),
    )])
    .expect("the fixed nested physical layout validates")
}

pub(super) fn observed_redock_layout(view: DockspaceView<'_>) -> Option<RedockLayout> {
    for id in [MAIN_ITEM, CHILD_ITEM, SIBLING_ITEM] {
        let item = view.item(id)?;
        if item.surface() != ROOT_SURFACE || item.root() != MAIN_ROOT || item.contained().is_some()
        {
            return None;
        }
    }
    if view.surfaces().count() != 1 {
        return None;
    }
    let surface = view.surface(ROOT_SURFACE)?;
    if surface.contained_count() != 0 {
        return None;
    }
    let root = surface.main_root()?;
    if root.id() != MAIN_ROOT {
        return None;
    }
    let [source, inner] = binary_children(
        root.content()?.split()?,
        DockspaceAxis::Horizontal,
        OUTER_WEIGHTS,
    )?;
    let source = source.tabs()?;
    // Keeping this empty central source prevents tear-off from collapsing either split.
    if !source.is_central() || !source.items().is_empty() || source.selected().is_some() {
        return None;
    }
    let [sibling, target] =
        binary_children(inner.split()?, DockspaceAxis::Vertical, INNER_WEIGHTS)?;
    if !singleton(sibling, SIBLING_ITEM) {
        return None;
    }
    if let Some(tabs) = target.tabs() {
        return (!tabs.is_central()
            && tabs.items() == [MAIN_ITEM, CHILD_ITEM]
            && tabs.selected() == Some(CHILD_ITEM))
        .then_some(RedockLayout::Nested(NestedPaneCase::Center));
    }
    let [child, main] = binary_children(target.split()?, DockspaceAxis::Horizontal, [0.5, 0.5])?;
    (singleton(child, CHILD_ITEM) && singleton(main, MAIN_ITEM))
        .then_some(RedockLayout::Nested(NestedPaneCase::West))
}

fn singleton(node: DockspaceNodeView<'_>, item: ItemId) -> bool {
    node.tabs().is_some_and(|tabs| {
        !tabs.is_central() && tabs.items() == [item] && tabs.selected() == Some(item)
    })
}

fn binary_children(
    split: DockspaceSplitView<'_>,
    axis: DockspaceAxis,
    weights: [f32; 2],
) -> Option<[DockspaceNodeView<'_>; 2]> {
    if split.axis() != axis
        || split.child_count() != 2
        || split
            .weights()
            .zip(weights)
            .any(|(actual, expected)| (actual - expected).abs() > f32::EPSILON)
    {
        return None;
    }
    let mut children = split.children();
    Some([children.next()?, children.next()?])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn merged_target(items: [ItemId; 2]) -> DockspaceNode {
        DockspaceNode::tabs_with_selection(items, Some(CHILD_ITEM)).unwrap()
    }

    fn edge_target(items: [ItemId; 2]) -> DockspaceNode {
        DockspaceNode::equal_split(
            DockspaceAxis::Horizontal,
            items.map(|item| DockspaceNode::tabs([item])),
        )
        .unwrap()
    }

    #[test]
    fn nested_fixture_keeps_main_below_two_different_split_axes() {
        let layout = smoke_layout();
        let view = layout.view();
        let [source, inner] = binary_children(
            view.root(MAIN_ROOT)
                .unwrap()
                .content()
                .unwrap()
                .split()
                .unwrap(),
            DockspaceAxis::Horizontal,
            OUTER_WEIGHTS,
        )
        .unwrap();
        assert!(source.is_central());
        assert_eq!(source.tabs().unwrap().items(), [CHILD_ITEM]);
        assert_eq!(source.tabs().unwrap().selected(), Some(CHILD_ITEM));
        let [sibling, main] = binary_children(
            inner.split().unwrap(),
            DockspaceAxis::Vertical,
            INNER_WEIGHTS,
        )
        .unwrap();
        assert!(singleton(sibling, SIBLING_ITEM));
        assert!(singleton(main, MAIN_ITEM));
        assert_eq!(observed_redock_layout(view), None);
    }

    #[test]
    fn nested_model_requires_exact_center_order_and_inner_west_payload() {
        for (target, expected) in [
            (
                merged_target([MAIN_ITEM, CHILD_ITEM]),
                NestedPaneCase::Center,
            ),
            (edge_target([CHILD_ITEM, MAIN_ITEM]), NestedPaneCase::West),
        ] {
            let layout = layout_with_target(DockspaceNode::central_tabs([]), target);
            assert_eq!(
                observed_redock_layout(layout.view()),
                Some(RedockLayout::Nested(expected))
            );
        }
        for wrong in [
            merged_target([CHILD_ITEM, MAIN_ITEM]),
            edge_target([MAIN_ITEM, CHILD_ITEM]),
            DockspaceNode::split(
                DockspaceAxis::Horizontal,
                [
                    (DockspaceNode::tabs([CHILD_ITEM]), 0.4),
                    (DockspaceNode::tabs([MAIN_ITEM]), 0.6),
                ],
            )
            .unwrap(),
        ] {
            let layout = layout_with_target(DockspaceNode::central_tabs([]), wrong);
            assert_eq!(observed_redock_layout(layout.view()), None);
        }
    }

    #[test]
    fn nested_model_rejects_outer_root_edge_and_changed_ancestor_weights() {
        let sibling_main = DockspaceNode::split(
            DockspaceAxis::Vertical,
            [
                (DockspaceNode::tabs([SIBLING_ITEM]), INNER_WEIGHTS[0]),
                (DockspaceNode::tabs([MAIN_ITEM]), INNER_WEIGHTS[1]),
            ],
        )
        .unwrap();
        let outer_edge = DockspaceNode::split(
            DockspaceAxis::Horizontal,
            [
                (DockspaceNode::tabs([CHILD_ITEM]), 0.5),
                (DockspaceNode::central_tabs([]), 0.125),
                (sibling_main, 0.375),
            ],
        )
        .unwrap();
        let changed_weights = DockspaceNode::equal_split(
            DockspaceAxis::Horizontal,
            [
                DockspaceNode::central_tabs([]),
                DockspaceNode::split(
                    DockspaceAxis::Vertical,
                    [
                        (DockspaceNode::tabs([SIBLING_ITEM]), INNER_WEIGHTS[0]),
                        (merged_target([MAIN_ITEM, CHILD_ITEM]), INNER_WEIGHTS[1]),
                    ],
                )
                .unwrap(),
            ],
        )
        .unwrap();
        for wrong in [outer_edge, changed_weights] {
            let layout = DockspaceLayout::new([DockspaceSurfaceLayout::new(
                ROOT_SURFACE,
                DockspaceRootLayout::new(MAIN_ROOT, wrong),
            )])
            .unwrap();
            assert_eq!(observed_redock_layout(layout.view()), None);
        }
    }

    #[test]
    fn nested_model_rejects_changed_inner_weights_and_sibling_order() {
        let changed_inner_weights = DockspaceNode::equal_split(
            DockspaceAxis::Vertical,
            [
                DockspaceNode::tabs([SIBLING_ITEM]),
                merged_target([MAIN_ITEM, CHILD_ITEM]),
            ],
        )
        .unwrap();
        let swapped_sibling = DockspaceNode::split(
            DockspaceAxis::Vertical,
            [
                (merged_target([MAIN_ITEM, CHILD_ITEM]), INNER_WEIGHTS[0]),
                (DockspaceNode::tabs([SIBLING_ITEM]), INNER_WEIGHTS[1]),
            ],
        )
        .unwrap();
        for inner in [changed_inner_weights, swapped_sibling] {
            let outer = DockspaceNode::split(
                DockspaceAxis::Horizontal,
                [
                    (DockspaceNode::central_tabs([]), OUTER_WEIGHTS[0]),
                    (inner, OUTER_WEIGHTS[1]),
                ],
            )
            .unwrap();
            let layout = DockspaceLayout::new([DockspaceSurfaceLayout::new(
                ROOT_SURFACE,
                DockspaceRootLayout::new(MAIN_ROOT, outer),
            )])
            .unwrap();
            assert_eq!(observed_redock_layout(layout.view()), None);
        }
    }
}
