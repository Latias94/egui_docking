use super::*;
use crate::graph::{RootRecord, SurfacePresentation};
use crate::ids::{ItemId, SurfaceId};

struct OverrideFixture {
    workspace: Workspace,
    root: RootId,
    other_root: RootId,
    leaves: Vec<NodeId>,
    splits: Vec<NodeId>,
    other_split: NodeId,
}

fn fixture(leaf_count: usize) -> OverrideFixture {
    let root = RootId::new(1);
    let other_root = RootId::new(2);
    let mut builder = Workspace::builder();
    let leaves = (0..leaf_count)
        .map(|index| {
            builder.insert_node(Node::tabs([ItemId::new(u64::try_from(index + 1).unwrap())]))
        })
        .collect::<Vec<_>>();
    let mut splits = Vec::new();
    let mut level = leaves.clone();
    let mut axis = Axis::Horizontal;
    while level.len() > 1 {
        level = level
            .chunks_exact(2)
            .map(|children| {
                let split =
                    builder.insert_node(Node::equal_split(axis, children.iter().copied()).unwrap());
                splits.push(split);
                split
            })
            .collect();
        axis = match axis {
            Axis::Horizontal => Axis::Vertical,
            Axis::Vertical => Axis::Horizontal,
        };
    }
    builder.set_root(root, RootRecord::new(level[0]));
    builder.set_surface(SurfaceId::new(1), SurfacePresentation::with_main(root));
    let other_leaves = [
        builder.insert_node(Node::tabs([ItemId::new(10_001)])),
        builder.insert_node(Node::tabs([ItemId::new(10_002)])),
    ];
    let other_split = builder.insert_node(Node::equal_split(Axis::Vertical, other_leaves).unwrap());
    builder.set_root(other_root, RootRecord::new(other_split));
    builder.set_surface(
        SurfaceId::new(2),
        SurfacePresentation::with_main(other_root),
    );
    OverrideFixture {
        workspace: builder.build().unwrap(),
        root,
        other_root,
        leaves,
        splits,
        other_split,
    }
}

fn bounds() -> LogicalRect {
    LogicalRect::new(0.0, 0.0, 2_048.0, 1_024.0).unwrap()
}

fn minimums(fixture: &OverrideFixture) -> BTreeMap<NodeId, LogicalSize> {
    fixture
        .leaves
        .iter()
        .map(|node| (*node, LogicalSize::new(1.0, 1.0).unwrap()))
        .collect()
}

fn constraints(fixture: &OverrideFixture) -> BTreeMap<NodeId, Constraints> {
    minimums(fixture)
        .into_iter()
        .map(|(node, minimum)| (node, Constraints::new(minimum, bounds().size()).unwrap()))
        .collect()
}

fn rejected_by_both_paths(
    fixture: &OverrideFixture,
    overrides: &[SplitWeightOverride<'_>],
) -> [LayoutError; 2] {
    let metrics = LayoutMetrics::new(1.0).unwrap();
    let version = WorkspaceVersion::initial();
    let index = WorkspaceIndex::build(&fixture.workspace, version).unwrap();
    let constraints = constraints(fixture);
    let public = project_root_with_overrides(
        &fixture.workspace,
        fixture.root,
        bounds(),
        |node| constraints.get(&node).copied(),
        metrics,
        overrides,
    );
    let indexed = measure_root_minimums(
        &fixture.workspace,
        fixture.root,
        &minimums(fixture),
        metrics,
    )
    .unwrap()
    .project_with_index(bounds(), overrides, &index, version);
    [
        public.expect_err("the public projection must reject invalid override authority"),
        indexed.expect_err("the indexed projection must preserve the same rejection"),
    ]
}

#[test]
fn override_batches_verify_owner_references_without_building_fingerprints() {
    for leaf_count in [4, 32, 128] {
        let fixture = fixture(leaf_count);
        let prototype = fixture
            .workspace
            .capture_node_source(fixture.root, fixture.splits[0])
            .unwrap();
        let sources = fixture
            .splits
            .iter()
            .map(|node| NodeSource {
                root: fixture.root,
                node: *node,
                reference: prototype.reference().clone(),
            })
            .collect::<Vec<_>>();
        let weights = SplitWeight::normalize([0.4, 0.6]).unwrap();
        let overrides = sources
            .iter()
            .map(|source| SplitWeightOverride::new(source, &weights))
            .collect::<Vec<_>>();
        let version = WorkspaceVersion::initial();
        let index = WorkspaceIndex::build(&fixture.workspace, version).unwrap();
        let metrics = LayoutMetrics::new(1.0).unwrap();
        let constraints = constraints(&fixture);

        crate::drop_resolver::structural_work::reset();
        let public = project_root_with_overrides(
            &fixture.workspace,
            fixture.root,
            bounds(),
            |node| constraints.get(&node).copied(),
            metrics,
            &overrides,
        )
        .unwrap();
        let work = crate::drop_resolver::structural_work::snapshot();
        assert_eq!(work.root_fingerprint_builds, 0);
        assert_eq!(work.root_fingerprint_node_visits, 0);

        crate::drop_resolver::structural_work::reset();
        let indexed = measure_root_minimums(
            &fixture.workspace,
            fixture.root,
            &minimums(&fixture),
            metrics,
        )
        .unwrap()
        .project_with_index(bounds(), &overrides, &index, version)
        .unwrap();
        assert_eq!(
            crate::drop_resolver::structural_work::snapshot().root_fingerprint_builds,
            0,
            "owner verification must not rebuild geometry fingerprints"
        );
        assert_eq!(public, indexed);
    }
}

#[test]
fn shared_owner_references_remain_valid_and_stale_duplicates_fail_before_duplicate_checks() {
    let mut fixture = fixture(8);
    let node = fixture.splits[0];
    let stale = fixture
        .workspace
        .capture_node_source(fixture.root, node)
        .unwrap();
    let mut changed = fixture.workspace.node(node).unwrap().clone();
    let Node::Split { weights, .. } = &mut changed else {
        panic!("fixture source must remain a split");
    };
    *weights = SplitWeight::normalize([0.3, 0.7]).unwrap();
    let mut draft = fixture.workspace.into_builder();
    draft.replace_node(node, changed).unwrap();
    fixture.workspace = draft.build().unwrap();
    let current = fixture
        .workspace
        .capture_node_source(fixture.root, node)
        .unwrap();
    let other = fixture
        .workspace
        .capture_node_source(fixture.root, fixture.splits[1])
        .unwrap();
    assert_eq!(current.reference(), other.reference());
    let weights = SplitWeight::normalize([0.4, 0.6]).unwrap();
    let distinct = [
        SplitWeightOverride::new(&current, &weights),
        SplitWeightOverride::new(&other, &weights),
    ];
    let version = WorkspaceVersion::initial();
    let index = WorkspaceIndex::build(&fixture.workspace, version).unwrap();
    for authority in [None, Some((&index, version))] {
        assert!(
            validate_split_weight_overrides(
                &fixture.workspace,
                fixture.root,
                &distinct,
                authority,
            )
            .is_ok()
        );
    }
    for error in rejected_by_both_paths(
        &fixture,
        &[
            SplitWeightOverride::new(&current, &weights),
            SplitWeightOverride::new(&stale, &weights),
        ],
    ) {
        assert!(matches!(
            error,
            LayoutError::InvalidSplitWeightOverrideSource {
                source: CommandError::StaleNode { node: rejected, .. },
            } if rejected == node
        ));
    }
}

#[test]
fn shared_owner_references_never_authorize_missing_or_foreign_nodes() {
    let fixture = fixture(4);
    let current = fixture
        .workspace
        .capture_node_source(fixture.root, fixture.splits[0])
        .unwrap();
    let weights = SplitWeight::normalize([0.4, 0.6]).unwrap();
    for node in [NodeId::default(), fixture.other_split] {
        let foreign = NodeSource {
            root: fixture.root,
            node,
            reference: current.reference().clone(),
        };
        for error in rejected_by_both_paths(
            &fixture,
            &[
                SplitWeightOverride::new(&current, &weights),
                SplitWeightOverride::new(&foreign, &weights),
            ],
        ) {
            match error {
                LayoutError::InvalidSplitWeightOverrideSource {
                    source: CommandError::MissingNode { node: rejected, .. },
                } => assert_eq!(rejected, NodeId::default()),
                LayoutError::InvalidSplitWeightOverrideSource {
                    source: CommandError::NodeOutsideRoot { node: rejected, .. },
                } => assert_eq!(rejected, fixture.other_split),
                other => panic!("membership must be checked for every source: {other}"),
            }
        }
    }
}

#[test]
fn batch_validation_preserves_root_duplicate_and_weight_error_priority() {
    let fixture = fixture(4);
    let current = fixture
        .workspace
        .capture_node_source(fixture.root, fixture.splits[0])
        .unwrap();
    let foreign = fixture
        .workspace
        .capture_node_source(fixture.other_root, fixture.other_split)
        .unwrap();
    let valid = SplitWeight::normalize([0.4, 0.6]).unwrap();
    let short = SplitWeight::normalize([1.0]).unwrap();
    let unnormalized = [SplitWeight::new(0.8).unwrap(); 2];

    for error in rejected_by_both_paths(&fixture, &[SplitWeightOverride::new(&foreign, &short)]) {
        assert!(matches!(
            error,
            LayoutError::SplitWeightOverrideRootMismatch { .. }
        ));
    }
    for error in rejected_by_both_paths(
        &fixture,
        &[
            SplitWeightOverride::new(&current, &valid),
            SplitWeightOverride::new(&current, &short),
        ],
    ) {
        assert!(matches!(
            error,
            LayoutError::DuplicateSplitWeightOverride { .. }
        ));
    }
    for weights in [&short[..], &unnormalized[..]] {
        for error in rejected_by_both_paths(
            &fixture,
            &[
                SplitWeightOverride::new(&current, weights),
                SplitWeightOverride::new(&foreign, &valid),
            ],
        ) {
            match weights.len() {
                1 => assert!(matches!(
                    error,
                    LayoutError::SplitWeightOverrideLengthMismatch { .. }
                )),
                _ => assert!(matches!(
                    error,
                    LayoutError::SplitWeightOverrideNotNormalized { .. }
                )),
            }
        }
    }
}
