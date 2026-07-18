use std::collections::BTreeMap;

use dockspace::geometry::{Constraints, LogicalRect, LogicalSize};
use dockspace::graph::{Axis, Node, RootRecord, SurfacePresentation, Workspace};
use dockspace::ids::{ItemId, NodeId, RootId, SurfaceId};
use dockspace::layout::{
    AxisConstraint, AxisConstraintError, LayoutError, LayoutMetrics, LayoutMetricsError,
    project_root, solve_axis,
};

fn constraints(values: &[(f64, f64)]) -> Vec<AxisConstraint> {
    values
        .iter()
        .map(|&(min, max)| AxisConstraint::new(min, max).expect("valid fixture constraint"))
        .collect()
}

fn assert_close(actual: f64, expected: f64) {
    assert!(
        (actual - expected).abs() <= 1.0e-9,
        "expected {actual} to be close to {expected}"
    );
}

fn assert_projection_close(actual: f64, expected: f64) {
    assert!(
        (actual - expected).abs() <= 1.0e-4,
        "expected projected {actual} to be close to {expected}"
    );
}

fn layout_metrics(splitter_thickness: f64) -> LayoutMetrics {
    LayoutMetrics::new(splitter_thickness).expect("valid layout metrics")
}

#[test]
fn normalizes_weights_and_conserves_available_extent() {
    let result = solve_axis(
        600.0,
        &[1.0, 2.0, 3.0],
        &constraints(&[(0.0, 600.0); 3]),
        None,
    )
    .expect("layout should solve");

    assert_eq!(result.sizes, vec![100.0, 200.0, 300.0]);
    assert_close(result.overflow, 0.0);
    assert_close(result.unallocated, 0.0);
    assert_close(result.sizes.iter().sum(), 600.0);
}

#[test]
fn central_child_receives_the_remaining_share() {
    let result = solve_axis(
        1_000.0,
        &[0.2, 0.0, 0.3],
        &constraints(&[(0.0, 1_000.0); 3]),
        Some(1),
    )
    .expect("layout should solve");

    assert_eq!(result.sizes, vec![200.0, 500.0, 300.0]);
}

#[test]
fn central_child_yields_when_siblings_over_allocate() {
    let result = solve_axis(
        1_000.0,
        &[0.8, 0.0, 0.7],
        &constraints(&[(0.0, 1_000.0); 3]),
        Some(1),
    )
    .expect("layout should solve");

    assert_close(result.sizes[0], 1_000.0 * 0.8 / 1.5);
    assert_close(result.sizes[1], 0.0);
    assert_close(result.sizes[2], 1_000.0 * 0.7 / 1.5);
    assert_close(result.sizes.iter().sum(), 1_000.0);
}

#[test]
fn clamped_children_redistribute_space_by_weight() {
    let result = solve_axis(
        600.0,
        &[1.0, 1.0, 1.0],
        &constraints(&[(0.0, 100.0), (0.0, 500.0), (0.0, 500.0)]),
        None,
    )
    .expect("layout should solve");

    assert_eq!(result.sizes, vec![100.0, 250.0, 250.0]);
    assert_close(result.sizes.iter().sum(), 600.0);
}

#[test]
fn minimum_conflict_is_reported_without_shrinking_children() {
    let result = solve_axis(
        200.0,
        &[1.0, 1.0],
        &constraints(&[(150.0, 500.0), (100.0, 500.0)]),
        None,
    )
    .expect("conflict is a layout outcome");

    assert_eq!(result.sizes, vec![150.0, 100.0]);
    assert_close(result.overflow, 50.0);
    assert_close(result.unallocated, 0.0);
}

#[test]
fn maximum_conflict_is_reported_without_exceeding_children() {
    let result = solve_axis(
        500.0,
        &[1.0, 1.0],
        &constraints(&[(0.0, 100.0), (0.0, 250.0)]),
        None,
    )
    .expect("conflict is a layout outcome");

    assert_eq!(result.sizes, vec![100.0, 250.0]);
    assert_close(result.overflow, 0.0);
    assert_close(result.unallocated, 150.0);
}

#[test]
fn repeated_solves_are_byte_deterministic() {
    let constraints = constraints(&[(10.0, 220.0), (25.0, 400.0), (5.0, 180.0)]);
    let expected =
        solve_axis(503.0, &[0.17, 0.0, 0.29], &constraints, Some(1)).expect("layout should solve");

    for _ in 0..1_000 {
        assert_eq!(
            solve_axis(503.0, &[0.17, 0.0, 0.29], &constraints, Some(1))
                .expect("layout should solve"),
            expected
        );
    }
}

#[test]
fn empty_axis_reports_all_space_as_unallocated() {
    let result = solve_axis(42.0, &[], &[], None).expect("empty layout should solve");

    assert!(result.sizes.is_empty());
    assert_close(result.overflow, 0.0);
    assert_close(result.unallocated, 42.0);
}

#[test]
fn rejects_invalid_scalar_inputs() {
    let one = constraints(&[(0.0, 10.0)]);

    assert!(matches!(
        solve_axis(f64::NAN, &[1.0], &one, None),
        Err(LayoutError::NonFiniteExtent { .. })
    ));
    assert!(matches!(
        solve_axis(-1.0, &[1.0], &one, None),
        Err(LayoutError::NegativeExtent { .. })
    ));
    assert!(matches!(
        solve_axis(1.0, &[f64::INFINITY], &one, None),
        Err(LayoutError::NonFiniteWeight { index: 0, .. })
    ));
    assert!(matches!(
        solve_axis(1.0, &[-1.0], &one, None),
        Err(LayoutError::NegativeWeight { index: 0, .. })
    ));
    assert!(matches!(
        solve_axis(1.0, &[0.0], &one, None),
        Err(LayoutError::ZeroWeightTotal)
    ));

    let infeasible = constraints(&[(2.0, 3.0)]);
    assert!(matches!(
        solve_axis(1.0, &[0.0], &infeasible, None),
        Err(LayoutError::ZeroWeightTotal)
    ));
}

#[test]
fn rejects_structural_input_mismatches() {
    let one = constraints(&[(0.0, 10.0)]);

    assert!(matches!(
        solve_axis(1.0, &[1.0, 1.0], &one, None),
        Err(LayoutError::LengthMismatch { .. })
    ));
    assert!(matches!(
        solve_axis(1.0, &[1.0], &one, Some(1)),
        Err(LayoutError::CentralIndexOutOfBounds { .. })
    ));
}

#[test]
fn rejects_invalid_constraints_at_construction() {
    let valid = AxisConstraint::new(2.0, 10.0).expect("constraint should be valid");
    assert_close(valid.min(), 2.0);
    assert_close(valid.max(), 10.0);

    assert!(matches!(
        AxisConstraint::new(f64::NAN, 10.0),
        Err(AxisConstraintError::NonFiniteMinimum { .. })
    ));
    assert!(matches!(
        AxisConstraint::new(0.0, f64::INFINITY),
        Err(AxisConstraintError::NonFiniteMaximum { .. })
    ));
    assert!(matches!(
        AxisConstraint::new(-1.0, 10.0),
        Err(AxisConstraintError::NegativeMinimum { .. })
    ));
    assert!(matches!(
        AxisConstraint::new(11.0, 10.0),
        Err(AxisConstraintError::MaximumBelowMinimum { .. })
    ));
}

#[test]
fn rejects_invalid_layout_metrics_at_construction() {
    assert_close(layout_metrics(8.0).splitter_thickness(), 8.0);
    assert!(matches!(
        LayoutMetrics::new(f64::NAN),
        Err(LayoutMetricsError::NonFiniteSplitterThickness { .. })
    ));
    assert!(matches!(
        LayoutMetrics::new(f64::INFINITY),
        Err(LayoutMetricsError::NonFiniteSplitterThickness { .. })
    ));
    assert!(matches!(
        LayoutMetrics::new(-1.0),
        Err(LayoutMetricsError::NegativeSplitterThickness { .. })
    ));
}

#[test]
fn finite_inputs_never_produce_non_finite_output() {
    let constraints = constraints(&[(0.0, 100.0), (20.0, 300.0), (0.0, 500.0)]);

    for extent in [0.0, 1.0, 20.0, 100.0, 333.3, 900.0, 1_200.0] {
        let result = solve_axis(extent, &[0.1, 0.0, 0.25], &constraints, Some(1))
            .expect("finite layout should solve");
        assert!(result.sizes.iter().all(|size| size.is_finite()));
        assert!(result.overflow.is_finite());
        assert!(result.unallocated.is_finite());
    }
}

fn unconstrained_leaf() -> Constraints {
    Constraints::new(
        LogicalSize::new(0.0, 0.0).expect("valid minimum"),
        LogicalSize::new(10_000.0, 10_000.0).expect("valid maximum"),
    )
    .expect("valid constraints")
}

fn three_leaf_workspace() -> (Workspace, RootId, [NodeId; 3], NodeId) {
    let mut builder = Workspace::builder();
    let left = builder.insert_node(Node::tabs([ItemId::new(1)]));
    let central = builder.insert_node(Node::tabs([ItemId::new(2)]));
    let right = builder.insert_node(Node::tabs([ItemId::new(3)]));
    let split = builder.insert_node(
        Node::split(Axis::Horizontal, [left, central, right], [0.2, 0.5, 0.3])
            .expect("valid split"),
    );
    let root = RootId::new(1);
    builder.set_root(root, RootRecord::new(split).with_central(central));
    builder.set_surface(SurfaceId::new(1), SurfacePresentation::new(root));
    (
        builder.build().expect("valid workspace"),
        root,
        [left, central, right],
        split,
    )
}

#[test]
fn projects_workspace_nodes_and_splitter_rectangles() {
    let (workspace, root, leaves, split) = three_leaf_workspace();
    let leaf_constraints = leaves
        .into_iter()
        .map(|leaf| (leaf, unconstrained_leaf()))
        .collect::<BTreeMap<_, _>>();

    let projection = project_root(
        &workspace,
        root,
        LogicalRect::new(100.0, 50.0, 1_000.0, 400.0).expect("valid bounds"),
        &leaf_constraints,
        layout_metrics(10.0),
    )
    .expect("workspace should project");

    assert_projection_close(projection.node_rects[&leaves[0]].width(), 196.0);
    assert_projection_close(projection.node_rects[&leaves[1]].width(), 490.0);
    assert_projection_close(projection.node_rects[&leaves[2]].width(), 294.0);
    assert_projection_close(projection.node_rects[&leaves[0]].height(), 400.0);
    let splitters = &projection.splits[&split].splitter_rects;
    assert_eq!(splitters.len(), 2);
    assert_projection_close(splitters[0].x(), 296.0);
    assert_projection_close(splitters[0].width(), 10.0);
    assert_projection_close(splitters[0].height(), 400.0);
    assert_projection_close(splitters[1].x(), 796.0);
    assert_projection_close(splitters[1].width(), 10.0);
    let assigned_width = leaves
        .iter()
        .map(|leaf| projection.node_rects[leaf].width())
        .sum::<f64>()
        + splitters.iter().map(|rect| rect.width()).sum::<f64>();
    assert_projection_close(assigned_width, 1_000.0);
    assert_projection_close(projection.splits[&split].overflow, 0.0);
    assert_projection_close(projection.splits[&split].unallocated, 0.0);
}

#[test]
fn projection_reports_leaf_minimum_overflow() {
    let (workspace, root, leaves, split) = three_leaf_workspace();
    let leaf_constraints = leaves
        .into_iter()
        .map(|leaf| {
            (
                leaf,
                Constraints::new(
                    LogicalSize::new(150.0, 0.0).expect("valid minimum"),
                    LogicalSize::new(500.0, 1_000.0).expect("valid maximum"),
                )
                .expect("valid constraints"),
            )
        })
        .collect::<BTreeMap<_, _>>();

    let projection = project_root(
        &workspace,
        root,
        LogicalRect::new(0.0, 0.0, 400.0, 200.0).expect("valid bounds"),
        &leaf_constraints,
        layout_metrics(10.0),
    )
    .expect("constraint conflict is a projection outcome");

    assert_projection_close(projection.splits[&split].overflow, 70.0);
    assert_projection_close(projection.splits[&split].unallocated, 0.0);
    assert_eq!(
        leaves
            .iter()
            .map(|leaf| projection.node_rects[leaf].width())
            .collect::<Vec<_>>(),
        vec![150.0, 150.0, 150.0]
    );
}

#[test]
fn projection_requires_explicit_constraints_for_every_leaf() {
    let (workspace, root, leaves, _) = three_leaf_workspace();
    let leaf_constraints = [(leaves[0], unconstrained_leaf())]
        .into_iter()
        .collect::<BTreeMap<_, _>>();

    assert!(matches!(
        project_root(
            &workspace,
            root,
            LogicalRect::new(0.0, 0.0, 1_000.0, 400.0).expect("valid bounds"),
            &leaf_constraints,
            layout_metrics(10.0),
        ),
        Err(LayoutError::MissingLeafConstraints { .. })
    ));
}

#[test]
fn central_leaf_semantics_propagate_through_ancestor_splits() {
    let mut builder = Workspace::builder();
    let left = builder.insert_node(Node::tabs([ItemId::new(10)]));
    let central = builder.insert_node(Node::tabs([ItemId::new(11)]));
    let bottom = builder.insert_node(Node::tabs([ItemId::new(12)]));
    let vertical = builder.insert_node(
        Node::split(Axis::Vertical, [central, bottom], [0.6, 0.4]).expect("valid split"),
    );
    let horizontal = builder.insert_node(
        Node::split(Axis::Horizontal, [left, vertical], [0.25, 0.75]).expect("valid split"),
    );
    let root = RootId::new(2);
    builder.set_root(root, RootRecord::new(horizontal).with_central(central));
    builder.set_surface(SurfaceId::new(2), SurfacePresentation::new(root));
    let workspace = builder.build().expect("valid workspace");
    let leaf_constraints = [left, central, bottom]
        .into_iter()
        .map(|leaf| (leaf, unconstrained_leaf()))
        .collect::<BTreeMap<_, _>>();

    let projection = project_root(
        &workspace,
        root,
        LogicalRect::new(0.0, 0.0, 1_000.0, 400.0).expect("valid bounds"),
        &leaf_constraints,
        layout_metrics(10.0),
    )
    .expect("workspace should project");

    assert_projection_close(projection.node_rects[&left].width(), 247.5);
    assert_projection_close(projection.node_rects[&vertical].width(), 742.5);
    assert_projection_close(projection.node_rects[&central].height(), 234.0);
    assert_projection_close(projection.node_rects[&bottom].height(), 156.0);
}

#[test]
fn projection_rejects_bounds_that_cannot_contain_splitters() {
    let (workspace, root, leaves, split) = three_leaf_workspace();
    let leaf_constraints = leaves
        .into_iter()
        .map(|leaf| (leaf, unconstrained_leaf()))
        .collect::<BTreeMap<_, _>>();

    assert!(matches!(
        project_root(
            &workspace,
            root,
            LogicalRect::new(0.0, 0.0, 100.0, 100.0).expect("valid bounds"),
            &leaf_constraints,
            layout_metrics(60.0),
        ),
        Err(LayoutError::InsufficientExtentForSplitters {
            node,
            required: 120.0,
            available: 100.0,
        }) if node == split
    ));
}

#[test]
fn nested_splitter_thickness_contributes_to_ancestor_constraints() {
    let mut builder = Workspace::builder();
    let first = builder.insert_node(Node::tabs([ItemId::new(20)]));
    let second = builder.insert_node(Node::tabs([ItemId::new(21)]));
    let third = builder.insert_node(Node::tabs([ItemId::new(22)]));
    let fourth = builder.insert_node(Node::tabs([ItemId::new(23)]));
    let inner = builder
        .insert_node(Node::equal_split(Axis::Horizontal, [first, second]).expect("valid split"));
    let middle = builder
        .insert_node(Node::equal_split(Axis::Vertical, [inner, third]).expect("valid split"));
    let outer = builder
        .insert_node(Node::equal_split(Axis::Horizontal, [middle, fourth]).expect("valid split"));
    let root = RootId::new(3);
    builder.set_root(root, RootRecord::new(outer));
    builder.set_surface(SurfaceId::new(3), SurfacePresentation::new(root));
    let workspace = builder.build().expect("valid workspace");
    let constrained = Constraints::new(
        LogicalSize::new(60.0, 0.0).expect("valid minimum"),
        LogicalSize::new(1_000.0, 1_000.0).expect("valid maximum"),
    )
    .expect("valid constraints");
    let leaf_constraints = [
        (first, constrained),
        (second, constrained),
        (third, unconstrained_leaf()),
        (fourth, unconstrained_leaf()),
    ]
    .into_iter()
    .collect::<BTreeMap<_, _>>();

    let projection = project_root(
        &workspace,
        root,
        LogicalRect::new(0.0, 0.0, 100.0, 100.0).expect("valid bounds"),
        &leaf_constraints,
        layout_metrics(10.0),
    )
    .expect("minimum conflict is a projection outcome");

    assert_projection_close(projection.splits[&outer].overflow, 40.0);
    assert_projection_close(projection.node_rects[&inner].width(), 130.0);
}

#[test]
fn projects_ten_thousand_nested_splits_without_call_stack_growth() {
    const DEPTH: usize = 10_000;

    let mut builder = Workspace::builder();
    let central = builder.insert_node(Node::tabs([ItemId::new(1)]));
    let mut leaf_constraints = BTreeMap::from([(central, unconstrained_leaf())]);
    let mut subtree = central;

    for depth in 0..DEPTH {
        let item = ItemId::new(u64::try_from(depth + 2).expect("fixture depth fits in u64"));
        let side = builder.insert_node(Node::tabs([item]));
        leaf_constraints.insert(side, unconstrained_leaf());
        let axis = if depth % 2 == 0 {
            Axis::Horizontal
        } else {
            Axis::Vertical
        };
        subtree = builder.insert_node(
            Node::equal_split(axis, [subtree, side]).expect("valid alternating split"),
        );
    }

    let root = RootId::new(4);
    builder.set_root(root, RootRecord::new(subtree).with_central(central));
    builder.set_surface(SurfaceId::new(4), SurfacePresentation::new(root));
    let workspace = builder.build().expect("valid deep workspace");

    let projection = project_root(
        &workspace,
        root,
        LogicalRect::new(0.0, 0.0, 1_000.0, 1_000.0).expect("valid bounds"),
        &leaf_constraints,
        layout_metrics(0.0),
    )
    .expect("deep workspace should project iteratively");

    assert_eq!(projection.node_rects.len(), DEPTH * 2 + 1);
    assert_eq!(projection.splits.len(), DEPTH);
    assert!(projection.node_rects.contains_key(&central));
}
