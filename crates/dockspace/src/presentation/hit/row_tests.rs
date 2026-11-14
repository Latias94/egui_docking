use super::*;

fn rect(x: f64, width: f64) -> LogicalRect {
    LogicalRect::new(x, 0.0, width, 20.0).unwrap()
}

fn region(index: u32, width: f64) -> PresentationHitRegion {
    PresentationHitRegion::new(
        PresentationHitRegionId::new(
            SurfaceId::new(1),
            PresentationHitRegionKind::ContainedResize {
                floating: FloatingPresentationId::new(u64::from(index)),
                direction: ContainedResizeDirection::East,
            },
        ),
        HitRegion::new(rect(f64::from(index), width)),
        PresentationPointerLanes::CLICK_AND_DRAG,
        PresentationHitStackKey::new(
            PresentationPlane::Workspace,
            SceneLayerKey::surface_base(),
            PresentationHitPrecedence::ContainedResize,
        ),
        PresentationHitBehavior::ExclusiveReceiver,
    )
}

fn id_fragments(
    regions: &[PresentationHitRegion],
    fragments: &[(usize, LogicalRect)],
) -> Vec<(PresentationHitRegionId, [u64; 4])> {
    fragments
        .iter()
        .map(|(row, rect)| {
            (
                regions[*row].id(),
                [rect.x(), rect.y(), rect.max().x(), rect.max().y()].map(f64::to_bits),
            )
        })
        .collect()
}

#[test]
fn filtering_empty_regions_keeps_touch_only_rows_and_fragment_identity_order() {
    let original = vec![
        region(0, 0.0),
        region(1, 10.0),
        region(2, 0.0),
        region(3, 0.0),
        region(4, 10.0),
        region(5, 0.0),
        region(6, 0.0),
    ];
    let mut fragments = vec![
        (2, rect(-0.0, 4.0)),
        (2, rect(12.0, 8.0)),
        (4, rect(30.0, 6.0)),
    ];
    let expected = id_fragments(&original, &fragments);
    let mut regions = original.clone();
    compact_region_rows(&mut regions, &mut fragments).unwrap();

    assert_eq!(
        regions.iter().map(|region| region.id()).collect::<Vec<_>>(),
        [original[1].id(), original[2].id(), original[4].id()]
    );
    assert_eq!(
        fragments.iter().map(|(row, _)| *row).collect::<Vec<_>>(),
        [1, 1, 2]
    );
    assert_eq!(id_fragments(&regions, &fragments), expected);
    assert_eq!(regions[1].hit().rect().width(), 0.0);
}

#[test]
fn empty_touch_replacements_do_not_preserve_empty_mouse_regions() {
    let mut regions = vec![region(0, 0.0), region(1, 10.0), region(2, 0.0)];
    let expected = regions[1];
    compact_region_rows(&mut regions, &mut []).unwrap();
    assert_eq!(regions, [expected]);
    compact_region_rows(&mut Vec::new(), &mut []).unwrap();
}

#[test]
fn out_of_range_or_reversed_touch_rows_cannot_finish_a_manifest() {
    for mut fragments in [
        vec![(3, rect(0.0, 1.0))],
        vec![(1, rect(0.0, 1.0)), (0, rect(2.0, 1.0))],
    ] {
        let mut regions = vec![region(0, 0.0), region(1, 10.0), region(2, 0.0)];
        assert_eq!(compact_region_rows(&mut regions, &mut fragments), Err(()));
    }
}
