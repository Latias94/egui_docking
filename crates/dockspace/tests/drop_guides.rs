use super::support;

use dockspace::command::Edge;
use dockspace::drop_guide::{
    DropGuideClusterId, DropGuideClusterRecord, DropGuideScope, DropGuideSlot,
};
use dockspace::drop_target::{DropTargetAvailability, DropTargetId, DropTargetUnavailable};
use dockspace::engine::DockEngine;
use dockspace::geometry::LogicalRect;
use dockspace::graph::{Axis, Node, RootRecord, SurfacePresentation, Workspace, WorkspaceBuilder};
use dockspace::ids::{ItemId, NodeId, RootId, SurfaceId};
use dockspace::policy::DockPolicy;
use dockspace::scene::PresentationPlan;

const SPLIT_ROOT: RootId = RootId::new(1);
const SINGLE_ROOT: RootId = RootId::new(2);
const SPLIT_SURFACE: SurfaceId = SurfaceId::new(1);
const SINGLE_SURFACE: SurfaceId = SurfaceId::new(2);

struct Fixture {
    workspace: Workspace,
    split_root_node: NodeId,
    split_tabs_a: NodeId,
    split_tabs_b: NodeId,
    single_tabs: NodeId,
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

    builder.set_root(
        SPLIT_ROOT,
        RootRecord::new(split_root_node).with_central(split_tabs_b),
    );
    builder.set_root(
        SINGLE_ROOT,
        RootRecord::new(single_tabs).with_central(single_tabs),
    );
    builder.set_surface(SPLIT_SURFACE, SurfacePresentation::with_main(SPLIT_ROOT));
    builder.set_surface(SINGLE_SURFACE, SurfacePresentation::with_main(SINGLE_ROOT));

    Fixture {
        workspace: builder.build().expect("fixture workspace is valid"),
        split_root_node,
        split_tabs_a,
        split_tabs_b,
        single_tabs,
    }
}

fn bounds() -> LogicalRect {
    LogicalRect::new(0.0, 0.0, 400.0, 300.0).expect("test rectangle is valid")
}

fn engine(fixture: &Fixture, policy: DockPolicy) -> (DockEngine, support::TestPresentationHost) {
    let mut engine =
        DockEngine::new(fixture.workspace.clone(), policy).expect("fixture engine is valid");
    let host = support::TestPresentationHost::new(&mut engine);
    (engine, host)
}

fn publish_all_surfaces(
    (mut engine, mut host): (DockEngine, support::TestPresentationHost),
) -> (DockEngine, support::TestPresentationHost) {
    support::publish_surfaces(
        &mut engine,
        &mut host,
        [(SPLIT_SURFACE, bounds()), (SINGLE_SURFACE, bounds())],
    );
    (engine, host)
}

fn ready_from(engine: &DockEngine, surface: SurfaceId) -> &PresentationPlan {
    support::next_plan(engine, surface)
}

fn cluster(ready: &PresentationPlan, scope: DropGuideScope) -> &DropGuideClusterRecord {
    ready
        .drop_guide_clusters()
        .iter()
        .find(|cluster| cluster.id().scope == scope)
        .unwrap_or_else(|| panic!("expected {scope:?} guide cluster"))
}

fn canonical_inner_slots() -> Vec<DropGuideSlot> {
    vec![
        DropGuideSlot::Center,
        DropGuideSlot::Edge(Edge::Left),
        DropGuideSlot::Edge(Edge::Right),
        DropGuideSlot::Edge(Edge::Top),
        DropGuideSlot::Edge(Edge::Bottom),
    ]
}

fn canonical_outer_slots() -> Vec<DropGuideSlot> {
    [Edge::Left, Edge::Right, Edge::Top, Edge::Bottom]
        .map(DropGuideSlot::Edge)
        .to_vec()
}

fn rect_contains(outer: LogicalRect, inner: LogicalRect) -> bool {
    outer.x() <= inner.x()
        && outer.y() <= inner.y()
        && outer.max().x() >= inner.max().x()
        && outer.max().y() >= inner.max().y()
}

#[test]
fn core_compiler_emits_only_complete_inner_and_outer_shapes() {
    let fixture = fixture();
    let (mut engine, mut host) = engine(&fixture, DockPolicy::default());
    support::install_surface_projection(&mut engine, &mut host, SPLIT_SURFACE, bounds());
    let ready = ready_from(&engine, SPLIT_SURFACE);

    for tabs in [fixture.split_tabs_a, fixture.split_tabs_b] {
        let inner = cluster(&ready, DropGuideScope::Inner(tabs));
        assert_eq!(
            inner.targets().map(|(slot, _)| slot).collect::<Vec<_>>(),
            canonical_inner_slots()
        );
        assert!(inner.target(DropGuideSlot::Center).is_some());
        assert!(inner.edges().is_some());
    }

    let outer = cluster(&ready, DropGuideScope::Outer);
    assert_eq!(
        outer.targets().map(|(slot, _)| slot).collect::<Vec<_>>(),
        canonical_outer_slots()
    );
    assert!(outer.target(DropGuideSlot::Center).is_none());
    assert!(outer.edges().is_some());
    for edge in [Edge::Left, Edge::Right, Edge::Top, Edge::Bottom] {
        assert!(matches!(
            outer
                .target(DropGuideSlot::Edge(edge))
                .expect("outer edge slot is complete")
                .id(),
            DropTargetId::OuterEdge { node, .. } if node == fixture.split_root_node
        ));
    }
}

#[test]
fn root_central_leaf_uses_center_only_inner_plus_outer_edges() {
    let fixture = fixture();
    let (mut engine, mut host) = engine(&fixture, DockPolicy::default());
    support::install_surface_projection(&mut engine, &mut host, SINGLE_SURFACE, bounds());
    let ready = ready_from(&engine, SINGLE_SURFACE);

    let inner = cluster(&ready, DropGuideScope::Inner(fixture.single_tabs));
    assert_eq!(
        inner.targets().map(|(slot, _)| slot).collect::<Vec<_>>(),
        vec![DropGuideSlot::Center]
    );
    assert!(inner.edges().is_none());

    let outer = cluster(&ready, DropGuideScope::Outer);
    assert_eq!(
        outer.targets().map(|(slot, _)| slot).collect::<Vec<_>>(),
        canonical_outer_slots()
    );
    for edge in [Edge::Left, Edge::Right, Edge::Top, Edge::Bottom] {
        assert!(matches!(
            outer
                .target(DropGuideSlot::Edge(edge))
                .expect("outer edge slot is complete")
                .id(),
            DropTargetId::OuterEdge { node, .. } if node == fixture.single_tabs
        ));
    }
}

#[test]
fn published_plans_keep_clusters_and_slots_in_canonical_order() {
    let fixture = fixture();
    let (engine, _host) = publish_all_surfaces(engine(&fixture, DockPolicy::default()));
    let ready = ready_from(&engine, SPLIT_SURFACE);

    let cluster_ids = ready
        .drop_guide_clusters()
        .iter()
        .map(DropGuideClusterRecord::id)
        .collect::<Vec<_>>();
    let mut sorted_cluster_ids = cluster_ids.clone();
    sorted_cluster_ids.sort_unstable();
    assert_eq!(cluster_ids, sorted_cluster_ids);
    assert_eq!(
        cluster_ids,
        [
            DropGuideClusterId::inner(SPLIT_SURFACE, SPLIT_ROOT, fixture.split_tabs_a),
            DropGuideClusterId::inner(SPLIT_SURFACE, SPLIT_ROOT, fixture.split_tabs_b),
            DropGuideClusterId::outer(SPLIT_SURFACE, SPLIT_ROOT),
        ]
        .into_iter()
        .collect::<Vec<_>>()
    );

    for cluster in ready.drop_guide_clusters() {
        let slots = cluster.targets().map(|(slot, _)| slot).collect::<Vec<_>>();
        match cluster.id().scope {
            DropGuideScope::Inner(_) => assert_eq!(slots, canonical_inner_slots()),
            DropGuideScope::Outer => assert_eq!(slots, canonical_outer_slots()),
        }
    }
}

#[test]
fn compiled_guide_geometry_separates_activation_draw_hit_and_preview() {
    let fixture = fixture();
    let (mut engine, mut host) = engine(&fixture, DockPolicy::default());
    support::install_surface_projection(&mut engine, &mut host, SPLIT_SURFACE, bounds());
    let ready = ready_from(&engine, SPLIT_SURFACE);

    for cluster in ready.drop_guide_clusters() {
        let activation = cluster.activation().rect();
        for (slot, guide) in cluster.targets() {
            let draw = guide.draw();
            let hit = guide.target().region().rect();
            let preview = guide.target().visual().rect();

            assert!(
                rect_contains(activation, hit),
                "{slot:?} hit geometry stays inside cluster activation"
            );
            assert!(
                rect_contains(hit, draw),
                "{slot:?} draw geometry stays inside the exact hit geometry"
            );
            assert!(
                rect_contains(activation, preview),
                "{slot:?} preview stays inside cluster activation"
            );
            assert_ne!(draw, hit, "{slot:?} draw and hit remain distinct");
            assert_ne!(draw, preview, "{slot:?} draw and preview remain distinct");
            assert_ne!(hit, preview, "{slot:?} hit and preview remain distinct");
            assert_eq!(cluster.layer(), guide.target().layer());
        }
    }
}

#[test]
fn policy_availability_is_compiled_into_every_guide_owned_target() {
    let fixture = fixture();
    let mut policy = DockPolicy::default();
    policy.set_allow_tab_merge(false);
    policy.set_allow_edge_split(false);
    let (engine, _host) = publish_all_surfaces(engine(&fixture, policy));
    let ready = ready_from(&engine, SPLIT_SURFACE);

    for cluster in ready.drop_guide_clusters() {
        for (_, target) in cluster.targets() {
            assert_eq!(
                target.target().availability(),
                DropTargetAvailability::Unavailable(DropTargetUnavailable::PolicyDisabled)
            );
        }
    }
}
