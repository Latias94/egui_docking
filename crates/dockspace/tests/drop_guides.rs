use dockspace::command::{DockFraction, DockTarget, Edge};
use dockspace::drop_guide::{
    DropGuideClusterId, DropGuideClusterRecord, DropGuideEdgeSet, DropGuideScope, DropGuideSlot,
    DropGuideTargetRecord,
};
use dockspace::drop_target::{
    DropTargetAvailability, DropTargetId, DropTargetRecord, DropTargetUnavailable, DropVisual,
    SceneLayerKey,
};
use dockspace::engine::DockEngine;
use dockspace::geometry::LogicalRect;
use dockspace::graph::{Axis, Node, RootRecord, SurfacePresentation, Workspace, WorkspaceBuilder};
use dockspace::hit_region::HitRegion;
use dockspace::ids::{ItemId, NodeId, RootId, SurfaceId};
use dockspace::policy::DockPolicy;
use dockspace::scene::{
    BuildingScene, ReadySurfaceScene, SceneBuildError, SealedScene, SurfaceScene,
};
use dockspace::transition::InputOutcome;

const SPLIT_ROOT: RootId = RootId::new(1);
const SINGLE_ROOT: RootId = RootId::new(2);
const SPLIT_SURFACE: SurfaceId = SurfaceId::new(1);
const SINGLE_SURFACE: SurfaceId = SurfaceId::new(2);
const LAYER: SceneLayerKey = SceneLayerKey::new(7);

struct Fixture {
    workspace: Workspace,
    split_root_node: NodeId,
    split_tabs_a: NodeId,
    split_tabs_b: NodeId,
    single_tabs: NodeId,
}

#[derive(Clone, Copy)]
struct GuideOwner {
    surface: SurfaceId,
    root: RootId,
    node: NodeId,
}

impl GuideOwner {
    const fn new(surface: SurfaceId, root: RootId, node: NodeId) -> Self {
        Self {
            surface,
            root,
            node,
        }
    }
}

#[derive(Clone, Copy)]
struct GuideGeometry {
    hit: LogicalRect,
    draw: LogicalRect,
    preview: LogicalRect,
}

impl GuideGeometry {
    const fn new(hit: LogicalRect, draw: LogicalRect, preview: LogicalRect) -> Self {
        Self { hit, draw, preview }
    }
}

fn fixture() -> Fixture {
    let mut builder = WorkspaceBuilder::new();
    let split_tabs_a = builder.insert_node(Node::tabs([ItemId::new(1)]));
    let split_tabs_b = builder.insert_node(Node::tabs([ItemId::new(2)]));
    let split_root_node = builder.insert_node(
        Node::equal_split(Axis::Horizontal, [split_tabs_a, split_tabs_b])
            .expect("two tabs form a valid split"),
    );
    let single_tabs = builder.insert_node(Node::tabs([ItemId::new(3)]));

    builder.set_root(SPLIT_ROOT, RootRecord::new(split_root_node));
    builder.set_root(SINGLE_ROOT, RootRecord::new(single_tabs));
    builder.set_surface(SPLIT_SURFACE, SurfacePresentation::new(SPLIT_ROOT));
    builder.set_surface(SINGLE_SURFACE, SurfacePresentation::new(SINGLE_ROOT));

    Fixture {
        workspace: builder.build().expect("fixture workspace is valid"),
        split_root_node,
        split_tabs_a,
        split_tabs_b,
        single_tabs,
    }
}

fn rect(x: f64, y: f64, width: f64, height: f64) -> LogicalRect {
    LogicalRect::new(x, y, width, height).expect("test rectangle is valid")
}

fn bounds() -> LogicalRect {
    rect(0.0, 0.0, 400.0, 300.0)
}

fn slot_hit(slot: DropGuideSlot) -> LogicalRect {
    match slot {
        DropGuideSlot::Center => rect(180.0, 130.0, 40.0, 40.0),
        DropGuideSlot::Edge(Edge::Left) => rect(130.0, 130.0, 40.0, 40.0),
        DropGuideSlot::Edge(Edge::Right) => rect(230.0, 130.0, 40.0, 40.0),
        DropGuideSlot::Edge(Edge::Top) => rect(180.0, 80.0, 40.0, 40.0),
        DropGuideSlot::Edge(Edge::Bottom) => rect(180.0, 180.0, 40.0, 40.0),
    }
}

fn slot_draw(slot: DropGuideSlot) -> LogicalRect {
    let hit = slot_hit(slot);
    rect(hit.x() + 5.0, hit.y() + 5.0, 30.0, 30.0)
}

fn slot_preview(slot: DropGuideSlot) -> LogicalRect {
    match slot {
        DropGuideSlot::Center => rect(80.0, 60.0, 240.0, 180.0),
        DropGuideSlot::Edge(Edge::Left) => rect(0.0, 0.0, 140.0, 300.0),
        DropGuideSlot::Edge(Edge::Right) => rect(260.0, 0.0, 140.0, 300.0),
        DropGuideSlot::Edge(Edge::Top) => rect(0.0, 0.0, 400.0, 105.0),
        DropGuideSlot::Edge(Edge::Bottom) => rect(0.0, 195.0, 400.0, 105.0),
    }
}

fn center_guide(
    fixture: &Fixture,
    owner: GuideOwner,
    layer: SceneLayerKey,
    geometry: GuideGeometry,
) -> DropGuideTargetRecord {
    DropGuideTargetRecord::new(
        DropTargetRecord::new(
            DropTargetId::Center {
                surface: owner.surface,
                root: owner.root,
                tabs: owner.node,
            },
            DockTarget::Center(
                fixture
                    .workspace
                    .capture_tab_target(owner.root, owner.node)
                    .expect("center target is current"),
            ),
            DropTargetAvailability::Available,
            HitRegion::new(geometry.hit),
            layer,
            DropVisual::new(geometry.preview),
        ),
        geometry.draw,
    )
}

fn edge_guide(
    fixture: &Fixture,
    owner: GuideOwner,
    edge: Edge,
    outer: bool,
    layer: SceneLayerKey,
    geometry: GuideGeometry,
) -> DropGuideTargetRecord {
    let id = if outer {
        DropTargetId::OuterEdge {
            surface: owner.surface,
            root: owner.root,
            node: owner.node,
            edge,
        }
    } else {
        DropTargetId::InnerEdge {
            surface: owner.surface,
            root: owner.root,
            node: owner.node,
            edge,
        }
    };
    DropGuideTargetRecord::new(
        DropTargetRecord::new(
            id,
            DockTarget::Edge(
                fixture
                    .workspace
                    .capture_edge_target(
                        owner.root,
                        owner.node,
                        edge,
                        DockFraction::new(0.35).expect("test fraction is valid"),
                    )
                    .expect("edge target is current"),
            ),
            DropTargetAvailability::Available,
            HitRegion::new(geometry.hit),
            layer,
            DropVisual::new(geometry.preview),
        ),
        geometry.draw,
    )
}

fn standard_center(
    fixture: &Fixture,
    surface: SurfaceId,
    root: RootId,
    tabs: NodeId,
) -> DropGuideTargetRecord {
    center_guide(
        fixture,
        GuideOwner::new(surface, root, tabs),
        LAYER,
        GuideGeometry::new(
            slot_hit(DropGuideSlot::Center),
            slot_draw(DropGuideSlot::Center),
            slot_preview(DropGuideSlot::Center),
        ),
    )
}

fn standard_edge(
    fixture: &Fixture,
    surface: SurfaceId,
    root: RootId,
    node: NodeId,
    edge: Edge,
    outer: bool,
) -> DropGuideTargetRecord {
    let slot = DropGuideSlot::Edge(edge);
    edge_guide(
        fixture,
        GuideOwner::new(surface, root, node),
        edge,
        outer,
        LAYER,
        GuideGeometry::new(slot_hit(slot), slot_draw(slot), slot_preview(slot)),
    )
}

fn edge_set(
    fixture: &Fixture,
    surface: SurfaceId,
    root: RootId,
    node: NodeId,
    outer: bool,
) -> DropGuideEdgeSet {
    DropGuideEdgeSet::new(
        standard_edge(fixture, surface, root, node, Edge::Left, outer),
        standard_edge(fixture, surface, root, node, Edge::Right, outer),
        standard_edge(fixture, surface, root, node, Edge::Top, outer),
        standard_edge(fixture, surface, root, node, Edge::Bottom, outer),
    )
}

fn nested_inner_cluster(fixture: &Fixture, tabs: NodeId) -> DropGuideClusterRecord {
    DropGuideClusterRecord::inner(
        SPLIT_SURFACE,
        SPLIT_ROOT,
        tabs,
        HitRegion::new(bounds()),
        LAYER,
        standard_center(fixture, SPLIT_SURFACE, SPLIT_ROOT, tabs),
        edge_set(fixture, SPLIT_SURFACE, SPLIT_ROOT, tabs, false),
    )
}

fn single_inner_cluster(fixture: &Fixture) -> DropGuideClusterRecord {
    DropGuideClusterRecord::inner(
        SINGLE_SURFACE,
        SINGLE_ROOT,
        fixture.single_tabs,
        HitRegion::new(bounds()),
        LAYER,
        standard_center(fixture, SINGLE_SURFACE, SINGLE_ROOT, fixture.single_tabs),
        edge_set(
            fixture,
            SINGLE_SURFACE,
            SINGLE_ROOT,
            fixture.single_tabs,
            true,
        ),
    )
}

fn outer_cluster(fixture: &Fixture) -> DropGuideClusterRecord {
    DropGuideClusterRecord::outer(
        SPLIT_SURFACE,
        SPLIT_ROOT,
        HitRegion::new(bounds()),
        LAYER,
        edge_set(
            fixture,
            SPLIT_SURFACE,
            SPLIT_ROOT,
            fixture.split_root_node,
            true,
        ),
    )
}

fn publish(
    fixture: &Fixture,
    surface: SurfaceId,
    ready: ReadySurfaceScene,
    policy: DockPolicy,
) -> Result<SealedScene, SceneBuildError> {
    let mut building = BuildingScene::new([surface]).expect("test roster is unique");
    building
        .insert_ready(ready)
        .expect("test scene identities are unique");
    publish_building(fixture, policy, building)
}

fn publish_building(
    fixture: &Fixture,
    policy: DockPolicy,
    building: BuildingScene,
) -> Result<SealedScene, SceneBuildError> {
    let mut engine =
        DockEngine::new(fixture.workspace.clone(), policy).expect("fixture engine must be valid");
    engine
        .enqueue_scene(building)
        .expect("scene input sequence is available");
    let transition = engine
        .reduce_pending()
        .expect("scene rejection is not a fatal engine error");
    match transition.reduced_inputs()[0].outcome() {
        InputOutcome::ScenePublished { .. } => Ok(engine
            .scene()
            .expect("published outcome installs a scene")
            .clone()),
        InputOutcome::SceneRejected { error } => Err(error.clone()),
        outcome => panic!("unexpected scene outcome: {outcome:?}"),
    }
}

fn ready_from(scene: &SealedScene, surface: SurfaceId) -> &ReadySurfaceScene {
    match scene.surface(surface) {
        Some(SurfaceScene::Ready(ready)) => ready,
        state => panic!("expected ready surface, got {state:?}"),
    }
}

fn assert_cluster_rejected(
    fixture: &Fixture,
    surface: SurfaceId,
    cluster: DropGuideClusterRecord,
    expected: impl FnOnce(&SceneBuildError) -> bool,
) {
    let mut ready = ReadySurfaceScene::new(surface, bounds());
    ready.push_drop_guide_cluster(cluster);
    let error = publish(fixture, surface, ready, DockPolicy::default())
        .expect_err("invalid guide cluster must fail scene sealing");
    assert!(expected(&error), "unexpected scene error: {error:?}");
}

struct GeometryFixture<'a> {
    fixture: &'a Fixture,
    tabs: NodeId,
}

impl<'a> GeometryFixture<'a> {
    const fn new(fixture: &'a Fixture) -> Self {
        Self {
            fixture,
            tabs: fixture.split_tabs_a,
        }
    }

    fn left(
        &self,
        layer: SceneLayerKey,
        hit: LogicalRect,
        draw: LogicalRect,
        preview: LogicalRect,
    ) -> DropGuideTargetRecord {
        edge_guide(
            self.fixture,
            GuideOwner::new(SPLIT_SURFACE, SPLIT_ROOT, self.tabs),
            Edge::Left,
            false,
            layer,
            GuideGeometry::new(hit, draw, preview),
        )
    }

    fn cluster(
        &self,
        activation: LogicalRect,
        left: DropGuideTargetRecord,
        layer: SceneLayerKey,
    ) -> DropGuideClusterRecord {
        self.cluster_with_center(
            activation,
            standard_center(self.fixture, SPLIT_SURFACE, SPLIT_ROOT, self.tabs),
            left,
            layer,
        )
    }

    fn cluster_with_center(
        &self,
        activation: LogicalRect,
        center: DropGuideTargetRecord,
        left: DropGuideTargetRecord,
        layer: SceneLayerKey,
    ) -> DropGuideClusterRecord {
        DropGuideClusterRecord::inner(
            SPLIT_SURFACE,
            SPLIT_ROOT,
            self.tabs,
            HitRegion::new(activation),
            layer,
            center,
            DropGuideEdgeSet::new(
                left,
                standard_edge(
                    self.fixture,
                    SPLIT_SURFACE,
                    SPLIT_ROOT,
                    self.tabs,
                    Edge::Right,
                    false,
                ),
                standard_edge(
                    self.fixture,
                    SPLIT_SURFACE,
                    SPLIT_ROOT,
                    self.tabs,
                    Edge::Top,
                    false,
                ),
                standard_edge(
                    self.fixture,
                    SPLIT_SURFACE,
                    SPLIT_ROOT,
                    self.tabs,
                    Edge::Bottom,
                    false,
                ),
            ),
        )
    }
}

#[test]
fn constructors_expose_only_complete_inner_and_outer_shapes() {
    let fixture = fixture();
    let inner = nested_inner_cluster(&fixture, fixture.split_tabs_a);
    let outer = outer_cluster(&fixture);

    assert_eq!(
        inner.id().scope,
        DropGuideScope::Inner(fixture.split_tabs_a)
    );
    assert_eq!(inner.targets().count(), 5);
    assert!(inner.target(DropGuideSlot::Center).is_some());
    assert_eq!(outer.id().scope, DropGuideScope::Outer);
    assert_eq!(outer.targets().count(), 4);
    assert!(outer.target(DropGuideSlot::Center).is_none());
    for edge in [Edge::Left, Edge::Right, Edge::Top, Edge::Bottom] {
        assert!(inner.target(DropGuideSlot::Edge(edge)).is_some());
        assert!(outer.target(DropGuideSlot::Edge(edge)).is_some());
    }

    assert_eq!(
        inner.targets().map(|(slot, _)| slot).collect::<Vec<_>>(),
        [
            DropGuideSlot::Center,
            DropGuideSlot::Edge(Edge::Left),
            DropGuideSlot::Edge(Edge::Right),
            DropGuideSlot::Edge(Edge::Top),
            DropGuideSlot::Edge(Edge::Bottom),
        ]
    );
}

#[test]
fn cluster_and_owned_target_identities_are_globally_unique() {
    let fixture = fixture();
    let cluster = nested_inner_cluster(&fixture, fixture.split_tabs_a);
    let cluster_id = cluster.id();
    let mut duplicate_cluster = ReadySurfaceScene::new(SPLIT_SURFACE, bounds());
    duplicate_cluster.push_drop_guide_cluster(cluster.clone());
    duplicate_cluster.push_drop_guide_cluster(cluster.clone());
    let mut building = BuildingScene::new([SPLIT_SURFACE]).expect("test roster is unique");
    assert_eq!(
        building.insert_ready(duplicate_cluster),
        Err(SceneBuildError::DuplicateDropGuideCluster { id: cluster_id })
    );

    let duplicate_target = cluster
        .target(DropGuideSlot::Center)
        .expect("inner cluster has a center")
        .target()
        .clone();
    let duplicate_target_id = duplicate_target.id();
    let mut duplicate_across_storage = ReadySurfaceScene::new(SPLIT_SURFACE, bounds());
    duplicate_across_storage.push_drop_target(duplicate_target);
    duplicate_across_storage.push_drop_guide_cluster(cluster);
    let mut building = BuildingScene::new([SPLIT_SURFACE]).expect("test roster is unique");
    assert_eq!(
        building.insert_ready(duplicate_across_storage),
        Err(SceneBuildError::DuplicateDropTarget {
            id: duplicate_target_id
        })
    );
}

#[test]
fn wrong_direction_in_a_named_edge_slot_is_rejected() {
    let fixture = fixture();
    let tabs = fixture.split_tabs_a;
    let swapped = DropGuideEdgeSet::new(
        standard_edge(&fixture, SPLIT_SURFACE, SPLIT_ROOT, tabs, Edge::Left, false),
        standard_edge(&fixture, SPLIT_SURFACE, SPLIT_ROOT, tabs, Edge::Top, false),
        standard_edge(
            &fixture,
            SPLIT_SURFACE,
            SPLIT_ROOT,
            tabs,
            Edge::Right,
            false,
        ),
        standard_edge(
            &fixture,
            SPLIT_SURFACE,
            SPLIT_ROOT,
            tabs,
            Edge::Bottom,
            false,
        ),
    );
    let cluster = DropGuideClusterRecord::inner(
        SPLIT_SURFACE,
        SPLIT_ROOT,
        tabs,
        HitRegion::new(bounds()),
        LAYER,
        standard_center(&fixture, SPLIT_SURFACE, SPLIT_ROOT, tabs),
        swapped,
    );

    assert_cluster_rejected(&fixture, SPLIT_SURFACE, cluster, |error| {
        matches!(
            error,
            SceneBuildError::DropGuideTargetSlotMismatch {
                slot: DropGuideSlot::Edge(Edge::Right),
                target: DropTargetId::InnerEdge {
                    edge: Edge::Top,
                    ..
                },
                ..
            }
        )
    });
}

#[test]
fn positive_hit_overlap_is_rejected_only_within_one_cluster() {
    let fixture = fixture();
    let tabs = fixture.split_tabs_a;
    let left_hit = slot_hit(DropGuideSlot::Edge(Edge::Left));
    let overlapping_edges = DropGuideEdgeSet::new(
        standard_edge(&fixture, SPLIT_SURFACE, SPLIT_ROOT, tabs, Edge::Left, false),
        edge_guide(
            &fixture,
            GuideOwner::new(SPLIT_SURFACE, SPLIT_ROOT, tabs),
            Edge::Right,
            false,
            LAYER,
            GuideGeometry::new(
                left_hit,
                rect(left_hit.x() + 5.0, left_hit.y() + 5.0, 30.0, 30.0),
                slot_preview(DropGuideSlot::Edge(Edge::Right)),
            ),
        ),
        standard_edge(&fixture, SPLIT_SURFACE, SPLIT_ROOT, tabs, Edge::Top, false),
        standard_edge(
            &fixture,
            SPLIT_SURFACE,
            SPLIT_ROOT,
            tabs,
            Edge::Bottom,
            false,
        ),
    );
    let cluster = DropGuideClusterRecord::inner(
        SPLIT_SURFACE,
        SPLIT_ROOT,
        tabs,
        HitRegion::new(bounds()),
        LAYER,
        standard_center(&fixture, SPLIT_SURFACE, SPLIT_ROOT, tabs),
        overlapping_edges,
    );

    assert_cluster_rejected(&fixture, SPLIT_SURFACE, cluster, |error| {
        matches!(
            error,
            SceneBuildError::OverlappingDropGuideHitRegions {
                first: DropGuideSlot::Edge(Edge::Left),
                second: DropGuideSlot::Edge(Edge::Right),
                ..
            }
        )
    });

    let mut cross_cluster_overlap = ReadySurfaceScene::new(SPLIT_SURFACE, bounds());
    cross_cluster_overlap
        .push_drop_guide_cluster(nested_inner_cluster(&fixture, fixture.split_tabs_a));
    cross_cluster_overlap
        .push_drop_guide_cluster(nested_inner_cluster(&fixture, fixture.split_tabs_b));
    cross_cluster_overlap.push_drop_guide_cluster(outer_cluster(&fixture));
    publish(
        &fixture,
        SPLIT_SURFACE,
        cross_cluster_overlap,
        DockPolicy::default(),
    )
    .expect("overlap across distinct clusters remains valid");
}

#[test]
fn single_tabs_inner_uses_outer_edges_while_nested_inner_uses_inner_edges() {
    let fixture = fixture();
    let single = single_inner_cluster(&fixture);
    let nested = nested_inner_cluster(&fixture, fixture.split_tabs_a);
    let outer = outer_cluster(&fixture);

    let mut single_ready = ReadySurfaceScene::new(SINGLE_SURFACE, bounds());
    single_ready.push_drop_guide_cluster(single);
    let single_scene = publish(
        &fixture,
        SINGLE_SURFACE,
        single_ready,
        DockPolicy::default(),
    )
    .expect("single-tabs inner guide is valid");
    let single_cluster = &ready_from(&single_scene, SINGLE_SURFACE).drop_guide_clusters()[0];
    for edge in [Edge::Left, Edge::Right, Edge::Top, Edge::Bottom] {
        assert!(matches!(
            single_cluster
                .target(DropGuideSlot::Edge(edge))
                .expect("edge slot is complete")
                .id(),
            DropTargetId::OuterEdge { node, .. } if node == fixture.single_tabs
        ));
    }

    let single_outer = DropGuideClusterRecord::outer(
        SINGLE_SURFACE,
        SINGLE_ROOT,
        HitRegion::new(bounds()),
        LAYER,
        edge_set(
            &fixture,
            SINGLE_SURFACE,
            SINGLE_ROOT,
            fixture.single_tabs,
            true,
        ),
    );
    assert_cluster_rejected(&fixture, SINGLE_SURFACE, single_outer, |error| {
        matches!(error, SceneBuildError::InvalidDropGuideCluster { .. })
    });

    let mut split_ready = ReadySurfaceScene::new(SPLIT_SURFACE, bounds());
    split_ready.push_drop_guide_cluster(outer);
    split_ready.push_drop_guide_cluster(nested);
    let split_scene = publish(&fixture, SPLIT_SURFACE, split_ready, DockPolicy::default())
        .expect("nested and outer guides are valid");
    let clusters = ready_from(&split_scene, SPLIT_SURFACE).drop_guide_clusters();
    let nested = clusters
        .iter()
        .find(|cluster| cluster.id().scope == DropGuideScope::Inner(fixture.split_tabs_a))
        .expect("nested cluster is present");
    let outer = clusters
        .iter()
        .find(|cluster| cluster.id().scope == DropGuideScope::Outer)
        .expect("outer cluster is present");
    for edge in [Edge::Left, Edge::Right, Edge::Top, Edge::Bottom] {
        assert!(matches!(
            nested
                .target(DropGuideSlot::Edge(edge))
                .expect("edge slot is complete")
                .id(),
            DropTargetId::InnerEdge { node, .. } if node == fixture.split_tabs_a
        ));
        assert!(matches!(
            outer
                .target(DropGuideSlot::Edge(edge))
                .expect("edge slot is complete")
                .id(),
            DropTargetId::OuterEdge { node, .. } if node == fixture.split_root_node
        ));
    }
}

#[test]
fn guide_activation_geometry_is_validated_independently() {
    let fixture = fixture();
    let tabs = fixture.split_tabs_a;
    let geometry = GeometryFixture::new(&fixture);

    assert_cluster_rejected(
        &fixture,
        SPLIT_SURFACE,
        geometry.cluster(
            rect(0.0, 0.0, 0.0, 300.0),
            standard_edge(&fixture, SPLIT_SURFACE, SPLIT_ROOT, tabs, Edge::Left, false),
            LAYER,
        ),
        |error| matches!(error, SceneBuildError::EmptyDropGuideActivation { .. }),
    );
    assert_cluster_rejected(
        &fixture,
        SPLIT_SURFACE,
        geometry.cluster(
            rect(-1.0, 0.0, 400.0, 300.0),
            standard_edge(&fixture, SPLIT_SURFACE, SPLIT_ROOT, tabs, Edge::Left, false),
            LAYER,
        ),
        |error| {
            matches!(
                error,
                SceneBuildError::DropGuideActivationOutsideSurface { .. }
            )
        },
    );
    let activation = rect(100.0, 0.0, 300.0, 300.0);
    let hit_outside_activation = geometry.cluster_with_center(
        activation,
        center_guide(
            &fixture,
            GuideOwner::new(SPLIT_SURFACE, SPLIT_ROOT, tabs),
            LAYER,
            GuideGeometry::new(
                slot_hit(DropGuideSlot::Center),
                slot_draw(DropGuideSlot::Center),
                rect(120.0, 60.0, 200.0, 180.0),
            ),
        ),
        geometry.left(
            LAYER,
            rect(50.0, 130.0, 40.0, 40.0),
            rect(55.0, 135.0, 30.0, 30.0),
            rect(100.0, 0.0, 140.0, 300.0),
        ),
        LAYER,
    );
    assert_cluster_rejected(&fixture, SPLIT_SURFACE, hit_outside_activation, |error| {
        matches!(
            error,
            SceneBuildError::DropGuideHitRegionOutsideActivation { .. }
        )
    });
    assert_cluster_rejected(
        &fixture,
        SPLIT_SURFACE,
        geometry.cluster(
            rect(0.0, 0.0, 300.0, 300.0),
            standard_edge(&fixture, SPLIT_SURFACE, SPLIT_ROOT, tabs, Edge::Left, false),
            LAYER,
        ),
        |error| {
            matches!(
                error,
                SceneBuildError::DropGuidePreviewOutsideActivation { .. }
            )
        },
    );
}

#[test]
fn guide_hit_geometry_is_validated_independently() {
    let fixture = fixture();
    let geometry = GeometryFixture::new(&fixture);

    assert_cluster_rejected(
        &fixture,
        SPLIT_SURFACE,
        geometry.cluster(
            bounds(),
            geometry.left(
                LAYER,
                rect(130.0, 130.0, 0.0, 40.0),
                rect(130.0, 130.0, 0.0, 30.0),
                slot_preview(DropGuideSlot::Edge(Edge::Left)),
            ),
            LAYER,
        ),
        |error| matches!(error, SceneBuildError::EmptyDropGuideHitRegion { .. }),
    );
    assert_cluster_rejected(
        &fixture,
        SPLIT_SURFACE,
        geometry.cluster(
            bounds(),
            geometry.left(
                LAYER,
                rect(-1.0, 130.0, 40.0, 40.0),
                rect(0.0, 135.0, 30.0, 30.0),
                slot_preview(DropGuideSlot::Edge(Edge::Left)),
            ),
            LAYER,
        ),
        |error| {
            matches!(
                error,
                SceneBuildError::DropGuideHitRegionOutsideSurface { .. }
            )
        },
    );
}

#[test]
fn guide_draw_geometry_and_layer_are_validated_independently() {
    let fixture = fixture();
    let geometry = GeometryFixture::new(&fixture);

    assert_cluster_rejected(
        &fixture,
        SPLIT_SURFACE,
        geometry.cluster(
            bounds(),
            geometry.left(
                LAYER,
                slot_hit(DropGuideSlot::Edge(Edge::Left)),
                rect(135.0, 135.0, 0.0, 30.0),
                slot_preview(DropGuideSlot::Edge(Edge::Left)),
            ),
            LAYER,
        ),
        |error| matches!(error, SceneBuildError::EmptyDropGuideDraw { .. }),
    );
    assert_cluster_rejected(
        &fixture,
        SPLIT_SURFACE,
        geometry.cluster(
            bounds(),
            geometry.left(
                LAYER,
                slot_hit(DropGuideSlot::Edge(Edge::Left)),
                rect(125.0, 135.0, 30.0, 30.0),
                slot_preview(DropGuideSlot::Edge(Edge::Left)),
            ),
            LAYER,
        ),
        |error| matches!(error, SceneBuildError::DropGuideDrawOutsideHitRegion { .. }),
    );
}

#[test]
fn guide_preview_geometry_and_target_layer_are_validated_independently() {
    let fixture = fixture();
    let geometry = GeometryFixture::new(&fixture);

    assert_cluster_rejected(
        &fixture,
        SPLIT_SURFACE,
        geometry.cluster(
            bounds(),
            geometry.left(
                LAYER,
                slot_hit(DropGuideSlot::Edge(Edge::Left)),
                slot_draw(DropGuideSlot::Edge(Edge::Left)),
                rect(0.0, 0.0, 0.0, 300.0),
            ),
            LAYER,
        ),
        |error| matches!(error, SceneBuildError::EmptyDropVisual { .. }),
    );
    assert_cluster_rejected(
        &fixture,
        SPLIT_SURFACE,
        geometry.cluster(
            bounds(),
            geometry.left(
                LAYER,
                slot_hit(DropGuideSlot::Edge(Edge::Left)),
                slot_draw(DropGuideSlot::Edge(Edge::Left)),
                rect(-1.0, 0.0, 140.0, 300.0),
            ),
            LAYER,
        ),
        |error| matches!(error, SceneBuildError::DropVisualOutsideSurface { .. }),
    );
    assert_cluster_rejected(
        &fixture,
        SPLIT_SURFACE,
        geometry.cluster(
            bounds(),
            geometry.left(
                SceneLayerKey::new(LAYER.get() + 1),
                slot_hit(DropGuideSlot::Edge(Edge::Left)),
                slot_draw(DropGuideSlot::Edge(Edge::Left)),
                slot_preview(DropGuideSlot::Edge(Edge::Left)),
            ),
            LAYER,
        ),
        |error| matches!(error, SceneBuildError::DropGuideTargetLayerMismatch { .. }),
    );
}

#[test]
fn guide_clusters_and_slots_have_canonical_order_after_sealing() {
    let fixture = fixture();
    let first_tabs = fixture.split_tabs_a.min(fixture.split_tabs_b);
    let second_tabs = fixture.split_tabs_a.max(fixture.split_tabs_b);
    let inserted = [
        DropGuideClusterId::outer(SPLIT_SURFACE, SPLIT_ROOT),
        DropGuideClusterId::inner(SPLIT_SURFACE, SPLIT_ROOT, second_tabs),
        DropGuideClusterId::inner(SPLIT_SURFACE, SPLIT_ROOT, first_tabs),
    ];
    let mut ready = ReadySurfaceScene::new(SPLIT_SURFACE, bounds());
    ready.push_drop_guide_cluster(outer_cluster(&fixture));
    ready.push_drop_guide_cluster(nested_inner_cluster(&fixture, second_tabs));
    ready.push_drop_guide_cluster(nested_inner_cluster(&fixture, first_tabs));

    let scene = publish(&fixture, SPLIT_SURFACE, ready, DockPolicy::default())
        .expect("complete guides seal successfully");
    let ready = ready_from(&scene, SPLIT_SURFACE);
    let ids = ready
        .drop_guide_clusters()
        .iter()
        .map(DropGuideClusterRecord::id)
        .collect::<Vec<_>>();
    let mut expected = inserted;
    expected.sort_unstable();
    assert_eq!(ids, expected);
    for cluster in ready.drop_guide_clusters() {
        let target_ids = cluster
            .targets()
            .map(|(_, target)| target.id())
            .collect::<Vec<_>>();
        let mut sorted = target_ids.clone();
        sorted.sort_unstable();
        assert_eq!(target_ids, sorted);
    }
}

#[test]
fn policy_canonicalization_applies_to_every_guide_owned_target() {
    let fixture = fixture();
    let mut policy = DockPolicy::default();
    policy.set_allow_tab_merge(false);
    policy.set_allow_edge_split(false);
    let mut ready = ReadySurfaceScene::new(SPLIT_SURFACE, bounds());
    ready.push_drop_guide_cluster(nested_inner_cluster(&fixture, fixture.split_tabs_a));

    let scene = publish(&fixture, SPLIT_SURFACE, ready, policy)
        .expect("policy disables targets without invalidating scene facts");
    let cluster = &ready_from(&scene, SPLIT_SURFACE).drop_guide_clusters()[0];
    for (_, target) in cluster.targets() {
        assert_eq!(
            target.target().availability(),
            DropTargetAvailability::Unavailable(DropTargetUnavailable::PolicyDisabled)
        );
    }
}
