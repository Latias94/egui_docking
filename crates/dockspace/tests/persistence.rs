#![cfg(feature = "serde")]

use std::cell::{Cell, RefCell};

use dockspace::engine::DockEngine;
use dockspace::event::WorkspaceEventKind;
use dockspace::geometry::{GeometryError, LogicalRect};
use dockspace::graph::{
    Axis, ContainedFloating, InvalidSplitWeight, Node, RootRecord, SurfacePresentation, Workspace,
};
use dockspace::ids::{FloatingPresentationId, ItemId, RootId, SurfaceId};
use dockspace::persistence::{
    SnapshotAxis, SnapshotEntityKind, SnapshotNode, SnapshotNodeRecord, SnapshotReferenceOwner,
    SnapshotRestoreError, SnapshotRootRecord, SnapshotSurfaceRecord, WORKSPACE_SNAPSHOT_VERSION,
    WorkspaceSnapshot, WorkspaceSnapshotEnvelope,
};
use dockspace::policy::DockPolicy;
use dockspace::validation::WorkspaceValidationError;

const MAIN_ROOT: RootId = RootId::new(10);
const FLOATING_ROOT: RootId = RootId::new(11);
const SURFACE: SurfaceId = SurfaceId::new(20);
const FLOATING: FloatingPresentationId = FloatingPresentationId::new(30);

fn sample_workspace() -> Workspace {
    let mut builder = Workspace::builder();
    let central = builder.insert_node(Node::tabs_with_selection(
        [ItemId::new(1), ItemId::new(2)],
        Some(ItemId::new(2)),
    ));
    let side = builder.insert_node(Node::tabs([ItemId::new(3)]));
    let main = builder.insert_node(
        Node::split(Axis::Horizontal, [central, side], [0.25, 0.75])
            .expect("sample split must be valid"),
    );
    let floating_node = builder.insert_node(Node::tabs([ItemId::new(4)]));

    builder.set_root(MAIN_ROOT, RootRecord::new(main).with_central(central));
    builder.set_root(FLOATING_ROOT, RootRecord::new(floating_node));
    builder.set_surface(SURFACE, SurfacePresentation::new(MAIN_ROOT));
    builder.set_contained_floating(ContainedFloating::new(
        FLOATING,
        FLOATING_ROOT,
        SURFACE,
        LogicalRect::new(12.5, 24.0, 640.0, 360.0).expect("sample geometry must be valid"),
        7,
    ));
    builder
        .attach_contained(SURFACE, FLOATING)
        .expect("sample surface must exist");
    builder.build().expect("sample workspace must be valid")
}

fn sample_snapshot() -> WorkspaceSnapshot {
    WorkspaceSnapshot::capture(&sample_workspace()).expect("sample workspace must be capturable")
}

fn candidate_error(snapshot: &WorkspaceSnapshot) -> SnapshotRestoreError {
    snapshot
        .build_candidate(|_| true)
        .expect_err("corrupted snapshot must be rejected")
}

fn assert_unknown_reference(
    error: &SnapshotRestoreError,
    owner: SnapshotReferenceOwner,
    kind: SnapshotEntityKind,
    id: u64,
) {
    assert_eq!(
        error,
        &SnapshotRestoreError::UnknownReference { owner, kind, id }
    );
}

fn assert_float_eq(actual: f64, expected: f64) {
    assert!((actual - expected).abs() <= f64::EPSILON);
}

fn split_weights(snapshot: &mut WorkspaceSnapshot) -> &mut Vec<f32> {
    snapshot
        .nodes
        .iter_mut()
        .find_map(|record| match &mut record.node {
            SnapshotNode::Split { weights, .. } => Some(weights),
            SnapshotNode::Tabs { .. } => None,
        })
        .expect("sample must contain a split")
}

#[test]
fn json_round_trip_preserves_the_complete_workspace_contract() {
    let snapshot = sample_snapshot();
    assert_eq!(snapshot.version, WORKSPACE_SNAPSHOT_VERSION);

    let json = serde_json::to_string_pretty(&snapshot).expect("snapshot JSON must encode");
    let wire: serde_json::Value =
        serde_json::from_str(&json).expect("snapshot wire JSON must decode");
    assert_eq!(wire[0], WORKSPACE_SNAPSHOT_VERSION);
    assert!(wire[1].is_object());
    let decoded = serde_json::from_str::<WorkspaceSnapshotEnvelope>(&json)
        .expect("snapshot JSON envelope must decode")
        .into_snapshot()
        .expect("snapshot version must be supported");

    let resolved = RefCell::new(Vec::new());
    let restored = decoded
        .build_candidate(|item| {
            resolved.borrow_mut().push(item);
            matches!(item.get(), 1..=4)
        })
        .expect("round-tripped snapshot must restore");

    assert_eq!(
        resolved.into_inner(),
        [1, 2, 3, 4].map(ItemId::new),
        "each distinct item is resolved exactly once in stable identity order"
    );
    assert_eq!(
        WorkspaceSnapshot::capture(&restored).expect("restored workspace must be capturable"),
        decoded
    );

    let main = restored.root(MAIN_ROOT).expect("main root must survive");
    let central = main.central.expect("central identity must survive");
    assert!(matches!(
        restored.node(central),
        Some(Node::Tabs {
            items,
            selected: Some(selected),
        }) if items == &[ItemId::new(1), ItemId::new(2)] && *selected == ItemId::new(2)
    ));
    let floating = restored
        .contained_floating(FLOATING)
        .expect("floating presentation must survive");
    assert_eq!(floating.root, FLOATING_ROOT);
    assert_eq!(floating.surface, SURFACE);
    assert_float_eq(floating.rect.x(), 12.5);
    assert_float_eq(floating.rect.y(), 24.0);
    assert_float_eq(floating.rect.width(), 640.0);
    assert_float_eq(floating.rect.height(), 360.0);
    assert_eq!(floating.z_order, 7);
    assert_eq!(
        restored
            .surface(SURFACE)
            .expect("surface must survive")
            .contained,
        [FLOATING]
    );
}

#[test]
fn unsupported_versions_and_duplicate_record_identities_are_typed() {
    let mut unsupported = sample_snapshot();
    unsupported.version += 1;
    assert_eq!(
        candidate_error(&unsupported),
        SnapshotRestoreError::UnsupportedVersion {
            found: WORKSPACE_SNAPSHOT_VERSION + 1,
            supported: WORKSPACE_SNAPSHOT_VERSION,
        }
    );

    let mut duplicate_node = sample_snapshot();
    duplicate_node.nodes.push(duplicate_node.nodes[0].clone());
    assert!(matches!(
        candidate_error(&duplicate_node),
        SnapshotRestoreError::DuplicateIdentity {
            kind: SnapshotEntityKind::Node,
            ..
        }
    ));

    let mut duplicate_root = sample_snapshot();
    duplicate_root.roots.push(duplicate_root.roots[0]);
    assert!(matches!(
        candidate_error(&duplicate_root),
        SnapshotRestoreError::DuplicateIdentity {
            kind: SnapshotEntityKind::Root,
            ..
        }
    ));

    let mut duplicate_surface = sample_snapshot();
    duplicate_surface
        .surfaces
        .push(duplicate_surface.surfaces[0].clone());
    assert!(matches!(
        candidate_error(&duplicate_surface),
        SnapshotRestoreError::DuplicateIdentity {
            kind: SnapshotEntityKind::Surface,
            ..
        }
    ));

    let mut duplicate_floating = sample_snapshot();
    duplicate_floating
        .contained_floatings
        .push(duplicate_floating.contained_floatings[0]);
    assert!(matches!(
        candidate_error(&duplicate_floating),
        SnapshotRestoreError::DuplicateIdentity {
            kind: SnapshotEntityKind::ContainedFloating,
            ..
        }
    ));
}

#[test]
fn json_rejects_unknown_fields_instead_of_silently_losing_state() {
    let mut value = serde_json::to_value(sample_snapshot()).expect("snapshot JSON must encode");
    value[1]
        .as_object_mut()
        .expect("workspace snapshot payload is an object")
        .insert(
            "future_semantics".into(),
            serde_json::json!({ "enabled": true }),
        );

    assert!(serde_json::from_value::<WorkspaceSnapshotEnvelope>(value).is_err());
}

#[test]
fn json_rejects_unknown_fields_at_every_nested_schema_boundary() {
    fn insert_unknown(value: &mut serde_json::Value) {
        value
            .as_object_mut()
            .expect("schema boundary must be an object")
            .insert("future_semantics".into(), serde_json::json!(true));
    }

    let base = serde_json::to_value(sample_snapshot()).expect("snapshot JSON must encode");
    let mut cases = Vec::new();

    let mut node_record = base.clone();
    insert_unknown(&mut node_record[1]["nodes"][0]);
    cases.push(node_record);

    let mut node_variant = base.clone();
    insert_unknown(&mut node_variant[1]["nodes"][0]["node"]);
    cases.push(node_variant);

    let mut root = base.clone();
    insert_unknown(&mut root[1]["roots"][0]);
    cases.push(root);

    let mut surface = base.clone();
    insert_unknown(&mut surface[1]["surfaces"][0]);
    cases.push(surface);

    let mut floating = base.clone();
    insert_unknown(&mut floating[1]["contained_floatings"][0]);
    cases.push(floating);

    let mut rect = base.clone();
    insert_unknown(&mut rect[1]["contained_floatings"][0]["rect"]);
    cases.push(rect);

    for value in cases {
        assert!(serde_json::from_value::<WorkspaceSnapshotEnvelope>(value).is_err());
    }

    let mut unknown_node_kind = base.clone();
    unknown_node_kind[1]["nodes"][0]["node"]["kind"] = serde_json::json!("future_node");
    assert!(serde_json::from_value::<WorkspaceSnapshotEnvelope>(unknown_node_kind).is_err());

    let mut unknown_axis = base;
    let split = unknown_axis[1]["nodes"]
        .as_array_mut()
        .expect("nodes must be an array")
        .iter_mut()
        .find(|record| record["node"]["kind"] == "split")
        .expect("sample must contain a split");
    split["node"]["axis"] = serde_json::json!("diagonal");
    assert!(serde_json::from_value::<WorkspaceSnapshotEnvelope>(unknown_axis).is_err());
}

#[test]
fn json_rejects_repeated_struct_fields() {
    let json = serde_json::to_string(&sample_snapshot()).expect("snapshot JSON must encode");
    let payload_start = json.find('{').expect("payload object must exist") + 1;
    let duplicate = format!(
        "{}\"nodes\":[],{}",
        &json[..payload_start],
        &json[payload_start..]
    );
    assert!(serde_json::from_str::<WorkspaceSnapshotEnvelope>(&duplicate).is_err());
}

#[test]
fn version_first_envelope_types_unsupported_documents_before_body_schema() {
    for (version, body) in [
        (2, r#"[2,{"nodes":{"future":"shape"}}]"#),
        (0, r"[0,[1,2,3]]"),
    ] {
        let envelope = serde_json::from_str::<WorkspaceSnapshotEnvelope>(body)
            .expect("unsupported document envelope must still decode");
        assert_eq!(envelope.version(), version);
        assert_eq!(
            envelope
                .into_snapshot()
                .expect_err("unsupported version must not produce a V1 snapshot"),
            SnapshotRestoreError::UnsupportedVersion {
                found: version,
                supported: WORKSPACE_SNAPSHOT_VERSION,
            }
        );
    }
}

#[test]
fn envelope_requires_exactly_version_then_payload() {
    let value = serde_json::to_value(sample_snapshot()).expect("snapshot JSON must encode");
    let payload = serde_json::to_string(&value[1]).expect("payload JSON must encode");
    for malformed in [
        "[]".to_owned(),
        format!("[{WORKSPACE_SNAPSHOT_VERSION}]"),
        format!("[{WORKSPACE_SNAPSHOT_VERSION},{payload},null]"),
        format!("[{payload},{WORKSPACE_SNAPSHOT_VERSION}]"),
    ] {
        assert!(serde_json::from_str::<WorkspaceSnapshotEnvelope>(&malformed).is_err());
    }
}

#[test]
fn every_unknown_reference_category_is_rejected_before_assembly() {
    let mut child = sample_snapshot();
    let split = child
        .nodes
        .iter_mut()
        .find(|record| matches!(record.node, SnapshotNode::Split { .. }))
        .expect("sample must contain a split");
    let split_id = split.id;
    let SnapshotNode::Split { children, .. } = &mut split.node else {
        unreachable!("record was selected as a split")
    };
    children[0] = 900;
    assert_unknown_reference(
        &candidate_error(&child),
        SnapshotReferenceOwner::Node(split_id),
        SnapshotEntityKind::Node,
        900,
    );

    let mut root_node = sample_snapshot();
    let root_id = root_node.roots[0].id;
    root_node.roots[0].node = 901;
    assert_unknown_reference(
        &candidate_error(&root_node),
        SnapshotReferenceOwner::Root(root_id),
        SnapshotEntityKind::Node,
        901,
    );

    let mut central = sample_snapshot();
    let root_id = central.roots[0].id;
    central.roots[0].central = Some(902);
    assert_unknown_reference(
        &candidate_error(&central),
        SnapshotReferenceOwner::Root(root_id),
        SnapshotEntityKind::Node,
        902,
    );

    let mut main_root = sample_snapshot();
    let surface_id = main_root.surfaces[0].id;
    main_root.surfaces[0].main_root = 903;
    assert_unknown_reference(
        &candidate_error(&main_root),
        SnapshotReferenceOwner::Surface(surface_id),
        SnapshotEntityKind::Root,
        903,
    );

    let mut roster = sample_snapshot();
    let surface_id = roster.surfaces[0].id;
    roster.surfaces[0].contained[0] = 904;
    assert_unknown_reference(
        &candidate_error(&roster),
        SnapshotReferenceOwner::Surface(surface_id),
        SnapshotEntityKind::ContainedFloating,
        904,
    );

    let mut floating_root = sample_snapshot();
    let floating_id = floating_root.contained_floatings[0].id;
    floating_root.contained_floatings[0].root = 905;
    assert_unknown_reference(
        &candidate_error(&floating_root),
        SnapshotReferenceOwner::ContainedFloating(floating_id),
        SnapshotEntityKind::Root,
        905,
    );

    let mut floating_surface = sample_snapshot();
    let floating_id = floating_surface.contained_floatings[0].id;
    floating_surface.contained_floatings[0].surface = 906;
    assert_unknown_reference(
        &candidate_error(&floating_surface),
        SnapshotReferenceOwner::ContainedFloating(floating_id),
        SnapshotEntityKind::Surface,
        906,
    );
}

#[test]
fn unknown_items_and_invalid_scalar_data_are_typed() {
    let snapshot = sample_snapshot();
    assert_eq!(
        snapshot
            .build_candidate(|item| item != ItemId::new(4))
            .expect_err("unknown application item must fail"),
        SnapshotRestoreError::UnknownItem {
            item: ItemId::new(4),
        }
    );

    let mut non_finite_geometry = sample_snapshot();
    non_finite_geometry.contained_floatings[0].rect.x = f64::NAN;
    assert!(matches!(
        candidate_error(&non_finite_geometry),
        SnapshotRestoreError::InvalidFloatingGeometry {
            source: GeometryError::NonFinite { .. },
            ..
        }
    ));

    let mut negative_geometry = sample_snapshot();
    negative_geometry.contained_floatings[0].rect.width = -1.0;
    assert!(matches!(
        candidate_error(&negative_geometry),
        SnapshotRestoreError::InvalidFloatingGeometry {
            source: GeometryError::Negative { .. },
            ..
        }
    ));

    let mut non_finite_weight = sample_snapshot();
    split_weights(&mut non_finite_weight)[0] = f32::INFINITY;
    assert!(matches!(
        candidate_error(&non_finite_weight),
        SnapshotRestoreError::InvalidSplitWeight {
            source: InvalidSplitWeight::NonFinite { .. },
            ..
        }
    ));

    let mut non_positive_weight = sample_snapshot();
    split_weights(&mut non_positive_weight)[0] = 0.0;
    assert!(matches!(
        candidate_error(&non_positive_weight),
        SnapshotRestoreError::InvalidSplitWeight {
            source: InvalidSplitWeight::NonPositive { .. },
            ..
        }
    ));

    let mut non_normalized = sample_snapshot();
    split_weights(&mut non_normalized).copy_from_slice(&[0.2, 0.2]);
    let SnapshotRestoreError::InvalidWorkspace(errors) = candidate_error(&non_normalized) else {
        panic!("non-normalized weights must fail strict workspace validation")
    };
    assert!(errors.errors().iter().any(|error| matches!(
        error,
        WorkspaceValidationError::SplitWeightsNotNormalized { .. }
    )));

    let mut mismatched = sample_snapshot();
    split_weights(&mut mismatched).pop();
    let SnapshotRestoreError::InvalidWorkspace(errors) = candidate_error(&mismatched) else {
        panic!("weight cardinality mismatch must fail strict workspace validation")
    };
    assert!(errors.errors().iter().any(|error| matches!(
        error,
        WorkspaceValidationError::SplitWeightCountMismatch { .. }
    )));
}

#[test]
fn registry_queries_run_after_validation_and_cover_every_distinct_item() {
    let mut cycle = sample_snapshot();
    let split = cycle
        .nodes
        .iter_mut()
        .find(|record| matches!(record.node, SnapshotNode::Split { .. }))
        .expect("sample must contain a split");
    let split_id = split.id;
    let SnapshotNode::Split { children, .. } = &mut split.node else {
        unreachable!("record was selected as a split")
    };
    children[0] = split_id;

    let invalid_calls = Cell::new(0);
    assert!(
        cycle
            .build_candidate(|_| {
                invalid_calls.set(invalid_calls.get() + 1);
                true
            })
            .is_err()
    );
    assert_eq!(invalid_calls.get(), 0);

    let queried = RefCell::new(Vec::new());
    let error = sample_snapshot()
        .build_candidate(|item| {
            queried.borrow_mut().push(item);
            !matches!(item.get(), 2 | 4)
        })
        .expect_err("missing registry items must reject the candidate");
    assert_eq!(
        error,
        SnapshotRestoreError::UnknownItem {
            item: ItemId::new(2),
        }
    );
    assert_eq!(queried.into_inner(), [1, 2, 3, 4].map(ItemId::new));
}

#[test]
fn cycle_shared_node_and_orphan_corruption_reach_strict_validation() {
    let mut cycle = sample_snapshot();
    let split = cycle
        .nodes
        .iter_mut()
        .find(|record| matches!(record.node, SnapshotNode::Split { .. }))
        .expect("sample must contain a split");
    let split_id = split.id;
    let SnapshotNode::Split { children, .. } = &mut split.node else {
        unreachable!("record was selected as a split")
    };
    children[0] = split_id;
    let SnapshotRestoreError::InvalidWorkspace(errors) = candidate_error(&cycle) else {
        panic!("cycle must reach strict validation")
    };
    assert!(
        errors
            .errors()
            .iter()
            .any(|error| matches!(error, WorkspaceValidationError::Cycle { .. }))
    );

    let mut shared = sample_snapshot();
    let split = shared
        .nodes
        .iter_mut()
        .find(|record| matches!(record.node, SnapshotNode::Split { .. }))
        .expect("sample must contain a split");
    let SnapshotNode::Split { children, .. } = &mut split.node else {
        unreachable!("record was selected as a split")
    };
    children[1] = children[0];
    let SnapshotRestoreError::InvalidWorkspace(errors) = candidate_error(&shared) else {
        panic!("shared node must reach strict validation")
    };
    assert!(
        errors
            .errors()
            .iter()
            .any(|error| matches!(error, WorkspaceValidationError::SharedNode { .. }))
    );

    let mut orphan = sample_snapshot();
    orphan.nodes.push(SnapshotNodeRecord {
        id: 999,
        node: SnapshotNode::Tabs {
            items: vec![99],
            selected: Some(99),
        },
    });
    let SnapshotRestoreError::InvalidWorkspace(errors) = candidate_error(&orphan) else {
        panic!("orphan must reach strict validation")
    };
    assert!(
        errors
            .errors()
            .iter()
            .any(|error| matches!(error, WorkspaceValidationError::OrphanNode { .. }))
    );

    let mut duplicate_item = sample_snapshot();
    let (items, selected) = duplicate_item
        .nodes
        .iter_mut()
        .find_map(|record| match &mut record.node {
            SnapshotNode::Tabs { items, selected } if items.contains(&3) => Some((items, selected)),
            _ => None,
        })
        .expect("sample must contain the side tab");
    items[0] = 1;
    *selected = Some(1);
    let SnapshotRestoreError::InvalidWorkspace(errors) = candidate_error(&duplicate_item) else {
        panic!("duplicate item must reach strict validation")
    };
    assert!(
        errors
            .errors()
            .iter()
            .any(|error| matches!(error, WorkspaceValidationError::DuplicateItem { .. }))
    );

    let mut invalid_selection = sample_snapshot();
    let selected = invalid_selection
        .nodes
        .iter_mut()
        .find_map(|record| match &mut record.node {
            SnapshotNode::Tabs { items, selected } if items.contains(&1) => Some(selected),
            _ => None,
        })
        .expect("sample must contain the central tabs");
    *selected = Some(999);
    let SnapshotRestoreError::InvalidWorkspace(errors) = candidate_error(&invalid_selection) else {
        panic!("invalid selection must reach strict validation")
    };
    assert!(
        errors
            .errors()
            .iter()
            .any(|error| matches!(error, WorkspaceValidationError::InvalidSelection { .. }))
    );
}

#[test]
fn restored_candidates_publish_only_through_an_epoch_advancing_engine_replacement() {
    let mut engine = DockEngine::new(sample_workspace(), DockPolicy::default())
        .expect("sample engine must be valid");
    let before_workspace = engine.workspace().clone();
    let before_policy = engine.policy().clone();
    let before_version = engine.version();
    let mut corrupted = sample_snapshot();
    let split = corrupted
        .nodes
        .iter_mut()
        .find(|record| matches!(record.node, SnapshotNode::Split { .. }))
        .expect("sample must contain a split");
    let split_id = split.id;
    let SnapshotNode::Split { children, .. } = &mut split.node else {
        unreachable!("record was selected as a split")
    };
    children[0] = split_id;

    assert!(
        engine
            .enqueue_snapshot_replacement(&corrupted, |_| true)
            .is_err()
    );
    assert_eq!(engine.workspace(), &before_workspace);
    assert_eq!(engine.policy(), &before_policy);
    assert_eq!(engine.version(), before_version);
    assert!(engine.pending_inputs().is_empty());

    let replacement = engine
        .enqueue_snapshot_replacement(&sample_snapshot(), |_| true)
        .expect("replacement input must be queued");
    assert_eq!(replacement.get(), 1);
    let transition = engine
        .reduce_pending()
        .expect("valid candidate must publish through the engine");
    assert_eq!(engine.version().epoch().get(), 1);
    assert!(matches!(
        transition.events(),
        [event] if event.kind() == &WorkspaceEventKind::WorkspaceReplaced
    ));
    assert_eq!(
        WorkspaceSnapshot::capture(engine.workspace()).expect("published workspace must be valid"),
        sample_snapshot()
    );
}

#[test]
fn deep_chain_restore_is_iterative() {
    const DEPTH: u64 = 4_096;

    let mut nodes = vec![SnapshotNodeRecord {
        id: 0,
        node: SnapshotNode::Tabs {
            items: vec![1],
            selected: Some(1),
        },
    }];
    let mut subtree = 0;
    for level in 0..DEPTH {
        let sibling = level * 2 + 1;
        let split = level * 2 + 2;
        nodes.push(SnapshotNodeRecord {
            id: sibling,
            node: SnapshotNode::Tabs {
                items: vec![level + 2],
                selected: Some(level + 2),
            },
        });
        nodes.push(SnapshotNodeRecord {
            id: split,
            node: SnapshotNode::Split {
                axis: if level % 2 == 0 {
                    SnapshotAxis::Horizontal
                } else {
                    SnapshotAxis::Vertical
                },
                children: vec![subtree, sibling],
                weights: vec![0.5, 0.5],
            },
        });
        subtree = split;
    }

    let snapshot = WorkspaceSnapshot {
        version: WORKSPACE_SNAPSHOT_VERSION,
        nodes,
        roots: vec![SnapshotRootRecord {
            id: 1,
            node: subtree,
            central: Some(0),
        }],
        surfaces: vec![SnapshotSurfaceRecord {
            id: 1,
            main_root: 1,
            contained: Vec::new(),
        }],
        contained_floatings: Vec::new(),
    };

    let restored = snapshot
        .build_candidate(|_| true)
        .expect("deep valid chain must restore without recursion");
    assert_eq!(
        restored.nodes().count(),
        usize::try_from(DEPTH * 2 + 1).expect("test depth must fit usize")
    );
    assert_eq!(
        restored.item_multiset().len(),
        usize::try_from(DEPTH + 1).expect("test depth must fit usize")
    );
}
