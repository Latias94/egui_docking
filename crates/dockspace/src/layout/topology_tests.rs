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
        proptest::prop_assert_eq!(actual, expected);

        let minimums = constraints.iter().map(|(&id, value)| (id, value.min())).collect::<BTreeMap<_, _>>();
        let expected = reference::project(&workspace, ROOT, bounds, LeafMeasurements::Minimums(&minimums), metrics, &[]);
        let version = WorkspaceVersion::initial();
        let index = WorkspaceIndex::build(&workspace, version).unwrap();
        let before = index.clone();
        for _ in 0..2 {
            let actual = measure_root_minimums_with_index(&workspace, ROOT, &minimums, metrics, &index, version)
                .and_then(|measured| measured.project(bounds, &[]));
            proptest::prop_assert_eq!(&actual, &expected);
        }
        proptest::prop_assert_eq!(index, before);
    }
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
        second.project(bounds, &[]),
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
    let mut changed = workspace.clone();
    let Node::Tabs { items, selected } = changed.nodes.get_mut(leaves[0]).unwrap() else {
        unreachable!()
    };
    let item = crate::ids::ItemId::new(1_000_000);
    items.push(item);
    *selected = Some(item);
    changed.tab_mru.get_mut(&leaves[0]).unwrap().insert(0, item);
    changed.validate().unwrap();
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
            old_target.fingerprint(),
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
        second.project(bounds, &[]),
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
    let mut changed = workspace.clone();
    let root = workspace.root(ROOT).unwrap().node;
    let Node::Split { children, .. } = changed.nodes.get_mut(root).unwrap() else {
        unreachable!()
    };
    children.swap(0, 1);
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
        current.project(bounds, &[]),
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
    for edit in 0..5 {
        let mut changed = workspace.clone();
        match edit {
            0 => {
                let Node::Split { axis, .. } = changed.nodes.get_mut(root_node).unwrap() else {
                    unreachable!()
                };
                *axis = Axis::Vertical;
            }
            1 => {
                let Node::Split { weights, .. } = changed.nodes.get_mut(root_node).unwrap() else {
                    unreachable!()
                };
                *weights = SplitWeight::normalize(std::iter::repeat_n(1.0, weights.len())).unwrap();
            }
            2 => {
                changed.roots.get_mut(&ROOT).unwrap().central = Some(leaves[0]);
            }
            3 => {
                changed.nodes.remove(leaves[0]);
            }
            _ => {
                let Node::Split { children, .. } = changed.nodes.get_mut(root_node).unwrap() else {
                    unreachable!()
                };
                children.swap(0, 1);
            }
        }
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
            actual.and_then(|measured| measured.project(bounds, &[])),
            expected
        );
        let refreshed = WorkspaceIndex::build_reusing_layout(&changed, version, &index);
        if edit == 3 {
            assert!(
                matches!(refreshed, Err(crate::error::CommandError::MissingNode { node, .. }) if node == leaves[0])
            );
        } else {
            let refreshed = refreshed.expect("a live structural edit must build a fresh index");
            assert!(
                refreshed
                    .layout_topology_slot(ROOT, version)
                    .unwrap()
                    .get()
                    .is_none()
            );
            assert_eq!(
                measure_root_minimums_with_index(
                    &changed, ROOT, &minimums, metrics, &refreshed, version
                )
                .and_then(|measured| measured.project(bounds, &[])),
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
        assert_eq!(actual, expected);
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
        public
    );
}

#[test]
fn malformed_topology_keeps_depth_first_measurement_error_priority() {
    let (mut workspace, leaves) = fixture(&mut 7, false);
    let root_node = workspace.root(ROOT).unwrap().node;
    let Node::Split { children, .. } = workspace.nodes.get_mut(root_node).unwrap() else {
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
            measure_root_minimums(&workspace, ROOT, &minimums, metrics)
                .and_then(|measured| measured.project(bounds, &[])),
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
}
