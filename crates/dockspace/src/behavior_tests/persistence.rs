#![cfg(feature = "serde")]

use super::support;

use std::cell::{Cell, RefCell};

use dockspace::engine::event::WorkspaceEventKind;
use dockspace::engine::{DockEngine, EngineInput};
use dockspace::geometry::{GeometryError, LogicalRect};
use dockspace::graph::validation::WorkspaceValidationError;
use dockspace::graph::{
    Axis, ContainedFloating, InvalidSplitWeight, Node, RootRecord, SurfacePresentation, Workspace,
};
use dockspace::ids::{FloatingPresentationId, ItemId, RootId, StableInputSourceId, SurfaceId};
use dockspace::persistence::snapshot::{
    SnapshotAxis, SnapshotEntityKind, SnapshotNode, SnapshotNodeRecord, SnapshotReferenceOwner,
    SnapshotRestoreError, SnapshotRootRecord, SnapshotSurfaceRecord, WorkspaceSnapshot,
};
use dockspace::policy::DockPolicy;
use support::{TestPresentationHost, submit_input};

const MAIN_ROOT: RootId = RootId::new(10);
const FLOATING_ROOT: RootId = RootId::new(11);
const SURFACE: SurfaceId = SurfaceId::new(20);
const FLOATING: FloatingPresentationId = FloatingPresentationId::new(30);
const SNAPSHOT_INPUT_SOURCE: StableInputSourceId = StableInputSourceId::new(0x5045_5253);

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
    builder.set_surface(SURFACE, SurfacePresentation::with_main(MAIN_ROOT));
    builder.set_contained_floating(
        FLOATING,
        ContainedFloating::new(
            FLOATING_ROOT,
            LogicalRect::new(12.5, 24.0, 640.0, 360.0).expect("sample geometry must be valid"),
        ),
    );
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

fn tab_state(
    snapshot: &mut WorkspaceSnapshot,
    contains: u64,
) -> (&mut Vec<u64>, &mut Option<u64>, &mut Vec<u64>) {
    snapshot
        .nodes
        .iter_mut()
        .find_map(|record| match &mut record.node {
            SnapshotNode::Tabs {
                items,
                selected,
                mru,
            } if items.contains(&contains) => Some((items, selected, mru)),
            SnapshotNode::Tabs { .. } | SnapshotNode::Split { .. } => None,
        })
        .expect("sample must contain requested tabs")
}

fn assert_zero_sized_contained_rect_is_rejected_without_mutation(
    snapshot: &WorkspaceSnapshot,
    expected_width: f64,
    expected_height: f64,
) {
    let json = serde_json::to_vec(snapshot).expect("zero-sized geometry must serialize");
    let decoded = serde_json::from_slice::<WorkspaceSnapshot>(&json)
        .expect("zero is a representable scalar before workspace validation");
    let decoded_rect = decoded.contained_floatings[0].rect;
    assert_float_eq(decoded_rect.width, expected_width);
    assert_float_eq(decoded_rect.height, expected_height);

    let expected = WorkspaceValidationError::NonPositiveContainedRect {
        floating: FLOATING,
        width: expected_width,
        height: expected_height,
    };
    let SnapshotRestoreError::InvalidWorkspace(errors) = candidate_error(&decoded) else {
        panic!("zero-sized geometry must reach strict workspace validation")
    };
    assert_eq!(errors.errors(), std::slice::from_ref(&expected));

    let engine = DockEngine::new(sample_workspace(), DockPolicy::default())
        .expect("sample engine must be valid");
    let untouched = format!("{engine:#?}");
    let SnapshotRestoreError::InvalidWorkspace(errors) = decoded
        .build_candidate(|_| true)
        .expect_err("invalid candidate must not be submitted")
    else {
        panic!("candidate construction must preserve the typed validation error")
    };
    assert_eq!(errors.errors(), std::slice::from_ref(&expected));
    assert_eq!(
        format!("{engine:#?}"),
        untouched,
        "failed restoration must leave all live engine state unchanged"
    );
}

#[test]
fn json_round_trip_preserves_the_complete_workspace_contract() {
    let snapshot = sample_snapshot();

    let json = serde_json::to_string_pretty(&snapshot).expect("snapshot JSON must encode");
    let wire: serde_json::Value =
        serde_json::from_str(&json).expect("snapshot wire JSON must decode");
    assert!(wire.is_object());
    let decoded = serde_json::from_str::<WorkspaceSnapshot>(&json)
        .expect("snapshot JSON payload must decode");

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
            selected: Some(selected), .. }) if items == &[ItemId::new(1), ItemId::new(2)] && *selected == ItemId::new(2)
    ));
    assert_eq!(
        restored.tab_mru(central),
        Some([ItemId::new(2), ItemId::new(1)].as_slice())
    );
    let floating = restored
        .contained_floating(FLOATING)
        .expect("floating presentation must survive");
    assert_eq!(floating.root, FLOATING_ROOT);
    assert_float_eq(floating.rect.x(), 12.5);
    assert_float_eq(floating.rect.y(), 24.0);
    assert_float_eq(floating.rect.width(), 640.0);
    assert_float_eq(floating.rect.height(), 360.0);
    assert_eq!(
        restored
            .surface(SURFACE)
            .expect("surface must survive")
            .contained,
        [FLOATING]
    );
}

#[test]
fn malformed_or_missing_persisted_mru_is_rejected() {
    let mut duplicate = sample_snapshot();
    *tab_state(&mut duplicate, 1).2 = vec![2, 2];
    let SnapshotRestoreError::InvalidWorkspace(errors) = candidate_error(&duplicate) else {
        panic!("duplicate MRU must reach strict workspace validation")
    };
    assert!(errors.errors().iter().any(|error| matches!(
        error,
        WorkspaceValidationError::DuplicateTabMruItem { item, .. }
            if *item == ItemId::new(2)
    )));

    let mut missing = sample_snapshot();
    *tab_state(&mut missing, 1).2 = vec![2];
    let SnapshotRestoreError::InvalidWorkspace(errors) = candidate_error(&missing) else {
        panic!("incomplete MRU must reach strict workspace validation")
    };
    assert!(errors.errors().iter().any(|error| matches!(
        error,
        WorkspaceValidationError::TabItemMissingFromMru { item, .. }
            if *item == ItemId::new(1)
    )));

    let mut foreign = sample_snapshot();
    *tab_state(&mut foreign, 1).2 = vec![2, 99];
    let SnapshotRestoreError::InvalidWorkspace(errors) = candidate_error(&foreign) else {
        panic!("foreign MRU item must reach strict workspace validation")
    };
    assert!(errors.errors().iter().any(|error| matches!(
        error,
        WorkspaceValidationError::TabMruItemNotInTabs { item, .. }
            if *item == ItemId::new(99)
    )));

    let mut selected_not_first = sample_snapshot();
    *tab_state(&mut selected_not_first, 1).2 = vec![1, 2];
    let SnapshotRestoreError::InvalidWorkspace(errors) = candidate_error(&selected_not_first)
    else {
        panic!("selected-not-first MRU must reach strict workspace validation")
    };
    assert!(errors.errors().iter().any(|error| matches!(
        error,
        WorkspaceValidationError::SelectedTabNotMostRecent {
            selected: Some(selected),
            most_recent: Some(most_recent),
            ..
        } if *selected == ItemId::new(2) && *most_recent == ItemId::new(1)
    )));

    let snapshot = sample_snapshot();
    let mut wire = serde_json::to_value(&snapshot).expect("snapshot must serialize");
    let nodes = wire["nodes"]
        .as_array_mut()
        .expect("snapshot payload must contain nodes");
    let tabs = nodes
        .iter_mut()
        .find(|record| record["node"]["kind"] == "tabs")
        .expect("sample must contain tabs");
    tabs["node"]
        .as_object_mut()
        .expect("tabs payload must be an object")
        .remove("mru");
    let error =
        serde_json::from_value::<WorkspaceSnapshot>(wire).expect_err("MRU field must be required");
    assert!(error.to_string().contains("missing field `mru`"));
}

#[test]
fn duplicate_record_identities_are_typed() {
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
fn rootless_round_trip_uses_roster_as_the_only_owner_and_order() {
    let mut builder = Workspace::builder();
    let root_a = RootId::new(41);
    let root_b = RootId::new(42);
    let floating_a = FloatingPresentationId::new(51);
    let floating_b = FloatingPresentationId::new(52);
    let node_a = builder.insert_node(Node::tabs([ItemId::new(41)]));
    let node_b = builder.insert_node(Node::tabs([ItemId::new(42)]));
    builder.set_root(root_a, RootRecord::new(node_a));
    builder.set_root(root_b, RootRecord::new(node_b));
    builder.set_surface(
        SURFACE,
        SurfacePresentation {
            main_root: None,
            contained: vec![floating_a, floating_b],
        },
    );
    builder.set_contained_floating(
        floating_a,
        ContainedFloating::new(
            root_a,
            LogicalRect::new(1.0, 2.0, 30.0, 40.0).expect("valid geometry"),
        ),
    );
    builder.set_contained_floating(
        floating_b,
        ContainedFloating::new(
            root_b,
            LogicalRect::new(5.0, 6.0, 70.0, 80.0).expect("valid geometry"),
        ),
    );
    let workspace = builder.build().expect("rootless workspace must be valid");

    let snapshot = WorkspaceSnapshot::capture(&workspace).expect("capture must succeed");
    assert_eq!(snapshot.surfaces[0].main_root, None);
    assert_eq!(snapshot.surfaces[0].contained, [51, 52]);
    let wire = serde_json::to_value(&snapshot).expect("snapshot must serialize");
    for floating in wire["contained_floatings"]
        .as_array()
        .expect("floating records must be an array")
    {
        assert!(floating.get("surface").is_none());
        assert!(floating.get("z_order").is_none());
    }

    let restored = snapshot
        .build_candidate(|_| true)
        .expect("rootless snapshot must restore");
    assert_eq!(
        restored.surface(SURFACE),
        Some(&SurfacePresentation {
            main_root: None,
            contained: vec![floating_a, floating_b],
        })
    );
    assert_eq!(
        WorkspaceSnapshot::capture(&restored).expect("restored workspace must recapture"),
        snapshot
    );
}

#[test]
fn unknown_contained_fields_are_rejected_independently() {
    let snapshot = serde_json::to_value(sample_snapshot()).expect("snapshot must encode");
    for (field, value) in [
        ("z_order", serde_json::json!(99)),
        ("surface", serde_json::json!(SURFACE.get())),
    ] {
        let mut unknown = snapshot.clone();
        unknown["contained_floatings"][0]
            .as_object_mut()
            .expect("contained floating must be an object")
            .insert(field.into(), value);
        assert!(
            serde_json::from_value::<WorkspaceSnapshot>(unknown).is_err(),
            "unknown field {field} must not be accepted"
        );
    }
}

#[test]
fn nullable_workspace_fields_are_required_and_accept_explicit_null() {
    let base = serde_json::to_value(sample_snapshot()).expect("snapshot must encode");
    let tabs = base["nodes"]
        .as_array()
        .expect("nodes must be an array")
        .iter()
        .position(|record| record["node"]["kind"] == "tabs")
        .expect("sample must contain tabs");
    for (path, field) in [
        (format!("/nodes/{tabs}/node"), "selected"),
        ("/roots/0".to_owned(), "central"),
        ("/surfaces/0".to_owned(), "main_root"),
    ] {
        let mut explicit_null = base.clone();
        explicit_null
            .pointer_mut(&path)
            .and_then(serde_json::Value::as_object_mut)
            .expect("record must be an object")
            .insert(field.into(), serde_json::Value::Null);
        let snapshot = serde_json::from_value::<WorkspaceSnapshot>(explicit_null.clone())
            .expect("an explicit null must decode");
        let encoded = serde_json::to_value(snapshot).expect("decoded snapshot must encode");
        assert_eq!(
            encoded.pointer(&format!("{path}/{field}")),
            Some(&serde_json::Value::Null),
            "{field} must remain explicitly null"
        );

        explicit_null
            .pointer_mut(&path)
            .and_then(serde_json::Value::as_object_mut)
            .expect("record must be an object")
            .remove(field)
            .expect("nullable field must be present before removal");
        let error = serde_json::from_value::<WorkspaceSnapshot>(explicit_null)
            .expect_err("an absent nullable field must not become an intentional null");
        assert!(
            error
                .to_string()
                .contains(&format!("missing field `{field}`")),
            "{error}"
        );
    }
}

#[test]
fn json_rejects_unknown_fields_instead_of_silently_losing_state() {
    let mut value = serde_json::to_value(sample_snapshot()).expect("snapshot JSON must encode");
    value
        .as_object_mut()
        .expect("workspace snapshot payload is an object")
        .insert(
            "future_semantics".into(),
            serde_json::json!({ "enabled": true }),
        );

    assert!(serde_json::from_value::<WorkspaceSnapshot>(value).is_err());
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
    insert_unknown(&mut node_record["nodes"][0]);
    cases.push(node_record);

    let mut node_variant = base.clone();
    insert_unknown(&mut node_variant["nodes"][0]["node"]);
    cases.push(node_variant);

    let mut root = base.clone();
    insert_unknown(&mut root["roots"][0]);
    cases.push(root);

    let mut surface = base.clone();
    insert_unknown(&mut surface["surfaces"][0]);
    cases.push(surface);

    let mut floating = base.clone();
    insert_unknown(&mut floating["contained_floatings"][0]);
    cases.push(floating);

    let mut rect = base.clone();
    insert_unknown(&mut rect["contained_floatings"][0]["rect"]);
    cases.push(rect);

    for value in cases {
        assert!(serde_json::from_value::<WorkspaceSnapshot>(value).is_err());
    }

    let mut unknown_node_kind = base.clone();
    unknown_node_kind["nodes"][0]["node"]["kind"] = serde_json::json!("future_node");
    assert!(serde_json::from_value::<WorkspaceSnapshot>(unknown_node_kind).is_err());

    let mut unknown_axis = base;
    let split = unknown_axis["nodes"]
        .as_array_mut()
        .expect("nodes must be an array")
        .iter_mut()
        .find(|record| record["node"]["kind"] == "split")
        .expect("sample must contain a split");
    split["node"]["axis"] = serde_json::json!("diagonal");
    assert!(serde_json::from_value::<WorkspaceSnapshot>(unknown_axis).is_err());
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
    assert!(serde_json::from_str::<WorkspaceSnapshot>(&duplicate).is_err());
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
    main_root.surfaces[0].main_root = Some(903);
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
fn restore_rejects_zero_width_contained_rect_without_mutating_live_state() {
    let mut snapshot = sample_snapshot();
    snapshot.contained_floatings[0].rect.width = 0.0;

    assert_zero_sized_contained_rect_is_rejected_without_mutation(&snapshot, 0.0, 360.0);
}

#[test]
fn restore_rejects_zero_height_contained_rect_without_mutating_live_state() {
    let mut snapshot = sample_snapshot();
    snapshot.contained_floatings[0].rect.height = 0.0;

    assert_zero_sized_contained_rect_is_rejected_without_mutation(&snapshot, 640.0, 0.0);
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
            mru: vec![99],
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
            SnapshotNode::Tabs {
                items, selected, ..
            } if items.contains(&3) => Some((items, selected)),
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
            SnapshotNode::Tabs {
                items, selected, ..
            } if items.contains(&1) => Some(selected),
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
    let mut host = TestPresentationHost::new(&mut engine);
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

    assert!(corrupted.build_candidate(|_| true).is_err());
    assert_eq!(engine.workspace(), &before_workspace);
    assert_eq!(engine.policy(), &before_policy);
    assert_eq!(engine.version(), before_version);
    let replacement = sample_snapshot()
        .build_candidate(|_| true)
        .expect("replacement candidate must build");
    let transition = submit_input(
        &mut engine,
        &mut host,
        SNAPSHOT_INPUT_SOURCE,
        EngineInput::ReplaceWorkspace(replacement),
    )
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
            mru: vec![1],
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
                mru: vec![level + 2],
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
        nodes,
        roots: vec![SnapshotRootRecord {
            id: 1,
            node: subtree,
            central: Some(0),
        }],
        surfaces: vec![SnapshotSurfaceRecord {
            id: 1,
            main_root: Some(1),
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
