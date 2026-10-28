use super::super::*;
use super::*;
use crate::graph::{Node, Workspace};
use crate::ids::{EngineAuthorityDomainId, ItemId, NodeId, WorkspaceEpoch};
use crate::policy::{PolicyRevision, TabBarInteraction};
use crate::presentation_config::{DockPresentationConfig, PresentationConfigRevision};
use crate::presentation_observation::PresentationOutputSerial;
use crate::scene::{
    PaneSceneId, PresentationMenuAnchorHost, PresentationMenuAnchorRecord, RootLayoutFacts,
    SplitterRecord, SurfaceSceneStamp, TabBarRecord, TabListMenuGeometryAvailability, TabSceneId,
};
use crate::scene_manifest::{
    Measurement, SurfaceMeasurementTicket, SurfaceMeasurements, SurfaceRequirementRevision,
    SurfaceRequirements, SurfaceSceneRevision,
};

fn rect(x: f64, y: f64, width: f64, height: f64) -> LogicalRect {
    LogicalRect::new(x, y, width, height).expect("test rectangle is valid")
}

fn point(x: f64, y: f64) -> LogicalPoint {
    LogicalPoint::new(x, y).expect("test point is valid")
}

fn node() -> NodeId {
    Workspace::builder().insert_node(Node::tabs([]))
}

fn ticket() -> SurfaceMeasurementTicket {
    SurfaceMeasurementTicket::new(
        EngineAuthorityDomainId::new_for_test(1),
        WorkspaceEpoch::new(1),
        PresentationConfigRevision::new(1),
        PolicyRevision::new(1),
        SurfaceRequirementRevision::new(1),
        SurfaceId::new(1),
    )
}

fn plan(config: DockPresentationConfig) -> PresentationPlan {
    let bounds = rect(0.0, 0.0, 600.0, 600.0);
    let requirements = SurfaceRequirements::new(
        ticket(),
        Default::default(),
        Default::default(),
        Default::default(),
        Default::default(),
    )
    .expect("empty exact requirements are valid");
    let mut measurements = SurfaceMeasurements::new(ticket());
    measurements
        .set_bounds(requirements.bounds(), Measurement::Measured(bounds))
        .expect("bounds are unique");
    let measurements = requirements
        .validate(&measurements)
        .expect("measurements satisfy the exact set")
        .require_authoritative()
        .expect("bounds are authoritative");
    let mut plan = PresentationPlan::new(SurfaceId::new(1), bounds);
    plan.install_layout_facts(config, measurements);
    plan.push_root_layout_facts(RootLayoutFacts::new(
        RootId::new(1),
        rect(40.0, 40.0, 520.0, 520.0),
        Default::default(),
    ));
    plan
}

fn region(
    kind: PresentationHitRegionKind,
    bounds: LogicalRect,
    layer: u64,
    precedence: PresentationHitPrecedence,
) -> PresentationHitRegion {
    PresentationHitRegion::new(
        PresentationHitRegionId::new(SurfaceId::new(1), kind),
        HitRegion::new(bounds),
        PresentationPointerLanes::CLICK_AND_DRAG,
        PresentationHitStackKey::new(
            PresentationPlane::Workspace,
            SceneLayerKey::new(layer),
            precedence,
        ),
        PresentationHitBehavior::ExclusiveReceiver,
    )
}

fn manifest(
    mut regions: Vec<PresentationHitRegion>,
    mut touch_regions: Vec<(PresentationHitRegionId, LogicalRect)>,
) -> PresentationHitManifest {
    regions.sort_unstable_by_key(|region| region.id());
    touch_regions.sort_by_key(|(id, _)| *id);
    PresentationHitManifest {
        output: SurfacePresentationOutputTicket::mint(
            ticket().authority_domain(),
            PresentationOutputSerial::new_for_test(1),
            SurfaceSceneStamp::new(ticket(), SurfaceSceneRevision::new(1)),
        ),
        regions,
        touch_regions,
    }
}

fn splitter(id: SplitterSceneId, draw: LogicalRect, axis: Axis) -> SplitterRecord {
    SplitterRecord::new(
        id,
        draw,
        HitRegion::new(draw),
        axis,
        rect(40.0, 40.0, 200.0, 200.0),
        rect(320.0, 320.0, 200.0, 200.0),
        0.0,
        0.0,
        520.0,
        520.0,
        vec![200.0, 200.0],
        None,
        Vec::new(),
        SceneLayerKey::surface_base(),
        true,
    )
}

#[test]
fn touch_resize_extents_preserve_larger_mouse_configuration() {
    for (requested, mouse_splitter, mouse_contained) in
        [(44.0, 80.0, 70.0), (60.0, 80.0, 90.0), (80.0, 20.0, 7.0)]
    {
        let config = DockPresentationConfig::builder()
            .touch_resize_hit_extent(requested)
            .splitter_hit_extent(mouse_splitter)
            .floating_resize_extent(mouse_contained)
            .build()
            .expect("custom extents are valid");
        let mut plan = plan(config);
        let north_south = SplitterSceneId {
            root: RootId::new(1),
            split: node(),
            index: 0,
        };
        let east_west = SplitterSceneId {
            index: 1,
            ..north_south
        };
        plan.push_splitter_record(splitter(
            north_south,
            rect(298.0, 40.0, 4.0, 520.0),
            Axis::Horizontal,
        ));
        plan.push_splitter_record(splitter(
            east_west,
            rect(40.0, 298.0, 520.0, 4.0),
            Axis::Vertical,
        ));
        let junction = SplitterJunctionId::new(
            Some(north_south),
            Some(east_west),
            Some(north_south),
            Some(east_west),
        );
        let handle = region(
            PresentationHitRegionKind::SplitterHandle(north_south),
            rect(298.0, 40.0, 4.0, 520.0),
            0,
            PresentationHitPrecedence::SplitterHandle,
        );
        let junction = region(
            PresentationHitRegionKind::SplitterJunction(junction),
            rect(298.0, 298.0, 4.0, 4.0),
            0,
            PresentationHitPrecedence::SplitterJunction,
        );
        let fragments = compile(&plan, &[handle, junction]);
        let extent = requested.max(mouse_splitter);
        assert_eq!(
            fragments,
            vec![
                (handle.id(), rect(300.0 - extent * 0.5, 40.0, extent, 520.0)),
                (
                    junction.id(),
                    rect(300.0 - extent * 0.5, 300.0 - extent * 0.5, extent, extent)
                ),
            ]
        );

        let floating = FloatingPresentationId::new(1);
        plan.push_drop_occlusion(DropOcclusionRecord::new(
            floating,
            HitRegion::new(rect(100.0, 100.0, 400.0, 400.0)),
            SceneLayerKey::new(1),
        ));
        let west = region(
            PresentationHitRegionKind::ContainedResize {
                floating,
                direction: ContainedResizeDirection::West,
            },
            rect(100.0, 170.0, mouse_contained, 260.0),
            1,
            PresentationHitPrecedence::ContainedResize,
        );
        let fragments = compile(&plan, &[west]);
        let extent = requested.max(mouse_contained);
        let expansion = (extent - mouse_contained) * 0.5;
        assert_eq!(
            fragments,
            vec![(
                west.id(),
                rect(
                    100.0 - expansion,
                    100.0 - expansion + extent,
                    extent,
                    400.0 + 2.0 * expansion - 2.0 * extent,
                )
            )]
        );
    }
}

#[test]
fn touch_replacements_are_device_specific_and_empty_never_falls_back() {
    let id = SplitterSceneId {
        root: RootId::new(1),
        split: node(),
        index: 0,
    };
    let handle = region(
        PresentationHitRegionKind::SplitterHandle(id),
        rect(48.0, 0.0, 4.0, 100.0),
        0,
        PresentationHitPrecedence::SplitterHandle,
    );
    let tab = region(
        PresentationHitRegionKind::TabBody(TabSceneId {
            root: id.root,
            tabs: id.split,
            item: ItemId::new(1),
        }),
        rect(200.0, 200.0, 100.0, 20.0),
        0,
        PresentationHitPrecedence::TabBody,
    );
    for fragments in [
        vec![(handle.id(), rect(28.0, 0.0, 44.0, 100.0))],
        Vec::new(),
    ] {
        let expanded = !fragments.is_empty();
        let manifest = manifest(vec![handle, tab], fragments);
        for device in [
            PointerDeviceKind::Mouse,
            PointerDeviceKind::Pen,
            PointerDeviceKind::Unknown,
        ] {
            assert!(manifest.region_contains_for_device(handle.id(), point(50.0, 50.0), device));
            assert!(!manifest.region_contains_for_device(handle.id(), point(30.0, 50.0), device));
        }
        assert_eq!(
            manifest.region_contains_for_device(
                handle.id(),
                point(30.0, 50.0),
                PointerDeviceKind::Touch
            ),
            expanded
        );
        assert_eq!(
            manifest.region_contains_for_device(
                handle.id(),
                point(50.0, 50.0),
                PointerDeviceKind::Touch
            ),
            expanded
        );
        assert_eq!(
            manifest
                .resolve_exclusive_for_device(
                    PresentationPointerLane::Drag,
                    point(30.0, 50.0),
                    PresentationHitSuppression::default(),
                    PointerDeviceKind::Touch,
                )
                .expect("one touch receiver is unambiguous")
                .map(|region| region.id()),
            expanded.then_some(handle.id())
        );
        assert!(
            manifest
                .resolve_exclusive(PresentationPointerLane::Drag, point(30.0, 50.0))
                .expect("unknown devices retain the base footprint")
                .is_none()
        );
        assert_eq!(manifest.region(handle.id()).copied(), Some(handle));
        assert!(manifest.region_contains_for_device(
            tab.id(),
            point(210.0, 210.0),
            PointerDeviceKind::Touch,
        ));
    }
}

#[test]
fn only_final_maximum_stack_can_be_ambiguous() {
    let id = SplitterSceneId {
        root: RootId::new(1),
        split: node(),
        index: 0,
    };
    let other = SplitterSceneId { index: 1, ..id };
    let bounds = rect(0.0, 0.0, 100.0, 100.0);
    let first = region(
        PresentationHitRegionKind::SplitterHandle(id),
        bounds,
        0,
        PresentationHitPrecedence::SplitterHandle,
    );
    let second = region(
        PresentationHitRegionKind::SplitterHandle(other),
        bounds,
        0,
        PresentationHitPrecedence::SplitterHandle,
    );
    let junction = region(
        PresentationHitRegionKind::SplitterJunction(SplitterJunctionId::new(
            Some(id),
            Some(other),
            Some(id),
            Some(other),
        )),
        bounds,
        0,
        PresentationHitPrecedence::SplitterJunction,
    );
    for regions in [
        [first, second, junction],
        [junction, second, first],
        [second, junction, first],
    ] {
        assert_eq!(
            resolve_exclusive_regions(
                &regions,
                PresentationPointerLane::Drag,
                point(50.0, 50.0),
                PresentationHitSuppression::default(),
            )
            .expect("a higher junction supersedes both lower handles")
            .map(|region| region.id()),
            Some(junction.id())
        );
    }
    let manifest = manifest(
        vec![first, second, junction],
        vec![
            (first.id(), bounds),
            (second.id(), bounds),
            (junction.id(), bounds),
        ],
    );
    assert_eq!(
        manifest
            .resolve_exclusive_for_device(
                PresentationPointerLane::Drag,
                point(50.0, 50.0),
                PresentationHitSuppression::default(),
                PointerDeviceKind::Touch,
            )
            .expect("touch keeps junction precedence")
            .map(|region| region.id()),
        Some(junction.id())
    );
    let other_junction = region(
        PresentationHitRegionKind::SplitterJunction(SplitterJunctionId::new(
            Some(other),
            Some(id),
            Some(other),
            Some(id),
        )),
        bounds,
        0,
        PresentationHitPrecedence::SplitterJunction,
    );
    for regions in [[first, second], [junction, other_junction]] {
        assert!(matches!(
            resolve_exclusive_regions(
                &regions,
                PresentationPointerLane::Drag,
                point(50.0, 50.0),
                PresentationHitSuppression::default(),
            ),
            Err(PresentationHitResolutionError::Ambiguous { .. })
        ));
    }
}

#[test]
fn touch_protection_uses_visible_chrome_and_real_front_occlusion() {
    let mut plan = plan(DockPresentationConfig::default());
    let floating = FloatingPresentationId::new(1);
    plan.push_drop_occlusion(DropOcclusionRecord::new(
        floating,
        HitRegion::new(rect(100.0, 100.0, 400.0, 400.0)),
        SceneLayerKey::new(1),
    ));
    plan.push_drop_occlusion(DropOcclusionRecord::new(
        FloatingPresentationId::new(2),
        HitRegion::new(rect(75.0, 300.0, 50.0, 30.0)),
        SceneLayerKey::new(2),
    ));
    let tabs = node();
    let tab = TabSceneId {
        root: RootId::new(1),
        tabs,
        item: ItemId::new(1),
    };
    let west = region(
        PresentationHitRegionKind::ContainedResize {
            floating,
            direction: ContainedResizeDirection::West,
        },
        rect(100.0, 107.0, 7.0, 386.0),
        1,
        PresentationHitPrecedence::ContainedResize,
    );
    let background = region(
        PresentationHitRegionKind::TabBody(tab),
        rect(70.0, 200.0, 45.0, 30.0),
        0,
        PresentationHitPrecedence::TabBody,
    );
    let close = region(
        PresentationHitRegionKind::ContainedClose(floating),
        rect(110.0, 260.0, 30.0, 20.0),
        1,
        PresentationHitPrecedence::ContainedClose,
    );
    let body = region(
        PresentationHitRegionKind::PaneBody(PaneSceneId {
            root: tab.root,
            tabs,
        }),
        plan.bounds(),
        0,
        PresentationHitPrecedence::PaneBody,
    );
    let bar = TabBarSceneId {
        root: tab.root,
        tabs,
    };
    plan.push_tab_bar_record(TabBarRecord::new(
        bar,
        rect(80.0, 350.0, 60.0, 20.0),
        rect(80.0, 350.0, 60.0, 20.0),
        0.0,
        0.0,
        Vec::new(),
        Vec::new(),
        None,
        None,
        TabBarInteraction::Disabled,
        TabListMenuGeometryAvailability::Unavailable,
        SceneLayerKey::new(1),
    ));
    plan.push_presentation_menu_anchor_record(PresentationMenuAnchorRecord::ready(
        tab.root,
        PresentationMenuAnchorHost::TabBar(bar),
        rect(85.0, 400.0, 20.0, 15.0),
        SceneLayerKey::new(1),
    ));
    let mut regions = vec![west, background, close, body];
    regions.sort_unstable_by_key(|region| region.id());
    let fragments = compile(&plan, &regions);
    let manifest = manifest(regions, fragments);
    for (position, expected) in [
        (point(90.0, 210.0), false), // Visible background chrome protects the outer band.
        (point(110.0, 210.0), true), // Hidden background chrome cannot punch an interior hole.
        (point(110.0, 250.0), true), // Pane bodies do not exclude resize footprints.
        (point(110.0, 270.0), false), // Same-layer close control remains protected.
        (point(110.0, 310.0), false), // Front occlusion retains its real bounds.
        (point(110.0, 360.0), false), // Disabled strips retain their complete chrome bounds.
        (point(90.0, 410.0), false), // Menu anchors protect their visible bounds.
    ] {
        assert_eq!(
            manifest.region_contains_for_device(west.id(), position, PointerDeviceKind::Touch),
            expected,
            "{position:?}"
        );
    }
}

#[test]
fn touch_geometry_clips_after_centering_and_keeps_small_window_partitions_disjoint() {
    let clip = rect(40.0, 40.0, 520.0, 520.0);
    assert_eq!(
        splitter_band(rect(38.0, 0.0, 4.0, 600.0), Axis::Horizontal, 44.0, clip),
        Some(rect(40.0, 40.0, 22.0, 520.0))
    );
    let mut plan = plan(DockPresentationConfig::default());
    let north_south = SplitterSceneId {
        root: RootId::new(1),
        split: node(),
        index: 0,
    };
    let east_west = SplitterSceneId {
        index: 1,
        ..north_south
    };
    plan.push_splitter_record(splitter(
        north_south,
        rect(38.0, 40.0, 4.0, 520.0),
        Axis::Horizontal,
    ));
    plan.push_splitter_record(splitter(
        east_west,
        rect(40.0, 38.0, 520.0, 4.0),
        Axis::Vertical,
    ));
    assert_eq!(
        junction_square(
            &plan,
            SplitterJunctionId::new(
                Some(north_south),
                Some(east_west),
                Some(north_south),
                Some(east_west),
            ),
            44.0,
            clip,
        ),
        Some(rect(40.0, 40.0, 22.0, 22.0)),
    );
    let surface = rect(0.0, 0.0, 600.0, 600.0);
    for extent in [1e20, f64::MAX] {
        assert_eq!(
            contained_partition(
                rect(100.0, 100.0, 100.0, 100.0),
                7.0,
                extent,
                ContainedResizeDirection::NorthWest,
                surface,
            ),
            Some(rect(0.0, 0.0, 150.0, 150.0)),
            "large finite targets must retain the original source center"
        );
        assert_eq!(
            contained_partition(
                rect(100.0, 100.0, 100.0, 100.0),
                7.0,
                extent,
                ContainedResizeDirection::SouthEast,
                surface,
            ),
            Some(rect(150.0, 150.0, 450.0, 450.0)),
        );
    }
    assert_eq!(
        contained_partition(
            rect(-20.0, 20.0, 100.0, 100.0),
            7.0,
            44.0,
            ContainedResizeDirection::West,
            surface
        ),
        Some(rect(0.0, 45.5, 5.5, 49.0))
    );
    let partitions = [
        ContainedResizeDirection::NorthWest,
        ContainedResizeDirection::North,
        ContainedResizeDirection::NorthEast,
        ContainedResizeDirection::East,
        ContainedResizeDirection::SouthEast,
        ContainedResizeDirection::South,
        ContainedResizeDirection::SouthWest,
        ContainedResizeDirection::West,
    ]
    .map(|direction| {
        contained_partition(rect(100.0, 100.0, 10.0, 8.0), 7.0, 44.0, direction, surface)
    });
    assert_eq!(partitions.iter().flatten().count(), 4);
    for (index, first) in partitions.iter().flatten().enumerate() {
        for second in partitions.iter().flatten().skip(index + 1) {
            assert!(intersection(*first, *second).is_none());
        }
    }
    for position in [point(82.0, 82.0), point(105.0, 104.0), point(128.0, 126.0)] {
        assert_eq!(
            partitions
                .iter()
                .flatten()
                .filter(|rect| rect.contains(position))
                .count(),
            1
        );
    }
}
