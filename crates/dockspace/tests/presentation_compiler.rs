use super::support;

use dockspace::command::{DockTarget, Edge};
use dockspace::drop_guide::{DropGuideClusterRecord, DropGuideScope, DropGuideSlot};
use dockspace::drop_target::{
    DropDestination, DropTargetAvailability, DropTargetId, DropTargetUnavailable,
};
use dockspace::engine::DockEngine;
use dockspace::geometry::LogicalRect;
use dockspace::graph::{Axis, ContainedFloating, Node, RootRecord, SurfacePresentation, Workspace};
use dockspace::ids::{FloatingPresentationId, ItemId, RootId, SurfaceId};
use dockspace::policy::{
    CloseCapability, DockItemRule, DockPolicy, DockSurfaceRule, DockTargetRule, DockTargetRuleKey,
    TabBarInteraction, TabBarPolicy, TabBarVisibility,
};
use dockspace::scene::{SplitterGapPresentation, TabBarSceneId};
use support::{TestPresentationHost, install_surface_projection, next_plan, publish_surface};

const SURFACE: SurfaceId = SurfaceId::new(1);
const ROOT: RootId = RootId::new(2);
const FLOATING: FloatingPresentationId = FloatingPresentationId::new(3);
const GUIDE_EXTENT: f64 = 24.0;
const GUIDE_GAP: f64 = 8.0;
const GUIDE_HIT_PADDING: f64 = 4.0;
const GUIDE_HIT_EXTENT: f64 = GUIDE_EXTENT + 2.0 * GUIDE_HIT_PADDING;
const GUIDE_INNER_OFFSET: f64 = GUIDE_EXTENT + GUIDE_GAP;
const INNER_GUIDE_REFERENCE_SPAN: f64 =
    3.0 * GUIDE_EXTENT + 2.0 * GUIDE_GAP + 2.0 * GUIDE_HIT_PADDING;
const OUTER_GUIDE_REFERENCE_SPAN: f64 =
    2.0 * (48.0 + 2.0 * (GUIDE_EXTENT * 0.5 + GUIDE_HIT_PADDING));

fn bounds() -> LogicalRect {
    LogicalRect::new(0.0, 0.0, 360.0, 260.0).expect("surface bounds are valid")
}

#[test]
fn ordinary_noncentral_panes_keep_exact_five_way_square_guides() {
    let mut builder = Workspace::builder();
    let left = builder.insert_node(Node::tabs([ItemId::new(10)]));
    let right = builder.insert_node(Node::tabs([ItemId::new(11)]));
    let split = builder
        .insert_node(Node::equal_split(Axis::Horizontal, [left, right]).expect("split is valid"));
    builder.set_root(ROOT, RootRecord::new(split).with_central(right));
    builder.set_surface(SURFACE, SurfacePresentation::with_main(ROOT));
    let workspace = builder.build().expect("workspace is valid");
    let mut engine = DockEngine::new(workspace, DockPolicy::default()).expect("engine is valid");
    let mut host = TestPresentationHost::new(&mut engine);
    let guide_sized_bounds =
        LogicalRect::new(0.0, 0.0, 241.0, 140.0).expect("guide-sized bounds are valid");
    install_surface_projection(&mut engine, &mut host, SURFACE, guide_sized_bounds);
    let ready = next_plan(&engine, SURFACE);

    for tabs in [left, right] {
        let cluster = ready
            .drop_guide_clusters()
            .iter()
            .find(|cluster| cluster.id().scope == DropGuideScope::Inner(tabs))
            .expect("every ordinary noncentral pane keeps its inner guide");
        assert_eq!(cluster.targets().count(), 5);
        for slot in [
            DropGuideSlot::Center,
            DropGuideSlot::Edge(Edge::Left),
            DropGuideSlot::Edge(Edge::Right),
            DropGuideSlot::Edge(Edge::Top),
            DropGuideSlot::Edge(Edge::Bottom),
        ] {
            let target = cluster.target(slot).expect("all five slots exist");
            assert_eq!(target.draw().width(), GUIDE_EXTENT);
            assert_eq!(target.draw().height(), GUIDE_EXTENT);
            assert_eq!(target.target().region().rect().width(), GUIDE_HIT_EXTENT);
            assert_eq!(target.target().region().rect().height(), GUIDE_HIT_EXTENT);
        }
        let center = cluster
            .target(DropGuideSlot::Center)
            .expect("center exists")
            .draw();
        let left_draw = cluster
            .target(DropGuideSlot::Edge(Edge::Left))
            .expect("left exists")
            .draw();
        let top_draw = cluster
            .target(DropGuideSlot::Edge(Edge::Top))
            .expect("top exists")
            .draw();
        assert_eq!(center.x() - left_draw.x(), GUIDE_INNER_OFFSET);
        assert_eq!(center.y() - top_draw.y(), GUIDE_INNER_OFFSET);
    }
}

fn rects_overlap(left: LogicalRect, right: LogicalRect) -> bool {
    left.x() < right.max().x()
        && right.x() < left.max().x()
        && left.y() < right.max().y()
        && right.y() < left.max().y()
}

fn rect_contains(outer: LogicalRect, inner: LogicalRect) -> bool {
    outer.x() <= inner.x()
        && outer.y() <= inner.y()
        && outer.max().x() >= inner.max().x()
        && outer.max().y() >= inner.max().y()
}

fn assert_compact_cluster_geometry(cluster: &DropGuideClusterRecord, surface: LogicalRect) {
    let activation = cluster.activation().rect();
    assert!(rect_contains(surface, activation));
    let targets = cluster.targets().collect::<Vec<_>>();
    for (slot, target) in &targets {
        let draw = target.draw();
        let hit = target.target().region().rect();
        let preview = target.target().visual().rect();
        assert!(draw.width() > 0.0 && draw.height() > 0.0, "{slot:?}");
        assert!(hit.width() > 0.0 && hit.height() > 0.0, "{slot:?}");
        assert!(rect_contains(surface, draw), "{slot:?}");
        assert!(rect_contains(surface, hit), "{slot:?}");
        assert!(rect_contains(hit, draw), "{slot:?}");
        assert!(rect_contains(activation, preview), "{slot:?}");
        assert_ne!(draw, hit, "default hit padding remains observable");
        assert_ne!(draw, preview, "draw and preview remain separate facts");
        assert_ne!(hit, preview, "hit and preview remain separate facts");
    }
    for (index, (_, first)) in targets.iter().enumerate() {
        for (_, second) in targets.iter().skip(index + 1) {
            assert!(!rects_overlap(
                first.target().region().rect(),
                second.target().region().rect(),
            ));
        }
    }
}

fn assert_close(actual: f64, expected: f64) {
    let tolerance = expected.abs().max(1.0) * 64.0 * f64::EPSILON;
    assert!(
        (actual - expected).abs() <= tolerance,
        "expected {expected}, got {actual}"
    );
}

fn compact_guide_workspace(root_central: bool) -> (Workspace, dockspace::ids::NodeId) {
    let mut builder = Workspace::builder();
    let tabs = builder.insert_node(Node::tabs([ItemId::new(10)]));
    let root = if root_central {
        RootRecord::new(tabs).with_central(tabs)
    } else {
        RootRecord::new(tabs)
    };
    builder.set_root(ROOT, root);
    builder.set_surface(SURFACE, SurfacePresentation::with_main(ROOT));
    (builder.build().expect("workspace is valid"), tabs)
}

#[test]
fn positive_narrow_and_flat_leaves_keep_complete_compact_guide_semantics() {
    for surface in [
        LogicalRect::new(10.0, 20.0, 0.75, 40.0).expect("narrow bounds are valid"),
        LogicalRect::new(10.0, 20.0, 40.0, 0.75).expect("flat bounds are valid"),
    ] {
        let (workspace, tabs) = compact_guide_workspace(false);
        let mut engine =
            DockEngine::new(workspace, DockPolicy::default()).expect("engine is valid");
        let mut host = TestPresentationHost::new(&mut engine);
        support::publish_surface(&mut engine, &mut host, SURFACE, surface);
        let first = support::painted_plan(&engine, SURFACE)
            .drop_guide_clusters()
            .to_vec();

        let inner = first
            .iter()
            .find(|cluster| cluster.id().scope == DropGuideScope::Inner(tabs))
            .expect("every positive ordinary leaf keeps an inner cluster");
        assert_eq!(inner.activation().rect(), surface);
        assert_eq!(inner.targets().count(), 5);
        assert_compact_cluster_geometry(inner, surface);
        let pane = support::painted_plan(&engine, SURFACE)
            .pane_records()
            .iter()
            .find(|pane| pane.id().tabs == tabs)
            .expect("compiled pane exists");
        let content = pane.content_bounds();
        let placement = if content.width() > 0.0 && content.height() > 0.0 {
            content
        } else {
            surface
        };
        let inner_scale = (placement.width() / INNER_GUIDE_REFERENCE_SPAN)
            .min(placement.height() / INNER_GUIDE_REFERENCE_SPAN)
            .min(1.0);
        let center = inner
            .target(DropGuideSlot::Center)
            .expect("inner center remains present");
        assert_eq!(center.target().visual().rect(), placement);
        assert!(rect_contains(placement, center.draw()));
        assert!(rect_contains(placement, center.target().region().rect()));
        assert_close(center.draw().width(), GUIDE_EXTENT * inner_scale);
        assert_close(
            center.target().region().rect().width(),
            GUIDE_HIT_EXTENT * inner_scale,
        );
        for edge in [Edge::Left, Edge::Right, Edge::Top, Edge::Bottom] {
            let preview = inner
                .target(DropGuideSlot::Edge(edge))
                .expect("every inner edge remains present")
                .target()
                .visual()
                .rect();
            assert_ne!(preview, inner.activation().rect());
            match edge {
                Edge::Left | Edge::Right => {
                    assert_close(preview.width(), surface.width() * 0.5);
                    assert_eq!(preview.height(), surface.height());
                }
                Edge::Top | Edge::Bottom => {
                    assert_eq!(preview.width(), surface.width());
                    assert_close(preview.height(), surface.height() * 0.5);
                }
            }
        }

        let outer = first
            .iter()
            .find(|cluster| cluster.id().scope == DropGuideScope::Outer)
            .expect("every positive root keeps an outer cluster");
        assert_eq!(outer.targets().count(), 4);
        assert_compact_cluster_geometry(outer, surface);
        let outer_scale = (surface.width() / OUTER_GUIDE_REFERENCE_SPAN)
            .min(surface.height() / OUTER_GUIDE_REFERENCE_SPAN)
            .min(1.0);
        for (_, target) in outer.targets() {
            assert_close(target.draw().width(), GUIDE_EXTENT * outer_scale);
            assert_close(
                target.target().region().rect().width(),
                GUIDE_HIT_EXTENT * outer_scale,
            );
        }

        install_surface_projection(&mut engine, &mut host, SURFACE, surface);
        assert_eq!(
            next_plan(&engine, SURFACE).drop_guide_clusters(),
            first,
            "the same config and bounds compile identical guide geometry"
        );
    }
}

#[test]
fn positive_root_central_leaf_keeps_center_only_inner_plus_compact_outer_four() {
    for surface in [
        LogicalRect::new(10.0, 20.0, 0.75, 40.0).expect("narrow bounds are valid"),
        LogicalRect::new(10.0, 20.0, 40.0, 0.75).expect("flat bounds are valid"),
    ] {
        let (workspace, tabs) = compact_guide_workspace(true);
        let mut engine =
            DockEngine::new(workspace, DockPolicy::default()).expect("engine is valid");
        let mut host = TestPresentationHost::new(&mut engine);
        install_surface_projection(&mut engine, &mut host, SURFACE, surface);
        let ready = next_plan(&engine, SURFACE);

        let inner = ready
            .drop_guide_clusters()
            .iter()
            .find(|cluster| cluster.id().scope == DropGuideScope::Inner(tabs))
            .expect("root-central leaf keeps its center cluster");
        assert_eq!(inner.targets().count(), 1);
        assert!(inner.target(DropGuideSlot::Center).is_some());
        assert_compact_cluster_geometry(inner, surface);
        let pane = ready
            .pane_records()
            .iter()
            .find(|pane| pane.id().tabs == tabs)
            .expect("compiled pane exists");
        let expected_center_preview =
            if pane.content_bounds().width() > 0.0 && pane.content_bounds().height() > 0.0 {
                pane.content_bounds()
            } else {
                surface
            };
        assert_eq!(
            inner
                .target(DropGuideSlot::Center)
                .expect("center exists")
                .target()
                .visual()
                .rect(),
            expected_center_preview
        );

        let outer = ready
            .drop_guide_clusters()
            .iter()
            .find(|cluster| cluster.id().scope == DropGuideScope::Outer)
            .expect("root-central leaf keeps a separate outer cluster");
        assert_eq!(outer.targets().count(), 4);
        assert_compact_cluster_geometry(outer, surface);
    }
}

#[test]
fn central_root_compiles_center_only_inner_and_exact_outer_guides() {
    let mut builder = Workspace::builder();
    let tabs = builder.insert_node(Node::tabs([ItemId::new(10), ItemId::new(11)]));
    builder.set_root(ROOT, RootRecord::new(tabs).with_central(tabs));
    builder.set_surface(SURFACE, SurfacePresentation::with_main(ROOT));
    let workspace = builder.build().expect("workspace is valid");
    let mut engine = DockEngine::new(workspace, DockPolicy::default()).expect("engine is valid");
    let mut host = TestPresentationHost::new(&mut engine);
    publish_surface(&mut engine, &mut host, SURFACE, bounds());
    let ready = next_plan(&engine, SURFACE);

    let inner = ready
        .drop_guide_clusters()
        .iter()
        .find(|cluster| cluster.id().scope == DropGuideScope::Inner(tabs))
        .expect("central leaf has an inner cluster");
    let outer = ready
        .drop_guide_clusters()
        .iter()
        .find(|cluster| cluster.id().scope == DropGuideScope::Outer)
        .expect("root has a distinct outer cluster");
    assert_eq!(inner.targets().count(), 1);
    assert!(inner.target(DropGuideSlot::Center).is_some());
    assert_eq!(outer.targets().count(), 4);
    assert_eq!(ready.drop_targets().len(), 3, "two tabs expose three gaps");

    let top = outer
        .target(DropGuideSlot::Edge(Edge::Top))
        .expect("top guide exists");
    assert_eq!(top.draw().width(), GUIDE_EXTENT);
    assert_eq!(top.draw().height(), GUIDE_EXTENT);
    assert_eq!(top.target().region().rect().width(), GUIDE_HIT_EXTENT);
    assert_eq!(top.target().region().rect().height(), GUIDE_HIT_EXTENT);
    assert!(matches!(
        top.target().destination(),
        DropDestination::Topology(DockTarget::OuterEdge(target))
            if target.edge() == Edge::Top
                && (f64::from(target.fraction().get()) - 0.5).abs() < f64::EPSILON
    ));
    assert_eq!(
        top.target().visual().rect().height(),
        ready.bounds().height() * 0.5
    );
}

#[test]
fn empty_central_root_compiles_a_center_and_four_explicit_outer_guides() {
    let mut builder = Workspace::builder();
    let tabs = builder.insert_node(Node::tabs([]));
    builder.set_root(ROOT, RootRecord::new(tabs).with_central(tabs));
    builder.set_surface(SURFACE, SurfacePresentation::with_main(ROOT));
    let workspace = builder.build().expect("empty central workspace is valid");
    let mut engine = DockEngine::new(workspace, DockPolicy::default()).expect("engine is valid");
    let mut host = TestPresentationHost::new(&mut engine);
    publish_surface(&mut engine, &mut host, SURFACE, bounds());
    let ready = next_plan(&engine, SURFACE);

    let inner = ready
        .drop_guide_clusters()
        .iter()
        .find(|cluster| cluster.id().scope == DropGuideScope::Inner(tabs))
        .expect("empty central leaf has an inner center cluster");
    assert_eq!(inner.targets().count(), 1);
    let center = inner
        .target(DropGuideSlot::Center)
        .expect("empty central center target exists");
    assert!(matches!(
        center.target().destination(),
        DropDestination::Topology(DockTarget::Center(target))
            if target.rule() == DockTargetRuleKey::Root(ROOT)
    ));

    let outer = ready
        .drop_guide_clusters()
        .iter()
        .find(|cluster| cluster.id().scope == DropGuideScope::Outer)
        .expect("empty central root has an outer edge cluster");
    assert_eq!(outer.targets().count(), 4);
    for edge in [Edge::Left, Edge::Right, Edge::Top, Edge::Bottom] {
        assert!(outer.target(DropGuideSlot::Edge(edge)).is_some());
    }
    assert!(
        ready.drop_targets().is_empty(),
        "empty tabs expose no tab gaps"
    );
}

#[test]
fn nested_central_leaf_keeps_all_inner_edges_alongside_root_outer_edges() {
    let mut builder = Workspace::builder();
    let left = builder.insert_node(Node::tabs([ItemId::new(10)]));
    let right = builder.insert_node(Node::tabs([ItemId::new(11)]));
    let split = builder
        .insert_node(Node::equal_split(Axis::Horizontal, [left, right]).expect("split is valid"));
    builder.set_root(ROOT, RootRecord::new(split).with_central(right));
    builder.set_surface(SURFACE, SurfacePresentation::with_main(ROOT));
    let workspace = builder.build().expect("workspace is valid");
    let mut engine = DockEngine::new(workspace, DockPolicy::default()).expect("engine is valid");
    let mut host = TestPresentationHost::new(&mut engine);
    publish_surface(&mut engine, &mut host, SURFACE, bounds());
    let ready = next_plan(&engine, SURFACE);

    assert_eq!(ready.drop_guide_clusters().len(), 3);
    for tabs in [left, right] {
        let inner = ready
            .drop_guide_clusters()
            .iter()
            .find(|cluster| cluster.id().scope == DropGuideScope::Inner(tabs))
            .expect("every nested leaf has an inner cluster");
        assert_eq!(inner.targets().count(), 5);
        let targets = inner.targets().collect::<Vec<_>>();
        for (index, (_, first)) in targets.iter().enumerate() {
            for (_, second) in targets.iter().skip(index + 1) {
                assert!(!rects_overlap(
                    first.target().region().rect(),
                    second.target().region().rect()
                ));
            }
        }
    }
    let outer = ready
        .drop_guide_clusters()
        .iter()
        .find(|cluster| cluster.id().scope == DropGuideScope::Outer)
        .expect("split root has an outer cluster");
    assert_eq!(outer.targets().count(), 4);
    assert_eq!(ready.splitter_gap_records().len(), 1);
    assert_eq!(
        ready.splitter_gap_records()[0].presentation(),
        SplitterGapPresentation::Rendered
    );
    assert_eq!(ready.splitter_records().len(), 1);
}

#[test]
fn rootless_contained_surface_compiles_background_occlusion_and_minimum() {
    let mut builder = Workspace::builder();
    let tabs = builder.insert_node(Node::tabs([ItemId::new(10)]));
    builder.set_root(ROOT, RootRecord::new(tabs).with_central(tabs));
    builder.set_surface(SURFACE, SurfacePresentation::rootless());
    builder.set_contained_floating(
        FLOATING,
        ContainedFloating::new(
            ROOT,
            LogicalRect::new(20.0, 20.0, 300.0, 220.0).expect("floating rect is valid"),
        ),
    );
    builder
        .attach_contained(SURFACE, FLOATING)
        .expect("surface exists");
    let workspace = builder.build().expect("workspace is valid");
    let mut engine = DockEngine::new(workspace, DockPolicy::default()).expect("engine is valid");
    let mut host = TestPresentationHost::new(&mut engine);
    publish_surface(&mut engine, &mut host, SURFACE, bounds());
    let ready = next_plan(&engine, SURFACE);

    let background = ready
        .surface_background()
        .expect("rootless surface has explicit background authority");
    assert!(matches!(
        background.id(),
        DropTargetId::SurfaceBackground { surface: SURFACE }
    ));
    assert_eq!(ready.drop_occlusions().len(), 1);
    assert_eq!(ready.drop_occlusions()[0].floating(), FLOATING);
    assert_eq!(ready.contained_minimums().len(), 1);
    assert!(
        ready.contained_minimums()[0].minimum_size().height()
            >= engine
                .presentation_config()
                .minimum_floating_size()
                .height()
    );
    let [contained] = ready.contained_records() else {
        panic!("one structural contained presentation produces one chrome record");
    };
    assert_eq!(contained.floating(), FLOATING);
    assert_eq!(contained.root(), ROOT);
    assert_eq!(contained.ordinal(), 0);
    assert_eq!(
        contained.outer_bounds(),
        LogicalRect::new(20.0, 20.0, 300.0, 220.0).expect("expected outer bounds are valid")
    );
    assert!(contained.title_bounds().height() > 0.0);
    assert!(contained.content_bounds().height() > 0.0);
    let close = contained
        .close_bounds()
        .expect("the closeable root has a close control");
    assert!(contained.title_drag_hit().rect().max().x() <= close.x());
    assert_eq!(contained.resize().len(), 8);
    for (index, first) in contained.resize().iter().enumerate() {
        for second in contained.resize().iter().skip(index + 1) {
            assert!(!rects_overlap(first.hit().rect(), second.hit().rect()));
        }
    }
}

#[test]
fn frozen_policy_is_the_only_close_affordance_authority() {
    const ENABLED_ROOT: RootId = RootId::new(20);
    const LOCKED_ROOT: RootId = RootId::new(21);
    const ENABLED_FLOATING: FloatingPresentationId = FloatingPresentationId::new(30);
    const LOCKED_FLOATING: FloatingPresentationId = FloatingPresentationId::new(31);
    const DISABLED_TAB: ItemId = ItemId::new(10);
    const IMMEDIATE_TAB: ItemId = ItemId::new(11);
    const DEFERRED_TAB: ItemId = ItemId::new(12);
    const LOCKED_CONTAINED_ITEM: ItemId = ItemId::new(31);

    let mut builder = Workspace::builder();
    let main_tabs = builder.insert_node(Node::tabs([DISABLED_TAB, IMMEDIATE_TAB, DEFERRED_TAB]));
    let enabled_tabs = builder.insert_node(Node::tabs([ItemId::new(20), ItemId::new(21)]));
    let locked_tabs = builder.insert_node(Node::tabs([ItemId::new(30), LOCKED_CONTAINED_ITEM]));
    builder.set_root(ROOT, RootRecord::new(main_tabs).with_central(main_tabs));
    builder.set_root(
        ENABLED_ROOT,
        RootRecord::new(enabled_tabs).with_central(enabled_tabs),
    );
    builder.set_root(
        LOCKED_ROOT,
        RootRecord::new(locked_tabs).with_central(locked_tabs),
    );
    builder.set_surface(SURFACE, SurfacePresentation::with_main(ROOT));
    builder.set_contained_floating(
        ENABLED_FLOATING,
        ContainedFloating::new(
            ENABLED_ROOT,
            LogicalRect::new(20.0, 80.0, 150.0, 150.0).expect("floating rect is valid"),
        ),
    );
    builder.set_contained_floating(
        LOCKED_FLOATING,
        ContainedFloating::new(
            LOCKED_ROOT,
            LogicalRect::new(190.0, 80.0, 150.0, 150.0).expect("floating rect is valid"),
        ),
    );
    builder
        .attach_contained(SURFACE, ENABLED_FLOATING)
        .expect("surface exists");
    builder
        .attach_contained(SURFACE, LOCKED_FLOATING)
        .expect("surface exists");

    let mut policy = DockPolicy::default();
    let mut disabled = DockItemRule::default();
    disabled.set_close_capability(Some(CloseCapability::Disabled));
    let _ = policy.set_item_rule(DISABLED_TAB, disabled);
    let mut deferred = DockItemRule::default();
    deferred.set_close_capability(Some(CloseCapability::DeferredAllowed));
    let _ = policy.set_item_rule(DEFERRED_TAB, deferred);
    let mut contained_deferred = DockItemRule::default();
    contained_deferred.set_close_capability(Some(CloseCapability::DeferredAllowed));
    let _ = policy.set_item_rule(ItemId::new(21), contained_deferred);
    let mut locked_contained = DockItemRule::default();
    locked_contained.set_close_capability(Some(CloseCapability::Disabled));
    let _ = policy.set_item_rule(LOCKED_CONTAINED_ITEM, locked_contained);

    let workspace = builder.build().expect("workspace is valid");
    let mut engine = DockEngine::new(workspace, policy).expect("engine is valid");
    let mut host = TestPresentationHost::new(&mut engine);
    install_surface_projection(&mut engine, &mut host, SURFACE, bounds());
    let ready = next_plan(&engine, SURFACE);

    assert_eq!(
        ready
            .measurement_ticket()
            .expect("compiled plan is measurement-backed")
            .policy(),
        engine.policy_snapshot().revision(),
        "close geometry must be bound to the exact frozen policy revision"
    );
    let close_for = |item| {
        ready
            .tab_records()
            .iter()
            .find(|record| record.id().item == item)
            .expect("policy fixture tab is visible")
            .close_bounds()
    };
    assert!(
        close_for(DISABLED_TAB).is_none(),
        "Disabled must omit the operable close affordance"
    );
    assert!(
        close_for(IMMEDIATE_TAB).is_some(),
        "Immediate must publish the close affordance"
    );
    assert!(
        close_for(DEFERRED_TAB).is_some(),
        "DeferredAllowed must publish the same close affordance"
    );

    let enabled = ready
        .contained_record(ENABLED_FLOATING)
        .expect("enabled contained root is visible");
    assert!(
        enabled.close_bounds().is_some(),
        "an all-closeable contained root publishes one root close affordance"
    );
    let locked = ready
        .contained_record(LOCKED_FLOATING)
        .expect("locked contained root is visible");
    assert!(
        locked.close_bounds().is_none(),
        "one Disabled payload item must omit the contained-root close affordance"
    );
}

#[test]
fn workspace_hidden_tab_bar_is_omitted_from_requirements_and_scene_geometry() {
    let mut builder = Workspace::builder();
    let tabs = builder.insert_node(Node::tabs([ItemId::new(10), ItemId::new(11)]));
    builder.set_root(ROOT, RootRecord::new(tabs));
    builder.set_surface(SURFACE, SurfacePresentation::with_main(ROOT));
    let workspace = builder.build().expect("workspace is valid");
    let mut policy = DockPolicy::default();
    policy.set_tab_bar(TabBarPolicy::new(
        TabBarVisibility::Hidden,
        TabBarInteraction::Enabled,
    ));
    let mut engine = DockEngine::new(workspace, policy).expect("engine is valid");
    let mut host = TestPresentationHost::new(&mut engine);

    let requirements = engine
        .presentation_requirements()
        .surface(SURFACE)
        .expect("surface requirements exist");
    assert_eq!(requirements.tab_strips().count(), 0);
    assert_eq!(requirements.tab_intrinsics().count(), 0);

    install_surface_projection(&mut engine, &mut host, SURFACE, bounds());
    let ready = next_plan(&engine, SURFACE);
    let pane = ready
        .pane_records()
        .iter()
        .find(|pane| pane.id().tabs == tabs)
        .expect("hidden tab bar keeps pane content");
    assert_eq!(pane.bounds(), bounds());
    assert_eq!(pane.content_bounds(), bounds());
    assert!(ready.tab_bar_records().is_empty());
    assert!(ready.tab_records().is_empty());
    assert!(
        ready
            .drop_targets()
            .iter()
            .all(|target| !matches!(target.id(), DropTargetId::TabGap { .. }))
    );
}

#[test]
fn surface_hidden_tab_bar_does_not_affect_another_surface() {
    const OTHER_SURFACE: SurfaceId = SurfaceId::new(4);
    const OTHER_ROOT: RootId = RootId::new(5);
    let mut builder = Workspace::builder();
    let hidden_tabs = builder.insert_node(Node::tabs([ItemId::new(10)]));
    let visible_tabs = builder.insert_node(Node::tabs([ItemId::new(11)]));
    builder.set_root(ROOT, RootRecord::new(hidden_tabs));
    builder.set_root(OTHER_ROOT, RootRecord::new(visible_tabs));
    builder.set_surface(SURFACE, SurfacePresentation::with_main(ROOT));
    builder.set_surface(OTHER_SURFACE, SurfacePresentation::with_main(OTHER_ROOT));
    let workspace = builder.build().expect("workspace is valid");
    let mut policy = DockPolicy::default();
    let mut surface_rule = DockSurfaceRule::default();
    surface_rule.set_tab_bar(TabBarPolicy::new(
        TabBarVisibility::Hidden,
        TabBarInteraction::Enabled,
    ));
    policy.set_surface_rule(SURFACE, surface_rule);
    let engine = DockEngine::new(workspace, policy).expect("engine is valid");

    let hidden = engine
        .presentation_requirements()
        .surface(SURFACE)
        .expect("hidden surface requirements exist");
    assert_eq!(hidden.tab_strips().count(), 0);
    assert_eq!(hidden.tab_intrinsics().count(), 0);
    let visible = engine
        .presentation_requirements()
        .surface(OTHER_SURFACE)
        .expect("visible surface requirements exist");
    assert_eq!(visible.tab_strips().count(), 1);
    assert_eq!(visible.tab_intrinsics().count(), 1);
}

#[test]
fn target_disabled_tab_bar_keeps_paint_records_without_semantic_actions() {
    const SELECTED: ItemId = ItemId::new(10);
    let mut builder = Workspace::builder();
    let tabs = builder.insert_node(Node::tabs([SELECTED, ItemId::new(11)]));
    builder.set_root(ROOT, RootRecord::new(tabs));
    builder.set_surface(SURFACE, SurfacePresentation::with_main(ROOT));
    let workspace = builder.build().expect("workspace is valid");
    let mut policy = DockPolicy::default();
    let mut target_rule = DockTargetRule::default();
    target_rule.set_tab_bar(TabBarPolicy::new(
        TabBarVisibility::Visible,
        TabBarInteraction::Disabled,
    ));
    policy.set_target_rule(DockTargetRuleKey::Item(SELECTED), target_rule);
    let mut engine = DockEngine::new(workspace, policy).expect("engine is valid");
    let mut host = TestPresentationHost::new(&mut engine);

    install_surface_projection(&mut engine, &mut host, SURFACE, bounds());
    let ready = next_plan(&engine, SURFACE);
    let bar_id = TabBarSceneId { root: ROOT, tabs };
    let bar = ready
        .tab_bar_records()
        .iter()
        .find(|bar| *bar.id() == bar_id)
        .expect("paint-only bar remains in the scene");
    assert!(bar.group_drag().is_none());
    assert_eq!(ready.tab_records().len(), 2);
    for tab in ready.tab_records() {
        assert!(tab.visible_bounds().width() > 0.0);
        assert!(tab.visible_bounds().height() > 0.0);
        assert!(tab.drag_hit().rect().width() <= 0.0 || tab.drag_hit().rect().height() <= 0.0);
        assert!(tab.close_bounds().is_none());
    }
    assert!(
        ready
            .drop_targets()
            .iter()
            .all(|target| !matches!(target.id(), DropTargetId::TabGap { .. }))
    );
}

#[test]
fn policy_rejection_is_compiled_into_every_edge_without_removing_guides() {
    let mut builder = Workspace::builder();
    let tabs = builder.insert_node(Node::tabs([ItemId::new(10)]));
    builder.set_root(ROOT, RootRecord::new(tabs).with_central(tabs));
    builder.set_surface(SURFACE, SurfacePresentation::with_main(ROOT));
    let workspace = builder.build().expect("workspace is valid");
    let mut policy = DockPolicy::default();
    policy.set_allow_edge_split(false);
    let mut engine = DockEngine::new(workspace, policy).expect("engine is valid");
    let mut host = TestPresentationHost::new(&mut engine);
    install_surface_projection(&mut engine, &mut host, SURFACE, bounds());
    let ready = next_plan(&engine, SURFACE);

    for cluster in ready.drop_guide_clusters() {
        for (slot, target) in cluster.targets() {
            let expected = match slot {
                DropGuideSlot::Center => DropTargetAvailability::Available,
                DropGuideSlot::Edge(_) => {
                    DropTargetAvailability::Unavailable(DropTargetUnavailable::PolicyDisabled)
                }
            };
            assert_eq!(target.target().availability(), expected);
        }
    }
}
