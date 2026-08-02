use dockspace::canonical::{CanonicalizationError, canonicalize_workspace};
use dockspace::graph::{
    Axis, InvalidSplitWeight, Node, RootRecord, SplitWeight, SurfacePresentation, Workspace,
    WorkspaceBuilder,
};
use dockspace::ids::{ItemId, NodeId, RootId, SurfaceId};

const ROOT: RootId = RootId::new(1);
const SURFACE: SurfaceId = SurfaceId::new(1);

fn item(value: u64) -> ItemId {
    ItemId::new(value)
}

fn weight(value: f32) -> SplitWeight {
    SplitWeight::new(value).expect("test weights must be valid")
}

fn draft_split(axis: Axis, children: Vec<NodeId>, weights: Vec<f32>) -> Node {
    Node::Split {
        axis,
        children,
        weights: weights.into_iter().map(weight).collect(),
    }
}

fn present_main(builder: &mut WorkspaceBuilder, root: RootRecord) {
    builder.set_root(ROOT, root);
    builder.set_surface(SURFACE, SurfacePresentation::with_main(ROOT));
}

fn split_parts(workspace: &Workspace, node: NodeId) -> (&[NodeId], Vec<f32>) {
    let Some(Node::Split {
        children, weights, ..
    }) = workspace.node(node)
    else {
        panic!("expected split node {node:?}");
    };
    (
        children,
        weights.iter().map(|weight| weight.get()).collect(),
    )
}

fn assert_weights(actual: &[f32], expected: &[f32]) {
    assert_eq!(actual.len(), expected.len());
    for (actual, expected) in actual.iter().zip(expected) {
        assert!(
            (actual - expected).abs() <= 1.0e-6,
            "expected {expected}, got {actual}"
        );
    }
}

// Regression semantics: Dockview/VS Code grid normalization (MIT), rewritten
// for dockspace. A nested child's effective share is parent * nested share.
#[test]
fn same_axis_splits_flatten_with_composed_weights_and_reach_a_fixed_point() {
    let mut builder = Workspace::builder();
    let first = builder.insert_node(Node::tabs([item(1)]));
    let second = builder.insert_node(Node::tabs([item(2)]));
    let third = builder.insert_node(Node::tabs([item(3)]));
    let nested = builder.insert_node(draft_split(
        Axis::Horizontal,
        vec![second, third],
        vec![0.4, 0.6],
    ));
    let root_node = builder.insert_node(draft_split(
        Axis::Horizontal,
        vec![first, nested],
        vec![0.5, 0.5],
    ));
    present_main(&mut builder, RootRecord::new(root_node));

    let report = builder
        .canonicalize()
        .expect("same-axis nesting is canonicalizable");
    assert!(report.changed);
    assert_eq!(report.removed_nodes, 1);

    let mut workspace = builder.build().expect("canonical draft must build");
    assert_eq!(
        workspace.item_multiset(),
        [(item(1), 1), (item(2), 1), (item(3), 1)].into()
    );
    assert!(workspace.node(nested).is_none());
    let (children, weights) = split_parts(&workspace, root_node);
    assert_eq!(children, [first, second, third]);
    assert_weights(&weights, &[0.5, 0.2, 0.3]);

    let fixed_point = workspace.clone();
    let second_report =
        canonicalize_workspace(&mut workspace).expect("canonicalization must be idempotent");
    assert!(!second_report.changed);
    assert_eq!(second_report.removed_nodes, 0);
    assert_eq!(workspace, fixed_point);
}

#[test]
fn empty_tabs_singletons_and_same_axis_wrappers_are_removed_without_rebinding_central() {
    let mut builder = Workspace::builder();
    let first = builder.insert_node(Node::tabs([item(1)]));
    let empty_non_central = builder.insert_node(Node::tabs([]));
    let central = builder.insert_node(Node::tabs([]));
    let second = builder.insert_node(Node::tabs([item(2)]));
    let singleton = builder.insert_node(draft_split(Axis::Vertical, vec![central], vec![1.0]));
    let nested = builder.insert_node(draft_split(
        Axis::Horizontal,
        vec![singleton, second],
        vec![0.4, 0.6],
    ));
    let root_node = builder.insert_node(draft_split(
        Axis::Horizontal,
        vec![first, empty_non_central, nested],
        vec![0.2, 0.3, 0.5],
    ));
    present_main(
        &mut builder,
        RootRecord::new(root_node).with_central(central),
    );

    let report = builder
        .canonicalize()
        .expect("empty non-central leaves and wrappers are canonicalizable");
    assert!(report.changed);
    assert_eq!(report.removed_nodes, 3);

    let workspace = builder.build().expect("canonical draft must build");
    let root = workspace.root(ROOT).expect("root must remain present");
    assert_eq!(root.node, root_node);
    assert_eq!(root.central, Some(central));
    assert!(matches!(
        workspace.node(central),
        Some(Node::Tabs { items, selected }) if items.is_empty() && selected.is_none()
    ));
    assert!(workspace.node(empty_non_central).is_none());
    assert!(workspace.node(singleton).is_none());
    assert!(workspace.node(nested).is_none());

    let (children, weights) = split_parts(&workspace, root_node);
    assert_eq!(children, [first, central, second]);
    assert_weights(&weights, &[2.0 / 7.0, 2.0 / 7.0, 3.0 / 7.0]);
    assert_eq!(
        workspace.item_multiset(),
        [(item(1), 1), (item(2), 1)].into()
    );
}

#[test]
fn promoted_root_keeps_the_exact_empty_central_leaf_identity() {
    let mut builder = Workspace::builder();
    let central = builder.insert_node(Node::tabs([]));
    let wrapper = builder.insert_node(draft_split(Axis::Horizontal, vec![central], vec![1.0]));
    present_main(&mut builder, RootRecord::new(wrapper).with_central(central));

    let report = builder
        .canonicalize()
        .expect("central singleton wrapper is canonicalizable");
    assert_eq!(report.removed_nodes, 1);
    let workspace = builder.build().expect("central-only root must be valid");
    assert_eq!(
        workspace.root(ROOT),
        Some(&RootRecord::new(central).with_central(central))
    );
    assert!(workspace.node(wrapper).is_none());
}

#[test]
fn empty_orphan_nodes_are_swept_but_item_bearing_orphans_fail_closed() {
    let mut empty_orphan_builder = Workspace::builder();
    let live = empty_orphan_builder.insert_node(Node::tabs([item(1)]));
    let empty_orphan = empty_orphan_builder.insert_node(Node::tabs([]));
    present_main(&mut empty_orphan_builder, RootRecord::new(live));

    let report = empty_orphan_builder
        .canonicalize()
        .expect("an empty orphan carries no pane ownership");
    assert_eq!(report.removed_nodes, 1);
    let workspace = empty_orphan_builder
        .build()
        .expect("swept draft must validate");
    assert!(workspace.node(empty_orphan).is_none());
    assert_eq!(workspace.item_multiset(), [(item(1), 1)].into());

    let mut owned_orphan_builder = Workspace::builder();
    let live = owned_orphan_builder.insert_node(Node::tabs([item(1)]));
    let owned_orphan = owned_orphan_builder.insert_node(Node::tabs([item(99)]));
    present_main(&mut owned_orphan_builder, RootRecord::new(live));

    let first_error = owned_orphan_builder
        .canonicalize()
        .expect_err("canonicalization must never discard an orphaned pane");
    assert!(matches!(
        first_error,
        CanonicalizationError::ItemMultisetChanged { .. }
    ));
    assert!(matches!(
        owned_orphan_builder
            .canonicalize()
            .expect_err("a failed transaction must leave the draft unchanged"),
        CanonicalizationError::ItemMultisetChanged { .. }
    ));
    assert!(
        owned_orphan_builder
            .validate()
            .expect_err("the unchanged draft still contains the orphan")
            .errors()
            .iter()
            .any(|error| matches!(
                error,
                dockspace::validation::WorkspaceValidationError::OrphanNode { node }
                    if *node == owned_orphan
            ))
    );
}

#[test]
fn invalid_graphs_return_structured_errors_without_partial_rewrites() {
    let mut mismatch_builder = Workspace::builder();
    let leaf = mismatch_builder.insert_node(Node::tabs([item(1)]));
    let split = mismatch_builder.insert_node(Node::Split {
        axis: Axis::Horizontal,
        children: vec![leaf],
        weights: Vec::new(),
    });
    present_main(&mut mismatch_builder, RootRecord::new(split));

    let first = mismatch_builder
        .canonicalize()
        .expect_err("mismatched cardinality is invalid, not canonicalizable");
    assert!(matches!(
        first,
        CanonicalizationError::SplitWeightCountMismatch {
            node,
            children: 1,
            weights: 0,
        } if node == split
    ));
    let second = mismatch_builder
        .canonicalize()
        .expect_err("failed canonicalization must be atomic");
    assert_eq!(first, second);

    let mut cycle_builder = Workspace::builder();
    let first_node = cycle_builder.insert_node(Node::tabs([item(1)]));
    let second_node = cycle_builder.insert_node(Node::tabs([item(2)]));
    cycle_builder
        .replace_node(
            first_node,
            draft_split(Axis::Horizontal, vec![second_node], vec![1.0]),
        )
        .expect("first placeholder exists");
    cycle_builder
        .replace_node(
            second_node,
            draft_split(Axis::Vertical, vec![first_node], vec![1.0]),
        )
        .expect("second placeholder exists");
    present_main(&mut cycle_builder, RootRecord::new(first_node));

    let cycle_error = cycle_builder
        .canonicalize()
        .expect_err("cycles are invalid, including before strict validation");
    assert!(matches!(cycle_error, CanonicalizationError::Cycle { .. }));
    assert_eq!(
        cycle_error,
        cycle_builder
            .canonicalize()
            .expect_err("cycle failure must not mutate the draft")
    );
}

#[test]
fn nonfinite_and_nonpositive_weights_are_unrepresentable() {
    assert!(matches!(
        SplitWeight::new(f32::NAN),
        Err(InvalidSplitWeight::NonFinite { .. })
    ));
    assert!(matches!(
        SplitWeight::new(f32::INFINITY),
        Err(InvalidSplitWeight::NonFinite { .. })
    ));
    assert!(matches!(
        SplitWeight::new(0.0),
        Err(InvalidSplitWeight::NonPositive { .. })
    ));
    assert!(matches!(
        SplitWeight::new(-1.0),
        Err(InvalidSplitWeight::NonPositive { .. })
    ));
}

#[test]
fn build_rejects_non_normalized_weights_without_reinterpreting_central_shares() {
    let mut builder = Workspace::builder();
    let left = builder.insert_node(Node::tabs([item(1)]));
    let central = builder.insert_node(Node::tabs([]));
    let right = builder.insert_node(Node::tabs([item(2)]));
    let split = builder.insert_node(draft_split(
        Axis::Horizontal,
        vec![left, central, right],
        vec![0.8, 0.2, 0.8],
    ));
    present_main(&mut builder, RootRecord::new(split).with_central(central));

    let validation_error = builder
        .validate()
        .expect_err("strict validation must reject non-normalized weights");
    assert!(validation_error.errors().iter().any(|error| matches!(
        error,
        dockspace::validation::WorkspaceValidationError::SplitWeightsNotNormalized {
            split: invalid,
            ..
        } if *invalid == split
    )));

    let first = builder
        .clone()
        .build()
        .expect_err("publishing must not silently reinterpret absolute sibling shares");
    assert!(matches!(
        first,
        CanonicalizationError::WeightsNotNormalized { node, .. } if node == split
    ));
    assert_eq!(
        builder
            .build()
            .expect_err("a repeated failed publication remains deterministic"),
        first
    );
}

#[test]
fn deeply_nested_singletons_use_bounded_heap_traversal() {
    const DEPTH: usize = 10_000;

    let mut builder = Workspace::builder();
    let central = builder.insert_node(Node::tabs([]));
    let mut root_node = central;
    for depth in 0..DEPTH {
        root_node = builder.insert_node(draft_split(
            if depth % 2 == 0 {
                Axis::Horizontal
            } else {
                Axis::Vertical
            },
            vec![root_node],
            vec![1.0],
        ));
    }
    present_main(
        &mut builder,
        RootRecord::new(root_node).with_central(central),
    );

    let report = builder
        .canonicalize()
        .expect("deep valid drafts must not depend on the call stack");
    assert_eq!(report.removed_nodes, DEPTH);
    let workspace = builder
        .build()
        .expect("promoted central root must validate");
    assert_eq!(
        workspace.root(ROOT),
        Some(&RootRecord::new(central).with_central(central))
    );
    assert_eq!(workspace.nodes().count(), 1);
}
