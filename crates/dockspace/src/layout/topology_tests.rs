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
        let actual = project_root(&workspace, ROOT, bounds, &constraints, metrics);
        proptest::prop_assert_eq!(reference::capture_result(&actual), expected);

        let minimums = constraints.iter().map(|(&id, value)| (id, value.min())).collect::<BTreeMap<_, _>>();
        let expected = reference::project(&workspace, ROOT, bounds, LeafMeasurements::Minimums(&minimums), metrics, &[]);
        let version = WorkspaceVersion::initial();
        let index = WorkspaceIndex::build(&workspace, version).unwrap();
        let before = index.clone();
        for _ in 0..2 {
            let actual = measure_root_minimums_with_index(&workspace, ROOT, &minimums, metrics, &index, version)
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
    let first = measure_root_minimums_with_index(
        &workspace,
        ROOT,
        &minimums,
        LayoutMetrics::new(1.0).unwrap(),
        &index,
        version,
    )
    .unwrap()
    .project(bounds, &[])
    .unwrap();
    let retained = reference::capture(&first);
    let expected_keys = retained.node_rects.keys().copied().collect::<Vec<_>>();
    assert_eq!(
        first.node_rects.keys().copied().collect::<Vec<_>>(),
        expected_keys
    );
    assert_eq!(
        first.node_constraints.keys().copied().collect::<Vec<_>>(),
        expected_keys
    );
    assert_eq!(
        first
            .node_rects
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
    let second = measure_root_minimums_with_index(
        &workspace,
        ROOT,
        &minimums,
        LayoutMetrics::new(3.0).unwrap(),
        &index,
        version,
    )
    .unwrap()
    .project(LogicalRect::new(8.0, 2.0, 250.0, 200.0).unwrap(), &[])
    .unwrap();
    assert_ne!(first, second);
    drop(index);
    drop(workspace);
    assert_eq!(reference::capture(&first), retained);
    assert_eq!(first.node_rects.values().len(), first.node_rects.len());
    assert_eq!(first.splits.values().len(), first.splits.len());
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
        &constraints,
        LayoutMetrics::new(0.0).unwrap(),
    )
    .unwrap();
    assert_eq!(projection.node_rects.len(), 1);
    assert_eq!(projection.node_constraints.len(), 1);
    assert!(projection.splits.is_empty());
    assert_eq!(
        projection.node_rects.keys().copied().collect::<Vec<_>>(),
        [leaf]
    );
    for stale in retired {
        assert_ne!(stale, leaf);
        assert!(!projection.node_rects.contains_key(&stale));
        assert_eq!(projection.node_constraints.get(&stale), None);
        assert_eq!(projection.splits.get(&stale), None);
    }
    drop(workspace);
    assert_eq!(projection.node_rects.get(&leaf), Some(&bounds));
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
    let cold = project_root(&workspace, ROOT, bounds, &constraints, metrics).unwrap();
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
    assert_eq!(cold.node_rects.keys().copied().collect::<Vec<_>>(), ordered);
    assert_eq!(cold.node_constraints.len(), preorder.len());
    assert_eq!(cold.splits.len(), 2);
    assert!(!cold.node_rects.contains_key(&kept[2]));
    for stale in retired {
        assert!(!cold.node_rects.contains_key(&stale));
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
    let version = WorkspaceVersion::initial();
    let index = WorkspaceIndex::build(&workspace, version).unwrap();
    let mut previous = None;
    for _ in 0..2 {
        let measured =
            measure_root_minimums_with_index(&workspace, ROOT, &minimums, metrics, &index, version)
                .unwrap();
        if let Some(previous) = &previous {
            assert!(Arc::ptr_eq(previous, &measured.topology));
        }
        previous = Some(Arc::clone(&measured.topology));
        let projection = measured.project(bounds, &[]).unwrap();
        assert_eq!(reference::capture(&projection), expected);
        assert_eq!(
            projection.node_rects.keys().copied().collect::<Vec<_>>(),
            ordered
        );
    }
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
        &leaf_constraints,
        LayoutMetrics::new(0.0).unwrap(),
    )
    .unwrap();
    assert_eq!(leaf_constraints.len(), DEPTH + 1);
    drop(leaf_constraints);
    drop(workspace);

    assert_eq!(projection.node_rects.len(), DEPTH * 2 + 1);
    assert_eq!(projection.node_constraints.len(), DEPTH * 2 + 1);
    assert_eq!(projection.splits.len(), DEPTH);
    assert_eq!(projection.node_rects[&subtree], bounds);
    assert_eq!(projection.node_constraints[&central], leaf);
    for split in projection.splits.values() {
        assert_eq!(split.child_minimum_extents.len(), 2);
        assert_eq!(split.child_maximum_extents.len(), 2);
        assert_eq!(split.child_extents.len(), 2);
        assert_eq!(split.splitter_rects.len(), 1);
    }
    drop(projection);
}

#[test]
fn cached_topology_reuses_structure_with_current_metrics_and_answers() {
    let (workspace, leaves) = fixture(&mut 37, true);
    let version = WorkspaceVersion::initial();
    let index = WorkspaceIndex::build(&workspace, version).unwrap();
    let minimums = leaves
        .iter()
        .map(|&id| (id, LogicalSize::new(4.0, 3.0).unwrap()))
        .collect::<BTreeMap<_, _>>();
    let first = measure_root_minimums_with_index(
        &workspace,
        ROOT,
        &minimums,
        LayoutMetrics::new(1.0).unwrap(),
        &index,
        version,
    )
    .unwrap();
    let mut changed = minimums.clone();
    changed.insert(leaves[0], LogicalSize::new(17.0, 29.0).unwrap());
    let metrics = LayoutMetrics::new(3.0).unwrap();
    let second =
        measure_root_minimums_with_index(&workspace, ROOT, &changed, metrics, &index, version)
            .unwrap();
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
fn refreshed_index_reuses_layout_without_reusing_semantic_authority() {
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
    let first =
        measure_root_minimums_with_index(&workspace, ROOT, &minimums, metrics, &index, version)
            .unwrap();
    let old_target = index
        .capture_tab_target(&workspace, version, ROOT, leaves[0])
        .unwrap();
    let mut draft = workspace.clone().into_builder();
    let mut tabs = workspace.node(leaves[0]).unwrap().clone();
    let Node::Tabs { items, selected } = &mut tabs else {
        unreachable!()
    };
    let item = crate::ids::ItemId::new(1_000_000);
    items.push(item);
    *selected = Some(item);
    draft.replace_node(leaves[0], tabs).unwrap();
    let mut mru = workspace.tab_mru(leaves[0]).unwrap().to_vec();
    mru.insert(0, item);
    draft.set_tab_mru(leaves[0], mru).unwrap();
    let changed = draft.build().unwrap();
    let refreshed = WorkspaceIndex::build_reusing_layout(&changed, next_version, &index).unwrap();
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
    let second = measure_root_minimums_with_index(
        &changed,
        ROOT,
        &minimums,
        metrics,
        &refreshed,
        next_version,
    )
    .unwrap();
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
fn refreshed_index_filters_a_hint_compiled_after_a_same_version_edit() {
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
    let stale =
        measure_root_minimums_with_index(&changed, ROOT, &minimums, metrics, &index, version)
            .unwrap();
    let refreshed = WorkspaceIndex::build_reusing_layout(&workspace, version, &index).unwrap();
    assert!(
        refreshed
            .layout_topology_slot(ROOT, version)
            .unwrap()
            .get()
            .is_none()
    );
    let current =
        measure_root_minimums_with_index(&workspace, ROOT, &minimums, metrics, &refreshed, version)
            .unwrap();
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
fn same_version_topology_edits_miss_without_replacing_the_original_hint() {
    let (workspace, leaves) = fixture(&mut 91, false);
    let version = WorkspaceVersion::initial();
    let index = WorkspaceIndex::build(&workspace, version).unwrap();
    let minimums = leaves
        .iter()
        .map(|&id| (id, LogicalSize::new(4.0, 3.0).unwrap()))
        .collect::<BTreeMap<_, _>>();
    let metrics = LayoutMetrics::new(2.0).unwrap();
    let first =
        measure_root_minimums_with_index(&workspace, ROOT, &minimums, metrics, &index, version)
            .unwrap();
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
                changed.draft_mut().roots.get_mut(&ROOT).unwrap().central = Some(leaves[0]);
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
        let actual =
            measure_root_minimums_with_index(&changed, ROOT, &minimums, metrics, &index, version);
        if let Ok(measured) = &actual {
            assert!(!Arc::ptr_eq(&first.topology, &measured.topology));
        }
        assert_eq!(
            reference::capture_result(&actual.and_then(|measured| measured.project(bounds, &[]))),
            expected
        );
        let refreshed = WorkspaceIndex::build_reusing_layout(&changed, version, &index);
        let refreshed = refreshed.expect("a live structural edit must build a fresh index");
        assert!(
            refreshed
                .layout_topology_slot(ROOT, version)
                .unwrap()
                .get()
                .is_none()
        );
        assert_eq!(
            reference::capture_result(
                &measure_root_minimums_with_index(
                    &changed, ROOT, &minimums, metrics, &refreshed, version
                )
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
            index
                .layout_topology_slot(ROOT, version)
                .unwrap()
                .get()
                .unwrap(),
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
            measure_root_minimums_with_index(&workspace, ROOT, &minimums, metrics, &index, version)
                .unwrap();
        }
        let actual = measure_root_minimums_with_index(
            &workspace,
            ROOT,
            &incomplete,
            metrics,
            &index,
            version,
        )
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
    let public =
        project_root_with_overrides(&workspace, ROOT, bounds, &constraints, metrics, &overrides);
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
