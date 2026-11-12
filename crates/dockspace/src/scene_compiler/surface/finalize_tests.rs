use super::*;
use crate::geometry::LogicalSize;
use crate::graph::{Axis, ContainedFloating, RootRecord, SurfacePresentation};
use crate::ids::ItemId;
use crate::policy::{DockPolicy, TabBarPolicy};
use crate::scene::{ContainedLayoutOwner, PresentationPlanValidator};
use crate::scene_manifest::{Measurement, PolicyRevision, TabIntrinsic, TabStripMetrics};
use proptest::prelude::*;

const SURFACE: SurfaceId = SurfaceId::new(1);
const ROOT: RootId = RootId::new(1);
const FLOATING: FloatingPresentationId = FloatingPresentationId::new(1);

fn workspace(empty: bool, split: bool) -> Workspace {
    let mut builder = Workspace::builder();
    let first = builder.insert_node(Node::tabs(if empty {
        vec![]
    } else {
        vec![ItemId::new(1), ItemId::new(2)]
    }));
    let node = if split {
        let second = builder.insert_node(Node::tabs([ItemId::new(3), ItemId::new(4)]));
        let third = builder.insert_node(Node::tabs([ItemId::new(5), ItemId::new(6)]));
        let nested =
            builder.insert_node(Node::split(Axis::Vertical, [second, third], [1.0, 2.0]).unwrap());
        builder.insert_node(Node::split(Axis::Horizontal, [first, nested], [2.0, 1.0]).unwrap())
    } else {
        first
    };
    builder.set_root(ROOT, RootRecord::new(node).with_central(first));
    builder.set_surface(SURFACE, SurfacePresentation::with_main(ROOT));
    if split {
        let floating = builder.insert_node(Node::tabs([ItemId::new(7), ItemId::new(8)]));
        builder.set_root(RootId::new(2), RootRecord::new(floating));
        builder.set_contained_floating(
            FLOATING,
            ContainedFloating::new(
                RootId::new(2),
                LogicalRect::new(75.0, 25.0, 360.0, 240.0).unwrap(),
            ),
        );
        builder.attach_contained(SURFACE, FLOATING).unwrap();
    }
    builder.build().unwrap()
}

fn inputs(
    workspace: &Workspace,
    policy: &DockPolicySnapshot,
    bounds: LogicalRect,
    strip: TabStripMetrics,
    minimum: f64,
    intrinsic: f64,
) -> (SceneRequirementManifest, SurfaceMeasurements) {
    let version = WorkspaceVersion::initial();
    let draft = derive_scene_requirement_draft(
        EngineAuthorityDomainId::new_for_test(1),
        workspace,
        version,
        PresentationConfigRevision::new(1),
        policy,
        RequirementRevision::new(1),
        &BTreeMap::from([(SURFACE, SurfaceRequirementRevision::new(1))]),
    )
    .unwrap();
    let requirements = draft.surface(SURFACE).unwrap();
    let mut measurements = SurfaceMeasurements::new(requirements.ticket());
    measurements
        .set_bounds(requirements.bounds(), Measurement::Measured(bounds))
        .unwrap();
    for key in requirements.pane_minimums() {
        measurements
            .insert_pane_minimum(
                key,
                Measurement::Measured(LogicalSize::new(minimum, minimum).unwrap()),
            )
            .unwrap();
    }
    for key in requirements.tab_intrinsics() {
        measurements
            .insert_tab_intrinsic(
                key,
                Measurement::Measured(TabIntrinsic::new(intrinsic).unwrap()),
            )
            .unwrap();
    }
    for key in requirements.tab_strips() {
        measurements
            .insert_tab_strip(key, Measurement::Measured(strip))
            .unwrap();
    }
    (
        draft.finalize(PopupPlaneRequirement::default()).unwrap(),
        measurements,
    )
}

fn compare(
    workspace: &Workspace,
    policy: &DockPolicySnapshot,
    config: &DockPresentationConfig,
    manifest: &SceneRequirementManifest,
    measurements: &SurfaceMeasurements,
    projection: Option<ContainedLayoutProjection>,
) -> Option<Result<PresentationPlan, SceneBuildError>> {
    let states = TabStripStateStore::default();
    let raw = compile_surface_measurements(
        workspace,
        WorkspaceVersion::initial(),
        policy,
        config,
        manifest,
        measurements,
        &states,
        &[],
        projection,
    );
    let compiled = compile_surface_plan(
        workspace,
        WorkspaceVersion::initial(),
        policy,
        config,
        manifest,
        measurements,
        &states,
        &[],
        projection,
    );
    match (raw, compiled) {
        (Err(expected), Err(actual)) => {
            assert_eq!(actual, expected);
            None
        }
        (Ok(raw), Ok(compiled)) => {
            let expected = PresentationPlanValidator::for_manifest_surface(
                workspace,
                WorkspaceVersion::initial(),
                manifest,
                policy,
                SURFACE,
            )
            .and_then(|validator| validator.with_contained_projection(projection))
            .and_then(|validator| validator.validate_and_canonicalize(raw));
            let actual = compiled.finalize();
            assert_eq!(actual, expected);
            Some(actual)
        }
        _ => panic!("the same compiler inputs must produce the same draft outcome"),
    }
}

proptest! {
    #[test]
    fn compiled_finalization_matches_full_validation_for_generated_measurements(
        empty in any::<bool>(),
        x in -4096_i32..4096,
        y in -4096_i32..4096,
        width in 0_u16..4096,
        height in 0_u16..4096,
        scale in prop_oneof![6 => Just(1.0), 1 => Just(1.0e-12), 1 => Just(1.0e12), 1 => Just(1.0e100)],
        tab_width in 2_u8..96,
        minimum in 0_u16..512,
        intrinsic in 0_u16..1024,
        reservation in 0_u16..1024,
        trailing_reservation in 0_u16..1024,
        after_tabs in 0_u16..1024,
        hide_grip in any::<bool>(),
        hidden in any::<bool>(),
        locked in any::<bool>(),
    ) {
        let workspace = workspace(empty, true);
        let mut policy = DockPolicy::default();
        policy.set_arrangement_locked(locked);
        if hidden { policy.set_tab_bar(TabBarPolicy::new(TabBarVisibility::Hidden, TabBarInteraction::Disabled)); }
        let policy = policy.snapshot(PolicyRevision::new(1));
        let config = DockPresentationConfig::builder()
            .tab_group_grip_extent(if hide_grip { 0.0 } else { 28.0 })
            .tab_min_width(f64::from(tab_width))
            .tab_max_width(f64::from(tab_width) + 32.0)
            .build().unwrap();
        let bounds = LogicalRect::new(f64::from(x)*scale, f64::from(y)*scale,
            f64::from(width)*scale, f64::from(height)*scale).unwrap();
        let (manifest, measurements) = inputs(&workspace, &policy, bounds,
            TabStripMetrics::new(f64::from(reservation), f64::from(trailing_reservation)).unwrap()
                .with_after_tabs_width(f64::from(after_tabs)).unwrap(),
            f64::from(minimum), f64::from(intrinsic));
        compare(&workspace, &policy, &config, &manifest, &measurements, None);
    }
}

#[test]
fn gripless_bar_keeps_background_dragging_outside_application_chrome() {
    let workspace = workspace(false, false);
    let policy = DockPolicy::default().snapshot(PolicyRevision::new(1));
    let config = DockPresentationConfig::builder()
        .tab_group_grip_extent(0.0)
        .build()
        .unwrap();
    let strip = TabStripMetrics::new(20.0, 40.0)
        .unwrap()
        .with_after_tabs_width(32.0)
        .unwrap();
    let (manifest, measurements) = inputs(
        &workspace,
        &policy,
        LogicalRect::new(0.0, 0.0, 800.0, 500.0).unwrap(),
        strip,
        0.0,
        60.0,
    );
    let plan = compare(&workspace, &policy, &config, &manifest, &measurements, None)
        .unwrap()
        .unwrap();
    let bar = &plan.tab_bar_records()[0];
    let after_tabs = bar.after_tabs_bounds().unwrap();
    assert!(bar.group_grip_bounds().is_none());
    assert_eq!(bar.leading_reserved_bounds().unwrap().x(), bar.bounds().x());
    assert_eq!(
        after_tabs.x(),
        bar.members().last().unwrap().full_bounds().max().x()
    );
    assert_eq!(after_tabs.width(), 32.0);
    let group = bar.group_drag().unwrap();
    assert!(group.leading_grip().is_none());
    assert_eq!(group.regions().count(), 1);
    let trailing = group.trailing_empty().unwrap().bounds();
    assert_eq!(trailing.x(), after_tabs.max().x());
    assert_eq!(trailing.max().x(), bar.viewport().max().x());
}

#[test]
fn scrolling_application_slot_contributes_to_overflow_without_squeezing_tab_targets() {
    let workspace = workspace(false, false);
    let policy = DockPolicy::default().snapshot(PolicyRevision::new(1));
    let config = DockPresentationConfig::builder()
        .tab_group_grip_extent(0.0)
        .build()
        .unwrap();
    let strip = TabStripMetrics::new(0.0, 0.0)
        .unwrap()
        .with_after_tabs_width(96.0)
        .unwrap();
    let (manifest, measurements) = inputs(
        &workspace,
        &policy,
        LogicalRect::new(0.0, 0.0, 180.0, 500.0).unwrap(),
        strip,
        0.0,
        60.0,
    );
    let plan = compare(&workspace, &policy, &config, &manifest, &measurements, None)
        .unwrap()
        .unwrap();
    let bar = &plan.tab_bar_records()[0];
    let total = bar
        .members()
        .iter()
        .map(|tab| tab.full_bounds().width())
        .sum::<f64>()
        + 96.0;
    assert!(
        bar.members()
            .iter()
            .all(|tab| tab.full_bounds().width() >= config.tab_min_width())
    );
    assert_eq!(bar.maximum_scroll_offset(), total - bar.viewport().width());
    assert!(bar.maximum_scroll_offset() > 0.0);
    assert_eq!(bar.after_tabs_bounds().unwrap().width(), 96.0);
    assert!(bar.group_drag().is_none());
}

#[test]
fn compiler_drafts_keep_external_geometry_rejections() {
    let policy = DockPolicy::default().snapshot(PolicyRevision::new(1));
    let bounds = LogicalRect::new(0.0, 0.0, 100.0, 100.0).unwrap();
    let workspace = workspace(false, false);
    let config = DockPresentationConfig::builder()
        .tab_min_width(8.0)
        .tab_max_width(8.0)
        .build()
        .unwrap();
    let strip = TabStripMetrics::new(0.0, 0.0).unwrap();
    let (manifest, measurements) = inputs(&workspace, &policy, bounds, strip, 0.0, 56.0);
    assert!(matches!(
        compare(&workspace, &policy, &config, &manifest, &measurements, None),
        Some(Err(SceneBuildError::InvalidTabBarRecord { .. }))
    ));
}

#[test]
fn oversized_reservations_are_clipped_to_the_empty_bar() {
    let policy = DockPolicy::default().snapshot(PolicyRevision::new(1));
    let bounds = LogicalRect::new(0.0, 0.0, 100.0, 100.0).unwrap();
    let workspace = workspace(true, false);
    let config = DockPresentationConfig::default();
    let strip = TabStripMetrics::new(1000.0, 1000.0).unwrap();
    let (manifest, measurements) = inputs(&workspace, &policy, bounds, strip, 0.0, 56.0);
    let plan = compare(&workspace, &policy, &config, &manifest, &measurements, None)
        .unwrap()
        .unwrap();
    let bar = &plan.tab_bar_records()[0];
    assert_eq!(bar.leading_reserved_bounds(), Some(bar.bounds()));
    assert_eq!(bar.trailing_reserved_bounds(), None);
    assert_eq!(bar.viewport().width(), 0.0);
}

#[test]
fn compiled_finalization_keeps_contained_projection_source_guards() {
    use crate::interaction::{ContainedTransformGeneration, ContainedTransformSessionId};
    let workspace = workspace(false, true);
    let policy = DockPolicy::default().snapshot(PolicyRevision::new(1));
    let config = DockPresentationConfig::default();
    let rect = workspace.contained_floating(FLOATING).unwrap().rect;
    let (manifest, measurements) = inputs(
        &workspace,
        &policy,
        LogicalRect::new(0.0, 0.0, 800.0, 600.0).unwrap(),
        TabStripMetrics::new(0.0, 0.0).unwrap(),
        0.0,
        56.0,
    );
    for wrong_root in [true, false] {
        let projection = ContainedLayoutProjection {
            owner: ContainedLayoutOwner::Transform(ContainedTransformSessionId::new(
                WorkspaceVersion::initial().epoch(),
                ContainedTransformGeneration::new(1),
            )),
            surface: SURFACE,
            root: if wrong_root {
                RootId::new(99)
            } else {
                RootId::new(2)
            },
            floating: FLOATING,
            source_rect: if wrong_root {
                rect
            } else {
                LogicalRect::new(0.0, 0.0, 50.0, 50.0).unwrap()
            },
            rect,
            target_basis: None,
        };
        assert!(matches!(
            compare(
                &workspace,
                &policy,
                &config,
                &manifest,
                &measurements,
                Some(projection)
            ),
            Some(Err(SceneBuildError::InvalidContainedRecord { .. }))
        ));
    }
}

#[test]
fn compiler_finalization_rejects_an_uncovered_target_under_joint_occlusion() {
    let mut builder = Workspace::builder();
    let main = builder.insert_node(Node::tabs([]));
    builder.set_root(ROOT, RootRecord::new(main).with_central(main));
    builder.set_surface(SURFACE, SurfacePresentation::with_main(ROOT));
    for ordinal in 0..2 {
        let tabs = builder.insert_node(Node::tabs([ItemId::new(ordinal + 1)]));
        let root = RootId::new(ordinal + 2);
        let floating = FloatingPresentationId::new(ordinal + 1);
        builder.set_root(root, RootRecord::new(tabs));
        builder.set_contained_floating(
            floating,
            ContainedFloating::new(
                root,
                LogicalRect::new(400.0 * ordinal as f64, 0.0, 400.0, 200.0).unwrap(),
            ),
        );
        builder.attach_contained(SURFACE, floating).unwrap();
    }
    let workspace = builder.build().unwrap();
    let policy = DockPolicy::default().snapshot(PolicyRevision::new(1));
    let (manifest, measurements) = inputs(
        &workspace,
        &policy,
        LogicalRect::new(0.0, 0.0, 800.0, 28.0).unwrap(),
        TabStripMetrics::new(0.0, 0.0).unwrap(),
        0.0,
        56.0,
    );
    assert!(matches!(
        compare(
            &workspace,
            &policy,
            &DockPresentationConfig::default(),
            &manifest,
            &measurements,
            None
        ),
        Some(Err(SceneBuildError::InvalidMagneticTargetField { .. }))
    ));
}
