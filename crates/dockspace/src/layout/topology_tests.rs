use super::*;
use crate::graph::{RootRecord, SurfacePresentation, WorkspaceBuilder};
use crate::ids::{ItemId, SurfaceId};

const ROOT: RootId = RootId::new(1);

fn next(seed: &mut u64) -> u64 {
    *seed = seed.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1);
    *seed
}

fn tree(
    builder: &mut WorkspaceBuilder,
    seed: &mut u64,
    depth: usize,
    axis: Axis,
    leaves: &mut Vec<NodeId>,
) -> NodeId {
    if depth == 0 {
        let item = ItemId::new(u64::try_from(leaves.len() + 1).unwrap());
        let leaf = builder.insert_node(Node::tabs([item]));
        leaves.push(leaf);
        return leaf;
    }
    let count = usize::try_from(next(seed) % 3 + 2).unwrap();
    let next_axis = match axis {
        Axis::Horizontal => Axis::Vertical,
        Axis::Vertical => Axis::Horizontal,
    };
    let children = (0..count)
        .map(|_| tree(builder, seed, depth - 1, next_axis, leaves))
        .collect::<Vec<_>>();
    let weights = (0..count)
        .map(|_| u16::try_from(next(seed) % 17 + 1).unwrap())
        .map(f32::from)
        .collect::<Vec<_>>();
    builder.insert_node(Node::split(axis, children, weights).unwrap())
}

fn fixture(seed: &mut u64, central: bool) -> (Workspace, Vec<NodeId>) {
    let mut builder = Workspace::builder();
    let mut leaves = Vec::new();
    let node = tree(&mut builder, seed, 3, Axis::Horizontal, &mut leaves);
    let mut record = RootRecord::new(node);
    if central {
        let row = usize::try_from(next(seed) % u64::try_from(leaves.len()).unwrap()).unwrap();
        record = record.with_central(leaves[row]);
    }
    builder.set_root(ROOT, record);
    builder.set_surface(SurfaceId::new(1), SurfacePresentation::with_main(ROOT));
    (builder.build().unwrap(), leaves)
}

proptest::proptest! {
    #[test]
    fn caller_owned_axis_matches_frozen_solver(
        mut seed in proptest::prelude::any::<u64>(),
        count in 0_usize..16,
        has_central in proptest::prelude::any::<bool>(),
        extent in 0_u16..8192,
    ) {
        let weights = (0..count).map(|_| {
            f64::from(u16::try_from(next(&mut seed) % 65).unwrap()) * 0.125
        }).collect::<Vec<_>>();
        let constraints = (0..count).map(|_| {
            let minimum = f64::from(u16::try_from(next(&mut seed) % 129).unwrap()) * 0.125;
            let extra = f64::from(u16::try_from(next(&mut seed) % 1025).unwrap()) * 0.125;
            AxisConstraint::new(minimum, minimum + extra).unwrap()
        }).collect::<Vec<_>>();
        let central = if has_central && count > 0 {
            Some(usize::try_from(next(&mut seed) % u64::try_from(count).unwrap()).unwrap())
        } else {
            None
        };
        let extent = f64::from(extent) * 0.125;
        let expected = reference::solve_axis(extent, &weights, &constraints, central);
        let actual = solve_axis(extent, &weights, &constraints, central);
        proptest::prop_assert_eq!(&actual, &expected);
        if let (Ok(actual), Ok(expected)) = (actual, expected) {
            proptest::prop_assert_eq!(
                actual.sizes.iter().map(|value| value.to_bits()).collect::<Vec<_>>(),
                expected.sizes.iter().map(|value| value.to_bits()).collect::<Vec<_>>()
            );
            proptest::prop_assert_eq!(actual.overflow.to_bits(), expected.overflow.to_bits());
            proptest::prop_assert_eq!(actual.unallocated.to_bits(), expected.unallocated.to_bits());
        }
    }

    #[test]
    fn dense_layout_matches_frozen_traversal(
        mut seed in proptest::prelude::any::<u64>(),
        central in proptest::prelude::any::<bool>(),
        width in 0_u16..4096,
        height in 0_u16..4096,
        thickness in 0_u8..32,
    ) {
        let (workspace, leaves) = fixture(&mut seed, central);
        let constraints = leaves.iter().map(|&node| {
            let x = f64::from(u16::try_from(next(&mut seed) % 64).unwrap());
            let y = f64::from(u16::try_from(next(&mut seed) % 64).unwrap());
            let extra_x = f64::from(u16::try_from(next(&mut seed) % 512).unwrap());
            let extra_y = f64::from(u16::try_from(next(&mut seed) % 512).unwrap());
            (node, Constraints::new(LogicalSize::new(x, y).unwrap(), LogicalSize::new(x + extra_x, y + extra_y).unwrap()).unwrap())
        }).collect::<BTreeMap<_, _>>();
        let bounds = LogicalRect::new(-17.25, 31.5, f64::from(width) * 0.25, f64::from(height) * 0.25).unwrap();
        let metrics = LayoutMetrics::new(f64::from(thickness) * 0.125).unwrap();
        let expected = reference::project(&workspace, ROOT, bounds, LeafMeasurements::Constraints(&constraints), metrics, &[]);
        let actual = project_root(&workspace, ROOT, bounds, |node| constraints.get(&node).copied(), metrics);
        proptest::prop_assert_eq!(reference::capture_result(&actual), expected);

        let minimums = constraints.iter().map(|(&id, value)| (id, value.min())).collect::<BTreeMap<_, _>>();
        let expected = reference::project(&workspace, ROOT, bounds, LeafMeasurements::Minimums(&minimums), metrics, &[]);
        let version = WorkspaceVersion::initial();
        let index = WorkspaceIndex::build(&workspace, version).unwrap();
        let before = index.clone();
        for _ in 0..2 {
            let actual = measure_root_minimums(&workspace, ROOT, &minimums, metrics)
                .and_then(|measured| measured.project(bounds, &[]));
            proptest::prop_assert_eq!(&reference::capture_result(&actual), &expected);
        }
        proptest::prop_assert_eq!(index, before);
    }
}

#[test]
fn dense_projection_keeps_identity_order_and_owned_old_outputs() {
    let (workspace, leaves) = fixture(&mut 37, true);
    let version = WorkspaceVersion::initial();
    let index = WorkspaceIndex::build(&workspace, version).unwrap();
    let minimums = leaves
        .iter()
        .map(|&node| (node, LogicalSize::new(4.0, 3.0).unwrap()))
        .collect::<BTreeMap<_, _>>();
    let bounds = LogicalRect::new(-17.25, 31.5, 1000.0, 900.0).unwrap();
    let first = measure_root_minimums(
        &workspace,
        ROOT,
        &minimums,
        LayoutMetrics::new(1.0).unwrap(),
    )
    .unwrap()
    .project(bounds, &[])
    .unwrap();
    let retained = reference::capture(&first);
    let cloned = first.clone();
    let nodes = &cloned.nodes;
    let retained_preorder = first
        .nodes
        .iter_preorder()
        .map(|(&node, &result)| (node, result))
        .collect::<Vec<_>>();
    let expected_keys = retained.node_rects.keys().copied().collect::<Vec<_>>();
    assert_eq!(
        first.nodes.keys().copied().collect::<Vec<_>>(),
        expected_keys
    );
    assert_eq!(
        first
            .nodes
            .iter()
            .rev()
            .map(|(&node, _)| node)
            .collect::<Vec<_>>(),
        expected_keys.iter().rev().copied().collect::<Vec<_>>()
    );
    assert_eq!(
        first.splits.keys().copied().collect::<Vec<_>>(),
        retained.splits.keys().copied().collect::<Vec<_>>()
    );
    let second = measure_root_minimums(
        &workspace,
        ROOT,
        &minimums,
        LayoutMetrics::new(3.0).unwrap(),
    )
    .unwrap()
    .project(LogicalRect::new(8.0, 2.0, 250.0, 200.0).unwrap(), &[])
    .unwrap();
    assert_ne!(first, second);
    drop(index);
    drop(workspace);
    assert_eq!(reference::capture(&first), retained);
    drop(first);
    assert_eq!(reference::capture(&cloned), retained);
    assert_eq!(
        nodes
            .iter()
            .map(|(&node, result)| (node, result.rect))
            .collect::<BTreeMap<_, _>>(),
        retained.node_rects
    );
    assert_eq!(
        nodes
            .iter()
            .map(|(&node, result)| (node, result.constraints))
            .collect::<BTreeMap<_, _>>(),
        retained.node_constraints
    );
    assert_eq!(
        nodes
            .iter_preorder()
            .map(|(&node, &result)| (node, result))
            .collect::<Vec<_>>(),
        retained_preorder
    );
    assert_eq!(nodes.values().len(), nodes.len());
    assert_eq!(cloned.splits.values().len(), cloned.splits.len());
}

#[test]
fn descendant_geometry_error_precedes_parent_gap_geometry_error() {
    for (height, component) in [(0.0, "x"), (f64::MAX * 0.5, "y")] {
        let mut builder = Workspace::builder();
        let leaves = [1, 2, 3].map(|item| builder.insert_node(Node::tabs([ItemId::new(item)])));
        let nested =
            builder.insert_node(Node::equal_split(Axis::Vertical, [leaves[0], leaves[1]]).unwrap());
        let root =
            builder.insert_node(Node::equal_split(Axis::Horizontal, [nested, leaves[2]]).unwrap());
        builder.set_root(ROOT, RootRecord::new(root));
        builder.set_surface(SurfaceId::new(1), SurfacePresentation::with_main(ROOT));
        let workspace = builder.build().unwrap();
        let extent = f64::MAX * 0.2;
        let first = LogicalSize::new(extent, height).unwrap();
        let zero = LogicalSize::new(0.0, 0.0).unwrap();
        let constraints = BTreeMap::from([
            (leaves[0], Constraints::new(first, first).unwrap()),
            (
                leaves[1],
                Constraints::new(zero, LogicalSize::new(extent, 0.0).unwrap()).unwrap(),
            ),
            (
                leaves[2],
                Constraints::new(zero, LogicalSize::new(0.0, f64::MAX * 0.75).unwrap()).unwrap(),
            ),
        ]);
        let bounds = LogicalRect::new(f64::MAX * 0.75, f64::MAX * 0.75, extent, 0.0).unwrap();
        let metrics = LayoutMetrics::new(extent).unwrap();
        let expected = LayoutError::Geometry(GeometryError::NonFinite {
            component,
            value: f64::INFINITY,
        });
        // The root gap overflows x; a descendant's overflowing y must win first.
        let actual = project_root(
            &workspace,
            ROOT,
            bounds,
            |node| constraints.get(&node).copied(),
            metrics,
        );
        assert_eq!(actual, Err(expected.clone()));
        assert_eq!(
            reference::project(
                &workspace,
                ROOT,
                bounds,
                LeafMeasurements::Constraints(&constraints),
                metrics,
                &[],
            ),
            Err(expected)
        );
    }
}

#[test]
fn sparse_arena_projection_rejects_retired_generations() {
    let mut draft = Workspace::builder();
    let retired = (1..=512)
        .map(|item| draft.insert_node(Node::tabs([ItemId::new(item)])))
        .collect::<Vec<_>>();
    for &node in &retired {
        draft.draft_mut().remove_runtime_node(node).unwrap();
    }
    let leaf = draft.insert_node(Node::tabs([ItemId::new(1024)]));
    draft.set_root(ROOT, RootRecord::new(leaf));
    draft.set_surface(SurfaceId::new(1), SurfacePresentation::with_main(ROOT));
    let workspace = draft.build().unwrap();
    let bounds = LogicalRect::new(1.0, 2.0, 10.0, 20.0).unwrap();
    let constraints = BTreeMap::from([(
        leaf,
        Constraints::new(LogicalSize::new(0.0, 0.0).unwrap(), bounds.size()).unwrap(),
    )]);
    let projection = project_root(
        &workspace,
        ROOT,
        bounds,
        |node| constraints.get(&node).copied(),
        LayoutMetrics::new(0.0).unwrap(),
    )
    .unwrap();
    assert_eq!(projection.nodes.len(), 1);
    assert!(projection.splits.is_empty());
    assert_eq!(projection.nodes.keys().copied().collect::<Vec<_>>(), [leaf]);
    for stale in retired {
        assert_ne!(stale, leaf);
        assert!(!projection.nodes.contains_key(&stale));
        assert_eq!(projection.nodes.get(&stale), None);
        assert_eq!(projection.splits.get(&stale), None);
    }
    drop(workspace);
    assert_eq!(
        projection.nodes.get(&leaf).map(|result| result.rect),
        Some(bounds)
    );
}

#[test]
fn sparse_recycled_tree_keeps_ordered_identity_and_cold_warm_oracle_results() {
    let mut builder = Workspace::builder();
    let original = (1..=512)
        .map(|item| builder.insert_node(Node::tabs([ItemId::new(item)])))
        .collect::<Vec<_>>();
    let kept = [original[0], original[17], original[500]];
    let retired = original
        .into_iter()
        .filter(|node| !kept.contains(node))
        .collect::<Vec<_>>();
    for &node in &retired {
        builder.draft_mut().remove_runtime_node(node).unwrap();
    }
    let recycled = [1001, 1002].map(|item| builder.insert_node(Node::tabs([ItemId::new(item)])));
    let nested = builder
        .insert_node(Node::split(Axis::Vertical, [kept[1], recycled[0]], [0.375, 0.625]).unwrap());
    let root_node = builder.insert_node(
        Node::split(
            Axis::Horizontal,
            [recycled[1], nested, kept[0]],
            [0.2, 0.5, 0.3],
        )
        .unwrap(),
    );
    builder.set_root(ROOT, RootRecord::new(root_node).with_central(kept[0]));
    builder.set_surface(SurfaceId::new(1), SurfacePresentation::with_main(ROOT));
    let other_root = RootId::new(2);
    builder.set_root(other_root, RootRecord::new(kept[2]));
    builder.set_surface(
        SurfaceId::new(2),
        SurfacePresentation::with_main(other_root),
    );
    let workspace = builder.build().unwrap();
    let preorder = [
        root_node,
        recycled[1],
        nested,
        kept[1],
        recycled[0],
        kept[0],
    ];
    let mut ordered = preorder.to_vec();
    ordered.sort();
    assert_ne!(preorder.as_slice(), ordered.as_slice());
    let leaves = [kept[1], recycled[0], kept[0], recycled[1], kept[2]];
    let constraints = leaves
        .into_iter()
        .enumerate()
        .map(|(index, node)| {
            let minimum = f64::from(u32::try_from(index + 1).unwrap());
            (
                node,
                Constraints::new(
                    LogicalSize::new(minimum, minimum * 0.5).unwrap(),
                    LogicalSize::new(512.0 + minimum, 256.0 + minimum).unwrap(),
                )
                .unwrap(),
            )
        })
        .collect::<BTreeMap<_, _>>();
    let metrics = LayoutMetrics::new(3.5).unwrap();
    let bounds = LogicalRect::new(-31.25, 18.5, 400.0, 200.0).unwrap();
    let leaf_order = [recycled[1], kept[1], recycled[0], kept[0]];
    let answers = leaf_order
        .into_iter()
        .map(|node| (node, constraints[&node]))
        .collect::<Vec<_>>();
    let mut answers = answers.into_iter().peekable();
    let mut queries = Vec::new();
    let cold = project_root(
        &workspace,
        ROOT,
        bounds,
        |node| {
            queries.push(node);
            answers
                .next_if(|(id, _)| *id == node)
                .map(|(_, constraints)| constraints)
        },
        metrics,
    )
    .unwrap();
    assert_eq!(queries, leaf_order);
    assert!(answers.next().is_none());
    assert_eq!(
        reference::capture(&cold),
        reference::project(
            &workspace,
            ROOT,
            bounds,
            LeafMeasurements::Constraints(&constraints),
            metrics,
            &[],
        )
        .unwrap()
    );
    assert_eq!(cold.nodes.keys().copied().collect::<Vec<_>>(), ordered);
    let frozen = reference::capture(&cold);
    let mut nodes = cold.nodes.iter_preorder();
    assert_eq!(nodes.len(), preorder.len());
    assert_eq!(nodes.size_hint(), (preorder.len(), Some(preorder.len())));
    for (index, expected) in preorder.iter().enumerate() {
        let (node, result) = nodes.next().unwrap();
        assert_eq!(node, expected);
        assert_eq!(result.rect, frozen.node_rects[expected]);
        assert_eq!(nodes.len(), preorder.len() - index - 1);
    }
    assert!(nodes.next().is_none());
    assert!(nodes.next_back().is_none());
    assert!(nodes.next().is_none());
    let mut mixed = cold
        .nodes
        .iter_preorder()
        .map(|(node, result)| (node, &result.constraints));
    assert_eq!(
        mixed.next(),
        Some((&root_node, &frozen.node_constraints[&root_node]))
    );
    assert_eq!(
        mixed.next_back(),
        Some((&kept[0], &frozen.node_constraints[&kept[0]]))
    );
    assert_eq!(
        mixed.map(|(&node, _)| node).collect::<Vec<_>>(),
        preorder[1..preorder.len() - 1]
    );
    assert_eq!(cold.nodes.len(), preorder.len());
    assert_eq!(cold.splits.len(), 2);
    assert!(!cold.nodes.contains_key(&kept[2]));
    for stale in retired {
        assert!(!cold.nodes.contains_key(&stale));
    }
    let minimums = constraints
        .iter()
        .map(|(&node, constraint)| (node, constraint.min()))
        .collect::<BTreeMap<_, _>>();
    let expected = reference::project(
        &workspace,
        ROOT,
        bounds,
        LeafMeasurements::Minimums(&minimums),
        metrics,
        &[],
    )
    .unwrap();
    let mut previous = None;
    for _ in 0..2 {
        let measured = measure_root_minimums(&workspace, ROOT, &minimums, metrics).unwrap();
        if let Some(previous) = &previous {
            assert!(Arc::ptr_eq(previous, &measured.topology));
        }
        previous = Some(Arc::clone(&measured.topology));
        let projection = measured.project(bounds, &[]).unwrap();
        assert_eq!(reference::capture(&projection), expected);
        assert_eq!(
            projection.nodes.keys().copied().collect::<Vec<_>>(),
            ordered
        );
    }
}

#[test]
fn constraint_reader_preserves_depth_first_errors_and_override_precedence() {
    let mut builder = Workspace::builder();
    let leaves = [1, 2, 3, 4].map(|item| builder.insert_node(Node::tabs([ItemId::new(item)])));
    let nested = builder
        .insert_node(Node::equal_split(Axis::Vertical, leaves[..2].iter().copied()).unwrap());
    let root_node = builder.insert_node(
        Node::split(
            Axis::Horizontal,
            [nested, leaves[2], leaves[3]],
            [0.5, 0.25, 0.25],
        )
        .unwrap(),
    );
    builder.set_root(ROOT, RootRecord::new(root_node));
    builder.set_surface(SurfaceId::new(1), SurfacePresentation::with_main(ROOT));
    let workspace = builder.build().unwrap();
    let metrics = LayoutMetrics::new(1.0).unwrap();
    let bounds = LogicalRect::new(0.0, 0.0, 512.0, 512.0).unwrap();
    let leaf = Constraints::new(
        LogicalSize::new(0.0, 0.0).unwrap(),
        LogicalSize::new(1024.0, 1024.0).unwrap(),
    )
    .unwrap();
    let constraints = leaves
        .map(|node| (node, leaf))
        .into_iter()
        .collect::<BTreeMap<_, _>>();
    for missing in [0, 2, 3] {
        let mut incomplete = constraints.clone();
        incomplete.remove(&leaves[missing]);
        let mut queries = Vec::new();
        let actual = project_root(
            &workspace,
            ROOT,
            bounds,
            |node| {
                queries.push(node);
                incomplete.get(&node).copied()
            },
            metrics,
        );
        assert_eq!(
            actual,
            Err(LayoutError::MissingLeafConstraints {
                node: leaves[missing]
            })
        );
        assert_eq!(queries, leaves[..=missing]);
        assert_eq!(
            reference::capture_result(&actual),
            reference::project(
                &workspace,
                ROOT,
                bounds,
                LeafMeasurements::Constraints(&incomplete),
                metrics,
                &[],
            )
        );
    }

    let source = workspace.capture_node_source(ROOT, root_node).unwrap();
    let mut queries = Vec::new();
    let actual = project_root_with_overrides(
        &workspace,
        ROOT,
        bounds,
        |node| {
            queries.push(node);
            None
        },
        metrics,
        &[SplitWeightOverride::new(&source, &[])],
    );
    assert!(matches!(
        actual,
        Err(LayoutError::SplitWeightOverrideLengthMismatch { .. })
    ));
    assert!(queries.is_empty());

    let mut incomplete = constraints;
    incomplete.remove(&leaves[2]);
    let overflowing = Constraints::new(
        LogicalSize::new(0.0, 0.0).unwrap(),
        LogicalSize::new(1024.0, f64::MAX).unwrap(),
    )
    .unwrap();
    for node in &leaves[..2] {
        incomplete.insert(*node, overflowing);
    }
    let actual = project_root(
        &workspace,
        ROOT,
        bounds,
        |node| {
            queries.push(node);
            incomplete.get(&node).copied()
        },
        metrics,
    );
    assert_eq!(
        actual,
        Err(LayoutError::NonFiniteAggregate {
            quantity: "subtree maximum height"
        })
    );
    assert_eq!(queries, leaves[..2]);
    assert_eq!(
        reference::capture_result(&actual),
        reference::project(
            &workspace,
            ROOT,
            bounds,
            LeafMeasurements::Constraints(&incomplete),
            metrics,
            &[],
        )
    );
}

#[test]
fn projects_ten_thousand_nested_splits_without_call_stack_growth() {
    const DEPTH: usize = 10_000;

    let leaf = Constraints::new(
        LogicalSize::new(0.0, 0.0).unwrap(),
        LogicalSize::new(10_000.0, 10_000.0).unwrap(),
    )
    .unwrap();
    let mut builder = Workspace::builder();
    let central = builder.insert_node(Node::tabs([ItemId::new(1)]));
    let mut leaf_constraints = BTreeMap::from([(central, leaf)]);
    let mut subtree = central;
    for depth in 0..DEPTH {
        let side =
            builder.insert_node(Node::tabs([ItemId::new(u64::try_from(depth + 2).unwrap())]));
        leaf_constraints.insert(side, leaf);
        let axis = if depth % 2 == 0 {
            Axis::Horizontal
        } else {
            Axis::Vertical
        };
        subtree = builder.insert_node(Node::equal_split(axis, [subtree, side]).unwrap());
    }
    builder.set_root(ROOT, RootRecord::new(subtree).with_central(central));
    builder.set_surface(SurfaceId::new(1), SurfacePresentation::with_main(ROOT));
    let workspace = builder.build().unwrap();
    let bounds = LogicalRect::new(0.0, 0.0, 1_000.0, 1_000.0).unwrap();
    let projection = project_root(
        &workspace,
        ROOT,
        bounds,
        |node| leaf_constraints.get(&node).copied(),
        LayoutMetrics::new(0.0).unwrap(),
    )
    .unwrap();
    assert_eq!(leaf_constraints.len(), DEPTH + 1);
    drop(leaf_constraints);
    drop(workspace);

    assert_eq!(projection.nodes.len(), DEPTH * 2 + 1);
    assert_eq!(projection.splits.len(), DEPTH);
    assert_eq!(projection.nodes[&subtree].rect, bounds);
    assert_eq!(projection.nodes[&central].constraints, leaf);
    for split in projection.splits.values() {
        assert_eq!(split.child_minimum_extents.len(), 2);
        assert_eq!(split.child_maximum_extents.len(), 2);
        assert_eq!(split.child_extents.len(), 2);
        assert_eq!(split.splitter_rects.len(), 1);
    }
    drop(projection);
}

#[test]
fn owned_structure_reuses_rows_with_current_metrics_and_answers() {
    let (workspace, leaves) = fixture(&mut 37, true);
    let minimums = leaves
        .iter()
        .map(|&id| (id, LogicalSize::new(4.0, 3.0).unwrap()))
        .collect::<BTreeMap<_, _>>();
    let first = measure_root_minimums(
        &workspace,
        ROOT,
        &minimums,
        LayoutMetrics::new(1.0).unwrap(),
    )
    .unwrap();
    let mut changed = minimums.clone();
    changed.insert(leaves[0], LogicalSize::new(17.0, 29.0).unwrap());
    let metrics = LayoutMetrics::new(3.0).unwrap();
    let second = measure_root_minimums(&workspace, ROOT, &changed, metrics).unwrap();
    assert!(Arc::ptr_eq(&first.topology, &second.topology));
    let bounds = LogicalRect::new(0.0, 0.0, 257.0, 39.0).unwrap();
    assert_eq!(
        reference::capture_result(&second.project(bounds, &[])),
        reference::project(
            &workspace,
            ROOT,
            bounds,
            LeafMeasurements::Minimums(&changed),
            metrics,
            &[]
        )
    );
}

#[test]
fn semantic_index_rebuild_keeps_the_actual_unchanged_owner_structure() {
    let (workspace, leaves) = fixture(&mut 37, true);
    let version = WorkspaceVersion::initial();
    let next_version =
        WorkspaceVersion::new(version.epoch(), version.revision().checked_next().unwrap());
    let index = WorkspaceIndex::build(&workspace, version).unwrap();
    let mut minimums = leaves
        .iter()
        .map(|&id| (id, LogicalSize::new(4.0, 3.0).unwrap()))
        .collect::<BTreeMap<_, _>>();
    let metrics = LayoutMetrics::new(1.0).unwrap();
    let first = measure_root_minimums(&workspace, ROOT, &minimums, metrics).unwrap();
    let old_target = index
        .capture_tab_target(&workspace, version, ROOT, leaves[0])
        .unwrap();
    let mut changed = workspace.clone();
    let item = crate::ids::ItemId::new(1_000_000);
    let target = changed.capture_tab_target(ROOT, leaves[0]).unwrap();
    crate::transaction::WorkspaceTransaction::from_commands([
        crate::command::WorkspaceCommand::Open {
            item,
            target: crate::command::DockTarget::Center(target),
        },
    ])
    .apply(&mut changed, &crate::policy::DockPolicySnapshot::default())
    .unwrap();
    let refreshed = WorkspaceIndex::build(&changed, next_version).unwrap();
    assert_eq!(
        refreshed,
        WorkspaceIndex::build(&changed, next_version).unwrap()
    );
    assert!(matches!(
        refreshed.verify_reference(
            &changed,
            next_version,
            ROOT,
            leaves[0],
            old_target.reference(),
            crate::error::ReferenceRole::Target
        ),
        Err(crate::error::CommandError::StaleNode { .. })
    ));
    minimums.insert(leaves[0], LogicalSize::new(19.0, 27.0).unwrap());
    let metrics = LayoutMetrics::new(3.0).unwrap();
    let second = measure_root_minimums(&changed, ROOT, &minimums, metrics).unwrap();
    assert!(Arc::ptr_eq(&first.topology, &second.topology));
    let bounds = LogicalRect::new(0.0, 0.0, 350.0, 240.0).unwrap();
    assert_eq!(
        reference::capture_result(&second.project(bounds, &[])),
        reference::project(
            &changed,
            ROOT,
            bounds,
            LeafMeasurements::Minimums(&minimums),
            metrics,
            &[]
        )
    );
}

#[test]
fn same_version_owner_layout_never_reads_an_independent_changed_body() {
    let (workspace, leaves) = fixture(&mut 37, true);
    let version = WorkspaceVersion::initial();
    let index = WorkspaceIndex::build(&workspace, version).unwrap();
    let root = workspace.root(ROOT).unwrap().node;
    let mut changed_root = workspace.node(root).unwrap().clone();
    let Node::Split { children, .. } = &mut changed_root else {
        unreachable!()
    };
    children.swap(0, 1);
    let mut draft = workspace.clone().into_builder();
    draft.replace_node(root, changed_root).unwrap();
    let changed = draft.build().unwrap();
    let minimums = leaves
        .iter()
        .map(|&id| (id, LogicalSize::new(4.0, 3.0).unwrap()))
        .collect::<BTreeMap<_, _>>();
    let metrics = LayoutMetrics::new(1.0).unwrap();
    let stale = measure_root_minimums(&changed, ROOT, &minimums, metrics).unwrap();
    assert!(matches!(
        index.capture_tab_target(&changed, version, ROOT, leaves[0]),
        Err(crate::error::CommandError::StaleNode { .. })
    ));
    let current = measure_root_minimums(&workspace, ROOT, &minimums, metrics).unwrap();
    assert!(!Arc::ptr_eq(&stale.topology, &current.topology));
    let bounds = LogicalRect::new(0.0, 0.0, 350.0, 240.0).unwrap();
    assert_eq!(
        reference::capture_result(&current.project(bounds, &[])),
        reference::project(
            &workspace,
            ROOT,
            bounds,
            LeafMeasurements::Minimums(&minimums),
            metrics,
            &[]
        )
    );
}

#[test]
fn same_version_edits_own_new_rows_and_keep_the_original_output_body() {
    let (workspace, leaves) = fixture(&mut 91, false);
    let version = WorkspaceVersion::initial();
    let index = WorkspaceIndex::build(&workspace, version).unwrap();
    let minimums = leaves
        .iter()
        .map(|&id| (id, LogicalSize::new(4.0, 3.0).unwrap()))
        .collect::<BTreeMap<_, _>>();
    let metrics = LayoutMetrics::new(2.0).unwrap();
    let first = measure_root_minimums(&workspace, ROOT, &minimums, metrics).unwrap();
    let root_node = workspace.root(ROOT).unwrap().node;
    for edit in [0, 1, 2, 4] {
        let mut changed = workspace.clone().into_builder();
        match edit {
            0 => {
                let Node::Split { axis, .. } =
                    changed.draft_mut().nodes.get_mut(root_node).unwrap()
                else {
                    unreachable!()
                };
                *axis = Axis::Vertical;
            }
            1 => {
                let Node::Split { weights, .. } =
                    changed.draft_mut().nodes.get_mut(root_node).unwrap()
                else {
                    unreachable!()
                };
                *weights = SplitWeight::normalize(std::iter::repeat_n(1.0, weights.len())).unwrap();
            }
            2 => {
                Arc::make_mut(&mut changed.draft_mut().roots)
                    .get_mut(&ROOT)
                    .unwrap()
                    .central = Some(leaves[0]);
            }
            _ => {
                let Node::Split { children, .. } =
                    changed.draft_mut().nodes.get_mut(root_node).unwrap()
                else {
                    unreachable!()
                };
                children.swap(0, 1);
            }
        }
        let changed = changed.build().unwrap();
        let bounds = LogicalRect::new(0.0, 0.0, 400.0, 300.0).unwrap();
        let expected = reference::project(
            &changed,
            ROOT,
            bounds,
            LeafMeasurements::Minimums(&minimums),
            metrics,
            &[],
        );
        let actual = measure_root_minimums(&changed, ROOT, &minimums, metrics);
        if let Ok(measured) = &actual {
            assert!(!Arc::ptr_eq(&first.topology, &measured.topology));
        }
        assert_eq!(
            reference::capture_result(&actual.and_then(|measured| measured.project(bounds, &[]))),
            expected
        );
        let refreshed = WorkspaceIndex::build(&changed, version);
        let refreshed = refreshed.expect("a live structural edit must build a fresh index");
        assert!(matches!(
            index.capture_tab_target(&changed, version, ROOT, leaves[0]),
            Err(crate::error::CommandError::StaleNode { .. })
        ));
        refreshed
            .capture_tab_target(&changed, version, ROOT, leaves[0])
            .unwrap();
        assert_eq!(
            reference::capture_result(
                &measure_root_minimums(&changed, ROOT, &minimums, metrics)
                    .and_then(|measured| measured.project(bounds, &[]))
            ),
            reference::project(
                &changed,
                ROOT,
                bounds,
                LeafMeasurements::Minimums(&minimums),
                metrics,
                &[]
            )
        );
        assert!(Arc::ptr_eq(
            workspace.root_structure(ROOT).unwrap(),
            &first.topology
        ));
    }
}

#[test]
fn minimum_measurement_errors_precede_override_validation_for_cold_and_warm_topology() {
    let (workspace, leaves) = fixture(&mut 37, false);
    let root_node = workspace.root(ROOT).unwrap().node;
    let source = workspace.capture_node_source(ROOT, root_node).unwrap();
    let overrides = [SplitWeightOverride::new(&source, &[])];
    let metrics = LayoutMetrics::new(1.0).unwrap();
    let bounds = LogicalRect::new(0.0, 0.0, 512.0, 512.0).unwrap();
    let minimums = leaves
        .iter()
        .map(|&id| (id, LogicalSize::new(1.0, 1.0).unwrap()))
        .collect::<BTreeMap<_, _>>();
    let mut incomplete = minimums.clone();
    incomplete.remove(&leaves[0]);
    let expected = Err(LayoutError::MissingLeafConstraints { node: leaves[0] });
    let version = WorkspaceVersion::initial();
    for warm in [false, true] {
        let index = WorkspaceIndex::build(&workspace, version).unwrap();
        if warm {
            measure_root_minimums(&workspace, ROOT, &minimums, metrics).unwrap();
        }
        let actual = measure_root_minimums(&workspace, ROOT, &incomplete, metrics)
            .and_then(|measured| measured.project_with_index(bounds, &overrides, &index, version));
        assert_eq!(reference::capture_result(&actual), expected);
        assert_eq!(
            reference::project(
                &workspace,
                ROOT,
                bounds,
                LeafMeasurements::Minimums(&incomplete),
                metrics,
                &overrides
            ),
            expected
        );
    }
    let constraints = BTreeMap::new();
    let public = project_root_with_overrides(
        &workspace,
        ROOT,
        bounds,
        |node| constraints.get(&node).copied(),
        metrics,
        &overrides,
    );
    assert!(matches!(
        public,
        Err(LayoutError::SplitWeightOverrideLengthMismatch { .. })
    ));
    assert_eq!(
        reference::project(
            &workspace,
            ROOT,
            bounds,
            LeafMeasurements::Constraints(&constraints),
            metrics,
            &overrides
        ),
        reference::capture_result(&public)
    );
}

#[test]
fn central_ancestor_depths_keep_fresh_reader_order_and_delayed_minimum_projection() {
    for depth in [0, 1, 4] {
        let mut builder = Workspace::builder();
        let central = builder.insert_node(Node::tabs([ItemId::new(1)]));
        let mut node = central;
        let mut leaves = vec![central];
        let mut ancestors = Vec::new();
        for level in 0..depth {
            let sibling = builder.insert_node(Node::tabs([ItemId::new(level + 2)]));
            let (children, central_index) = if level % 2 == 0 {
                leaves.insert(0, sibling);
                ([sibling, node], 1)
            } else {
                leaves.push(sibling);
                ([node, sibling], 0)
            };
            let axis = if level % 2 == 0 {
                Axis::Horizontal
            } else {
                Axis::Vertical
            };
            node = builder.insert_node(Node::split(axis, children, [0.25, 0.75]).unwrap());
            ancestors.push((node, central_index));
        }
        builder.set_root(ROOT, RootRecord::new(node).with_central(central));
        builder.set_surface(SurfaceId::new(1), SurfacePresentation::with_main(ROOT));
        let workspace = builder.build().unwrap();
        let leaf = Constraints::new(
            LogicalSize::new(1.0, 2.0).unwrap(),
            LogicalSize::new(512.0, 512.0).unwrap(),
        )
        .unwrap();
        let constraints = leaves
            .iter()
            .map(|&node| (node, leaf))
            .collect::<BTreeMap<_, _>>();
        let bounds = LogicalRect::new(3.0, 5.0, 240.0, 180.0).unwrap();
        let metrics = LayoutMetrics::new(1.0).unwrap();
        let mut reads = Vec::new();
        let actual = project_root(
            &workspace,
            ROOT,
            bounds,
            |node| {
                reads.push(node);
                constraints.get(&node).copied()
            },
            metrics,
        );
        assert_eq!(reads, leaves);
        assert_eq!(
            reference::capture_result(&actual),
            reference::project(
                &workspace,
                ROOT,
                bounds,
                LeafMeasurements::Constraints(&constraints),
                metrics,
                &[],
            )
        );
        let projection = actual.unwrap();
        for &(split, index) in &ancestors {
            assert_eq!(
                projection.splits.get(&split).unwrap().central_index,
                Some(index)
            );
        }
        let minimums = leaves
            .iter()
            .map(|&node| (node, leaf.min()))
            .collect::<BTreeMap<_, _>>();
        let measured = measure_root_minimums(&workspace, ROOT, &minimums, metrics).unwrap();
        let projected = measured.project(bounds, &[]);
        assert_eq!(
            reference::capture_result(&projected),
            reference::project(
                &workspace,
                ROOT,
                bounds,
                LeafMeasurements::Minimums(&minimums),
                metrics,
                &[],
            )
        );
    }
}

#[test]
fn raw_shared_central_subtree_keeps_membership_in_both_parent_branches() {
    let mut builder = Workspace::builder();
    let leaves = [1, 2, 3, 4].map(|item| builder.insert_node(Node::tabs([ItemId::new(item)])));
    let first =
        builder.insert_node(Node::equal_split(Axis::Vertical, [leaves[0], leaves[1]]).unwrap());
    let second =
        builder.insert_node(Node::equal_split(Axis::Vertical, [leaves[2], leaves[3]]).unwrap());
    let root = builder.insert_node(Node::equal_split(Axis::Horizontal, [first, second]).unwrap());
    builder.set_root(ROOT, RootRecord::new(root).with_central(leaves[0]));
    builder.set_surface(SurfaceId::new(1), SurfacePresentation::with_main(ROOT));
    let mut draft = builder.build().unwrap().into_builder();
    let Node::Split { children, .. } = draft.draft_mut().nodes.get_mut(second).unwrap() else {
        unreachable!()
    };
    children[0] = first;
    let leaf = Constraints::new(
        LogicalSize::new(1.0, 2.0).unwrap(),
        LogicalSize::new(512.0, 512.0).unwrap(),
    )
    .unwrap();
    let constraints = leaves
        .map(|node| (node, leaf))
        .into_iter()
        .collect::<BTreeMap<_, _>>();
    let minimums = leaves
        .map(|node| (node, leaf.min()))
        .into_iter()
        .collect::<BTreeMap<_, _>>();
    let bounds = LogicalRect::new(0.0, 0.0, 512.0, 512.0).unwrap();
    let metrics = LayoutMetrics::new(1.0).unwrap();
    for measurements in [
        LeafMeasurements::Constraints(&constraints),
        LeafMeasurements::Minimums(&minimums),
    ] {
        let actual = project_draft(&draft, ROOT, bounds, measurements, metrics);
        assert_eq!(
            reference::capture_result(&actual),
            reference::project_draft(&draft, ROOT, bounds, measurements, metrics)
        );
        let projection = actual.unwrap();
        for split in [root, first, second] {
            assert_eq!(
                projection.splits.get(&split).unwrap().central_index,
                Some(0)
            );
        }
    }
}

#[test]
fn completed_subtree_reuse_keeps_active_cycle_and_leaf_error_order() {
    let mut builder = Workspace::builder();
    let leaves = [1, 2, 3, 4].map(|item| builder.insert_node(Node::tabs([ItemId::new(item)])));
    let first =
        builder.insert_node(Node::equal_split(Axis::Vertical, [leaves[0], leaves[1]]).unwrap());
    let second =
        builder.insert_node(Node::equal_split(Axis::Vertical, [leaves[2], leaves[3]]).unwrap());
    let root = builder.insert_node(Node::equal_split(Axis::Horizontal, [first, second]).unwrap());
    builder.set_root(ROOT, RootRecord::new(root));
    builder.set_surface(SurfaceId::new(1), SurfacePresentation::with_main(ROOT));
    let workspace = builder.build().unwrap();
    let bounds = LogicalRect::new(0.0, 0.0, 512.0, 512.0).unwrap();
    let metrics = LayoutMetrics::new(1.0).unwrap();
    let leaf = Constraints::new(
        LogicalSize::new(0.0, 0.0).unwrap(),
        LogicalSize::new(512.0, 512.0).unwrap(),
    )
    .unwrap();

    for cycle in [false, true] {
        let mut changed = workspace.clone().into_builder();
        let Node::Split { children, .. } = changed.draft_mut().nodes.get_mut(second).unwrap()
        else {
            unreachable!()
        };
        // The first subtree is complete before this second edge reaches it.
        children[0] = first;
        if cycle {
            children[1] = root;
        }
        for omit_first in [false, true] {
            let constraints = leaves
                .iter()
                .copied()
                .filter(|&node| !omit_first || node != leaves[0])
                .map(|node| (node, leaf))
                .collect();
            let actual = project_draft(
                &changed,
                ROOT,
                bounds,
                LeafMeasurements::Constraints(&constraints),
                metrics,
            );
            assert_eq!(
                reference::capture_result(&actual),
                reference::project_draft(
                    &changed,
                    ROOT,
                    bounds,
                    LeafMeasurements::Constraints(&constraints),
                    metrics,
                )
            );
            if omit_first {
                assert_eq!(
                    actual,
                    Err(LayoutError::MissingLeafConstraints { node: leaves[0] })
                );
            } else if cycle {
                assert_eq!(actual, Err(LayoutError::Cycle { node: root }));
            } else {
                assert!(actual.is_ok());
            }
        }
    }
}

#[test]
fn malformed_topology_keeps_depth_first_measurement_error_priority() {
    let (workspace, leaves) = fixture(&mut 7, false);
    let root_node = workspace.root(ROOT).unwrap().node;
    let mut draft = workspace.into_builder();
    let Node::Split { children, .. } = draft.draft_mut().nodes.get_mut(root_node).unwrap() else {
        unreachable!()
    };
    *children.last_mut().unwrap() = root_node;
    let bounds = LogicalRect::new(0.0, 0.0, 512.0, 512.0).unwrap();
    let metrics = LayoutMetrics::new(1.0).unwrap();
    for omit_first in [true, false] {
        let minimums = leaves
            .iter()
            .copied()
            .filter(|&id| !omit_first || id != leaves[0])
            .map(|id| (id, LogicalSize::new(0.0, 0.0).unwrap()))
            .collect::<BTreeMap<_, _>>();
        assert_eq!(
            reference::capture_result(&project_draft(
                &draft,
                ROOT,
                bounds,
                LeafMeasurements::Minimums(&minimums),
                metrics
            )),
            reference::project_draft(
                &draft,
                ROOT,
                bounds,
                LeafMeasurements::Minimums(&minimums),
                metrics,
            )
        );
    }
}

#[test]
fn raw_missing_node_keeps_depth_first_measurement_error_priority() {
    let (workspace, leaves) = fixture(&mut 7, false);
    let missing = *leaves.last().unwrap();
    let mut draft = workspace.into_builder();
    draft.draft_mut().remove_runtime_node(missing).unwrap();
    let bounds = LogicalRect::new(0.0, 0.0, 512.0, 512.0).unwrap();
    let metrics = LayoutMetrics::new(1.0).unwrap();
    for omit_first in [true, false] {
        let minimums = leaves
            .iter()
            .copied()
            .filter(|&node| !omit_first || node != leaves[0])
            .map(|node| (node, LogicalSize::new(0.0, 0.0).unwrap()))
            .collect::<BTreeMap<_, _>>();
        let actual = project_draft(
            &draft,
            ROOT,
            bounds,
            LeafMeasurements::Minimums(&minimums),
            metrics,
        );
        assert_eq!(
            reference::capture_result(&actual),
            reference::project_draft(
                &draft,
                ROOT,
                bounds,
                LeafMeasurements::Minimums(&minimums),
                metrics,
            )
        );
        assert_eq!(
            actual,
            Err(if omit_first {
                LayoutError::MissingLeafConstraints { node: leaves[0] }
            } else {
                LayoutError::MissingNode { node: missing }
            })
        );
    }
}

#[test]
fn owned_node_rows_keep_previous_outputs_after_late_geometry_error() {
    let mut builder = Workspace::builder();
    let leaves = [1, 2].map(|item| builder.insert_node(Node::tabs([ItemId::new(item)])));
    let split = builder.insert_node(Node::equal_split(Axis::Horizontal, leaves).unwrap());
    builder.set_root(ROOT, RootRecord::new(split));
    builder.set_surface(SurfaceId::new(1), SurfacePresentation::with_main(ROOT));
    let workspace = builder.build().unwrap();
    let minimum = f64::MAX * 0.375;
    let leaf = Constraints::new(
        LogicalSize::new(minimum, 0.0).unwrap(),
        LogicalSize::new(minimum, 10.0).unwrap(),
    )
    .unwrap();
    let constraints = leaves
        .map(|node| (node, leaf))
        .into_iter()
        .collect::<BTreeMap<_, _>>();
    let metrics = LayoutMetrics::new(0.0).unwrap();
    let first_bounds = LogicalRect::new(0.0, 0.0, f64::MAX, 10.0).unwrap();
    let first = project_root(
        &workspace,
        ROOT,
        first_bounds,
        |node| constraints.get(&node).copied(),
        metrics,
    )
    .unwrap();
    let expected = reference::project(
        &workspace,
        ROOT,
        first_bounds,
        LeafMeasurements::Constraints(&constraints),
        metrics,
        &[],
    )
    .unwrap();
    assert_eq!(reference::capture(&first), expected);
    let cloned = first.clone();
    let nodes = &cloned.nodes;

    // Minima fit in f64; the second child's endpoint overflows after the first visit.
    let failing_bounds = LogicalRect::new(f64::MAX * 0.5, 0.0, f64::MAX * 0.5, 10.0).unwrap();
    let actual = project_root(
        &workspace,
        ROOT,
        failing_bounds,
        |node| constraints.get(&node).copied(),
        metrics,
    );
    let failed = reference::project(
        &workspace,
        ROOT,
        failing_bounds,
        LeafMeasurements::Constraints(&constraints),
        metrics,
        &[],
    );
    let error = LayoutError::Geometry(GeometryError::NonFinite {
        component: "x",
        value: f64::INFINITY,
    });
    assert_eq!(actual, Err(error.clone()));
    assert_eq!(failed, Err(error));
    drop(first);
    drop(constraints);
    drop(workspace);
    assert_eq!(reference::capture(&cloned), expected);
    assert_eq!(
        nodes.get(&leaves[0]).map(|result| &result.rect),
        expected.node_rects.get(&leaves[0])
    );
    assert_eq!(
        nodes.get(&leaves[1]).map(|result| &result.constraints),
        expected.node_constraints.get(&leaves[1])
    );
}
