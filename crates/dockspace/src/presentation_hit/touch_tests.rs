use super::super::*;
use super::midpoint;
use super::*;
use crate::drop_target::DropOcclusionRecord;
use crate::geometry::{Constraints, LogicalSize};
use crate::graph::{Node, RootRecord, SurfacePresentation, Workspace};
use crate::ids::{EngineAuthorityDomainId, ItemId, NodeId, WorkspaceEpoch};
use crate::layout::{LayoutMetrics, project_root};
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

#[test]
fn disjoint_and_empty_cuts_keep_the_fragment_buffer_without_rebuilding() {
    let original = rect(0.0, 0.0, 44.0, 44.0);
    let mut fragments = vec![original];
    FRAGMENT_BUFFER_CUTS.set(0);
    for index in 0..1_024 {
        subtract_from(
            &mut fragments,
            rect(44.0 + f64::from(index), 0.0, 1.0, 44.0),
        );
    }
    for empty in [rect(20.0, 20.0, 0.0, 10.0), rect(20.0, 20.0, 10.0, 0.0)] {
        subtract_from(&mut fragments, empty);
    }
    assert_eq!(fragments, [original]);
    assert_eq!(FRAGMENT_BUFFER_CUTS.get(), 0);

    subtract_from(&mut fragments, rect(20.0, 0.0, 4.0, 44.0));
    assert_eq!(FRAGMENT_BUFFER_CUTS.get(), 1);
    assert_eq!(
        fragments,
        [rect(0.0, 0.0, 20.0, 44.0), rect(24.0, 0.0, 20.0, 44.0)]
    );
    subtract_from(&mut fragments, original);
    assert!(fragments.is_empty());
    assert_eq!(FRAGMENT_BUFFER_CUTS.get(), 2);
    subtract_from(&mut fragments, original);
    assert_eq!(FRAGMENT_BUFFER_CUTS.get(), 2);
}

pub(super) fn subtract_reference(fragments: &mut Vec<LogicalRect>, cut: LogicalRect) {
    let mut remaining = Vec::new();
    for rect in fragments.drain(..) {
        let Some(overlap) = intersection(rect, cut) else {
            remaining.push(rect);
            continue;
        };
        for (left, top, right, bottom) in [
            (rect.x(), rect.y(), rect.max().x(), overlap.y()),
            (rect.x(), overlap.max().y(), rect.max().x(), rect.max().y()),
            (rect.x(), overlap.y(), overlap.x(), overlap.max().y()),
            (
                overlap.max().x(),
                overlap.y(),
                rect.max().x(),
                overlap.max().y(),
            ),
        ] {
            if let Some(fragment) = clipped_edges(left, top, right, bottom, rect) {
                remaining.push(fragment);
            }
        }
    }
    *fragments = remaining;
}

fn fragment_bits(fragments: &[LogicalRect]) -> Vec<[u64; 4]> {
    fragments
        .iter()
        .map(|rect| [rect.x(), rect.y(), rect.max().x(), rect.max().y()].map(f64::to_bits))
        .collect()
}

fn compile(
    plan: &PresentationPlan,
    regions: &[PresentationHitRegion],
) -> Vec<(PresentationHitRegionId, LogicalRect)> {
    let actual = super::compile(plan, regions);
    struct ResetReference;
    impl Drop for ResetReference {
        fn drop(&mut self) {
            REFERENCE_SUBTRACTION.set(false);
        }
    }
    REFERENCE_SUBTRACTION.set(true);
    let reset = ResetReference;
    let expected = super::compile(plan, regions);
    drop(reset);
    assert_eq!(
        actual.iter().map(|(id, _)| *id).collect::<Vec<_>>(),
        expected.iter().map(|(id, _)| *id).collect::<Vec<_>>()
    );
    let rects = |fragments: &[(PresentationHitRegionId, LogicalRect)]| {
        fragment_bits(&fragments.iter().map(|(_, rect)| *rect).collect::<Vec<_>>())
    };
    assert_eq!(rects(&actual), rects(&expected));
    actual
}

#[test]
fn retained_fragment_buffer_matches_ordered_subtraction_bitwise() {
    for (origin, extent) in [
        (0.0, 128.0),
        (-0.0, 128.0),
        (-256.5, 512.25),
        (1e100, 1e90),
        (-1e100, 1e90),
    ] {
        let original = rect(origin, origin, extent, extent);
        let cuts = [
            rect(origin + extent, origin, extent, extent),
            rect(origin, origin, 0.0, extent),
            rect(origin, origin, extent, 0.0),
            rect(
                origin + extent * 0.3,
                origin + extent * 0.3,
                extent * 0.4,
                extent * 0.4,
            ),
            rect(
                origin + extent * 0.2,
                origin - extent,
                extent * 0.1,
                extent * 3.0,
            ),
            rect(
                origin - extent,
                origin + extent * 0.5,
                extent * 3.0,
                extent * 0.1,
            ),
            rect(
                origin + extent * 0.3,
                origin + extent * 0.3,
                extent * 0.4,
                extent * 0.4,
            ),
            original,
        ];
        let mut actual = vec![original];
        let mut expected = actual.clone();
        for cut in cuts {
            subtract_from(&mut actual, cut);
            subtract_reference(&mut expected, cut);
            assert_eq!(fragment_bits(&actual), fragment_bits(&expected));
        }
    }

    let initial = (0..32)
        .map(|index| {
            rect(
                f64::from(index) * 9.0,
                f64::from(index % 7) * 11.0,
                24.0,
                19.0,
            )
        })
        .collect::<Vec<_>>();
    let mut actual = initial.clone();
    let mut expected = initial;
    let mut random = 0x6b43_84b7_3251_cfe9_u64;
    for _ in 0..384 {
        random ^= random << 13;
        random ^= random >> 7;
        random ^= random << 17;
        let x = f64::from((random & 0xff) as u8) - 8.0;
        let y = f64::from(((random >> 8) & 0x7f) as u8) - 8.0;
        let width = f64::from(((random >> 16) & 0x1f) as u8);
        let height = f64::from(((random >> 24) & 0x1f) as u8);
        let cut = rect(x, y, width, height);
        subtract_from(&mut actual, cut);
        subtract_reference(&mut expected, cut);
        assert_eq!(fragment_bits(&actual), fragment_bits(&expected));
    }
}

#[test]
fn sparse_touch_partition_matches_exhaustive_partition_with_overlapping_bases() {
    let split = node();
    for variant in 0..3 {
        let mut allocations = Vec::new();
        let mut protected = Vec::new();
        for index in 0..144 {
            let x = 40.0 + f64::from(index % 12) * 40.0;
            let y = 40.0 + f64::from(index / 12) * 40.0;
            let base = match (index + variant) % 3 {
                0 => rect(x, y, 70.0, 6.0),
                1 => rect(x, y, 6.0, 70.0),
                _ => rect(x, y + 0.5, 6.0, 6.0),
            };
            let id = SplitterSceneId {
                root: RootId::new(1),
                split,
                index: usize::try_from(index).unwrap(),
            };
            let layer = u64::try_from(index % 2).unwrap();
            let region = region(
                PresentationHitRegionKind::SplitterHandle(id),
                base,
                layer,
                PresentationHitPrecedence::SplitterHandle,
            );
            let expanded = rect(
                base.x() - 19.0,
                base.y() - 19.0,
                base.width() + 38.0,
                base.height() + 38.0,
            );
            allocations.push(TouchAllocation {
                region,
                anchor: base,
                expanded,
                fragments: vec![expanded],
            });
            protected.push((SceneLayerKey::new(layer), rect(x, y + 8.0, 30.0, 20.0)));
        }
        let neighbors = TouchNeighbors::build(&allocations, &protected);
        assert!(matches!(&neighbors, TouchNeighbors::Sparse(_)));
        // A full scan is the reference oracle for both the partition and every
        // chrome cut that the broad phase must retain.
        for (index, allocation) in allocations.iter().enumerate() {
            for (protected_index, &(layer, bounds)) in protected.iter().enumerate() {
                if layer <= allocation.region.stack().layer()
                    && intersection(allocation.expanded, bounds).is_some()
                {
                    assert!(
                        neighbors
                            .for_region(index)
                            .binary_search(&(allocations.len() + protected_index))
                            .is_ok()
                    );
                }
            }
        }
        let mut exhaustive = allocations.clone();
        let dense = TouchNeighbors::Dense((0..allocations.len() + protected.len()).collect());
        partition_allocations(&mut allocations, &neighbors);
        partition_allocations(&mut exhaustive, &dense);
        for (sparse, dense) in allocations.iter().zip(&exhaustive) {
            assert_eq!(sparse.fragments, dense.fragments);
        }
    }
}

#[test]
fn dense_touch_overlap_uses_the_bounded_exhaustive_fallback() {
    let split = node();
    let bounds = rect(0.0, 0.0, 600.0, 600.0);
    let allocations = (0..100)
        .map(|index| TouchAllocation {
            region: region(
                PresentationHitRegionKind::SplitterHandle(SplitterSceneId {
                    root: RootId::new(1),
                    split,
                    index,
                }),
                bounds,
                0,
                PresentationHitPrecedence::SplitterHandle,
            ),
            anchor: bounds,
            expanded: bounds,
            fragments: vec![bounds],
        })
        .collect::<Vec<_>>();
    let neighbors = TouchNeighbors::build(&allocations, &[]);
    assert!(matches!(&neighbors, TouchNeighbors::Dense(all) if all.len() == allocations.len()));
}

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
    let root = RootId::new(1);
    let root_bounds = rect(40.0, 40.0, 520.0, 520.0);
    let mut builder = Workspace::builder();
    let leaf = builder.insert_node(Node::tabs([ItemId::new(1)]));
    builder.set_root(root, RootRecord::new(leaf));
    builder.set_surface(SurfaceId::new(1), SurfacePresentation::with_main(root));
    let workspace = builder.build().expect("single-root fixture is valid");
    let constraints = Constraints::new(
        LogicalSize::new(0.0, 0.0).expect("zero minimum is valid"),
        root_bounds.size(),
    )
    .expect("root constraints are valid");
    let projection = project_root(
        &workspace,
        root,
        root_bounds,
        &[(leaf, constraints)].into_iter().collect(),
        LayoutMetrics::new(config.splitter_thickness()).expect("splitter metric is valid"),
    )
    .expect("root projection is valid");
    let mut plan = PresentationPlan::new(SurfaceId::new(1), bounds);
    plan.install_layout_facts(config, measurements);
    plan.push_root_layout_facts(RootLayoutFacts::new(
        root,
        root_bounds,
        Default::default(),
        std::sync::Arc::new(projection),
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
    let touch_regions = touch_regions
        .into_iter()
        .map(|(id, bounds)| {
            (
                regions
                    .binary_search_by_key(&id, |region| region.id())
                    .unwrap(),
                bounds,
            )
        })
        .collect::<Vec<_>>()
        .into_boxed_slice();
    PresentationHitManifest {
        output: SurfacePresentationOutputTicket::mint(
            ticket().authority_domain(),
            PresentationOutputSerial::new_for_test(1),
            SurfaceSceneStamp::new(ticket(), SurfaceSceneRevision::new(1)),
        ),
        regions: regions.into_boxed_slice(),
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
        let splitter_manifest = manifest(vec![handle, junction], fragments);
        for (position, expected) in [
            (point(300.0 - extent * 0.5, 100.0), handle.id()),
            (point(300.0, 300.0 - extent * 0.5 - 1.0), handle.id()),
            (point(300.0, 290.0), handle.id()),
            (point(300.0, 300.0), junction.id()),
            (
                point(300.0 + extent * 0.5 - 1.0, 300.0 + extent * 0.5 - 1.0),
                junction.id(),
            ),
        ] {
            assert_eq!(
                splitter_manifest
                    .resolve_exclusive_for_device(
                        PresentationPointerLane::Drag,
                        position,
                        PresentationHitSuppression::default(),
                        PointerDeviceKind::Touch,
                    )
                    .expect("touch allocations must resolve uniquely")
                    .map(|region| region.id()),
                Some(expected),
                "{position:?}: expansion preserves original handles and junction precedence"
            );
        }

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
        let expanded_band = rect(
            100.0 - expansion,
            100.0 - expansion + extent,
            extent,
            400.0 + 2.0 * expansion - 2.0 * extent,
        );
        assert!(fragments.iter().all(|(id, _)| *id == west.id()));
        let manifest = manifest(vec![west], fragments);
        for covered in [west.hit().rect(), expanded_band] {
            for x_fraction in [0.0, 0.25, 0.5, 0.75] {
                for y_fraction in [0.0, 0.25, 0.5, 0.75] {
                    let position = point(
                        covered.x() + covered.width() * x_fraction,
                        covered.y() + covered.height() * y_fraction,
                    );
                    assert_eq!(
                        manifest
                            .resolve_exclusive_for_device(
                                PresentationPointerLane::Drag,
                                position,
                                PresentationHitSuppression::default(),
                                PointerDeviceKind::Touch,
                            )
                            .expect("the original and expanded bands have one touch owner")
                            .map(|region| region.id()),
                        Some(west.id()),
                        "{position:?}: touch preserves the complete original mouse coverage"
                    );
                }
            }
        }
        assert_eq!(
            manifest
                .resolve_exclusive_for_device(
                    PresentationPointerLane::Drag,
                    point(expanded_band.x() - 1.0, 300.0),
                    PresentationHitSuppression::default(),
                    PointerDeviceKind::Touch,
                )
                .expect("outside the allocated band remains unambiguous"),
            None
        );
    }
}

#[test]
fn parallel_touch_bands_partition_geometrically_even_with_extreme_extents() {
    for extent in [256.0, f64::MAX] {
        for axis in [Axis::Horizontal, Axis::Vertical] {
            for second_at in [205.0, 398.0] {
                let config = DockPresentationConfig::builder()
                    .touch_resize_hit_extent(extent)
                    .build()
                    .unwrap();
                let mut plan = plan(config);
                let first = SplitterSceneId {
                    root: RootId::new(1),
                    split: node(),
                    index: 0,
                };
                let second = SplitterSceneId { index: 1, ..first };
                let draw = |at| match axis {
                    Axis::Horizontal => rect(at, 40.0, 4.0, 520.0),
                    Axis::Vertical => rect(40.0, at, 520.0, 4.0),
                };
                let regions = [(first, 198.0), (second, second_at)].map(|(id, at)| {
                    let bounds = draw(at);
                    plan.push_splitter_record(splitter(id, bounds, axis));
                    region(
                        PresentationHitRegionKind::SplitterHandle(id),
                        bounds,
                        0,
                        PresentationHitPrecedence::SplitterHandle,
                    )
                });
                let fragments = compile(&plan, &regions);
                for (index, (first_id, first_rect)) in fragments.iter().enumerate() {
                    for (second_id, second_rect) in fragments.iter().skip(index + 1) {
                        if first_id != second_id {
                            assert!(intersection(*first_rect, *second_rect).is_none());
                        }
                    }
                }
                let manifest = manifest(regions.to_vec(), fragments);
                let boundary = midpoint(202.0, second_at);
                for (at, expected) in [
                    (198.0, first),
                    (201.9, first),
                    (boundary - 0.1, first),
                    (boundary, second),
                    (second_at, second),
                    (second_at + 3.9, second),
                ] {
                    let position = match axis {
                        Axis::Horizontal => point(at, 100.0),
                        Axis::Vertical => point(100.0, at),
                    };
                    assert_eq!(
                        manifest
                            .resolve_exclusive_for_device(
                                PresentationPointerLane::Drag,
                                position,
                                PresentationHitSuppression::default(),
                                PointerDeviceKind::Touch,
                            )
                            .expect("geometric cells must never require an arbitrary tie break")
                            .map(|region| region.id().kind()),
                        Some(PresentationHitRegionKind::SplitterHandle(expected)),
                        "{axis:?}, {extent}, {position:?}"
                    );
                }
            }
        }
    }
}

#[test]
fn adjacent_junction_halos_preserve_visible_arms_and_each_junction() {
    let config = DockPresentationConfig::builder()
        .touch_resize_hit_extent(256.0)
        .build()
        .unwrap();
    let mut plan = plan(config);
    let vertical = SplitterSceneId {
        root: RootId::new(1),
        split: node(),
        index: 0,
    };
    let upper = SplitterSceneId {
        index: 1,
        ..vertical
    };
    let lower = SplitterSceneId {
        index: 2,
        ..vertical
    };
    let mut regions = Vec::new();
    for (id, bounds, axis) in [
        (vertical, rect(298.0, 40.0, 4.0, 520.0), Axis::Horizontal),
        (upper, rect(40.0, 238.0, 258.0, 4.0), Axis::Vertical),
        (lower, rect(40.0, 358.0, 258.0, 4.0), Axis::Vertical),
    ] {
        plan.push_splitter_record(splitter(id, bounds, axis));
        regions.push(region(
            PresentationHitRegionKind::SplitterHandle(id),
            bounds,
            0,
            PresentationHitPrecedence::SplitterHandle,
        ));
    }
    let mut junctions = Vec::new();
    for (horizontal, y) in [(upper, 238.0), (lower, 358.0)] {
        let id = SplitterJunctionId::new(Some(vertical), Some(horizontal), Some(vertical), None);
        let junction = region(
            PresentationHitRegionKind::SplitterJunction(id),
            rect(298.0, y, 4.0, 4.0),
            0,
            PresentationHitPrecedence::SplitterJunction,
        );
        regions.push(junction);
        junctions.push(junction.id());
    }
    regions.sort_unstable_by_key(|region| region.id());
    let fragments = compile(&plan, &regions);
    let manifest = manifest(regions, fragments);
    for position in [
        point(300.0, 100.0),
        point(300.0, 240.0),
        point(300.0, 280.0),
        point(300.0, 300.0),
        point(300.0, 360.0),
        point(300.0, 500.0),
        point(200.0, 240.0),
        point(200.0, 360.0),
    ] {
        let winner = |device| {
            manifest
                .resolve_exclusive_for_device(
                    PresentationPointerLane::Drag,
                    position,
                    PresentationHitSuppression::default(),
                    device,
                )
                .expect("visible arms and junctions must stay unambiguous")
                .map(|region| region.id())
        };
        assert_eq!(
            winner(PointerDeviceKind::Touch),
            winner(PointerDeviceKind::Mouse)
        );
        assert!(winner(PointerDeviceKind::Mouse).is_some());
    }
    for (position, expected) in [
        (point(320.0, 260.0), junctions[0]),
        (point(320.0, 340.0), junctions[1]),
    ] {
        assert_eq!(
            manifest
                .resolve_exclusive_for_device(
                    PresentationPointerLane::Drag,
                    position,
                    PresentationHitSuppression::default(),
                    PointerDeviceKind::Touch,
                )
                .expect("adjacent junction expansions must have separate cells")
                .map(|region| region.id()),
            Some(expected),
        );
    }
}

#[test]
fn extreme_touch_corners_do_not_erase_original_contained_edges() {
    let config = DockPresentationConfig::builder()
        .touch_resize_hit_extent(f64::MAX)
        .build()
        .unwrap();
    let mut plan = plan(config);
    let floating = FloatingPresentationId::new(1);
    plan.push_drop_occlusion(DropOcclusionRecord::new(
        floating,
        HitRegion::new(rect(100.0, 100.0, 400.0, 400.0)),
        SceneLayerKey::new(1),
    ));
    let mut regions = [
        (
            ContainedResizeDirection::NorthWest,
            rect(100.0, 100.0, 7.0, 7.0),
        ),
        (
            ContainedResizeDirection::North,
            rect(107.0, 100.0, 386.0, 7.0),
        ),
        (
            ContainedResizeDirection::NorthEast,
            rect(493.0, 100.0, 7.0, 7.0),
        ),
        (
            ContainedResizeDirection::East,
            rect(493.0, 107.0, 7.0, 386.0),
        ),
        (
            ContainedResizeDirection::SouthEast,
            rect(493.0, 493.0, 7.0, 7.0),
        ),
        (
            ContainedResizeDirection::South,
            rect(107.0, 493.0, 386.0, 7.0),
        ),
        (
            ContainedResizeDirection::SouthWest,
            rect(100.0, 493.0, 7.0, 7.0),
        ),
        (
            ContainedResizeDirection::West,
            rect(100.0, 107.0, 7.0, 386.0),
        ),
    ]
    .map(|(direction, bounds)| {
        region(
            PresentationHitRegionKind::ContainedResize {
                floating,
                direction,
            },
            bounds,
            1,
            PresentationHitPrecedence::ContainedResize,
        )
    });
    regions.sort_unstable_by_key(|region| region.id());
    let fragments = compile(&plan, &regions);
    let manifest = manifest(regions.to_vec(), fragments);
    for original in regions {
        let bounds = original.hit().rect();
        let center = point(
            midpoint(bounds.x(), bounds.max().x()),
            midpoint(bounds.y(), bounds.max().y()),
        );
        assert_eq!(
            manifest
                .resolve_exclusive_for_device(
                    PresentationPointerLane::Drag,
                    center,
                    PresentationHitSuppression::default(),
                    PointerDeviceKind::Touch,
                )
                .expect("a large corner must not erase another original handle")
                .map(|region| region.id()),
            Some(original.id()),
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
    let tab = TabSceneId {
        root: id.root,
        tabs: id.split,
        item: ItemId::new(1),
    };
    let first = region(
        PresentationHitRegionKind::TabBody(tab),
        bounds,
        0,
        PresentationHitPrecedence::TabBody,
    );
    let second = region(
        PresentationHitRegionKind::TabBody(TabSceneId {
            item: ItemId::new(2),
            ..tab
        }),
        bounds,
        0,
        PresentationHitPrecedence::TabBody,
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
    for regions in [[first, second], [second, first]] {
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
    for regions in [[junction, other_junction], [other_junction, junction]] {
        assert_eq!(
            resolve_exclusive_regions(
                &regions,
                PresentationPointerLane::Drag,
                point(50.0, 50.0),
                PresentationHitSuppression::default(),
            )
            .unwrap()
            .map(|region| region.id()),
            Some(junction.id().min(other_junction.id())),
        );
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
