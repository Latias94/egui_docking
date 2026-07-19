use dockspace::command::{DockFraction, DockTarget, Edge, MovePayload};
use dockspace::drop_guide::{
    DropGuideClusterRecord, DropGuideEdgeSet, DropGuideScope, DropGuideSlot, DropGuideTargetRecord,
};
use dockspace::drop_resolver::{
    DropGuideEligibility, DropRejectionReason, DropResolution, query_drop, resolve_drop,
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
use dockspace::policy::{DockPolicy, PolicyRejection};
use dockspace::scene::{BuildingScene, ReadySurfaceScene, SealedScene};

const SOURCE_ROOT: RootId = RootId::new(1);
const TARGET_ROOT: RootId = RootId::new(2);
const FLOATING_ROOT: RootId = RootId::new(3);
const OTHER_FLOATING_ROOT: RootId = RootId::new(4);
const SOURCE_SURFACE: SurfaceId = SurfaceId::new(1);
const TARGET_SURFACE: SurfaceId = SurfaceId::new(2);
const FLOATING: FloatingPresentationId = FloatingPresentationId::new(1);
const OTHER_FLOATING: FloatingPresentationId = FloatingPresentationId::new(2);

#[derive(Debug)]
struct Fixture {
    workspace: Workspace,
    source_tabs: NodeId,
    source_root_node: NodeId,
    target_root_node: NodeId,
    target_tabs: NodeId,
    floating_root_node: NodeId,
    floating_tabs: NodeId,
}

fn fixture() -> Fixture {
    fixture_with_floating_tabs([ItemId::new(20)], false)
}

fn fixture_with_floating_tabs(items: impl IntoIterator<Item = ItemId>, central: bool) -> Fixture {
    build_fixture(items.into_iter().collect(), false, central)
}

fn fixture_with_split_floating() -> Fixture {
    build_fixture(vec![ItemId::new(20)], true, false)
}

fn build_fixture(floating_items: Vec<ItemId>, split_floating: bool, central: bool) -> Fixture {
    let mut builder = WorkspaceBuilder::new();

    let source_tabs_a = builder.insert_node(Node::tabs([ItemId::new(1)]));
    let source_tabs_b = builder.insert_node(Node::tabs([ItemId::new(2)]));
    let source_root_node = builder.insert_node(
        Node::equal_split(Axis::Vertical, [source_tabs_a, source_tabs_b])
            .expect("source split is valid"),
    );

    let target_tabs = builder.insert_node(Node::tabs([ItemId::new(10)]));
    let target_sibling = builder.insert_node(Node::tabs([ItemId::new(11)]));
    let target_root_node = builder.insert_node(
        Node::equal_split(Axis::Horizontal, [target_tabs, target_sibling])
            .expect("target split is valid"),
    );
    let floating_tabs = builder.insert_node(Node::tabs(floating_items));
    let floating_root_node = if split_floating {
        let floating_sibling = builder.insert_node(Node::tabs([ItemId::new(21)]));
        builder.insert_node(
            Node::equal_split(Axis::Horizontal, [floating_tabs, floating_sibling])
                .expect("floating split is valid"),
        )
    } else {
        floating_tabs
    };
    let other_floating_tabs = builder.insert_node(Node::tabs([ItemId::new(30)]));

    builder.set_root(SOURCE_ROOT, RootRecord::new(source_root_node));
    builder.set_root(TARGET_ROOT, RootRecord::new(target_root_node));
    let floating_record = if central {
        RootRecord::new(floating_root_node).with_central(floating_tabs)
    } else {
        RootRecord::new(floating_root_node)
    };
    builder.set_root(FLOATING_ROOT, floating_record);
    builder.set_root(OTHER_FLOATING_ROOT, RootRecord::new(other_floating_tabs));
    builder.set_surface(SOURCE_SURFACE, SurfacePresentation::new(SOURCE_ROOT));
    builder.set_surface(TARGET_SURFACE, SurfacePresentation::new(TARGET_ROOT));
    builder.set_contained_floating(ContainedFloating::new(
        FLOATING,
        FLOATING_ROOT,
        TARGET_SURFACE,
        rect(20.0, 20.0, 60.0, 60.0),
        2,
    ));
    builder.set_contained_floating(ContainedFloating::new(
        OTHER_FLOATING,
        OTHER_FLOATING_ROOT,
        TARGET_SURFACE,
        rect(20.0, 20.0, 60.0, 60.0),
        3,
    ));
    builder
        .attach_contained(TARGET_SURFACE, FLOATING)
        .expect("target surface exists");
    builder
        .attach_contained(TARGET_SURFACE, OTHER_FLOATING)
        .expect("target surface exists");

    Fixture {
        workspace: builder.build().expect("fixture workspace is valid"),
        source_tabs: source_tabs_b,
        source_root_node,
        target_root_node,
        target_tabs,
        floating_root_node,
        floating_tabs,
    }
}

fn rect(x: f64, y: f64, width: f64, height: f64) -> LogicalRect {
    LogicalRect::new(x, y, width, height).expect("test rectangle is valid")
}

fn point(x: f64, y: f64) -> LogicalPoint {
    LogicalPoint::new(x, y).expect("test point is finite")
}

fn session() -> DragSessionId {
    DragSessionId::new(WorkspaceEpoch::default(), DragGeneration::new(1))
}

fn item_payload(fixture: &Fixture) -> MovePayload {
    MovePayload::Item(
        fixture
            .workspace
            .capture_item_source(SOURCE_ROOT, fixture.source_tabs, ItemId::new(2))
            .expect("source item is current"),
    )
}

fn subtree_payload(fixture: &Fixture) -> MovePayload {
    MovePayload::Subtree(
        fixture
            .workspace
            .capture_node_source(SOURCE_ROOT, fixture.source_root_node)
            .expect("source subtree is current"),
    )
}

fn floating_item_payload(fixture: &Fixture) -> MovePayload {
    MovePayload::Item(
        fixture
            .workspace
            .capture_item_source(FLOATING_ROOT, fixture.floating_tabs, ItemId::new(20))
            .expect("floating item is current"),
    )
}

fn floating_tabs_payload(fixture: &Fixture) -> MovePayload {
    MovePayload::Tabs(
        fixture
            .workspace
            .capture_node_source(FLOATING_ROOT, fixture.floating_tabs)
            .expect("floating tabs are current"),
    )
}

fn floating_subtree_payload(fixture: &Fixture, node: NodeId) -> MovePayload {
    MovePayload::Subtree(
        fixture
            .workspace
            .capture_node_source(FLOATING_ROOT, node)
            .expect("floating subtree is current"),
    )
}

fn guide_target(
    id: DropTargetId,
    target: DockTarget,
    hit: LogicalRect,
    preview: LogicalRect,
    layer: SceneLayerKey,
) -> DropGuideTargetRecord {
    DropGuideTargetRecord::new(
        DropTargetRecord::new(
            id,
            target,
            DropTargetAvailability::Available,
            HitRegion::new(hit),
            layer,
            DropVisual::new(preview),
        ),
        hit,
    )
}

fn edge_guide_target(
    fixture: &Fixture,
    edge: Edge,
    spec: EdgeGuideTargetSpec,
) -> DropGuideTargetRecord {
    let id = if spec.outer {
        DropTargetId::OuterEdge {
            surface: TARGET_SURFACE,
            root: spec.root,
            node: spec.node,
            edge,
        }
    } else {
        DropTargetId::InnerEdge {
            surface: TARGET_SURFACE,
            root: spec.root,
            node: spec.node,
            edge,
        }
    };
    guide_target(
        id,
        DockTarget::Edge(
            fixture
                .workspace
                .capture_edge_target(
                    spec.root,
                    spec.node,
                    edge,
                    DockFraction::new(0.35).expect("fraction is valid"),
                )
                .expect("edge target is current"),
        ),
        spec.hit,
        spec.preview,
        spec.layer,
    )
}

#[derive(Debug, Clone, Copy)]
struct EdgeGuideTargetSpec {
    root: RootId,
    node: NodeId,
    outer: bool,
    hit: LogicalRect,
    preview: LogicalRect,
    layer: SceneLayerKey,
}

fn edge_set(
    fixture: &Fixture,
    root: RootId,
    node: NodeId,
    outer: bool,
    hits: [LogicalRect; 4],
    preview: LogicalRect,
    layer: SceneLayerKey,
) -> DropGuideEdgeSet {
    let spec = |hit| EdgeGuideTargetSpec {
        root,
        node,
        outer,
        hit,
        preview,
        layer,
    };
    DropGuideEdgeSet::new(
        edge_guide_target(fixture, Edge::Left, spec(hits[0])),
        edge_guide_target(fixture, Edge::Right, spec(hits[1])),
        edge_guide_target(fixture, Edge::Top, spec(hits[2])),
        edge_guide_target(fixture, Edge::Bottom, spec(hits[3])),
    )
}

fn target_inner_cluster(fixture: &Fixture) -> DropGuideClusterRecord {
    let layer = SceneLayerKey::new(1);
    let preview = rect(0.0, 0.0, 50.0, 100.0);
    let center = guide_target(
        DropTargetId::Center {
            surface: TARGET_SURFACE,
            root: TARGET_ROOT,
            tabs: fixture.target_tabs,
        },
        DockTarget::Center(
            fixture
                .workspace
                .capture_tab_target(TARGET_ROOT, fixture.target_tabs)
                .expect("center target is current"),
        ),
        rect(21.0, 46.0, 8.0, 8.0),
        preview,
        layer,
    );
    let edges = edge_set(
        fixture,
        TARGET_ROOT,
        fixture.target_tabs,
        false,
        [
            rect(9.0, 46.0, 8.0, 8.0),
            rect(33.0, 46.0, 8.0, 8.0),
            rect(21.0, 34.0, 8.0, 8.0),
            rect(21.0, 58.0, 8.0, 8.0),
        ],
        preview,
        layer,
    );
    DropGuideClusterRecord::inner(
        TARGET_SURFACE,
        TARGET_ROOT,
        fixture.target_tabs,
        HitRegion::new(preview),
        layer,
        center,
        edges,
    )
}

fn target_outer_cluster(fixture: &Fixture) -> DropGuideClusterRecord {
    target_outer_cluster_with_hits(
        fixture,
        [
            rect(1.0, 46.0, 8.0, 8.0),
            rect(91.0, 46.0, 8.0, 8.0),
            rect(46.0, 1.0, 8.0, 8.0),
            rect(46.0, 91.0, 8.0, 8.0),
        ],
    )
}

fn target_outer_cluster_with_hits(
    fixture: &Fixture,
    hits: [LogicalRect; 4],
) -> DropGuideClusterRecord {
    let layer = SceneLayerKey::new(1);
    let bounds = rect(0.0, 0.0, 100.0, 100.0);
    DropGuideClusterRecord::outer(
        TARGET_SURFACE,
        TARGET_ROOT,
        HitRegion::new(bounds),
        layer,
        edge_set(
            fixture,
            TARGET_ROOT,
            fixture.target_root_node,
            true,
            hits,
            bounds,
            layer,
        ),
    )
}

fn floating_inner_cluster(fixture: &Fixture) -> DropGuideClusterRecord {
    let layer = SceneLayerKey::new(2);
    let activation = rect(20.0, 20.0, 60.0, 60.0);
    let center = guide_target(
        DropTargetId::Center {
            surface: TARGET_SURFACE,
            root: FLOATING_ROOT,
            tabs: fixture.floating_tabs,
        },
        DockTarget::Center(
            fixture
                .workspace
                .capture_tab_target(FLOATING_ROOT, fixture.floating_tabs)
                .expect("floating center target is current"),
        ),
        rect(21.0, 46.0, 8.0, 8.0),
        activation,
        layer,
    );
    DropGuideClusterRecord::inner(
        TARGET_SURFACE,
        FLOATING_ROOT,
        fixture.floating_tabs,
        HitRegion::new(activation),
        layer,
        center,
        edge_set(
            fixture,
            FLOATING_ROOT,
            fixture.floating_tabs,
            fixture.floating_root_node == fixture.floating_tabs,
            [
                rect(33.0, 46.0, 8.0, 8.0),
                rect(57.0, 46.0, 8.0, 8.0),
                rect(45.0, 34.0, 8.0, 8.0),
                rect(45.0, 58.0, 8.0, 8.0),
            ],
            activation,
            layer,
        ),
    )
}

fn legacy_center_record(
    fixture: &Fixture,
    root: RootId,
    tabs: NodeId,
    region: LogicalRect,
    layer: SceneLayerKey,
) -> DropTargetRecord {
    DropTargetRecord::new(
        DropTargetId::Center {
            surface: TARGET_SURFACE,
            root,
            tabs,
        },
        DockTarget::Center(
            fixture
                .workspace
                .capture_tab_target(root, tabs)
                .expect("legacy center target is current"),
        ),
        DropTargetAvailability::Available,
        HitRegion::new(region),
        layer,
        DropVisual::new(region),
    )
}

fn tab_gap_record(fixture: &Fixture, index: usize, region: LogicalRect) -> DropTargetRecord {
    DropTargetRecord::new(
        DropTargetId::TabGap {
            surface: TARGET_SURFACE,
            root: TARGET_ROOT,
            tabs: fixture.target_tabs,
            index,
        },
        DockTarget::TabGap {
            target: fixture
                .workspace
                .capture_tab_target(TARGET_ROOT, fixture.target_tabs)
                .expect("tab target is current"),
            index,
        },
        DropTargetAvailability::Available,
        HitRegion::new(region),
        SceneLayerKey::new(1),
        DropVisual::new(region),
    )
}

fn seal_scene(
    fixture: &Fixture,
    clusters: impl IntoIterator<Item = DropGuideClusterRecord>,
    targets: impl IntoIterator<Item = DropTargetRecord>,
    occlusions: impl IntoIterator<Item = DropOcclusionRecord>,
) -> SealedScene {
    seal_scene_with_policy(
        fixture,
        clusters,
        targets,
        occlusions,
        DockPolicy::default(),
    )
}

fn seal_scene_with_policy(
    fixture: &Fixture,
    clusters: impl IntoIterator<Item = DropGuideClusterRecord>,
    targets: impl IntoIterator<Item = DropTargetRecord>,
    occlusions: impl IntoIterator<Item = DropOcclusionRecord>,
    policy: DockPolicy,
) -> SealedScene {
    let mut ready = ReadySurfaceScene::new(TARGET_SURFACE, rect(0.0, 0.0, 100.0, 100.0));
    for cluster in clusters {
        ready.push_drop_guide_cluster(cluster);
    }
    for target in targets {
        ready.push_drop_target(target);
    }
    for occlusion in occlusions {
        ready.push_drop_occlusion(occlusion);
    }

    let mut building = BuildingScene::new([TARGET_SURFACE]).expect("roster is valid");
    building
        .insert_ready(ready)
        .expect("surface facts are unique");
    let mut engine =
        DockEngine::new(fixture.workspace.clone(), policy).expect("fixture engine is valid");
    engine
        .enqueue_scene(building)
        .expect("scene sequence is available");
    engine
        .reduce_pending()
        .expect("scene publication is not fatal");
    engine.scene().expect("scene was published").clone()
}

#[test]
fn scene_unavailable_button_remains_active_disabled() {
    let fixture = fixture();
    let mut scene_policy = DockPolicy::default();
    scene_policy.set_allow_edge_split(false);
    let scene = seal_scene_with_policy(
        &fixture,
        [target_inner_cluster(&fixture)],
        [],
        [],
        scene_policy,
    );

    let result = query(
        &fixture,
        &scene,
        &DockPolicy::default(),
        item_payload(&fixture),
        point(25.0, 38.0),
    );
    let active = result
        .affordance()
        .and_then(|affordance| affordance.active_target())
        .expect("unavailable guide remains active");

    assert!(matches!(
        active.eligibility(),
        DropGuideEligibility::Rejected(DropRejectionReason::SceneUnavailable(
            DropTargetUnavailable::PolicyDisabled
        ))
    ));
    assert!(matches!(result.resolution(), DropResolution::Rejected(_)));
}

fn query(
    fixture: &Fixture,
    scene: &SealedScene,
    policy: &DockPolicy,
    payload: MovePayload,
    at: LogicalPoint,
) -> dockspace::drop_resolver::DropQuery {
    query_drop(
        scene,
        &fixture.workspace,
        policy,
        session(),
        payload,
        TARGET_SURFACE,
        at,
    )
    .expect("query cannot expose an invariant failure")
}

fn main_and_floating_scene(fixture: &Fixture) -> SealedScene {
    seal_scene(
        fixture,
        [
            target_inner_cluster(fixture),
            floating_inner_cluster(fixture),
        ],
        [],
        [DropOcclusionRecord::new(
            FLOATING,
            HitRegion::new(rect(20.0, 20.0, 60.0, 60.0)),
            SceneLayerKey::new(2),
        )],
    )
}

fn assert_active_guide_root(fixture: &Fixture, payload: MovePayload, expected: RootId) {
    let scene = main_and_floating_scene(fixture);
    let result = query(
        fixture,
        &scene,
        &DockPolicy::default(),
        payload,
        point(25.0, 50.0),
    );
    let active = result
        .affordance()
        .and_then(|affordance| affordance.active_target())
        .expect("one exact center guide is active");

    assert!(matches!(
        active.target_id(),
        DropTargetId::Center { root, .. } if root == expected
    ));
}

#[test]
fn selected_inner_and_matching_outer_clusters_are_complete_and_owned() {
    let fixture = fixture();
    let scene = seal_scene(
        &fixture,
        [
            target_inner_cluster(&fixture),
            target_outer_cluster(&fixture),
        ],
        [],
        [],
    );

    let result = query(
        &fixture,
        &scene,
        &DockPolicy::default(),
        item_payload(&fixture),
        point(30.0, 50.0),
    );
    drop(scene);
    let affordance = result.affordance().expect("inner activation shows guides");

    assert_eq!(affordance.clusters().len(), 2);
    assert_eq!(affordance.clusters()[0].id().scope, DropGuideScope::Outer);
    assert_eq!(affordance.clusters()[0].targets().len(), 4);
    assert!(matches!(
        affordance.clusters()[1].id().scope,
        DropGuideScope::Inner(node) if node == fixture.target_tabs
    ));
    assert_eq!(affordance.clusters()[1].targets().len(), 5);
    assert!(affordance.clusters().iter().all(|cluster| {
        cluster
            .targets()
            .iter()
            .all(|target| target.eligibility().is_eligible())
    }));
}

#[test]
fn outer_cluster_resolves_without_an_active_inner_cluster() {
    let fixture = fixture();
    let scene = seal_scene(&fixture, [target_outer_cluster(&fixture)], [], []);

    let result = query(
        &fixture,
        &scene,
        &DockPolicy::default(),
        item_payload(&fixture),
        point(50.0, 5.0),
    );
    let affordance = result
        .affordance()
        .expect("outer activation independently shows guides");
    let active = affordance
        .active_target()
        .expect("outer top button is active");

    assert_eq!(affordance.clusters().len(), 1);
    assert_eq!(affordance.clusters()[0].id().scope, DropGuideScope::Outer);
    assert_eq!(affordance.clusters()[0].targets().len(), 4);
    assert_eq!(active.slot(), DropGuideSlot::Edge(Edge::Top));
    assert!(matches!(
        result.resolution(),
        DropResolution::Resolved(resolved)
            if matches!(resolved.target_id(), DropTargetId::OuterEdge { edge: Edge::Top, .. })
    ));
}

#[test]
fn activation_without_button_hit_returns_guide_only_affordance() {
    let fixture = fixture();
    let scene = seal_scene(
        &fixture,
        [
            target_inner_cluster(&fixture),
            target_outer_cluster(&fixture),
        ],
        [],
        [],
    );

    let result = query(
        &fixture,
        &scene,
        &DockPolicy::default(),
        item_payload(&fixture),
        point(30.0, 50.0),
    );

    assert!(matches!(result.resolution(), DropResolution::KnownNone(_)));
    assert!(result.affordance().is_some());
    assert_eq!(
        result
            .affordance()
            .expect("guides are visible")
            .active_target(),
        None
    );
}

#[test]
fn exact_center_hit_is_active_and_resolved() {
    let fixture = fixture();
    let scene = seal_scene(&fixture, [target_inner_cluster(&fixture)], [], []);

    let result = query(
        &fixture,
        &scene,
        &DockPolicy::default(),
        item_payload(&fixture),
        point(25.0, 50.0),
    );
    let active = result
        .affordance()
        .and_then(|affordance| affordance.active_target())
        .expect("center button is active");

    assert_eq!(active.slot(), DropGuideSlot::Center);
    assert!(matches!(
        result.resolution(),
        DropResolution::Resolved(resolved) if resolved.target_id() == active.target_id()
    ));
}

#[test]
fn top_and_bottom_buttons_resolve_their_physical_edges() {
    let fixture = fixture();
    let scene = seal_scene(&fixture, [target_inner_cluster(&fixture)], [], []);

    for (at, expected_edge) in [
        (point(25.0, 38.0), Edge::Top),
        (point(25.0, 62.0), Edge::Bottom),
    ] {
        let result = query(
            &fixture,
            &scene,
            &DockPolicy::default(),
            item_payload(&fixture),
            at,
        );
        let active = result
            .affordance()
            .and_then(|affordance| affordance.active_target())
            .expect("edge button is active");

        assert_eq!(active.slot(), DropGuideSlot::Edge(expected_edge));
        assert!(matches!(
            result.resolution(),
            DropResolution::Resolved(resolved)
                if resolved.target_id()
                    == (DropTargetId::InnerEdge {
                        surface: TARGET_SURFACE,
                        root: TARGET_ROOT,
                        node: fixture.target_tabs,
                        edge: expected_edge,
                    })
        ));
    }
}

#[test]
fn rejected_exact_guide_does_not_fall_through_to_tab_gap() {
    let fixture = fixture();
    let top_hit = rect(21.0, 34.0, 8.0, 8.0);
    let scene = seal_scene(
        &fixture,
        [target_inner_cluster(&fixture)],
        [tab_gap_record(&fixture, 1, top_hit)],
        [],
    );
    let mut query_policy = DockPolicy::default();
    query_policy.set_allow_edge_split(false);

    let result = query(
        &fixture,
        &scene,
        &query_policy,
        item_payload(&fixture),
        point(25.0, 38.0),
    );
    let active = result
        .affordance()
        .and_then(|affordance| affordance.active_target())
        .expect("rejected guide remains active");

    assert!(matches!(
        active.eligibility(),
        DropGuideEligibility::Rejected(DropRejectionReason::Policy(
            PolicyRejection::EdgeSplitDisabled
        ))
    ));
    let DropResolution::Rejected(rejected) = result.resolution() else {
        panic!("exact guide rejection must not fall through: {result:?}");
    };
    assert_eq!(rejected.candidates().len(), 1);
    assert_eq!(rejected.candidates()[0].target_id(), active.target_id());
}

#[test]
fn overlapping_inner_and_outer_buttons_choose_the_local_inner_target() {
    let fixture = fixture();
    let inner_top = rect(21.0, 34.0, 8.0, 8.0);
    let scene = seal_scene(
        &fixture,
        [
            target_inner_cluster(&fixture),
            target_outer_cluster_with_hits(
                &fixture,
                [
                    rect(1.0, 46.0, 8.0, 8.0),
                    rect(91.0, 46.0, 8.0, 8.0),
                    inner_top,
                    rect(46.0, 91.0, 8.0, 8.0),
                ],
            ),
        ],
        [],
        [],
    );

    let result = query(
        &fixture,
        &scene,
        &DockPolicy::default(),
        item_payload(&fixture),
        point(25.0, 38.0),
    );
    let active = result
        .affordance()
        .and_then(|affordance| affordance.active_target())
        .expect("overlapping guide has an exact winner");

    assert!(matches!(active.cluster().scope, DropGuideScope::Inner(_)));
    assert!(matches!(
        result
            .affordance()
            .expect("guides remain visible")
            .clusters()
            .last()
            .expect("inner cluster paints last")
            .id()
            .scope,
        DropGuideScope::Inner(_)
    ));
    assert!(matches!(
        active.target_id(),
        DropTargetId::InnerEdge {
            edge: Edge::Top,
            ..
        }
    ));
}

#[test]
fn rejected_exact_tab_gap_does_not_fall_through_to_an_overlapping_gap() {
    let fixture = fixture();
    let overlap = rect(70.0, 46.0, 10.0, 10.0);
    let scene = seal_scene(
        &fixture,
        [target_inner_cluster(&fixture)],
        [
            tab_gap_record(&fixture, 0, overlap),
            tab_gap_record(&fixture, 1, overlap),
        ],
        [],
    );
    let mut query_policy = DockPolicy::default();
    query_policy.set_allow_tab_merge(false);

    let result = query(
        &fixture,
        &scene,
        &query_policy,
        item_payload(&fixture),
        point(75.0, 50.0),
    );

    assert!(result.affordance().is_none());
    let DropResolution::Rejected(rejected) = result.resolution() else {
        panic!("exact tab-gap rejection must not fall through: {result:?}");
    };
    assert_eq!(rejected.candidates().len(), 1);
    assert_eq!(
        rejected.candidates()[0].target_id(),
        DropTargetId::TabGap {
            surface: TARGET_SURFACE,
            root: TARGET_ROOT,
            tabs: fixture.target_tabs,
            index: 0,
        }
    );
}

#[test]
fn guide_hit_region_excludes_its_maximum_edges() {
    let fixture = fixture();
    let scene = seal_scene(&fixture, [target_inner_cluster(&fixture)], [], []);

    let inside = query(
        &fixture,
        &scene,
        &DockPolicy::default(),
        item_payload(&fixture),
        point(28.999_999, 50.0),
    );
    let at_max = query(
        &fixture,
        &scene,
        &DockPolicy::default(),
        item_payload(&fixture),
        point(29.0, 50.0),
    );

    assert!(matches!(inside.resolution(), DropResolution::Resolved(_)));
    assert!(
        inside
            .affordance()
            .expect("cluster remains visible")
            .active_target()
            .is_some()
    );
    assert!(matches!(at_max.resolution(), DropResolution::KnownNone(_)));
    assert_eq!(
        at_max
            .affordance()
            .expect("cluster remains visible")
            .active_target(),
        None
    );
}

#[test]
fn payload_specific_prevalidation_marks_center_without_hiding_other_guides() {
    let fixture = fixture();
    let scene = seal_scene(&fixture, [target_inner_cluster(&fixture)], [], []);

    let result = query(
        &fixture,
        &scene,
        &DockPolicy::default(),
        subtree_payload(&fixture),
        point(30.0, 50.0),
    );
    let cluster = &result
        .affordance()
        .expect("cluster remains visible")
        .clusters()[0];
    let center = cluster
        .targets()
        .iter()
        .find(|target| target.slot() == DropGuideSlot::Center)
        .expect("inner cluster has center");

    assert!(matches!(
        center.eligibility(),
        DropGuideEligibility::Rejected(DropRejectionReason::Prevalidation(_))
    ));
    assert!(
        cluster
            .targets()
            .iter()
            .filter(|target| target.slot() != DropGuideSlot::Center)
            .all(|target| target.eligibility().is_eligible())
    );
}

#[test]
fn contained_occlusion_selects_only_the_frontmost_floating_cluster() {
    let fixture = fixture();
    let scene = seal_scene(
        &fixture,
        [
            target_inner_cluster(&fixture),
            floating_inner_cluster(&fixture),
        ],
        [],
        [DropOcclusionRecord::new(
            FLOATING,
            HitRegion::new(rect(20.0, 20.0, 60.0, 60.0)),
            SceneLayerKey::new(2),
        )],
    );

    let result = query(
        &fixture,
        &scene,
        &DockPolicy::default(),
        item_payload(&fixture),
        point(25.0, 50.0),
    );
    let affordance = result
        .affordance()
        .expect("foreground floating guide is visible");

    assert_eq!(affordance.clusters().len(), 1);
    assert_eq!(affordance.clusters()[0].id().root, FLOATING_ROOT);
    assert!(matches!(
        result.resolution(),
        DropResolution::Resolved(resolved)
            if matches!(resolved.target_id(), DropTargetId::Center { root, .. } if root == FLOATING_ROOT)
    ));
}

#[test]
fn single_tabs_root_exposes_one_five_slot_cluster_without_outer_duplicate() {
    let fixture = fixture();
    let scene = seal_scene(&fixture, [floating_inner_cluster(&fixture)], [], []);

    let result = query(
        &fixture,
        &scene,
        &DockPolicy::default(),
        item_payload(&fixture),
        point(30.0, 50.0),
    );
    let affordance = result.affordance().expect("single-root guide is visible");

    assert_eq!(affordance.clusters().len(), 1);
    assert!(matches!(
        affordance.clusters()[0].id().scope,
        DropGuideScope::Inner(node) if node == fixture.floating_tabs
    ));
    assert_eq!(affordance.clusters()[0].targets().len(), 5);
}

#[test]
fn whole_contained_item_suppresses_its_presentation_and_exposes_main_guide() {
    let fixture = fixture();

    assert_active_guide_root(&fixture, floating_item_payload(&fixture), TARGET_ROOT);
}

#[test]
fn whole_contained_tabs_and_subtree_suppress_their_presentations() {
    let tabs_fixture = fixture();
    assert_active_guide_root(
        &tabs_fixture,
        floating_tabs_payload(&tabs_fixture),
        TARGET_ROOT,
    );

    let subtree_fixture = fixture_with_split_floating();
    assert_active_guide_root(
        &subtree_fixture,
        floating_subtree_payload(&subtree_fixture, subtree_fixture.floating_root_node),
        TARGET_ROOT,
    );
}

#[test]
fn partial_contained_item_tabs_and_subtree_keep_source_occlusion_and_guides() {
    let item_fixture = fixture_with_floating_tabs([ItemId::new(20), ItemId::new(22)], false);
    assert_active_guide_root(
        &item_fixture,
        floating_item_payload(&item_fixture),
        FLOATING_ROOT,
    );

    let node_fixture = fixture_with_split_floating();
    assert_active_guide_root(
        &node_fixture,
        floating_tabs_payload(&node_fixture),
        FLOATING_ROOT,
    );
    assert_active_guide_root(
        &node_fixture,
        floating_subtree_payload(&node_fixture, node_fixture.floating_tabs),
        FLOATING_ROOT,
    );
}

#[test]
fn central_single_item_root_is_not_suppressed_because_cleanup_preserves_it() {
    let fixture = fixture_with_floating_tabs([ItemId::new(20)], true);

    assert_active_guide_root(&fixture, floating_item_payload(&fixture), FLOATING_ROOT);
}

#[test]
fn consumed_source_root_targets_are_excluded_even_without_an_underlying_target() {
    let fixture = fixture();
    let scene = seal_scene(
        &fixture,
        [floating_inner_cluster(&fixture)],
        [],
        [DropOcclusionRecord::new(
            FLOATING,
            HitRegion::new(rect(20.0, 20.0, 60.0, 60.0)),
            SceneLayerKey::new(2),
        )],
    );

    let result = query(
        &fixture,
        &scene,
        &DockPolicy::default(),
        floating_item_payload(&fixture),
        point(25.0, 50.0),
    );

    assert!(matches!(result.resolution(), DropResolution::KnownNone(_)));
    assert!(result.affordance().is_none());
}

#[test]
fn suppressing_source_occlusion_does_not_remove_another_floating_occlusion() {
    let fixture = fixture();
    let scene = seal_scene(
        &fixture,
        [
            target_inner_cluster(&fixture),
            floating_inner_cluster(&fixture),
        ],
        [],
        [
            DropOcclusionRecord::new(
                FLOATING,
                HitRegion::new(rect(20.0, 20.0, 60.0, 60.0)),
                SceneLayerKey::new(2),
            ),
            DropOcclusionRecord::new(
                OTHER_FLOATING,
                HitRegion::new(rect(20.0, 20.0, 60.0, 60.0)),
                SceneLayerKey::new(3),
            ),
        ],
    );

    let result = query(
        &fixture,
        &scene,
        &DockPolicy::default(),
        floating_item_payload(&fixture),
        point(25.0, 50.0),
    );

    assert!(matches!(result.resolution(), DropResolution::KnownNone(_)));
    assert!(result.affordance().is_none());
}

#[test]
fn legacy_resolution_suppresses_consumed_source_root_and_occlusion() {
    let fixture = fixture();
    let overlap = rect(20.0, 40.0, 20.0, 20.0);
    let scene = seal_scene(
        &fixture,
        [],
        [
            legacy_center_record(
                &fixture,
                TARGET_ROOT,
                fixture.target_tabs,
                overlap,
                SceneLayerKey::new(1),
            ),
            legacy_center_record(
                &fixture,
                FLOATING_ROOT,
                fixture.floating_tabs,
                overlap,
                SceneLayerKey::new(2),
            ),
        ],
        [DropOcclusionRecord::new(
            FLOATING,
            HitRegion::new(rect(20.0, 20.0, 60.0, 60.0)),
            SceneLayerKey::new(2),
        )],
    );

    let resolution = resolve_drop(
        &scene,
        &fixture.workspace,
        &DockPolicy::default(),
        session(),
        floating_item_payload(&fixture),
        TARGET_SURFACE,
        point(25.0, 50.0),
    )
    .expect("legacy resolution cannot expose an invariant failure");

    assert!(matches!(
        resolution,
        DropResolution::Resolved(resolved)
            if matches!(resolved.target_id(), DropTargetId::Center { root, .. } if root == TARGET_ROOT)
    ));
}
