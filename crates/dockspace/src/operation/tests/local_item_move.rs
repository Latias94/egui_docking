use super::*;
use crate::canonical::CanonicalizationError;
use crate::drop_resolver::structural_work;
use crate::validation::WorkspaceValidationError;

const LOCAL_ROOT: RootId = RootId::new(1);
const OTHER_ROOT: RootId = RootId::new(2);
const MOVED: ItemId = ItemId::new(2);

struct Fixture {
    workspace: Workspace,
    source: NodeId,
    same_target: NodeId,
    other_target: NodeId,
}

fn fixture(axis: Axis, last: bool, central: u8, selected: bool) -> Fixture {
    let mut builder = Workspace::builder();
    let items = if last {
        vec![MOVED]
    } else {
        vec![ItemId::new(1), MOVED, ItemId::new(9)]
    };
    let current = if last || selected { MOVED } else { items[0] };
    let source = builder.insert_node(Node::tabs_with_selection(items, Some(current)));
    if !last {
        builder
            .set_tab_mru(
                source,
                if selected { [2, 9, 1] } else { [1, 9, 2] }.map(ItemId::new),
            )
            .unwrap();
    }
    let left = builder.insert_node(Node::tabs([ItemId::new(3)]));
    let right = builder.insert_node(Node::tabs([ItemId::new(4)]));
    let nested = builder.insert_node(Node::split(axis, [left, right], [0.3, 0.7]).unwrap());
    let opposite = match axis {
        Axis::Horizontal => Axis::Vertical,
        Axis::Vertical => Axis::Horizontal,
    };
    let branch =
        builder.insert_node(Node::split(opposite, [source, nested], [0.25, 0.75]).unwrap());
    let same_target = builder.insert_node(Node::tabs([ItemId::new(5), ItemId::new(6)]));
    let root_node =
        builder.insert_node(Node::split(axis, [branch, same_target], [0.6, 0.4]).unwrap());
    let mut record = RootRecord::new(root_node);
    record.central = match central {
        1 => Some(source),
        2 => Some(left),
        _ => None,
    };
    builder.set_root(LOCAL_ROOT, record);
    builder.set_surface(
        SurfaceId::new(1),
        SurfacePresentation::with_main(LOCAL_ROOT),
    );
    let other_target = builder.insert_node(Node::tabs([ItemId::new(7), ItemId::new(8)]));
    builder.set_root(OTHER_ROOT, RootRecord::new(other_target));
    builder.set_surface(
        SurfaceId::new(2),
        SurfacePresentation::with_main(OTHER_ROOT),
    );
    Fixture {
        workspace: builder.build().unwrap(),
        source,
        same_target,
        other_target,
    }
}

fn command(fixture: &Fixture, destination: u8, kind: u8) -> WorkspaceCommand {
    let workspace = &fixture.workspace;
    let (root, tabs) = match destination {
        0 => (LOCAL_ROOT, fixture.source),
        1 => (LOCAL_ROOT, fixture.same_target),
        _ => (OTHER_ROOT, fixture.other_target),
    };
    let fraction = DockFraction::new(0.5).unwrap();
    let target = match kind {
        0 => DockTarget::Center(workspace.capture_tab_target(root, tabs).unwrap()),
        1 | 2 => DockTarget::TabGap {
            target: workspace.capture_tab_target(root, tabs).unwrap(),
            index: usize::from(kind == 1),
        },
        3..=6 => {
            let edge = [Edge::Left, Edge::Top, Edge::Right, Edge::Bottom][usize::from(kind - 3)];
            DockTarget::InnerEdge(
                workspace
                    .capture_inner_edge_target(root, tabs, edge, fraction)
                    .unwrap(),
            )
        }
        _ => {
            let edge = [Edge::Left, Edge::Top, Edge::Right, Edge::Bottom][usize::from(kind - 7)];
            DockTarget::OuterEdge(
                workspace
                    .capture_outer_edge_target(root, edge, fraction)
                    .unwrap(),
            )
        }
    };
    WorkspaceCommand::Move {
        payload: MovePayload::Item(
            workspace
                .capture_item_source(LOCAL_ROOT, fixture.source, MOVED)
                .unwrap(),
        ),
        target,
    }
}

#[test]
fn local_item_moves_match_complete_normalization_and_exact_mru() {
    let policy = default_policy();
    for axis in [Axis::Horizontal, Axis::Vertical] {
        for last in [false, true] {
            for central in 0..3 {
                for selected in [false, true] {
                    if last && !selected {
                        continue;
                    }
                    let fixture = fixture(axis, last, central, selected);
                    for destination in 0..3 {
                        for kind in 0..11 {
                            let command = command(&fixture, destination, kind);
                            let commands = std::slice::from_ref(&command);
                            let expected = prepare_general_candidate(
                                &fixture.workspace,
                                TransactionAuthority::Policy(&policy),
                                commands,
                                fixture.workspace.clone(),
                            )
                            .map(|prepared| prepared.into_parts(&fixture.workspace));
                            structural_work::reset();
                            let actual = prepare_transaction(&fixture.workspace, &policy, commands)
                                .map(|prepared| prepared.into_parts(&fixture.workspace));
                            let work = structural_work::snapshot();
                            assert_eq!(work.item_multiset_scans, 0);
                            assert_eq!(work.canonical_full_passes, 0);
                            assert_eq!(work.workspace_full_validations, 0);
                            assert_eq!(work.workspace_deep_clones.transaction_candidates.calls, 1);
                            assert_eq!(work.root_fingerprint_builds, 0);
                            assert_eq!(work.root_fingerprint_node_visits, 0);
                            assert_eq!(
                                actual, expected,
                                "axis={axis:?} last={last} central={central} selected={selected} destination={destination} kind={kind}"
                            );
                            if let Ok((candidate, _)) = actual {
                                super::owner_reference::assert_actual_placement(&candidate);
                                candidate.validate().unwrap();
                                assert_eq!(
                                    candidate.item_multiset(),
                                    fixture.workspace.item_multiset()
                                );
                            }
                        }
                    }
                }
            }
        }
    }
}

#[test]
fn last_item_retires_only_its_source_presentation_and_candidate_can_be_discarded() {
    for contained in [false, true] {
        let mut builder = Workspace::builder();
        let source = builder.insert_node(Node::tabs([MOVED]));
        let target = builder.insert_node(Node::tabs([ItemId::new(3), ItemId::new(4)]));
        builder.set_root(LOCAL_ROOT, RootRecord::new(source));
        builder.set_root(OTHER_ROOT, RootRecord::new(target));
        builder.set_surface(
            SurfaceId::new(2),
            SurfacePresentation::with_main(OTHER_ROOT),
        );
        if contained {
            builder.set_contained_floating(
                FloatingPresentationId::new(1),
                ContainedFloating::new(LOCAL_ROOT, rect(0.0)),
            );
            builder
                .attach_contained(SurfaceId::new(2), FloatingPresentationId::new(1))
                .unwrap();
        } else {
            builder.set_surface(
                SurfaceId::new(1),
                SurfacePresentation::with_main(LOCAL_ROOT),
            );
        }
        let workspace = builder.build().unwrap();
        let original = workspace.clone();
        let movement = WorkspaceCommand::Move {
            payload: MovePayload::Item(
                workspace
                    .capture_item_source(LOCAL_ROOT, source, MOVED)
                    .unwrap(),
            ),
            target: DockTarget::Center(workspace.capture_tab_target(OTHER_ROOT, target).unwrap()),
        };
        let prepared = prepare_transaction(
            &workspace,
            &default_policy(),
            std::slice::from_ref(&movement),
        )
        .unwrap();
        let (candidate, report) = prepared.into_parts(&workspace);
        assert_eq!(workspace, original);
        assert!(candidate.root(LOCAL_ROOT).is_none());
        assert!(candidate.node(source).is_none());
        assert!(candidate.surface(SurfaceId::new(1)).is_none());
        assert!(
            candidate
                .contained_floating(FloatingPresentationId::new(1))
                .is_none()
        );
        assert_eq!(
            candidate.tab_mru(target),
            Some([MOVED, ItemId::new(3), ItemId::new(4)].as_slice())
        );
        candidate.validate().unwrap();
        let expected = candidate.clone();
        drop(candidate);
        let mut retried = workspace;
        let retried_report = crate::transaction::WorkspaceTransaction::from_commands([movement])
            .apply(&mut retried, &default_policy())
            .unwrap();
        assert_eq!(retried_report, report);
        assert_eq!(retried, expected);
    }
}

#[test]
fn a_mixed_program_selects_complete_fallback_before_any_edit() {
    let fixture = fixture(Axis::Horizontal, true, 0, true);
    let movement = command(&fixture, 1, 3);
    let noop = WorkspaceCommand::Select {
        source: fixture
            .workspace
            .capture_item_source(LOCAL_ROOT, fixture.source, MOVED)
            .unwrap(),
    };
    let (local, _) = prepare_transaction(
        &fixture.workspace,
        &default_policy(),
        std::slice::from_ref(&movement),
    )
    .unwrap()
    .into_parts(&fixture.workspace);
    structural_work::reset();
    let (general, report) =
        prepare_transaction(&fixture.workspace, &default_policy(), &[noop, movement])
            .unwrap()
            .into_parts(&fixture.workspace);
    let work = structural_work::snapshot();
    assert!(work.item_multiset_scans > 0);
    assert_eq!(work.canonical_full_passes, 1);
    assert_eq!(work.workspace_full_validations, 1);
    assert_eq!(work.workspace_deep_clones.transaction_candidates.calls, 1);
    assert_eq!(report.outcomes().len(), 2);
    assert_eq!(general, local);
}

#[test]
fn outer_flattening_weight_failure_still_discards_the_entire_move() {
    let mut builder = Workspace::builder();
    let source = builder.insert_node(Node::tabs([ItemId::new(1), MOVED]));
    let left = builder.insert_node(Node::tabs([ItemId::new(3)]));
    let right = builder.insert_node(Node::tabs([ItemId::new(4)]));
    let target = builder.insert_node(
        Node::split(Axis::Horizontal, [left, right], [f32::from_bits(1), 1.0]).unwrap(),
    );
    builder.set_root(OTHER_ROOT, RootRecord::new(source));
    builder.set_root(LOCAL_ROOT, RootRecord::new(target));
    builder.set_surface(
        SurfaceId::new(1),
        SurfacePresentation::with_main(LOCAL_ROOT),
    );
    builder.set_surface(
        SurfaceId::new(2),
        SurfacePresentation::with_main(OTHER_ROOT),
    );
    let mut workspace = builder.build().unwrap();
    let original = workspace.clone();
    let movement = WorkspaceCommand::Move {
        payload: MovePayload::Item(
            workspace
                .capture_item_source(OTHER_ROOT, source, MOVED)
                .unwrap(),
        ),
        target: DockTarget::OuterEdge(
            workspace
                .capture_outer_edge_target(
                    LOCAL_ROOT,
                    Edge::Right,
                    DockFraction::new(1.0 - f32::EPSILON).unwrap(),
                )
                .unwrap(),
        ),
    };
    let expected = prepare_general_candidate(
        &workspace,
        TransactionAuthority::Policy(&default_policy()),
        std::slice::from_ref(&movement),
        workspace.clone(),
    )
    .unwrap_err();
    assert!(matches!(expected, TransactionError::Canonicalization(_)));
    let actual = crate::transaction::WorkspaceTransaction::from_commands([movement])
        .apply(&mut workspace, &default_policy())
        .unwrap_err();
    assert_eq!(actual, expected);
    assert_eq!(workspace, original);
}

#[test]
fn inner_edge_input_weight_sum_witness_matches_general_rejection_and_rolls_back() {
    let mut builder = Workspace::builder();
    let source = builder.insert_node(Node::tabs([ItemId::new(1), MOVED]));
    let left = builder.insert_node(Node::tabs([ItemId::new(3)]));
    let right = builder.insert_node(Node::tabs([ItemId::new(4)]));
    let target = builder.insert_node(Node::Split {
        axis: Axis::Horizontal,
        children: vec![left, right],
        weights: [0x3c75_c28f, 0x3f7c_284e]
            .map(|bits| SplitWeight::new(f32::from_bits(bits)).unwrap())
            .to_vec(),
    });
    builder.set_root(LOCAL_ROOT, RootRecord::new(source));
    builder.set_root(OTHER_ROOT, RootRecord::new(target));
    builder.set_surface(
        SurfaceId::new(1),
        SurfacePresentation::with_main(LOCAL_ROOT),
    );
    builder.set_surface(
        SurfaceId::new(2),
        SurfacePresentation::with_main(OTHER_ROOT),
    );
    let mut workspace = builder.build().unwrap();
    workspace.validate().unwrap();
    let original = workspace.clone();
    let movement = WorkspaceCommand::Move {
        payload: MovePayload::Item(
            workspace
                .capture_item_source(LOCAL_ROOT, source, MOVED)
                .unwrap(),
        ),
        target: DockTarget::InnerEdge(
            workspace
                .capture_inner_edge_target(
                    OTHER_ROOT,
                    left,
                    Edge::Left,
                    DockFraction::new(0.3).unwrap(),
                )
                .unwrap(),
        ),
    };
    let policy = default_policy();
    let expected = prepare_general_candidate(
        &workspace,
        TransactionAuthority::Policy(&policy),
        std::slice::from_ref(&movement),
        workspace.clone(),
    )
    .unwrap_err();
    assert!(matches!(
        &expected,
        TransactionError::Canonicalization(CanonicalizationError::WeightsNotNormalized { node, .. })
            if *node == target
    ));
    let actual = crate::transaction::WorkspaceTransaction::from_commands([movement])
        .apply(&mut workspace, &policy)
        .unwrap_err();
    assert_eq!(actual, expected);
    assert_eq!(workspace, original);
    workspace.validate().unwrap();
}

#[test]
fn source_collapse_output_weight_sum_witness_matches_general_rejection_and_rolls_back() {
    let mut builder = Workspace::builder();
    let children: Vec<_> = (0..2000)
        .map(|index| {
            let item = if index == 0 {
                MOVED
            } else {
                ItemId::new(index + 10)
            };
            builder.insert_node(Node::tabs([item]))
        })
        .collect();
    let source = children[0];
    let source_split = builder.insert_node(Node::Split {
        axis: Axis::Horizontal,
        children,
        weights: (0..2000)
            .map(|index| {
                let bits = if index == 1999 {
                    0x3a03_0f3f
                } else {
                    0x3a03_126f
                };
                SplitWeight::new(f32::from_bits(bits)).unwrap()
            })
            .collect(),
    });
    let target = builder.insert_node(Node::tabs([ItemId::new(3), ItemId::new(4)]));
    builder.set_root(LOCAL_ROOT, RootRecord::new(source_split));
    builder.set_root(OTHER_ROOT, RootRecord::new(target));
    builder.set_surface(
        SurfaceId::new(1),
        SurfacePresentation::with_main(LOCAL_ROOT),
    );
    builder.set_surface(
        SurfaceId::new(2),
        SurfacePresentation::with_main(OTHER_ROOT),
    );
    let mut workspace = builder.build().unwrap();
    workspace.validate().unwrap();
    let original = workspace.clone();
    let movement = WorkspaceCommand::Move {
        payload: MovePayload::Item(
            workspace
                .capture_item_source(LOCAL_ROOT, source, MOVED)
                .unwrap(),
        ),
        target: DockTarget::Center(workspace.capture_tab_target(OTHER_ROOT, target).unwrap()),
    };
    let policy = default_policy();
    let expected = prepare_general_candidate(
        &workspace,
        TransactionAuthority::Policy(&policy),
        std::slice::from_ref(&movement),
        workspace.clone(),
    )
    .unwrap_err();
    assert!(matches!(
        &expected,
        TransactionError::Canonicalization(CanonicalizationError::InvalidOutput(errors))
            if matches!(
                errors.errors(),
                [WorkspaceValidationError::SplitWeightsNotNormalized { split, .. }]
                    if *split == source_split
            )
    ));
    let actual = crate::transaction::WorkspaceTransaction::from_commands([movement])
        .apply(&mut workspace, &policy)
        .unwrap_err();
    assert_eq!(actual, expected);
    assert_eq!(workspace, original);
    workspace.validate().unwrap();
}
