use dockspace::command::{DockFraction, DockTarget, Edge, MovePayload};
use dockspace::drop_resolver::{
    DropRejectionReason, DropResolution, DropSurfaceUnavailable, resolve_drop,
};
use dockspace::drop_target::{
    DropOcclusionRecord, DropTargetAvailability, DropTargetId, DropTargetRecord,
    DropTargetUnavailable, DropVisual, SceneLayerKey,
};
use dockspace::engine::DockEngine;
use dockspace::geometry::{LogicalPoint, LogicalRect};
use dockspace::graph::{
    Axis, ContainedFloating, Node, RootRecord, SurfacePresentation, Workspace, WorkspaceBuilder,
};
use dockspace::hit_region::HitRegion;
use dockspace::ids::{FloatingPresentationId, ItemId, NodeId, RootId, SurfaceId, WorkspaceEpoch};
use dockspace::interaction::{DragGeneration, DragSessionId};
use dockspace::policy::DockPolicy;
use dockspace::scene::{
    BuildingScene, NodeSceneId, ReadySurfaceScene, SceneBuildError, SemanticRect, SplitterSceneId,
    SurfaceScene, TabBarSceneId, TabSceneId,
};
use dockspace::transaction::WorkspaceTransaction;
use dockspace::transition::InputOutcome;

const SOURCE_ROOT: RootId = RootId::new(1);
const TARGET_ROOT: RootId = RootId::new(2);
const FLOATING_ROOT: RootId = RootId::new(3);
const SOURCE_SURFACE: SurfaceId = SurfaceId::new(1);
const TARGET_SURFACE: SurfaceId = SurfaceId::new(2);
const FLOATING: FloatingPresentationId = FloatingPresentationId::new(1);

#[derive(Debug)]
struct Fixture {
    workspace: Workspace,
    source_tabs: NodeId,
    source_subtree: NodeId,
    target_root_node: NodeId,
    target_tabs_a: NodeId,
    target_tabs_b: NodeId,
    floating_tabs: NodeId,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PayloadSpec {
    Item,
    Tabs,
    Subtree,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TargetSpec {
    TabGap,
    Center,
    InnerEdge,
    OuterEdge,
}

fn fixture() -> Fixture {
    let mut builder = WorkspaceBuilder::new();

    let source_tabs_a = builder.insert_node(Node::tabs([ItemId::new(1), ItemId::new(2)]));
    let source_tabs_b = builder.insert_node(Node::tabs([ItemId::new(3)]));
    let source_subtree = builder.insert_node(
        Node::equal_split(Axis::Vertical, [source_tabs_a, source_tabs_b])
            .expect("valid source subtree"),
    );
    let source_tabs = builder.insert_node(Node::tabs([ItemId::new(4)]));
    let source_root_node = builder.insert_node(
        Node::equal_split(Axis::Horizontal, [source_subtree, source_tabs])
            .expect("valid source root"),
    );

    let target_tabs_a = builder.insert_node(Node::tabs([ItemId::new(10), ItemId::new(11)]));
    let target_tabs_b = builder.insert_node(Node::tabs([ItemId::new(12)]));
    let target_root_node = builder.insert_node(
        Node::equal_split(Axis::Horizontal, [target_tabs_a, target_tabs_b])
            .expect("valid target root"),
    );
    let floating_tabs = builder.insert_node(Node::tabs([ItemId::new(20)]));

    builder.set_root(SOURCE_ROOT, RootRecord::new(source_root_node));
    builder.set_root(TARGET_ROOT, RootRecord::new(target_root_node));
    builder.set_root(FLOATING_ROOT, RootRecord::new(floating_tabs));
    builder.set_surface(SOURCE_SURFACE, SurfacePresentation::new(SOURCE_ROOT));
    builder.set_surface(TARGET_SURFACE, SurfacePresentation::new(TARGET_ROOT));
    builder.set_contained_floating(ContainedFloating::new(
        FLOATING,
        FLOATING_ROOT,
        TARGET_SURFACE,
        rect(20.0, 20.0, 60.0, 60.0),
        1,
    ));
    builder
        .attach_contained(TARGET_SURFACE, FLOATING)
        .expect("target surface exists");

    Fixture {
        workspace: builder.build().expect("fixture workspace is valid"),
        source_tabs,
        source_subtree,
        target_root_node,
        target_tabs_a,
        target_tabs_b,
        floating_tabs,
    }
}

fn session() -> DragSessionId {
    DragSessionId::new(WorkspaceEpoch::default(), DragGeneration::new(1))
}

fn rect(x: f64, y: f64, width: f64, height: f64) -> LogicalRect {
    LogicalRect::new(x, y, width, height).expect("test rectangle is valid")
}

fn point(x: f64, y: f64) -> LogicalPoint {
    LogicalPoint::new(x, y).expect("test point is finite")
}

fn payload(fixture: &Fixture, spec: PayloadSpec) -> MovePayload {
    match spec {
        PayloadSpec::Item => MovePayload::Item(
            fixture
                .workspace
                .capture_item_source(SOURCE_ROOT, fixture.source_tabs, ItemId::new(4))
                .expect("item source is valid"),
        ),
        PayloadSpec::Tabs => MovePayload::Tabs(
            fixture
                .workspace
                .capture_node_source(SOURCE_ROOT, fixture.source_tabs)
                .expect("tabs source is valid"),
        ),
        PayloadSpec::Subtree => MovePayload::Subtree(
            fixture
                .workspace
                .capture_node_source(SOURCE_ROOT, fixture.source_subtree)
                .expect("subtree source is valid"),
        ),
    }
}

fn target_record(
    fixture: &Fixture,
    spec: TargetSpec,
    availability: DropTargetAvailability,
    region: LogicalRect,
    layer: u64,
) -> DropTargetRecord {
    let fraction = DockFraction::new(0.35).expect("valid fraction");
    let (id, target) = match spec {
        TargetSpec::TabGap => (
            DropTargetId::TabGap {
                surface: TARGET_SURFACE,
                root: TARGET_ROOT,
                tabs: fixture.target_tabs_a,
                index: 1,
            },
            DockTarget::TabGap {
                target: fixture
                    .workspace
                    .capture_tab_target(TARGET_ROOT, fixture.target_tabs_a)
                    .expect("tab target is valid"),
                index: 1,
            },
        ),
        TargetSpec::Center => (
            DropTargetId::Center {
                surface: TARGET_SURFACE,
                root: TARGET_ROOT,
                tabs: fixture.target_tabs_a,
            },
            DockTarget::Center(
                fixture
                    .workspace
                    .capture_tab_target(TARGET_ROOT, fixture.target_tabs_a)
                    .expect("center target is valid"),
            ),
        ),
        TargetSpec::InnerEdge => (
            DropTargetId::InnerEdge {
                surface: TARGET_SURFACE,
                root: TARGET_ROOT,
                node: fixture.target_tabs_a,
                edge: Edge::Left,
            },
            DockTarget::Edge(
                fixture
                    .workspace
                    .capture_edge_target(TARGET_ROOT, fixture.target_tabs_a, Edge::Left, fraction)
                    .expect("inner edge target is valid"),
            ),
        ),
        TargetSpec::OuterEdge => (
            DropTargetId::OuterEdge {
                surface: TARGET_SURFACE,
                root: TARGET_ROOT,
                node: fixture.target_root_node,
                edge: Edge::Right,
            },
            DockTarget::Edge(
                fixture
                    .workspace
                    .capture_edge_target(
                        TARGET_ROOT,
                        fixture.target_root_node,
                        Edge::Right,
                        fraction,
                    )
                    .expect("outer edge target is valid"),
            ),
        ),
    };
    DropTargetRecord::new(
        id,
        target,
        availability,
        HitRegion::new(region),
        SceneLayerKey::new(layer),
        DropVisual::new(region),
    )
}

fn center_record(
    fixture: &Fixture,
    tabs: NodeId,
    region: LogicalRect,
    layer: u64,
) -> DropTargetRecord {
    DropTargetRecord::new(
        DropTargetId::Center {
            surface: TARGET_SURFACE,
            root: TARGET_ROOT,
            tabs,
        },
        DockTarget::Center(
            fixture
                .workspace
                .capture_tab_target(TARGET_ROOT, tabs)
                .expect("center target is valid"),
        ),
        DropTargetAvailability::Available,
        HitRegion::new(region),
        SceneLayerKey::new(layer),
        DropVisual::new(region),
    )
}

fn floating_outer_edge_record(
    fixture: &Fixture,
    region: LogicalRect,
    layer: u64,
) -> DropTargetRecord {
    let fraction = DockFraction::new(0.35).expect("valid fraction");
    DropTargetRecord::new(
        DropTargetId::OuterEdge {
            surface: TARGET_SURFACE,
            root: FLOATING_ROOT,
            node: fixture.floating_tabs,
            edge: Edge::Right,
        },
        DockTarget::Edge(
            fixture
                .workspace
                .capture_edge_target(FLOATING_ROOT, fixture.floating_tabs, Edge::Right, fraction)
                .expect("floating edge target is valid"),
        ),
        DropTargetAvailability::Available,
        HitRegion::new(region),
        SceneLayerKey::new(layer),
        DropVisual::new(region),
    )
}

fn floating_center_record(fixture: &Fixture, region: LogicalRect, layer: u64) -> DropTargetRecord {
    DropTargetRecord::new(
        DropTargetId::Center {
            surface: TARGET_SURFACE,
            root: FLOATING_ROOT,
            tabs: fixture.floating_tabs,
        },
        DockTarget::Center(
            fixture
                .workspace
                .capture_tab_target(FLOATING_ROOT, fixture.floating_tabs)
                .expect("floating center target is valid"),
        ),
        DropTargetAvailability::Available,
        HitRegion::new(region),
        SceneLayerKey::new(layer),
        DropVisual::new(region),
    )
}

fn floating_occlusion(region: LogicalRect, layer: u64) -> DropOcclusionRecord {
    DropOcclusionRecord::new(FLOATING, HitRegion::new(region), SceneLayerKey::new(layer))
}

fn seal_scene(
    fixture: &Fixture,
    policy: &DockPolicy,
    records: impl IntoIterator<Item = DropTargetRecord>,
) -> dockspace::scene::SealedScene {
    let mut ready = ReadySurfaceScene::new(TARGET_SURFACE, rect(0.0, 0.0, 100.0, 100.0));
    for record in records {
        ready.push_drop_target(record);
    }
    let mut building = BuildingScene::new([TARGET_SURFACE]).expect("valid roster");
    building
        .insert_ready(ready)
        .expect("ready facts are unique");
    publish_building(fixture, policy, building).expect("scene facts are valid")
}

fn seal_scene_with_occlusions(
    fixture: &Fixture,
    policy: &DockPolicy,
    records: impl IntoIterator<Item = DropTargetRecord>,
    occlusions: impl IntoIterator<Item = DropOcclusionRecord>,
) -> dockspace::scene::SealedScene {
    let mut ready = ReadySurfaceScene::new(TARGET_SURFACE, rect(0.0, 0.0, 100.0, 100.0));
    for record in records {
        ready.push_drop_target(record);
    }
    for occlusion in occlusions {
        ready.push_drop_occlusion(occlusion);
    }
    let mut building = BuildingScene::new([TARGET_SURFACE]).expect("valid roster");
    building
        .insert_ready(ready)
        .expect("ready facts are unique");
    publish_building(fixture, policy, building).expect("scene facts are valid")
}

fn publish_building(
    fixture: &Fixture,
    policy: &DockPolicy,
    building: BuildingScene,
) -> Result<dockspace::scene::SealedScene, SceneBuildError> {
    let mut engine = DockEngine::new(fixture.workspace.clone(), policy.clone())
        .expect("fixture engine must be valid");
    engine
        .enqueue_scene(building)
        .expect("scene sequence must be available");
    let transition = engine
        .reduce_pending()
        .expect("scene publication cannot fail fatally");
    match transition.reduced_inputs()[0].outcome() {
        InputOutcome::ScenePublished { .. } => Ok(engine
            .scene()
            .expect("published outcome must install a scene")
            .clone()),
        InputOutcome::SceneRejected { error } => Err(error.clone()),
        outcome => panic!("unexpected scene outcome: {outcome:?}"),
    }
}

fn assert_scene_error(
    fixture: &Fixture,
    policy: &DockPolicy,
    ready: ReadySurfaceScene,
    predicate: fn(&SceneBuildError) -> bool,
) {
    let mut building = BuildingScene::new([TARGET_SURFACE]).expect("valid roster");
    building.insert_ready(ready).expect("facts are unique");
    let error =
        publish_building(fixture, policy, building).expect_err("malformed fact must fail seal");
    assert!(predicate(&error), "unexpected error: {error:?}");
}

fn resolve(
    scene: &dockspace::scene::SealedScene,
    fixture: &Fixture,
    policy: &DockPolicy,
    source: MovePayload,
    at: LogicalPoint,
) -> DropResolution {
    resolve_drop(
        scene,
        &fixture.workspace,
        policy,
        session(),
        source,
        TARGET_SURFACE,
        at,
    )
    .expect("fixture resolution cannot expose an invariant failure")
}

#[test]
fn all_sixteen_target_eligibility_masks_follow_normative_class_order() {
    let fixture = fixture();
    let policy = DockPolicy::default();
    let specs = [
        TargetSpec::TabGap,
        TargetSpec::Center,
        TargetSpec::InnerEdge,
        TargetSpec::OuterEdge,
    ];

    for mask in 0_u8..16 {
        let records: Vec<_> = specs
            .iter()
            .copied()
            .enumerate()
            .map(|(index, spec)| {
                let availability = if mask & (1 << index) != 0 {
                    DropTargetAvailability::Available
                } else {
                    DropTargetAvailability::Unavailable(DropTargetUnavailable::AdapterUnavailable)
                };
                target_record(
                    &fixture,
                    spec,
                    availability,
                    rect(10.0, 10.0, 80.0, 80.0),
                    1,
                )
            })
            .collect();
        let expected = specs
            .iter()
            .copied()
            .enumerate()
            .find(|(index, _)| mask & (1 << index) != 0)
            .map(|(_, spec)| {
                target_record(
                    &fixture,
                    spec,
                    DropTargetAvailability::Available,
                    rect(10.0, 10.0, 80.0, 80.0),
                    1,
                )
                .id()
            });
        let scene = seal_scene(&fixture, &policy, records);
        let resolution = resolve(
            &scene,
            &fixture,
            &policy,
            payload(&fixture, PayloadSpec::Item),
            point(50.0, 50.0),
        );

        match (expected, resolution) {
            (Some(expected), DropResolution::Resolved(resolved)) => {
                assert_eq!(resolved.target_id(), expected, "mask {mask:04b}");
            }
            (None, DropResolution::Rejected(rejected)) => {
                assert_eq!(rejected.candidates().len(), 4, "mask {mask:04b}");
            }
            pair => panic!("unexpected mask result {mask:04b}: {pair:?}"),
        }
    }
}

#[test]
fn payload_target_and_policy_matrix_is_prevalidated_by_u3() {
    let fixture = fixture();
    let payloads = [PayloadSpec::Item, PayloadSpec::Tabs, PayloadSpec::Subtree];
    let targets = [
        TargetSpec::TabGap,
        TargetSpec::Center,
        TargetSpec::InnerEdge,
        TargetSpec::OuterEdge,
    ];

    for payload_spec in payloads {
        for target_spec in targets {
            for enabled in [false, true] {
                let mut policy = DockPolicy::default();
                match target_spec {
                    TargetSpec::TabGap | TargetSpec::Center => {
                        policy.set_allow_tab_merge(enabled);
                    }
                    TargetSpec::InnerEdge | TargetSpec::OuterEdge => {
                        policy.set_allow_edge_split(enabled);
                    }
                }
                let scene = seal_scene(
                    &fixture,
                    &policy,
                    [target_record(
                        &fixture,
                        target_spec,
                        DropTargetAvailability::Available,
                        rect(0.0, 0.0, 100.0, 100.0),
                        1,
                    )],
                );
                let result = resolve(
                    &scene,
                    &fixture,
                    &policy,
                    payload(&fixture, payload_spec),
                    point(50.0, 50.0),
                );
                let split_into_tabs = payload_spec == PayloadSpec::Subtree
                    && matches!(target_spec, TargetSpec::TabGap | TargetSpec::Center);

                if enabled && !split_into_tabs {
                    assert!(
                        matches!(result, DropResolution::Resolved(_)),
                        "{payload_spec:?} x {target_spec:?} x enabled"
                    );
                } else {
                    assert!(
                        matches!(result, DropResolution::Rejected(_)),
                        "{payload_spec:?} x {target_spec:?} x enabled={enabled}"
                    );
                }
            }
        }
    }
}

#[test]
fn overlap_uses_frontmost_layer_then_lowest_structural_id() {
    let fixture = fixture();
    let policy = DockPolicy::default();
    let overlap = rect(0.0, 0.0, 100.0, 100.0);
    let back = center_record(&fixture, fixture.target_tabs_a, overlap, 2);
    let front = center_record(&fixture, fixture.target_tabs_b, overlap, 9);
    let scene = seal_scene(&fixture, &policy, [front.clone(), back]);
    let resolved = resolve(
        &scene,
        &fixture,
        &policy,
        payload(&fixture, PayloadSpec::Item),
        point(50.0, 50.0),
    );
    assert!(matches!(
        resolved,
        DropResolution::Resolved(resolved) if resolved.target_id() == front.id()
    ));

    let first = center_record(&fixture, fixture.target_tabs_a, overlap, 7);
    let second = center_record(&fixture, fixture.target_tabs_b, overlap, 7);
    let expected = first.id().min(second.id());
    let scene = seal_scene(&fixture, &policy, [second, first]);
    let resolved = resolve(
        &scene,
        &fixture,
        &policy,
        payload(&fixture, PayloadSpec::Item),
        point(50.0, 50.0),
    );
    assert!(matches!(
        resolved,
        DropResolution::Resolved(resolved) if resolved.target_id() == expected
    ));
}

#[test]
fn contained_chrome_occludes_lower_layer_drop_targets() {
    let fixture = fixture();
    let policy = DockPolicy::default();
    let background = target_record(
        &fixture,
        TargetSpec::TabGap,
        DropTargetAvailability::Available,
        rect(0.0, 0.0, 100.0, 100.0),
        1,
    );
    let scene = seal_scene_with_occlusions(
        &fixture,
        &policy,
        [background],
        [floating_occlusion(rect(20.0, 20.0, 60.0, 60.0), 2)],
    );

    assert!(matches!(
        resolve(
            &scene,
            &fixture,
            &policy,
            payload(&fixture, PayloadSpec::Item),
            point(25.0, 25.0),
        ),
        DropResolution::KnownNone(_)
    ));
}

#[test]
fn front_contained_layer_wins_before_back_target_class_priority() {
    let fixture = fixture();
    let policy = DockPolicy::default();
    let overlap = rect(30.0, 40.0, 40.0, 30.0);
    let background = target_record(
        &fixture,
        TargetSpec::TabGap,
        DropTargetAvailability::Available,
        overlap,
        1,
    );
    let foreground = floating_outer_edge_record(&fixture, overlap, 2);
    let foreground_id = foreground.id();
    let scene = seal_scene_with_occlusions(
        &fixture,
        &policy,
        [background, foreground],
        [floating_occlusion(rect(20.0, 20.0, 60.0, 60.0), 2)],
    );

    assert!(matches!(
        resolve(
            &scene,
            &fixture,
            &policy,
            payload(&fixture, PayloadSpec::Item),
            point(50.0, 50.0),
        ),
        DropResolution::Resolved(resolved) if resolved.target_id() == foreground_id
    ));
}

#[test]
fn rejected_front_candidate_never_falls_through_its_occlusion() {
    let fixture = fixture();
    let policy = DockPolicy::default();
    let overlap = rect(30.0, 40.0, 40.0, 30.0);
    let background = target_record(
        &fixture,
        TargetSpec::OuterEdge,
        DropTargetAvailability::Available,
        overlap,
        1,
    );
    let foreground = floating_center_record(&fixture, overlap, 2);
    let foreground_id = foreground.id();
    let scene = seal_scene_with_occlusions(
        &fixture,
        &policy,
        [background, foreground],
        [floating_occlusion(rect(20.0, 20.0, 60.0, 60.0), 2)],
    );

    let DropResolution::Rejected(rejected) = resolve(
        &scene,
        &fixture,
        &policy,
        payload(&fixture, PayloadSpec::Subtree),
        point(50.0, 50.0),
    ) else {
        panic!("the visible center target must reject a subtree without exposing the back layer");
    };
    assert_eq!(rejected.candidates().len(), 1);
    assert_eq!(rejected.candidates()[0].target_id(), foreground_id);
}

#[test]
fn targets_remain_resolvable_outside_contained_occlusion() {
    let fixture = fixture();
    let policy = DockPolicy::default();
    let background = target_record(
        &fixture,
        TargetSpec::Center,
        DropTargetAvailability::Available,
        rect(0.0, 0.0, 100.0, 100.0),
        1,
    );
    let background_id = background.id();
    let scene = seal_scene_with_occlusions(
        &fixture,
        &policy,
        [background],
        [floating_occlusion(rect(20.0, 20.0, 60.0, 60.0), 2)],
    );

    assert!(matches!(
        resolve(
            &scene,
            &fixture,
            &policy,
            payload(&fixture, PayloadSpec::Item),
            point(10.0, 10.0),
        ),
        DropResolution::Resolved(resolved) if resolved.target_id() == background_id
    ));
}

#[test]
fn half_open_regions_assign_shared_edges_once_and_exclude_all_max_edges() {
    let fixture = fixture();
    let policy = DockPolicy::default();
    let left = center_record(
        &fixture,
        fixture.target_tabs_a,
        rect(0.0, 0.0, 50.0, 100.0),
        1,
    );
    let right = center_record(
        &fixture,
        fixture.target_tabs_b,
        rect(50.0, 0.0, 50.0, 100.0),
        1,
    );
    let left_id = left.id();
    let right_id = right.id();
    let scene = seal_scene(&fixture, &policy, [right, left]);

    for (at, expected) in [
        (point(0.0, 0.0), Some(left_id)),
        (point(49.999_999, 99.999_999), Some(left_id)),
        (point(50.0, 20.0), Some(right_id)),
        (point(99.999_999, 99.999_999), Some(right_id)),
        (point(100.0, 20.0), None),
        (point(20.0, 100.0), None),
    ] {
        let result = resolve(
            &scene,
            &fixture,
            &policy,
            payload(&fixture, PayloadSpec::Item),
            at,
        );
        match expected {
            Some(expected) => assert!(matches!(
                result,
                DropResolution::Resolved(resolved) if resolved.target_id() == expected
            )),
            None => assert!(matches!(result, DropResolution::KnownNone(_))),
        }
    }
}

#[test]
fn zero_area_target_has_no_geometric_hit() {
    let fixture = fixture();
    let policy = DockPolicy::default();
    let target = target_record(
        &fixture,
        TargetSpec::Center,
        DropTargetAvailability::Available,
        rect(0.0, 0.0, 100.0, 100.0),
        1,
    );
    let zero_hit = DropTargetRecord::new(
        target.id(),
        target.target().clone(),
        target.availability(),
        HitRegion::new(rect(40.0, 10.0, 0.0, 80.0)),
        target.layer(),
        DropVisual::new(rect(40.0, 10.0, 10.0, 80.0)),
    );
    let scene = seal_scene(&fixture, &policy, [zero_hit]);

    assert!(matches!(
        resolve(
            &scene,
            &fixture,
            &policy,
            payload(&fixture, PayloadSpec::Item),
            point(40.0, 40.0),
        ),
        DropResolution::KnownNone(_)
    ));
}

#[test]
fn missing_bootstrap_and_ready_surfaces_are_distinct() {
    let fixture = fixture();
    let policy = DockPolicy::default();
    let bootstrap_building = BuildingScene::new([TARGET_SURFACE]).expect("valid roster");
    let bootstrap =
        publish_building(&fixture, &policy, bootstrap_building).expect("bootstrap scene is valid");
    let missing_building = BuildingScene::new([]).expect("valid empty roster");
    let missing =
        publish_building(&fixture, &policy, missing_building).expect("empty scene is valid");
    let ready = seal_scene(&fixture, &policy, []);
    let source = payload(&fixture, PayloadSpec::Item);

    let bootstrap_result = resolve(
        &bootstrap,
        &fixture,
        &policy,
        source.clone(),
        point(10.0, 10.0),
    );
    assert!(matches!(
        bootstrap_result,
        DropResolution::Unavailable(unavailable)
            if unavailable.reason() == DropSurfaceUnavailable::Bootstrap
    ));
    let missing_result = resolve(
        &missing,
        &fixture,
        &policy,
        source.clone(),
        point(10.0, 10.0),
    );
    assert!(matches!(
        missing_result,
        DropResolution::Unavailable(unavailable)
            if unavailable.reason() == DropSurfaceUnavailable::MissingSurface
    ));
    assert!(matches!(
        resolve(&ready, &fixture, &policy, source, point(10.0, 10.0),),
        DropResolution::KnownNone(_)
    ));
}

#[test]
fn split_subtree_into_tabs_is_an_expected_prevalidation_rejection() {
    let fixture = fixture();
    let policy = DockPolicy::default();
    let scene = seal_scene(
        &fixture,
        &policy,
        [target_record(
            &fixture,
            TargetSpec::Center,
            DropTargetAvailability::Available,
            rect(0.0, 0.0, 100.0, 100.0),
            1,
        )],
    );

    let DropResolution::Rejected(rejected) = resolve(
        &scene,
        &fixture,
        &policy,
        payload(&fixture, PayloadSpec::Subtree),
        point(50.0, 50.0),
    ) else {
        panic!("split subtree must be rejected by exact command prevalidation");
    };
    assert!(matches!(
        rejected.candidates()[0].reason(),
        DropRejectionReason::Prevalidation(dockspace::error::TransactionError::Command {
            source: dockspace::error::CommandError::SplitPayloadIntoTabs { .. },
            ..
        })
    ));
}

#[test]
fn stale_target_is_retained_as_explicitly_unavailable() {
    let mut fixture = fixture();
    let policy = DockPolicy::default();
    let old_target = target_record(
        &fixture,
        TargetSpec::Center,
        DropTargetAvailability::Available,
        rect(0.0, 0.0, 100.0, 100.0),
        1,
    );
    let select = fixture
        .workspace
        .capture_item_source(TARGET_ROOT, fixture.target_tabs_a, ItemId::new(11))
        .expect("selection source exists");
    WorkspaceTransaction::from_commands([dockspace::command::WorkspaceCommand::Select {
        source: select,
    }])
    .apply(&mut fixture.workspace, &policy)
    .expect("selection changes target fingerprint");

    let scene = seal_scene(&fixture, &policy, [old_target]);
    let SurfaceScene::Ready(ready) = scene
        .surface(TARGET_SURFACE)
        .expect("target surface is rostered")
    else {
        panic!("target surface must be ready");
    };
    assert_eq!(
        ready.drop_targets()[0].availability(),
        DropTargetAvailability::Unavailable(DropTargetUnavailable::Stale)
    );
}

fn permutations(values: &mut [usize], start: usize, output: &mut Vec<Vec<usize>>) {
    if start == values.len() {
        output.push(values.to_vec());
        return;
    }
    for index in start..values.len() {
        values.swap(start, index);
        permutations(values, start + 1, output);
        values.swap(start, index);
    }
}

fn semantic_ready(fixture: &Fixture, reverse: bool) -> ReadySurfaceScene {
    let bounds = rect(0.0, 0.0, 20.0, 20.0);
    let mut ready = ReadySurfaceScene::new(TARGET_SURFACE, rect(0.0, 0.0, 100.0, 100.0));
    let mut nodes = vec![
        SemanticRect::new(
            NodeSceneId {
                root: TARGET_ROOT,
                node: fixture.target_root_node,
            },
            bounds,
            SceneLayerKey::new(0),
        ),
        SemanticRect::new(
            NodeSceneId {
                root: TARGET_ROOT,
                node: fixture.target_tabs_a,
            },
            bounds,
            SceneLayerKey::new(1),
        ),
        SemanticRect::new(
            NodeSceneId {
                root: TARGET_ROOT,
                node: fixture.target_tabs_b,
            },
            bounds,
            SceneLayerKey::new(2),
        ),
    ];
    if reverse {
        nodes.reverse();
    }
    for node in nodes {
        ready.push_node(node);
    }
    let tab_bars = if reverse {
        [fixture.target_tabs_b, fixture.target_tabs_a]
    } else {
        [fixture.target_tabs_a, fixture.target_tabs_b]
    };
    for tabs in tab_bars {
        ready.push_tab_bar(SemanticRect::new(
            TabBarSceneId {
                root: TARGET_ROOT,
                tabs,
            },
            bounds,
            SceneLayerKey::new(3),
        ));
    }
    let tabs = if reverse {
        [
            (fixture.target_tabs_b, ItemId::new(12)),
            (fixture.target_tabs_a, ItemId::new(10)),
        ]
    } else {
        [
            (fixture.target_tabs_a, ItemId::new(10)),
            (fixture.target_tabs_b, ItemId::new(12)),
        ]
    };
    for (tabs, item) in tabs {
        ready.push_tab(SemanticRect::new(
            TabSceneId {
                root: TARGET_ROOT,
                tabs,
                item,
            },
            bounds,
            SceneLayerKey::new(4),
        ));
    }
    ready.push_splitter(SemanticRect::new(
        SplitterSceneId {
            root: TARGET_ROOT,
            split: fixture.target_root_node,
            index: 0,
        },
        bounds,
        SceneLayerKey::new(5),
    ));
    ready
}

#[test]
fn every_target_insertion_permutation_seals_and_resolves_identically() {
    let fixture = fixture();
    let policy = DockPolicy::default();
    let records = [
        target_record(
            &fixture,
            TargetSpec::TabGap,
            DropTargetAvailability::Available,
            rect(0.0, 0.0, 100.0, 100.0),
            1,
        ),
        target_record(
            &fixture,
            TargetSpec::Center,
            DropTargetAvailability::Available,
            rect(0.0, 0.0, 100.0, 100.0),
            9,
        ),
        target_record(
            &fixture,
            TargetSpec::InnerEdge,
            DropTargetAvailability::Available,
            rect(0.0, 0.0, 100.0, 100.0),
            20,
        ),
        target_record(
            &fixture,
            TargetSpec::OuterEdge,
            DropTargetAvailability::Available,
            rect(0.0, 0.0, 100.0, 100.0),
            30,
        ),
    ];
    let mut orders = Vec::new();
    permutations(&mut [0, 1, 2, 3], 0, &mut orders);
    let mut reference_scene = None;

    for order in orders {
        let scene = seal_scene(
            &fixture,
            &policy,
            order.into_iter().map(|index| records[index].clone()),
        );
        if let Some(reference) = &reference_scene {
            assert_eq!(&scene, reference);
        } else {
            reference_scene = Some(scene.clone());
        }
        let result = resolve(
            &scene,
            &fixture,
            &policy,
            payload(&fixture, PayloadSpec::Item),
            point(50.0, 50.0),
        );
        assert!(matches!(
            result,
            DropResolution::Resolved(resolved)
                if resolved.target_id().kind() == dockspace::drop_target::DropTargetKind::TabGap
        ));
    }
}

#[test]
fn semantic_fact_order_is_canonical_and_sealed_snapshot_is_independent() {
    let fixture = fixture();
    let policy = DockPolicy::default();

    let forward = semantic_ready(&fixture, false);
    let retained_adapter_copy = forward.clone();
    let mut forward_build = BuildingScene::new([TARGET_SURFACE]).expect("valid roster");
    forward_build
        .insert_ready(forward)
        .expect("forward facts are unique");
    let forward_scene =
        publish_building(&fixture, &policy, forward_build).expect("forward scene is valid");

    let mut reverse_build = BuildingScene::new([TARGET_SURFACE]).expect("valid roster");
    reverse_build
        .insert_ready(semantic_ready(&fixture, true))
        .expect("reverse facts are unique");
    let reverse_scene =
        publish_building(&fixture, &policy, reverse_build).expect("reverse scene is valid");

    assert_eq!(forward_scene, reverse_scene);
    assert_eq!(retained_adapter_copy.nodes().len(), 3);
}

#[test]
fn malformed_semantic_facts_and_target_id_mismatches_fail_seal() {
    let fixture = fixture();
    let policy = DockPolicy::default();
    let bounds = rect(0.0, 0.0, 100.0, 100.0);

    let mut invalid_node = ReadySurfaceScene::new(TARGET_SURFACE, bounds);
    invalid_node.push_node(SemanticRect::new(
        NodeSceneId {
            root: SOURCE_ROOT,
            node: fixture.source_tabs,
        },
        bounds,
        SceneLayerKey::new(0),
    ));
    assert_scene_error(&fixture, &policy, invalid_node, |error| {
        matches!(error, SceneBuildError::InvalidNodeSemantic { .. })
    });

    let mut invalid_bar = ReadySurfaceScene::new(TARGET_SURFACE, bounds);
    invalid_bar.push_tab_bar(SemanticRect::new(
        TabBarSceneId {
            root: TARGET_ROOT,
            tabs: fixture.target_root_node,
        },
        bounds,
        SceneLayerKey::new(0),
    ));
    assert_scene_error(&fixture, &policy, invalid_bar, |error| {
        matches!(error, SceneBuildError::InvalidTabBarSemantic { .. })
    });

    let mut invalid_tab = ReadySurfaceScene::new(TARGET_SURFACE, bounds);
    invalid_tab.push_tab(SemanticRect::new(
        TabSceneId {
            root: TARGET_ROOT,
            tabs: fixture.target_tabs_a,
            item: ItemId::new(999),
        },
        bounds,
        SceneLayerKey::new(0),
    ));
    assert_scene_error(&fixture, &policy, invalid_tab, |error| {
        matches!(error, SceneBuildError::InvalidTabSemantic { .. })
    });

    let mut invalid_splitter = ReadySurfaceScene::new(TARGET_SURFACE, bounds);
    invalid_splitter.push_splitter(SemanticRect::new(
        SplitterSceneId {
            root: TARGET_ROOT,
            split: fixture.target_tabs_a,
            index: 0,
        },
        bounds,
        SceneLayerKey::new(0),
    ));
    assert_scene_error(&fixture, &policy, invalid_splitter, |error| {
        matches!(error, SceneBuildError::InvalidSplitterSemantic { .. })
    });

    let mut invalid_occlusion = ReadySurfaceScene::new(TARGET_SURFACE, bounds);
    invalid_occlusion.push_drop_occlusion(DropOcclusionRecord::new(
        FloatingPresentationId::new(999),
        HitRegion::new(bounds),
        SceneLayerKey::new(1),
    ));
    assert_scene_error(&fixture, &policy, invalid_occlusion, |error| {
        matches!(error, SceneBuildError::InvalidDropOcclusion { .. })
    });

    let edge = target_record(
        &fixture,
        TargetSpec::InnerEdge,
        DropTargetAvailability::Available,
        bounds,
        0,
    );
    let mismatched = DropTargetRecord::new(
        DropTargetId::Center {
            surface: TARGET_SURFACE,
            root: TARGET_ROOT,
            tabs: fixture.target_tabs_a,
        },
        edge.target().clone(),
        DropTargetAvailability::Available,
        HitRegion::new(bounds),
        SceneLayerKey::new(0),
        DropVisual::new(bounds),
    );
    let mut invalid_target = ReadySurfaceScene::new(TARGET_SURFACE, bounds);
    invalid_target.push_drop_target(mismatched);
    assert_scene_error(&fixture, &policy, invalid_target, |error| {
        matches!(error, SceneBuildError::TargetSemanticMismatch { .. })
    });
}

#[test]
fn invalid_inner_edges_and_unpaintable_visuals_fail_seal() {
    let fixture = fixture();
    let policy = DockPolicy::default();
    let bounds = rect(0.0, 0.0, 100.0, 100.0);
    let split_edge = fixture
        .workspace
        .capture_edge_target(
            TARGET_ROOT,
            fixture.target_root_node,
            Edge::Left,
            DockFraction::new(0.35).expect("valid fraction"),
        )
        .expect("split edge reference is current");
    let mut invalid_inner_edge = ReadySurfaceScene::new(TARGET_SURFACE, bounds);
    invalid_inner_edge.push_drop_target(DropTargetRecord::new(
        DropTargetId::InnerEdge {
            surface: TARGET_SURFACE,
            root: TARGET_ROOT,
            node: fixture.target_root_node,
            edge: Edge::Left,
        },
        DockTarget::Edge(split_edge),
        DropTargetAvailability::Available,
        HitRegion::new(bounds),
        SceneLayerKey::new(0),
        DropVisual::new(bounds),
    ));
    assert_scene_error(&fixture, &policy, invalid_inner_edge, |error| {
        matches!(error, SceneBuildError::TargetSemanticMismatch { .. })
    });

    let center = center_record(&fixture, fixture.target_tabs_a, bounds, 0);
    let mut empty_visual = ReadySurfaceScene::new(TARGET_SURFACE, bounds);
    empty_visual.push_drop_target(DropTargetRecord::new(
        center.id(),
        center.target().clone(),
        center.availability(),
        center.region(),
        center.layer(),
        DropVisual::new(rect(10.0, 10.0, 0.0, 30.0)),
    ));
    assert_scene_error(&fixture, &policy, empty_visual, |error| {
        matches!(error, SceneBuildError::EmptyDropVisual { .. })
    });

    let mut outside_visual = ReadySurfaceScene::new(TARGET_SURFACE, bounds);
    outside_visual.push_drop_target(DropTargetRecord::new(
        center.id(),
        center.target().clone(),
        center.availability(),
        center.region(),
        center.layer(),
        DropVisual::new(rect(90.0, 10.0, 20.0, 30.0)),
    ));
    assert_scene_error(&fixture, &policy, outside_visual, |error| {
        matches!(error, SceneBuildError::DropVisualOutsideSurface { .. })
    });
}

#[test]
fn duplicate_ready_and_semantic_ids_are_rejected_before_seal() {
    let fixture = fixture();
    let bounds = rect(0.0, 0.0, 100.0, 100.0);
    let duplicate_id = NodeSceneId {
        root: TARGET_ROOT,
        node: fixture.target_tabs_a,
    };
    let mut ready = ReadySurfaceScene::new(TARGET_SURFACE, bounds);
    ready.push_node(SemanticRect::new(
        duplicate_id,
        bounds,
        SceneLayerKey::new(0),
    ));
    ready.push_node(SemanticRect::new(
        duplicate_id,
        bounds,
        SceneLayerKey::new(1),
    ));
    let mut building = BuildingScene::new([TARGET_SURFACE]).expect("valid roster");
    assert!(matches!(
        building.insert_ready(ready),
        Err(SceneBuildError::DuplicateNode { id }) if id == duplicate_id
    ));

    let mut duplicate_occlusion = ReadySurfaceScene::new(TARGET_SURFACE, bounds);
    let occlusion = floating_occlusion(bounds, 1);
    duplicate_occlusion.push_drop_occlusion(occlusion);
    duplicate_occlusion.push_drop_occlusion(occlusion);
    assert!(matches!(
        building.insert_ready(duplicate_occlusion),
        Err(SceneBuildError::DuplicateDropOcclusion { floating }) if floating == FLOATING
    ));

    let ready = ReadySurfaceScene::new(TARGET_SURFACE, bounds);
    building
        .insert_ready(ready.clone())
        .expect("first ready fact succeeds");
    assert!(matches!(
        building.insert_ready(ready),
        Err(SceneBuildError::DuplicateReadySurface { surface }) if surface == TARGET_SURFACE
    ));
}
