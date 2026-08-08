use super::support;

use std::collections::BTreeSet;

use dockspace::drop_guide::DropGuideScope;
use dockspace::engine::DockEngine;
use dockspace::geometry::LogicalRect;
use dockspace::graph::{Axis, ContainedFloating, Node, RootRecord, SurfacePresentation, Workspace};
use dockspace::ids::{FloatingPresentationId, ItemId, RootId, SurfaceId};
use dockspace::policy::DockPolicy;
use dockspace::presentation_hit::{
    PresentationHitManifest, PresentationHitRegionKind, PresentationPointerLane,
};
use dockspace::scene::PresentationPlan;
use support::{TestPresentationHost, install_surface_projection, publish_surface};

const SURFACE: SurfaceId = SurfaceId::new(1);
const MAIN_ROOT: RootId = RootId::new(10);
const FLOATING_ROOT: RootId = RootId::new(20);
const FLOATING: FloatingPresentationId = FloatingPresentationId::new(30);

fn bounds() -> LogicalRect {
    LogicalRect::new(0.0, 0.0, 640.0, 480.0).expect("surface bounds are valid")
}

fn workspace() -> Workspace {
    let mut builder = Workspace::builder();
    let top_left = builder.insert_node(Node::tabs([ItemId::new(1), ItemId::new(2)]));
    let bottom_left = builder.insert_node(Node::tabs([ItemId::new(3)]));
    let left = builder.insert_node(
        Node::equal_split(Axis::Vertical, [top_left, bottom_left]).expect("left split is valid"),
    );
    let right = builder.insert_node(Node::tabs([ItemId::new(4), ItemId::new(5)]));
    let main = builder.insert_node(
        Node::equal_split(Axis::Horizontal, [left, right]).expect("main split is valid"),
    );
    let floating_tabs = builder.insert_node(Node::tabs([ItemId::new(6), ItemId::new(7)]));

    builder.set_root(MAIN_ROOT, RootRecord::new(main).with_central(bottom_left));
    builder.set_root(
        FLOATING_ROOT,
        RootRecord::new(floating_tabs).with_central(floating_tabs),
    );
    builder.set_surface(SURFACE, SurfacePresentation::with_main(MAIN_ROOT));
    builder.set_contained_floating(
        FLOATING,
        ContainedFloating::new(
            FLOATING_ROOT,
            LogicalRect::new(330.0, 90.0, 250.0, 260.0).expect("floating bounds are valid"),
        ),
    );
    builder
        .attach_contained(SURFACE, FLOATING)
        .expect("surface accepts the contained presentation");
    builder.build().expect("workspace is valid")
}

fn rootless_workspace() -> Workspace {
    let mut builder = Workspace::builder();
    let tabs = builder.insert_node(Node::tabs([ItemId::new(40)]));
    builder.set_root(FLOATING_ROOT, RootRecord::new(tabs).with_central(tabs));
    builder.set_surface(SURFACE, SurfacePresentation::rootless());
    builder.set_contained_floating(
        FLOATING,
        ContainedFloating::new(
            FLOATING_ROOT,
            LogicalRect::new(120.0, 90.0, 280.0, 220.0).expect("floating bounds are valid"),
        ),
    );
    builder
        .attach_contained(SURFACE, FLOATING)
        .expect("rootless surface accepts contained content");
    builder.build().expect("rootless workspace is valid")
}

fn has_area(rect: LogicalRect) -> bool {
    rect.width() > 0.0 && rect.height() > 0.0
}

fn expected_kinds(plan: &PresentationPlan) -> BTreeSet<PresentationHitRegionKind> {
    let mut expected = BTreeSet::new();
    for pane in plan.pane_records() {
        if has_area(pane.content_bounds()) {
            expected.insert(PresentationHitRegionKind::PaneBody(pane.id()));
        }
    }
    for tab in plan.tab_records() {
        if has_area(tab.drag_hit().rect()) {
            expected.insert(PresentationHitRegionKind::TabBody(*tab.id()));
        }
        if tab.close_bounds().is_some_and(has_area) {
            expected.insert(PresentationHitRegionKind::TabClose(*tab.id()));
        }
    }
    for bar in plan.tab_bar_records() {
        if bar
            .group_drag()
            .is_some_and(|group| has_area(group.hit().rect()))
        {
            expected.insert(PresentationHitRegionKind::TabGroupGrip(*bar.id()));
        }
    }
    for splitter in plan
        .splitter_records()
        .iter()
        .filter(|splitter| splitter.operable() && has_area(splitter.hit().rect()))
    {
        expected.insert(PresentationHitRegionKind::SplitterHandle(*splitter.id()));
    }
    for junction in plan
        .splitter_junction_records()
        .iter()
        .filter(|junction| has_area(junction.hit().rect()))
    {
        expected.insert(PresentationHitRegionKind::SplitterJunction(junction.id()));
    }
    for contained in plan.contained_records() {
        let floating = contained.floating();
        if has_area(contained.outer_bounds()) {
            expected.insert(PresentationHitRegionKind::ContainedFrameBlocker(floating));
        }
        if has_area(contained.title_drag_hit().rect()) {
            expected.insert(PresentationHitRegionKind::ContainedTitle(floating));
        }
        if contained.close_bounds().is_some_and(has_area) {
            expected.insert(PresentationHitRegionKind::ContainedClose(floating));
        }
        for resize in contained
            .resize()
            .iter()
            .filter(|resize| has_area(resize.hit().rect()))
        {
            expected.insert(PresentationHitRegionKind::ContainedResize {
                floating,
                direction: resize.direction(),
            });
        }
    }
    for cluster in plan.drop_guide_clusters() {
        if has_area(cluster.activation().rect()) {
            expected.insert(PresentationHitRegionKind::DropGuideActivation(cluster.id()));
        }
        for (_, target) in cluster
            .targets()
            .filter(|(_, target)| has_area(target.target().region().rect()))
        {
            expected.insert(PresentationHitRegionKind::DropTarget(target.id()));
        }
    }
    for target in plan
        .drop_targets()
        .iter()
        .chain(plan.surface_background())
        .filter(|target| has_area(target.region().rect()))
    {
        expected.insert(PresentationHitRegionKind::DropTarget(target.id()));
    }
    expected
}

fn assert_exact_manifest(plan: &PresentationPlan, manifest: &PresentationHitManifest) {
    let actual = manifest
        .regions()
        .iter()
        .map(|region| {
            assert_eq!(region.id().surface(), plan.surface());
            region.id().kind()
        })
        .collect::<BTreeSet<_>>();
    assert_eq!(actual, expected_kinds(plan));
    assert_eq!(actual.len(), manifest.regions().len());
    assert!(
        manifest
            .regions()
            .windows(2)
            .all(|pair| pair[0].id() < pair[1].id()),
        "manifest storage order is canonical identity order, not hit precedence"
    );
}

#[test]
fn candidate_and_presented_output_keep_one_exact_hit_inventory() {
    let mut engine = DockEngine::new(workspace(), DockPolicy::default()).expect("engine is valid");
    let mut host = TestPresentationHost::new(&mut engine);
    install_surface_projection(&mut engine, &mut host, SURFACE, bounds());

    let candidate_scene = engine
        .scene()
        .surface(SURFACE)
        .expect("candidate surface exists");
    assert!(engine.interaction_projection(SURFACE).is_none());
    let candidate = candidate_scene
        .paint_projection()
        .expect("candidate remains paintable");
    assert_eq!(candidate.hit_manifest().output(), candidate.output_ticket());
    assert_exact_manifest(candidate.plan(), candidate.hit_manifest());

    publish_surface(&mut engine, &mut host, SURFACE, bounds());
    let presented = engine
        .interaction_projection(SURFACE)
        .expect("observed output becomes interactive");
    assert_eq!(presented.hit_manifest().output(), presented.output_ticket());
    assert!(
        presented
            .authority()
            .matches_output(presented.output_ticket())
    );
    assert_exact_manifest(presented.plan(), presented.hit_manifest());
}

#[test]
fn typed_stack_keys_encode_chrome_and_guide_precedence_without_vector_order() {
    let mut engine = DockEngine::new(workspace(), DockPolicy::default()).expect("engine is valid");
    let mut host = TestPresentationHost::new(&mut engine);
    publish_surface(&mut engine, &mut host, SURFACE, bounds());
    let interaction = engine
        .interaction_projection(SURFACE)
        .expect("surface is interactive");
    let manifest = interaction.hit_manifest();

    let junction = manifest
        .regions()
        .iter()
        .find(|region| {
            matches!(
                region.id().kind(),
                PresentationHitRegionKind::SplitterJunction(_)
            )
        })
        .expect("perpendicular splitters publish a junction");
    let junction_id = match junction.id().kind() {
        PresentationHitRegionKind::SplitterJunction(id) => id,
        _ => unreachable!(),
    };
    for handle in manifest.regions().iter().filter(|region| {
        matches!(
            region.id().kind(),
            PresentationHitRegionKind::SplitterHandle(id)
                if junction_id.splitters().contains(&id)
        )
    }) {
        assert!(junction.stack() > handle.stack());
    }

    let blocker = manifest
        .regions()
        .iter()
        .find(|region| {
            region.id().kind() == PresentationHitRegionKind::ContainedFrameBlocker(FLOATING)
        })
        .expect("contained frame blocker exists");
    assert!(
        blocker.lanes().contains(PresentationPointerLane::HoverDrop),
        "a non-source contained frame blocks lower drop targets"
    );
    for chrome in manifest.regions().iter().filter(|region| {
        matches!(
            region.id().kind(),
            PresentationHitRegionKind::ContainedTitle(FLOATING)
                | PresentationHitRegionKind::ContainedClose(FLOATING)
                | PresentationHitRegionKind::ContainedResize {
                    floating: FLOATING,
                    ..
                }
        )
    }) {
        assert!(chrome.stack() > blocker.stack());
    }

    let outer = manifest
        .regions()
        .iter()
        .filter(|region| {
            matches!(region.id().kind(), PresentationHitRegionKind::DropTarget(_))
                && region.lanes().contains(PresentationPointerLane::HoverDrop)
        })
        .find(|region| {
            let target = match region.id().kind() {
                PresentationHitRegionKind::DropTarget(target) => target,
                _ => unreachable!(),
            };
            interaction
                .plan()
                .drop_guide_clusters()
                .iter()
                .any(|cluster| {
                    cluster.id().scope == DropGuideScope::Outer
                        && cluster.targets().any(|(_, entry)| entry.id() == target)
                })
        })
        .expect("outer guide target exists");
    let inner = manifest
        .regions()
        .iter()
        .filter(|region| {
            matches!(region.id().kind(), PresentationHitRegionKind::DropTarget(_))
                && region.stack().layer() == outer.stack().layer()
        })
        .find(|region| {
            let target = match region.id().kind() {
                PresentationHitRegionKind::DropTarget(target) => target,
                _ => unreachable!(),
            };
            interaction
                .plan()
                .drop_guide_clusters()
                .iter()
                .any(|cluster| {
                    matches!(cluster.id().scope, DropGuideScope::Inner(_))
                        && cluster.targets().any(|(_, entry)| entry.id() == target)
                })
        })
        .expect("same-layer inner guide target exists");
    assert!(outer.stack() > inner.stack());
    assert!(manifest.regions().iter().any(|region| {
        matches!(
            region.id().kind(),
            PresentationHitRegionKind::DropGuideActivation(_)
        ) && region.is_passive()
    }));
}

#[test]
fn rootless_background_is_one_output_bound_hover_drop_receiver() {
    let mut engine =
        DockEngine::new(rootless_workspace(), DockPolicy::default()).expect("engine is valid");
    let mut host = TestPresentationHost::new(&mut engine);
    publish_surface(&mut engine, &mut host, SURFACE, bounds());
    let interaction = engine
        .interaction_projection(SURFACE)
        .expect("rootless surface is interactive");
    let backgrounds = interaction
        .hit_manifest()
        .regions()
        .iter()
        .filter(|region| {
            matches!(
                region.id().kind(),
                PresentationHitRegionKind::DropTarget(
                    dockspace::drop_target::DropTargetId::SurfaceBackground { surface: SURFACE }
                )
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(backgrounds.len(), 1);
    assert!(
        backgrounds[0]
            .lanes()
            .contains(PresentationPointerLane::HoverDrop)
    );
    assert!(!backgrounds[0].is_passive());
    assert_eq!(backgrounds[0].hit().rect(), bounds());
    assert_exact_manifest(interaction.plan(), interaction.hit_manifest());
}

#[test]
fn resize_policy_is_frozen_into_splitter_and_junction_hit_rosters() {
    let mut policy = DockPolicy::default();
    policy.set_allow_resize_axis(Axis::Horizontal, false);
    let mut engine = DockEngine::new(workspace(), policy).expect("engine is valid");
    let mut host = TestPresentationHost::new(&mut engine);
    publish_surface(&mut engine, &mut host, SURFACE, bounds());
    let interaction = engine
        .interaction_projection(SURFACE)
        .expect("surface is interactive");
    let plan = interaction.plan();
    assert!(
        plan.splitter_records()
            .iter()
            .any(|splitter| splitter.axis() == Axis::Vertical && splitter.operable())
    );
    assert!(
        plan.splitter_records()
            .iter()
            .any(|splitter| splitter.axis() == Axis::Horizontal && !splitter.operable())
    );
    assert!(plan.splitter_junction_records().is_empty());

    for region in interaction.hit_manifest().regions() {
        match region.id().kind() {
            PresentationHitRegionKind::SplitterHandle(id) => {
                assert_eq!(
                    plan.splitter_record(id)
                        .expect("manifest handle belongs to the exact plan")
                        .axis(),
                    Axis::Vertical
                );
            }
            PresentationHitRegionKind::SplitterJunction(_) => {
                panic!("a disabled axis cannot publish a junction receiver")
            }
            _ => {}
        }
    }
}
