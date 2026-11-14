use super::{HitGeometry, canonicalize_regions, canonicalize_reusing_order};
use crate::geometry::{LogicalPoint, LogicalRect};
use crate::ids::{FloatingPresentationId, ItemId, NodeId, RootId, SurfaceId};
use crate::interaction::targeting::guide::DropGuideClusterId;
use crate::interaction::targeting::targets::DropTargetId;
use crate::presentation::hit::region::HitRegion;
use crate::presentation::hit::{
    PaneSceneId, PresentationHitBehavior, PresentationHitPrecedence, PresentationHitRegion,
    PresentationHitRegionId, PresentationHitRegionKind, PresentationHitStackKey, PresentationPlane,
    PresentationPointerLanes, SplitterJunctionId, SplitterSceneId, TabBarSceneId,
    TabGroupDragRegionKind, TabSceneId,
};
use crate::presentation::scene::ContainedResizeDirection;
use crate::presentation::tab_strip::{TabListMenuSessionId, TabStripControlId, TabStripStateKey};

fn geometry(marker: usize) -> LogicalRect {
    let corners = match marker % 4 {
        0 => [-0.0, -0.0, 1.0, 2.0],
        1 => [1e100, -1e100, 1e100 + 1e90, -1e100 + 1e90],
        2 => [-256.5, 13.75, -200.25, 17.0],
        _ => [-1e-200, 0.0, 1e-200, 1e-200],
    };
    LogicalRect::from_min_max(
        LogicalPoint::new(corners[0], corners[1]).unwrap(),
        LogicalPoint::new(corners[2], corners[3]).unwrap(),
    )
    .unwrap()
}

fn record(
    surface: SurfaceId,
    kind: PresentationHitRegionKind,
    marker: usize,
) -> PresentationHitRegion {
    PresentationHitRegion::new(
        PresentationHitRegionId::new(surface, kind),
        HitRegion::new(geometry(marker)),
        if marker.is_multiple_of(2) {
            PresentationPointerLanes::ALL
        } else {
            PresentationPointerLanes::CLICK_AND_DRAG
        },
        PresentationHitStackKey::new(
            PresentationPlane::Workspace,
            crate::interaction::targeting::targets::SceneLayerKey::new(
                u64::try_from(marker).unwrap(),
            ),
            PresentationHitPrecedence::PaneBody,
        ),
        if marker.is_multiple_of(3) {
            PresentationHitBehavior::PassiveAffordance
        } else {
            PresentationHitBehavior::ExclusiveReceiver
        },
    )
    .with_anchor(geometry(marker + 1))
}

fn pane(marker: usize) -> PresentationHitRegion {
    record(
        SurfaceId::new(1),
        PresentationHitRegionKind::PaneBody(PaneSceneId {
            root: RootId::new(u64::try_from(marker + 1).unwrap()),
            tabs: NodeId::default(),
        }),
        marker,
    )
}

fn endpoint_bits(rect: LogicalRect) -> [u64; 4] {
    [rect.x(), rect.y(), rect.max().x(), rect.max().y()].map(f64::to_bits)
}

fn assert_matches_record_sort(mut actual: Vec<PresentationHitRegion>) {
    let emitted = actual.clone();
    let mut expected = actual.clone();
    expected.sort_unstable_by_key(|region| region.id());
    let emission_rows = canonicalize_regions(&mut actual);
    for (emitted, &row) in emitted.iter().zip(&emission_rows) {
        assert_eq!(*emitted, actual[row]);
        assert_eq!(
            endpoint_bits(emitted.hit.rect()),
            endpoint_bits(actual[row].hit.rect())
        );
        assert_eq!(
            endpoint_bits(emitted.anchor),
            endpoint_bits(actual[row].anchor)
        );
    }
    assert_eq!(actual, expected);
    for (actual, expected) in actual.iter().zip(&expected) {
        assert_eq!(
            endpoint_bits(actual.hit.rect()),
            endpoint_bits(expected.hit.rect())
        );
        assert_eq!(endpoint_bits(actual.anchor), endpoint_bits(expected.anchor));
    }
}

#[test]
fn empty_singleton_and_sorted_records_keep_all_fields() {
    assert_matches_record_sort(Vec::new());
    assert_matches_record_sort(vec![pane(0)]);
    assert_matches_record_sort((0..32).map(pane).collect());
}

#[test]
fn new_to_old_three_cycle_uses_the_correct_permutation_direction() {
    // Sorted rows must take originals [2, 0, 1], not its inverse [1, 2, 0].
    assert_matches_record_sort(vec![pane(1), pane(2), pane(0)]);
}

#[test]
fn reversed_rotated_and_disjoint_cycles_match_record_sort() {
    for count in [2, 3, 7, 32, 129, 1_024] {
        assert_matches_record_sort((0..count).rev().map(pane).collect());
        let mut rotated = (0..count).map(pane).collect::<Vec<_>>();
        rotated.rotate_left(count / 2);
        assert_matches_record_sort(rotated);
        let mut disjoint = (0..count).map(pane).collect::<Vec<_>>();
        for chunk in disjoint.chunks_mut(3) {
            chunk.rotate_left(1);
        }
        assert_matches_record_sort(disjoint);
    }
}

#[test]
fn mixed_full_identities_and_junction_arms_match_record_sort_bitwise() {
    let surface = SurfaceId::new(1);
    let root = RootId::new(2);
    let tabs = NodeId::default();
    let bar = TabBarSceneId { root, tabs };
    let tab = TabSceneId {
        root,
        tabs,
        item: ItemId::new(3),
    };
    let menu = TabListMenuSessionId::new_for_test(TabStripStateKey::new(surface, bar), 1);
    let floating = FloatingPresentationId::new(4);
    let splitter = |index| SplitterSceneId {
        root,
        split: tabs,
        index,
    };
    let kinds = [
        PresentationHitRegionKind::DropTarget(DropTargetId::SurfaceBackground { surface }),
        PresentationHitRegionKind::SplitterJunction(SplitterJunctionId::new(
            Some(splitter(0)),
            Some(splitter(2)),
            Some(splitter(0)),
            Some(splitter(3)),
        )),
        PresentationHitRegionKind::TabClose(tab),
        PresentationHitRegionKind::ContainedTitle(floating),
        PresentationHitRegionKind::TabListMenuRow { menu, tab },
        PresentationHitRegionKind::DropGuideActivation(DropGuideClusterId::inner(
            surface, root, tabs,
        )),
        PresentationHitRegionKind::TabStripControl(TabStripControlId::TabListMenu(bar)),
        PresentationHitRegionKind::SplitterHandle(splitter(0)),
        PresentationHitRegionKind::TabListMenuBackdrop(menu),
        PresentationHitRegionKind::PaneBody(PaneSceneId { root, tabs }),
        PresentationHitRegionKind::SplitterJunction(SplitterJunctionId::new(
            Some(splitter(0)),
            Some(splitter(1)),
            Some(splitter(0)),
            Some(splitter(3)),
        )),
        PresentationHitRegionKind::TabStripScroll(bar),
        PresentationHitRegionKind::ContainedResize {
            floating,
            direction: ContainedResizeDirection::East,
        },
        PresentationHitRegionKind::TabGroupDrag {
            bar,
            region: TabGroupDragRegionKind::LeadingGrip,
        },
        PresentationHitRegionKind::TabBody(tab),
        PresentationHitRegionKind::ContainedFrameBlocker(floating),
        PresentationHitRegionKind::TabListMenuBlocker(menu),
        PresentationHitRegionKind::ContainedClose(floating),
        PresentationHitRegionKind::TabListMenuScroll(menu),
    ];
    let mut regions = kinds
        .into_iter()
        .enumerate()
        .map(|(marker, kind)| record(surface, kind, marker))
        .collect::<Vec<_>>();
    regions.push(record(
        SurfaceId::new(0),
        PresentationHitRegionKind::TabBody(tab),
        20,
    ));
    assert_matches_record_sort(regions);
}

#[test]
fn exact_identity_replay_reuses_order_while_preserving_changed_geometry_bits() {
    let mut previous_regions = (0..129).map(pane).collect::<Vec<_>>();
    previous_regions.rotate_left(7);
    let emitted = previous_regions.clone();
    let emission_rows = canonicalize_regions(&mut previous_regions);
    let mut previous = HitGeometry {
        touch_order: Box::new([]),
        regions: previous_regions.into_boxed_slice(),
        touch_regions: Box::new([]),
        emission_rows: Some(emission_rows),
    };
    for variant in 0..5 {
        let mut actual = emitted.clone();
        for (index, region) in actual.iter_mut().enumerate() {
            region.hit = HitRegion::new(geometry(index + 1));
            region.anchor = geometry(index + 2);
        }
        match variant {
            0 => {}
            1 => actual[7].id = pane(500).id,
            2 => {
                actual.pop();
            }
            3 => actual.reverse(),
            _ => previous.emission_rows = None,
        }
        let mut expected = actual.clone();
        let expected_rows = canonicalize_regions(&mut expected);
        let rows = canonicalize_reusing_order(&mut actual, Some(&previous));
        assert_eq!(rows, expected_rows);
        assert_eq!(actual, expected);
        for (actual, expected) in actual.iter().zip(&expected) {
            assert_eq!(
                endpoint_bits(actual.hit.rect()),
                endpoint_bits(expected.hit.rect())
            );
            assert_eq!(endpoint_bits(actual.anchor), endpoint_bits(expected.anchor));
        }
    }
}
