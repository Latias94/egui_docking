use super::*;
use crate::drop_target::DropOcclusionRecord;
use crate::geometry::{Constraints, LogicalSize};
use crate::graph::{Axis, Node, RootRecord, SurfacePresentation, Workspace};
use crate::ids::{EngineAuthorityDomainId, ItemId, WorkspaceEpoch};
use crate::layout::{LayoutMetrics, project_root};
use crate::policy::{PolicyRevision, TabBarInteraction};
use crate::presentation_config::{DockPresentationConfig, PresentationConfigRevision};
use crate::presentation_observation::PresentationOutputSerial;
use crate::scene::{
    PaneRecord, RootLayoutFacts, SplitterRecord, SurfaceProgramRootOperation,
    SurfaceProgramRootPresentation, SurfaceSceneStamp, TabBarRecord,
    TabListMenuGeometryAvailability,
};
use crate::scene_manifest::{
    Measurement, SurfaceMeasurementTicket, SurfaceMeasurements, SurfaceRequirementRevision,
    SurfaceRequirements, SurfaceSceneRevision,
};

fn rect(x: f64, y: f64, width: f64, height: f64) -> LogicalRect {
    LogicalRect::new(x, y, width, height).unwrap()
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

fn output(serial: u64) -> SurfacePresentationOutputTicket {
    SurfacePresentationOutputTicket::mint(
        ticket().authority_domain(),
        PresentationOutputSerial::new_for_test(serial),
        SurfaceSceneStamp::new(ticket(), SurfaceSceneRevision::new(serial)),
    )
}

#[derive(Clone)]
struct Fixture {
    bounds: LogicalRect,
    root_bounds: LogicalRect,
    pane_bounds: Option<LogicalRect>,
    draw_bounds: LogicalRect,
    hit_bounds: LogicalRect,
    axis: Axis,
    chrome_bounds: LogicalRect,
    selected: ItemId,
    config: DockPresentationConfig,
    occlusions: Vec<DropOcclusionRecord>,
}

impl Default for Fixture {
    fn default() -> Self {
        Self {
            bounds: rect(0.0, 0.0, 600.0, 600.0),
            root_bounds: rect(40.0, 40.0, 520.0, 520.0),
            pane_bounds: Some(rect(0.0, 0.0, 600.0, 600.0)),
            draw_bounds: rect(298.0, 40.0, 4.0, 520.0),
            hit_bounds: rect(298.0, 40.0, 4.0, 520.0),
            axis: Axis::Horizontal,
            chrome_bounds: rect(280.0, 120.0, 40.0, 20.0),
            selected: ItemId::new(1),
            config: DockPresentationConfig::default(),
            occlusions: Vec::new(),
        }
    }
}

impl Fixture {
    fn plan(&self) -> PresentationPlan {
        let requirements = SurfaceRequirements::new(
            ticket(),
            Default::default(),
            Default::default(),
            Default::default(),
            Default::default(),
        )
        .unwrap();
        let mut measurements = SurfaceMeasurements::new(ticket());
        measurements
            .set_bounds(requirements.bounds(), Measurement::Measured(self.bounds))
            .unwrap();
        let measured = requirements
            .validate(&measurements)
            .unwrap()
            .require_authoritative()
            .unwrap();
        let root = RootId::new(1);
        let mut builder = Workspace::builder();
        let tabs = builder.insert_node(Node::tabs([ItemId::new(1), ItemId::new(2)]));
        builder.set_root(root, RootRecord::new(tabs));
        builder.set_surface(ticket().surface(), SurfacePresentation::with_main(root));
        let workspace = builder.build().unwrap();
        let constraints =
            Constraints::new(LogicalSize::new(0.0, 0.0).unwrap(), self.root_bounds.size()).unwrap();
        let projection = project_root(
            &workspace,
            root,
            self.root_bounds,
            &[(tabs, constraints)].into_iter().collect(),
            LayoutMetrics::new(self.config.splitter_thickness()).unwrap(),
        )
        .unwrap();
        let mut plan = PresentationPlan::new(ticket().surface(), self.bounds);
        plan.install_layout_facts(self.config.clone(), measured);
        plan.push_root_layout_facts(RootLayoutFacts::new(
            root,
            self.root_bounds,
            Default::default(),
            Arc::new(projection),
        ));
        plan.begin_surface_program_root(root, SurfaceProgramRootPresentation::Main)
            .unwrap();
        if let Some(bounds) = self.pane_bounds {
            let pane = PaneSceneId { root, tabs };
            plan.push_pane_record(PaneRecord::new(
                pane,
                bounds,
                bounds,
                Some(self.selected),
                SceneLayerKey::surface_base(),
            ));
            plan.push_surface_program_pane_operation(pane, Some(self.selected))
                .unwrap();
        }
        let bar = TabBarSceneId { root, tabs };
        plan.push_tab_bar_record(TabBarRecord::new(
            bar,
            self.chrome_bounds,
            self.chrome_bounds,
            0.0,
            0.0,
            Vec::new(),
            Vec::new(),
            None,
            None,
            TabBarInteraction::Disabled,
            TabListMenuGeometryAvailability::Unavailable,
            SceneLayerKey::surface_base(),
        ));
        plan.push_surface_program_root_operation(SurfaceProgramRootOperation::TabBar(bar))
            .unwrap();
        let splitter = SplitterSceneId {
            root,
            split: tabs,
            index: 0,
        };
        plan.push_splitter_record(SplitterRecord::new(
            splitter,
            self.draw_bounds,
            HitRegion::new(self.hit_bounds),
            self.axis,
            self.root_bounds,
            self.root_bounds,
            0.0,
            0.0,
            520.0,
            520.0,
            vec![200.0, 200.0],
            None,
            Vec::new(),
            SceneLayerKey::surface_base(),
            true,
        ));
        plan.push_surface_program_root_operation(SurfaceProgramRootOperation::Splitter(splitter))
            .unwrap();
        for &occlusion in &self.occlusions {
            plan.push_drop_occlusion(occlusion);
        }
        plan.end_surface_program_root(root).unwrap();
        plan.finish_surface_program().unwrap();
        plan.link_surface_program().unwrap();
        plan
    }
}

fn assert_full_compile_matches(reused: &PresentationHitManifest, plan: &PresentationPlan) {
    let full = PresentationHitManifest::compile(reused.output(), plan).unwrap();
    assert_eq!(*reused, full);
    for (left, right) in reused.regions().iter().zip(full.regions()) {
        assert!(same_region(left, right));
    }
    assert_eq!(
        reused
            .geometry
            .touch_regions
            .iter()
            .map(|(row, rect)| (*row, rect_bits(*rect)))
            .collect::<Vec<_>>(),
        full.geometry
            .touch_regions
            .iter()
            .map(|(row, rect)| (*row, rect_bits(*rect)))
            .collect::<Vec<_>>()
    );
}

#[test]
fn selected_item_change_replays_the_full_program_and_rebinds_shared_geometry() {
    let fixture = Fixture::default();
    let previous_plan = fixture.plan();
    let previous = PresentationHitManifest::compile(output(1), &previous_plan).unwrap();
    assert!(!previous.geometry.touch_regions.is_empty());
    let changed_plan = Fixture {
        selected: ItemId::new(2),
        ..fixture
    }
    .plan();
    assert_ne!(previous_plan, changed_plan);
    let changed = PresentationHitManifest::compile_reusing(
        output(2),
        &changed_plan,
        Some((&previous_plan, &previous)),
    )
    .unwrap();
    assert!(Arc::ptr_eq(&previous.geometry, &changed.geometry));
    assert_ne!(previous.output(), changed.output());
    assert_eq!(changed.output(), output(2));
    assert_full_compile_matches(&changed, &changed_plan);
}

#[test]
fn touch_context_changes_reject_reuse_even_when_receiver_regions_are_unchanged() {
    let fixture = Fixture::default();
    let previous_plan = fixture.plan();
    let previous = PresentationHitManifest::compile(output(1), &previous_plan).unwrap();
    let mut cases = Vec::new();
    let mut changed = fixture.clone();
    changed.chrome_bounds = rect(280.0, 220.0, 40.0, 20.0);
    cases.push(changed);
    let mut changed = fixture.clone();
    changed.root_bounds = rect(60.0, 60.0, 480.0, 480.0);
    cases.push(changed);
    for config in [
        DockPresentationConfig::builder()
            .touch_resize_hit_extent(60.0)
            .build()
            .unwrap(),
        DockPresentationConfig::builder()
            .splitter_hit_extent(24.0)
            .build()
            .unwrap(),
        DockPresentationConfig::builder()
            .floating_resize_extent(10.0)
            .build()
            .unwrap(),
    ] {
        cases.push(Fixture {
            config,
            ..fixture.clone()
        });
    }
    let mut changed = fixture.clone();
    changed.occlusions.push(DropOcclusionRecord::new(
        FloatingPresentationId::new(7),
        HitRegion::new(rect(280.0, 320.0, 40.0, 30.0)),
        SceneLayerKey::new(1),
    ));
    cases.push(changed);
    for changed in cases {
        let plan = changed.plan();
        let mut emitted = Vec::new();
        emit_regions(&plan, &mut |region| emitted.push(region)).unwrap();
        let mut old_emitted = Vec::new();
        emit_regions(&previous_plan, &mut |region| old_emitted.push(region)).unwrap();
        assert_eq!(emitted.len(), old_emitted.len());
        assert!(
            emitted
                .iter()
                .zip(&old_emitted)
                .all(|(a, b)| same_region(a, b))
        );
        assert!(!touch::same_context(&plan, &previous_plan));
        let actual = PresentationHitManifest::compile_reusing(
            output(2),
            &plan,
            Some((&previous_plan, &previous)),
        )
        .unwrap();
        assert!(!Arc::ptr_eq(&actual.geometry, &previous.geometry));
        assert_full_compile_matches(&actual, &plan);
    }
}

#[test]
fn draw_axis_surface_bounds_and_emission_count_changes_use_full_compilation() {
    let fixture = Fixture::default();
    let previous_plan = fixture.plan();
    let previous = PresentationHitManifest::compile(output(1), &previous_plan).unwrap();
    let cases = [
        Fixture {
            draw_bounds: rect(300.0, 40.0, 4.0, 520.0),
            ..fixture.clone()
        },
        Fixture {
            axis: Axis::Vertical,
            ..fixture.clone()
        },
        Fixture {
            bounds: rect(0.0, 0.0, 620.0, 600.0),
            ..fixture.clone()
        },
        Fixture {
            pane_bounds: None,
            ..fixture
        },
    ];
    for changed in cases {
        let plan = changed.plan();
        let actual = PresentationHitManifest::compile_reusing(
            output(2),
            &plan,
            Some((&previous_plan, &previous)),
        )
        .unwrap();
        assert!(!Arc::ptr_eq(&actual.geometry, &previous.geometry));
        assert_full_compile_matches(&actual, &plan);
    }
}

#[test]
fn raw_signed_zero_changes_never_reuse_numerically_equal_receiver_geometry() {
    let previous_plan = Fixture::default().plan();
    let previous = PresentationHitManifest::compile(output(1), &previous_plan).unwrap();
    let plan = Fixture {
        pane_bounds: Some(rect(-0.0, -0.0, 600.0, 600.0)),
        ..Fixture::default()
    }
    .plan();
    assert_eq!(previous_plan.pane_records(), plan.pane_records());
    let actual = PresentationHitManifest::compile_reusing(
        output(2),
        &plan,
        Some((&previous_plan, &previous)),
    )
    .unwrap();
    assert!(!Arc::ptr_eq(&actual.geometry, &previous.geometry));
    assert_eq!(
        rect_bits(actual.regions()[0].hit.rect())[0],
        (-0.0_f64).to_bits()
    );
    assert_full_compile_matches(&actual, &plan);
}

#[test]
fn any_compacted_row_disables_reuse_and_cache_hints_do_not_change_equality() {
    let plan = Fixture {
        draw_bounds: rect(1_000.0, 1_000.0, 0.0, 0.0),
        hit_bounds: rect(1_000.0, 1_000.0, 0.0, 0.0),
        ..Fixture::default()
    }
    .plan();
    let previous = PresentationHitManifest::compile(output(1), &plan).unwrap();
    assert_eq!(previous.regions().len(), 1);
    assert!(previous.geometry.emission_rows.is_none());
    let actual =
        PresentationHitManifest::compile_reusing(output(2), &plan, Some((&plan, &previous)))
            .unwrap();
    assert!(!Arc::ptr_eq(&actual.geometry, &previous.geometry));
    assert_full_compile_matches(&actual, &plan);

    let complete = PresentationHitManifest::compile(output(1), &Fixture::default().plan()).unwrap();
    assert!(complete.geometry.emission_rows.is_some());
    let without_hint = PresentationHitManifest {
        output: complete.output,
        geometry: Arc::new(HitGeometry {
            touch_order: complete.geometry.touch_order.clone(),
            regions: complete.geometry.regions.clone(),
            touch_regions: complete.geometry.touch_regions.clone(),
            emission_rows: None,
        }),
    };
    assert_eq!(complete, without_hint);
}
