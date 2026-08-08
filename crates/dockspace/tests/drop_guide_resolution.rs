use super::support;

use dockspace::command::{Edge, MovePayload};
use dockspace::drop_guide::{
    DropGuideClusterRecord, DropGuideScope, DropGuideSlot, DropGuideTargetRecord,
};
use dockspace::drop_resolver::{
    DropGuideEligibility, DropRejectionReason, DropResolution, resolve_drop,
};
use dockspace::drop_target::{DropTargetAvailability, DropTargetId, DropTargetUnavailable};
use dockspace::engine::DockEngine;
use dockspace::geometry::{LogicalPoint, LogicalRect};
use dockspace::graph::{
    Axis, ContainedFloating, Node, RootRecord, SurfacePresentation, Workspace, WorkspaceBuilder,
};
use dockspace::ids::{FloatingPresentationId, ItemId, NodeId, RootId, SurfaceId, WorkspaceEpoch};
use dockspace::interaction::{DragGeneration, DragSessionId};
use dockspace::policy::{DockPolicy, PolicyRejection};
use dockspace::scene::{PresentationPlan, SurfaceSceneSet};

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
    target_tabs: NodeId,
    floating_root_node: NodeId,
    floating_tabs: NodeId,
}

fn fixture() -> Fixture {
    build_fixture(
        vec![ItemId::new(20)],
        false,
        false,
        rect(500.0, 340.0, 120.0, 120.0),
        rect(500.0, 20.0, 120.0, 120.0),
    )
}

fn floating_fixture() -> Fixture {
    fixture_with_floating_tabs([ItemId::new(20)], false)
}

fn fixture_with_floating_tabs(items: impl IntoIterator<Item = ItemId>, central: bool) -> Fixture {
    build_fixture(
        items.into_iter().collect(),
        false,
        central,
        rect(80.0, 120.0, 220.0, 240.0),
        rect(500.0, 340.0, 120.0, 120.0),
    )
}

fn fixture_with_split_floating() -> Fixture {
    build_fixture(
        vec![ItemId::new(20)],
        true,
        false,
        rect(80.0, 120.0, 220.0, 240.0),
        rect(500.0, 340.0, 120.0, 120.0),
    )
}

fn overlapping_floating_fixture() -> Fixture {
    let overlap = rect(80.0, 120.0, 220.0, 240.0);
    build_fixture(vec![ItemId::new(20)], false, false, overlap, overlap)
}

fn build_fixture(
    floating_items: Vec<ItemId>,
    split_floating: bool,
    central: bool,
    floating_rect: LogicalRect,
    other_floating_rect: LogicalRect,
) -> Fixture {
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
    builder.set_surface(SOURCE_SURFACE, SurfacePresentation::with_main(SOURCE_ROOT));
    builder.set_surface(TARGET_SURFACE, SurfacePresentation::with_main(TARGET_ROOT));
    builder.set_contained_floating(
        FLOATING,
        ContainedFloating::new(FLOATING_ROOT, floating_rect),
    );
    builder.set_contained_floating(
        OTHER_FLOATING,
        ContainedFloating::new(OTHER_FLOATING_ROOT, other_floating_rect),
    );
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
        target_tabs,
        floating_root_node,
        floating_tabs,
    }
}

fn rect(x: f64, y: f64, width: f64, height: f64) -> LogicalRect {
    LogicalRect::new(x, y, width, height).expect("test rectangle is valid")
}

fn surface_bounds() -> LogicalRect {
    rect(0.0, 0.0, 640.0, 480.0)
}

fn point(x: f64, y: f64) -> LogicalPoint {
    LogicalPoint::new(x, y).expect("test point is finite")
}

fn midpoint(region: LogicalRect) -> LogicalPoint {
    point(
        region.x() + region.width() * 0.5,
        region.y() + region.height() * 0.5,
    )
}

fn previous_float(value: f64) -> f64 {
    assert!(value.is_finite() && value > 0.0);
    f64::from_bits(value.to_bits() - 1)
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

fn painted_scene_set(fixture: &Fixture) -> SurfaceSceneSet {
    painted_scene_set_with_policy(fixture, DockPolicy::default())
}

fn painted_scene_set_with_policy(fixture: &Fixture, policy: DockPolicy) -> SurfaceSceneSet {
    let mut engine =
        DockEngine::new(fixture.workspace.clone(), policy).expect("fixture engine is valid");
    let mut host = support::TestPresentationHost::new(&mut engine);
    support::publish_surfaces(
        &mut engine,
        &mut host,
        [
            (SOURCE_SURFACE, surface_bounds()),
            (TARGET_SURFACE, surface_bounds()),
        ],
    );
    engine.scene().clone()
}

fn target_plan(scene: &SurfaceSceneSet) -> &PresentationPlan {
    scene
        .ready_surface(TARGET_SURFACE)
        .map(|ready| ready.plan())
        .expect("target surface has an acknowledged painted plan")
}

fn inner_cluster(plan: &PresentationPlan, root: RootId, tabs: NodeId) -> &DropGuideClusterRecord {
    plan.drop_guide_clusters()
        .iter()
        .find(|cluster| {
            cluster.id().root == root && cluster.id().scope == DropGuideScope::Inner(tabs)
        })
        .expect("core compiler emitted the requested inner cluster")
}

fn outer_cluster(plan: &PresentationPlan, root: RootId) -> &DropGuideClusterRecord {
    plan.drop_guide_clusters()
        .iter()
        .find(|cluster| cluster.id().root == root && cluster.id().scope == DropGuideScope::Outer)
        .expect("core compiler emitted the requested outer cluster")
}

fn guide(
    plan: &PresentationPlan,
    root: RootId,
    scope: DropGuideScope,
    slot: DropGuideSlot,
) -> &DropGuideTargetRecord {
    plan.drop_guide_clusters()
        .iter()
        .find(|cluster| cluster.id().root == root && cluster.id().scope == scope)
        .and_then(|cluster| cluster.target(slot))
        .expect("core compiler emitted the requested guide target")
}

fn guide_point(
    plan: &PresentationPlan,
    root: RootId,
    scope: DropGuideScope,
    slot: DropGuideSlot,
) -> LogicalPoint {
    midpoint(guide(plan, root, scope, slot).target().region().rect())
}

fn target_region(plan: &PresentationPlan, id: DropTargetId) -> LogicalRect {
    plan.drop_targets()
        .iter()
        .find(|target| target.id() == id)
        .map(|target| target.region().rect())
        .expect("core compiler emitted the requested structural target")
}

fn point_in_region_excluding(region: LogicalRect, exclusions: &[LogicalRect]) -> LogicalPoint {
    let mut xs = vec![region.x(), region.max().x()];
    let mut ys = vec![region.y(), region.max().y()];
    for exclusion in exclusions {
        xs.extend([exclusion.x(), exclusion.max().x()]);
        ys.extend([exclusion.y(), exclusion.max().y()]);
    }
    xs.sort_by(f64::total_cmp);
    ys.sort_by(f64::total_cmp);
    xs.dedup();
    ys.dedup();

    for x in xs.windows(2).filter(|pair| pair[0] < pair[1]) {
        for y in ys.windows(2).filter(|pair| pair[0] < pair[1]) {
            let candidate = point((x[0] + x[1]) * 0.5, (y[0] + y[1]) * 0.5);
            if region.contains(candidate)
                && exclusions
                    .iter()
                    .all(|exclusion| !exclusion.contains(candidate))
            {
                return candidate;
            }
        }
    }
    panic!("region has no positive-area point outside exclusions")
}

fn guide_free_point(plan: &PresentationPlan, cluster: &DropGuideClusterRecord) -> LogicalPoint {
    let exclusions = plan
        .drop_guide_clusters()
        .iter()
        .flat_map(DropGuideClusterRecord::targets)
        .map(|(_, target)| target.target().region().rect())
        .chain(
            plan.drop_targets()
                .iter()
                .map(|target| target.region().rect()),
        )
        .collect::<Vec<_>>();
    point_in_region_excluding(cluster.activation().rect(), &exclusions)
}

fn query(
    fixture: &Fixture,
    scene: &SurfaceSceneSet,
    policy: &DockPolicy,
    payload: MovePayload,
    at: LogicalPoint,
) -> dockspace::drop_resolver::DropQuery {
    let policy = policy.snapshot(Default::default());
    resolve_drop(
        scene,
        &fixture.workspace,
        &policy,
        session(),
        payload,
        None,
        TARGET_SURFACE,
        at,
    )
    .expect("query cannot expose an invariant failure")
}

fn assert_affordance_root(fixture: &Fixture, payload: MovePayload, expected: RootId) {
    let scene = painted_scene_set(fixture);
    let at = guide_point(
        target_plan(&scene),
        FLOATING_ROOT,
        DropGuideScope::Inner(fixture.floating_tabs),
        DropGuideSlot::Center,
    );
    let result = query(fixture, &scene, &DockPolicy::default(), payload, at);
    let affordance = result.affordance().expect("one guide root remains visible");

    assert!(
        affordance
            .clusters()
            .iter()
            .all(|cluster| cluster.id().root == expected),
        "unexpected visible guide roots: {affordance:?}"
    );
}

#[test]
fn scene_unavailable_button_remains_active_disabled() {
    let fixture = fixture();
    let mut scene_policy = DockPolicy::default();
    scene_policy.set_allow_edge_split(false);
    let scene = painted_scene_set_with_policy(&fixture, scene_policy);
    let at = guide_point(
        target_plan(&scene),
        TARGET_ROOT,
        DropGuideScope::Inner(fixture.target_tabs),
        DropGuideSlot::Edge(Edge::Top),
    );

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
        .expect("unavailable guide remains active");

    assert!(matches!(
        active.eligibility(),
        DropGuideEligibility::Rejected(DropRejectionReason::SceneUnavailable(
            DropTargetUnavailable::PolicyDisabled
        ))
    ));
    assert!(matches!(result.resolution(), DropResolution::Rejected(_)));
}

#[test]
fn selected_inner_and_matching_outer_clusters_are_complete_and_owned() {
    let fixture = fixture();
    let scene = painted_scene_set(&fixture);
    let at = guide_point(
        target_plan(&scene),
        TARGET_ROOT,
        DropGuideScope::Inner(fixture.target_tabs),
        DropGuideSlot::Center,
    );

    let result = query(
        &fixture,
        &scene,
        &DockPolicy::default(),
        item_payload(&fixture),
        at,
    );
    let affordance = result.affordance().expect("inner activation shows guides");
    let inner = affordance
        .clusters()
        .iter()
        .find(|cluster| cluster.id().scope == DropGuideScope::Inner(fixture.target_tabs))
        .expect("selected inner cluster is visible");
    let outer = affordance
        .clusters()
        .iter()
        .find(|cluster| cluster.id().scope == DropGuideScope::Outer)
        .expect("matching outer cluster is visible");

    assert_eq!(inner.targets().len(), 5);
    assert_eq!(outer.targets().len(), 4);
    assert!(affordance.clusters().iter().all(|cluster| {
        cluster.id().root == TARGET_ROOT
            && cluster
                .targets()
                .iter()
                .all(|target| target.eligibility().is_eligible())
    }));
}

#[test]
fn outer_cluster_resolves_without_an_active_inner_cluster() {
    let fixture = fixture();
    let scene = painted_scene_set(&fixture);
    let plan = target_plan(&scene);
    let outer_top = guide(
        plan,
        TARGET_ROOT,
        DropGuideScope::Outer,
        DropGuideSlot::Edge(Edge::Top),
    );
    let inner_activations = plan
        .drop_guide_clusters()
        .iter()
        .filter(|cluster| {
            cluster.id().root == TARGET_ROOT
                && matches!(cluster.id().scope, DropGuideScope::Inner(_))
        })
        .map(|cluster| cluster.activation().rect())
        .collect::<Vec<_>>();
    let at = point_in_region_excluding(outer_top.target().region().rect(), &inner_activations);

    let result = query(
        &fixture,
        &scene,
        &DockPolicy::default(),
        item_payload(&fixture),
        at,
    );
    let affordance = result
        .affordance()
        .expect("outer activation independently shows guides");
    let active = affordance
        .active_target()
        .expect("outer top button is active");

    assert!(
        affordance
            .clusters()
            .iter()
            .all(|cluster| cluster.id().scope == DropGuideScope::Outer)
    );
    assert_eq!(active.slot(), DropGuideSlot::Edge(Edge::Top));
    assert!(matches!(
        result.resolution(),
        DropResolution::Resolved(resolved)
            if matches!(resolved.target_id(), DropTargetId::OuterEdge { edge: Edge::Top, .. })
    ));
}

#[test]
fn activation_without_button_or_tab_gap_hit_returns_guide_only_affordance() {
    let fixture = fixture();
    let scene = painted_scene_set(&fixture);
    let plan = target_plan(&scene);
    let inner = inner_cluster(plan, TARGET_ROOT, fixture.target_tabs);
    let at = guide_free_point(plan, inner);

    let result = query(
        &fixture,
        &scene,
        &DockPolicy::default(),
        item_payload(&fixture),
        at,
    );

    assert!(matches!(result.resolution(), DropResolution::KnownNone(_)));
    assert_eq!(
        result
            .affordance()
            .expect("guides are visible")
            .active_target(),
        None
    );
}

#[test]
fn activation_without_button_hit_resolves_exact_tab_gap_and_keeps_affordance() {
    let fixture = fixture();
    let scene = painted_scene_set(&fixture);
    let id = DropTargetId::TabGap {
        surface: TARGET_SURFACE,
        root: TARGET_ROOT,
        tabs: fixture.target_tabs,
        index: 1,
    };
    let at = midpoint(target_region(target_plan(&scene), id));

    let result = query(
        &fixture,
        &scene,
        &DockPolicy::default(),
        item_payload(&fixture),
        at,
    );

    let affordance = result
        .affordance()
        .expect("guide activation remains a visible affordance");
    assert_eq!(affordance.active_target(), None);
    assert!(matches!(
        result.resolution(),
        DropResolution::Resolved(resolved) if resolved.target_id() == id
    ));
}

#[test]
fn exact_center_hit_is_active_and_resolved() {
    let fixture = fixture();
    let scene = painted_scene_set(&fixture);
    let at = guide_point(
        target_plan(&scene),
        TARGET_ROOT,
        DropGuideScope::Inner(fixture.target_tabs),
        DropGuideSlot::Center,
    );

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
    let scene = painted_scene_set(&fixture);

    for expected_edge in [Edge::Top, Edge::Bottom] {
        let at = guide_point(
            target_plan(&scene),
            TARGET_ROOT,
            DropGuideScope::Inner(fixture.target_tabs),
            DropGuideSlot::Edge(expected_edge),
        );
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
fn adjacent_guide_hit_regions_are_half_open_at_the_shared_edge() {
    let fixture = fixture();
    let scene = painted_scene_set(&fixture);
    let hit = guide(
        target_plan(&scene),
        TARGET_ROOT,
        DropGuideScope::Inner(fixture.target_tabs),
        DropGuideSlot::Center,
    )
    .target()
    .region()
    .rect();
    let y = hit.y() + hit.height() * 0.5;

    let inside = query(
        &fixture,
        &scene,
        &DockPolicy::default(),
        item_payload(&fixture),
        point(previous_float(hit.max().x()), y),
    );
    let at_max = query(
        &fixture,
        &scene,
        &DockPolicy::default(),
        item_payload(&fixture),
        point(hit.max().x(), y),
    );

    assert_eq!(
        inside
            .affordance()
            .expect("cluster remains visible")
            .active_target()
            .expect("center remains active immediately before its maximum edge")
            .slot(),
        DropGuideSlot::Center
    );
    assert_eq!(
        at_max
            .affordance()
            .expect("cluster remains visible")
            .active_target()
            .expect("adjacent right edge owns the shared boundary")
            .slot(),
        DropGuideSlot::Edge(Edge::Right)
    );
}

#[test]
fn payload_specific_prevalidation_marks_center_without_hiding_other_guides() {
    let fixture = fixture();
    let scene = painted_scene_set(&fixture);
    let plan = target_plan(&scene);
    let inner = inner_cluster(plan, TARGET_ROOT, fixture.target_tabs);
    let at = guide_free_point(plan, inner);

    let result = query(
        &fixture,
        &scene,
        &DockPolicy::default(),
        subtree_payload(&fixture),
        at,
    );
    let cluster = result
        .affordance()
        .expect("cluster remains visible")
        .clusters()
        .iter()
        .find(|cluster| cluster.id().scope == DropGuideScope::Inner(fixture.target_tabs))
        .expect("target inner cluster remains visible");
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
    let fixture = floating_fixture();
    let scene = painted_scene_set(&fixture);
    let at = guide_point(
        target_plan(&scene),
        FLOATING_ROOT,
        DropGuideScope::Inner(fixture.floating_tabs),
        DropGuideSlot::Center,
    );

    let result = query(
        &fixture,
        &scene,
        &DockPolicy::default(),
        item_payload(&fixture),
        at,
    );
    let affordance = result
        .affordance()
        .expect("foreground floating guide is visible");

    assert!(
        affordance
            .clusters()
            .iter()
            .all(|cluster| cluster.id().root == FLOATING_ROOT)
    );
    assert!(matches!(
        result.resolution(),
        DropResolution::Resolved(resolved)
            if matches!(resolved.target_id(), DropTargetId::Center { root, .. } if root == FLOATING_ROOT)
    ));
}

#[test]
fn central_tabs_root_exposes_center_only_inner_and_distinct_outer_cluster() {
    let fixture = fixture_with_floating_tabs([ItemId::new(20)], true);
    let scene = painted_scene_set(&fixture);
    let plan = target_plan(&scene);
    let at = guide_point(
        plan,
        FLOATING_ROOT,
        DropGuideScope::Outer,
        DropGuideSlot::Edge(Edge::Left),
    );

    let result = query(
        &fixture,
        &scene,
        &DockPolicy::default(),
        item_payload(&fixture),
        at,
    );
    let affordance = result.affordance().expect("central-root guide is visible");
    let inner = inner_cluster(plan, FLOATING_ROOT, fixture.floating_tabs);
    let outer = outer_cluster(plan, FLOATING_ROOT);

    assert_eq!(inner.targets().count(), 1);
    assert_eq!(outer.targets().count(), 4);
    assert!(
        affordance
            .clusters()
            .iter()
            .all(|cluster| cluster.id().root == FLOATING_ROOT)
    );
    assert!(matches!(
        result.resolution(),
        DropResolution::Resolved(resolved)
            if matches!(resolved.target_id(), DropTargetId::OuterEdge { root, edge: Edge::Left, .. } if root == FLOATING_ROOT)
    ));
}

#[test]
fn whole_contained_item_suppresses_its_presentation_and_exposes_main_guide() {
    let fixture = floating_fixture();

    assert_affordance_root(&fixture, floating_item_payload(&fixture), TARGET_ROOT);
}

#[test]
fn whole_contained_tabs_and_subtree_suppress_their_presentations() {
    let tabs_fixture = floating_fixture();
    assert_affordance_root(
        &tabs_fixture,
        floating_tabs_payload(&tabs_fixture),
        TARGET_ROOT,
    );

    let subtree_fixture = fixture_with_split_floating();
    assert_affordance_root(
        &subtree_fixture,
        floating_subtree_payload(&subtree_fixture, subtree_fixture.floating_root_node),
        TARGET_ROOT,
    );
}

#[test]
fn partial_contained_item_tabs_and_subtree_keep_source_occlusion_and_guides() {
    let item_fixture = fixture_with_floating_tabs([ItemId::new(20), ItemId::new(22)], false);
    assert_affordance_root(
        &item_fixture,
        floating_item_payload(&item_fixture),
        FLOATING_ROOT,
    );

    let node_fixture = fixture_with_split_floating();
    assert_affordance_root(
        &node_fixture,
        floating_tabs_payload(&node_fixture),
        FLOATING_ROOT,
    );
    assert_affordance_root(
        &node_fixture,
        floating_subtree_payload(&node_fixture, node_fixture.floating_tabs),
        FLOATING_ROOT,
    );
}

#[test]
fn central_single_item_root_is_not_suppressed_because_cleanup_preserves_it() {
    let fixture = fixture_with_floating_tabs([ItemId::new(20)], true);

    assert_affordance_root(&fixture, floating_item_payload(&fixture), FLOATING_ROOT);
}

#[test]
fn suppressing_source_occlusion_keeps_the_frontmost_sibling_floating_authoritative() {
    let fixture = overlapping_floating_fixture();
    let scene = painted_scene_set(&fixture);
    let at = guide_point(
        target_plan(&scene),
        OTHER_FLOATING_ROOT,
        DropGuideScope::Inner(
            fixture
                .workspace
                .root(OTHER_FLOATING_ROOT)
                .expect("other floating root exists")
                .node,
        ),
        DropGuideSlot::Center,
    );

    let result = query(
        &fixture,
        &scene,
        &DockPolicy::default(),
        floating_item_payload(&fixture),
        at,
    );
    let affordance = result
        .affordance()
        .expect("frontmost sibling floating remains authoritative");

    assert!(
        affordance
            .clusters()
            .iter()
            .all(|cluster| cluster.id().root == OTHER_FLOATING_ROOT)
    );
    assert!(matches!(
        result.resolution(),
        DropResolution::Resolved(resolved)
            if matches!(resolved.target_id(), DropTargetId::Center { root, .. } if root == OTHER_FLOATING_ROOT)
    ));
}

#[test]
fn rejected_core_guide_does_not_fall_through_after_policy_rejection() {
    let fixture = fixture();
    let scene = painted_scene_set(&fixture);
    let at = guide_point(
        target_plan(&scene),
        TARGET_ROOT,
        DropGuideScope::Inner(fixture.target_tabs),
        DropGuideSlot::Edge(Edge::Top),
    );
    let mut query_policy = DockPolicy::default();
    query_policy.set_allow_edge_split(false);

    let result = query(&fixture, &scene, &query_policy, item_payload(&fixture), at);
    let active = result
        .affordance()
        .and_then(|affordance| affordance.active_target())
        .expect("rejected guide remains the exact winner");

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
fn core_compiled_targets_expose_only_core_owned_availability_states() {
    let fixture = fixture();
    let scene = painted_scene_set(&fixture);

    assert!(
        target_plan(&scene)
            .drop_guide_clusters()
            .iter()
            .flat_map(DropGuideClusterRecord::targets)
            .all(|(_, target)| {
                target.target().availability() == DropTargetAvailability::Available
            })
    );
}
