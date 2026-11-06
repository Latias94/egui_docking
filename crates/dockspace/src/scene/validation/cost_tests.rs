use super::*;
use crate::engine::{DockEngine, PreparedSurfacePaintCandidate};
use crate::geometry::LogicalSize;
use crate::graph::{Axis, SplitWeight};
use crate::hit_region::HitRegion;
use crate::model::{
    DockspaceAxis, DockspaceLayout, DockspaceNode, DockspaceRootLayout, DockspaceSurfaceLayout,
};
use crate::policy::DockPolicy;
use crate::scene_manifest::{Measurement, SurfaceMeasurements, TabIntrinsic, TabStripMetrics};

const SURFACE: SurfaceId = SurfaceId::new(1);
const ROOT: RootId = RootId::new(1);

fn compiled_split(children: u16) -> (DockEngine, PresentationPlan) {
    compiled_split_with_tabs(children, 1)
}

fn compiled_split_with_tabs(children: u16, tabs: u16) -> (DockEngine, PresentationPlan) {
    let content = DockspaceNode::equal_split(
        DockspaceAxis::Horizontal,
        (0..children).map(|pane| {
            DockspaceNode::tabs(
                (1..=tabs)
                    .map(|tab| ItemId::new(u64::from(pane) * u64::from(tabs) + u64::from(tab))),
            )
        }),
    )
    .unwrap();
    let layout = DockspaceLayout::new([DockspaceSurfaceLayout::new(
        SURFACE,
        DockspaceRootLayout::new(ROOT, content),
    )])
    .unwrap();
    let engine = DockEngine::from_layout_with_presentation_config(
        layout,
        DockPolicy::default(),
        DockPresentationConfig::default(),
    )
    .unwrap();
    let requirements = engine.presentation_requirements().surface(SURFACE).unwrap();
    let mut measurements = SurfaceMeasurements::new(requirements.ticket());
    measurements
        .set_bounds(
            requirements.bounds(),
            Measurement::Measured(
                LogicalRect::new(
                    0.0,
                    0.0,
                    f64::from(children) * f64::from(tabs) * 128.0,
                    600.0,
                )
                .unwrap(),
            ),
        )
        .unwrap();
    for key in requirements.pane_minimums() {
        measurements
            .insert_pane_minimum(
                key,
                Measurement::Measured(LogicalSize::new(0.0, 0.0).unwrap()),
            )
            .unwrap();
    }
    for key in requirements.tab_intrinsics() {
        measurements
            .insert_tab_intrinsic(key, Measurement::Measured(TabIntrinsic::new(24.0).unwrap()))
            .unwrap();
    }
    for key in requirements.tab_strips() {
        measurements
            .insert_tab_strip(
                key,
                Measurement::Measured(TabStripMetrics::new(0.0, 0.0).unwrap()),
            )
            .unwrap();
    }
    let contribution = engine
        .prepare_surface_contribution(
            engine.begin_surface_contribution(SURFACE).unwrap(),
            measurements,
        )
        .unwrap();
    let PreparedSurfacePaintCandidate::Ready(candidate) = contribution.paint_candidate() else {
        panic!("complete measurements must produce a ready split");
    };
    let plan = candidate.plan().clone();
    (engine, plan)
}

#[test]
fn a_wide_split_validates_shared_resize_arrays_once() {
    for children in [4, 128] {
        SPLITTER_RESIZE_ARRAY_VALIDATIONS.set(0);
        let (_, plan) = compiled_split(children);
        assert_eq!(plan.splitter_records().len(), usize::from(children) - 1);
        assert_eq!(SPLITTER_RESIZE_ARRAY_VALIDATIONS.get(), 1);
    }
}

fn with_resize_arrays(
    record: &SplitterRecord,
    axis: Axis,
    extents: Arc<[f64]>,
    weights: Arc<[SplitWeight]>,
) -> SplitterRecord {
    SplitterRecord::new(
        *record.id(),
        record.draw_bounds(),
        HitRegion::new(record.hit().rect()),
        axis,
        record.before_bounds(),
        record.after_bounds(),
        record.before_minimum_extent(),
        record.after_minimum_extent(),
        record.before_maximum_extent(),
        record.after_maximum_extent(),
        extents,
        record.central_index(),
        weights,
        record.layer(),
        record.operable(),
    )
}

#[test]
fn shared_resize_proof_does_not_hide_a_later_gap_or_foreign_array_defect() {
    let (engine, original) = compiled_split(4);
    let weights = Arc::<[SplitWeight]>::from(original.splitter_records()[0].weights());
    let extents = Arc::<[f64]>::from(original.splitter_records()[0].child_extents());
    let last = original.splitter_records().len() - 1;
    let rejected_id = *original.splitter_records()[last].id();

    for defect in 0..3 {
        let mut plan = original.clone();
        for (index, record) in plan.splitter_records.iter_mut().enumerate() {
            let mut axis = record.axis();
            let mut gap_weights = Arc::clone(&weights);
            let mut gap_extents = Arc::clone(&extents);
            if index == last {
                match defect {
                    0 => axis = Axis::Vertical,
                    1 => {
                        gap_weights = vec![SplitWeight::new(1.0).unwrap(); 4].into();
                    }
                    _ => {
                        let mut invalid = extents.to_vec();
                        // This value is outside the last gap's adjacent pair.
                        invalid[0] = f64::NAN;
                        gap_extents = invalid.into();
                    }
                }
            }
            *record = with_resize_arrays(record, axis, gap_extents, gap_weights);
        }
        SPLITTER_RESIZE_ARRAY_VALIDATIONS.set(0);
        let result = PresentationPlanValidator::for_manifest_surface(
            engine.workspace(),
            engine.version(),
            engine.presentation_requirements(),
            engine.policy_snapshot(),
            SURFACE,
        )
        .unwrap()
        .validate_and_canonicalize(plan);
        assert!(matches!(
            result,
            Err(SceneBuildError::InvalidSplitterRecord { id, .. }) if id == rejected_id
        ));
        assert_eq!(
            SPLITTER_RESIZE_ARRAY_VALIDATIONS.get(),
            if defect == 0 { 1 } else { 2 }
        );
    }
    assert!(original.splitter_records()[0].child_extents()[0].is_finite());
    assert_eq!(original.splitter_records()[last].axis(), Axis::Horizontal);
}

#[test]
fn duplicate_validation_keeps_the_smallest_identity_and_record_class_priority() {
    let (_, original) = compiled_split(4);
    let low = original.pane_records()[0];
    let high = *original.pane_records().last().unwrap();
    let expected = low.id().min(high.id());
    let bounds = original.bounds();
    let mut plan = PresentationPlan::new(SURFACE, bounds);
    plan.pane_records = vec![high, high, low, low];
    let minimum = ContainedMinimumMeasurement::new(
        FloatingPresentationId::new(0),
        LogicalSize::new(0.0, 0.0).unwrap(),
    );
    plan.contained_minimums = vec![minimum, minimum];

    for _ in 0..2 {
        assert!(matches!(
            validate_semantic_uniqueness(&plan),
            Err(SceneBuildError::DuplicatePane { id }) if id == expected
        ));
        plan.pane_records.reverse();
    }
}

fn revalidate(
    engine: &DockEngine,
    plan: PresentationPlan,
) -> Result<PresentationPlan, SceneBuildError> {
    PresentationPlanValidator::for_manifest_surface(
        engine.workspace(),
        engine.version(),
        engine.presentation_requirements(),
        engine.policy_snapshot(),
        SURFACE,
    )
    .unwrap()
    .validate_and_canonicalize(plan)
}

#[test]
fn semantic_row_orders_restore_all_records_and_linked_slots_after_permutation() {
    let (engine, original) = compiled_split_with_tabs(4, 4);
    let mut plan = original.clone();
    for offset in 0..7 {
        plan.pane_records.reverse();
        plan.tab_bar_records
            .rotate_left(offset % original.tab_bar_records.len());
        plan.tab_records.rotate_right(offset);
        plan.splitter_gap_records.reverse();
        plan.splitter_records
            .rotate_left(offset % original.splitter_records.len());
        plan = revalidate(&engine, plan).unwrap();
        assert_eq!(plan, original);
    }
}

#[test]
fn tab_directory_preserves_input_error_order_with_multiple_invalid_visible_records() {
    let (engine, original) = compiled_split_with_tabs(4, 4);
    let first_bar = *original.tab_bar_records()[0].id();
    let mutate_selection = |record: &TabRecord| {
        TabRecord::new(
            *record.id(),
            record.full_bounds(),
            record.visible_bounds(),
            record.text_bounds(),
            record.drag_hit(),
            record.close_visual_bounds(),
            record.close_bounds(),
            !record.selected(),
            record.ordinal(),
            record.layer(),
        )
    };
    let mut plan = original.clone();
    for record in &mut plan.tab_records {
        if record.id().tabs == first_bar.tabs {
            *record = mutate_selection(record);
        }
    }
    for reverse in [false, true] {
        if reverse {
            plan.tab_records.reverse();
        }
        let expected = *plan
            .tab_records
            .iter()
            .find(|record| record.id().tabs == first_bar.tabs)
            .unwrap()
            .id();
        assert!(matches!(
            revalidate(&engine, plan.clone()),
            Err(SceneBuildError::InvalidTabRecord { id, .. }) if id == expected
        ));
    }
}

#[test]
fn semantic_record_duplicates_keep_every_class_in_the_original_priority() {
    let (_, original) = compiled_split_with_tabs(4, 4);
    for class in 0..5 {
        let mut plan = original.clone();
        let expected = match class {
            0 => SceneBuildError::DuplicatePane {
                id: plan.pane_records[0].id(),
            },
            1 => SceneBuildError::DuplicateTabBar {
                id: *plan.tab_bar_records[0].id(),
            },
            2 => SceneBuildError::DuplicateTab {
                id: *plan.tab_records[0].id(),
            },
            3 => SceneBuildError::DuplicateSplitterGap {
                id: plan.splitter_gap_records[0].id(),
            },
            _ => SceneBuildError::DuplicateSplitter {
                id: *plan.splitter_records[0].id(),
            },
        };
        if class == 0 {
            plan.pane_records.push(plan.pane_records[0]);
        }
        if class <= 1 {
            plan.tab_bar_records.push(plan.tab_bar_records[0].clone());
        }
        if class <= 2 {
            plan.tab_records.push(plan.tab_records[0].clone());
        }
        if class <= 3 {
            plan.splitter_gap_records.push(plan.splitter_gap_records[0]);
        }
        plan.splitter_records.push(plan.splitter_records[0].clone());
        assert_eq!(validate_semantic_uniqueness(&plan).err(), Some(expected));
    }
}
