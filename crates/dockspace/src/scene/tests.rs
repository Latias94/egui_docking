use std::collections::{BTreeMap, BTreeSet};

use super::*;
use crate::command::DockTarget;
use crate::drop_guide::DropGuideTargetRecord;
use crate::drop_target::{DropTargetId, DropTargetRecord, DropVisual, SurfaceBackground};
use crate::geometry::LogicalSize;
use crate::graph::{ContainedFloating, RootRecord, SurfacePresentation};
use crate::hit_region::HitRegion;
use crate::ids::{EngineAuthorityDomainId, WorkspaceEpoch, WorkspaceRevision};
use crate::presentation_config::PresentationConfigRevision;
use crate::presentation_observation::{PresentationOutputSerial, PresentedSurfaceAuthority};
use crate::scene_manifest::{
    PolicyRevision, RequirementRevision, SceneRequirementDraft, SurfaceRequirementRevision,
    SurfaceRequirements,
};
use crate::transition::WorkspaceVersion;

fn bounds() -> LogicalRect {
    LogicalRect::new(0.0, 0.0, 800.0, 600.0).expect("valid bounds")
}

fn occlusion(floating: u64, rect: LogicalRect, layer: u64) -> DropOcclusionRecord {
    DropOcclusionRecord::new(
        FloatingPresentationId::new(floating),
        HitRegion::new(rect),
        SceneLayerKey::new(layer),
    )
}

#[test]
fn splitter_extent_constraints_require_ordered_finite_bounds_and_an_in_range_extent() {
    assert!(splitter_extent_satisfies_constraints(
        Some(40.0),
        20.0,
        60.0
    ));
    assert!(!splitter_extent_satisfies_constraints(
        Some(40.0),
        20.0,
        f64::INFINITY
    ));
    assert!(!splitter_extent_satisfies_constraints(
        Some(40.0),
        60.0,
        20.0
    ));
    assert!(!splitter_extent_satisfies_constraints(
        Some(10.0),
        20.0,
        60.0
    ));
    assert!(!splitter_extent_satisfies_constraints(
        Some(70.0),
        20.0,
        60.0
    ));
}

#[test]
fn splitter_operability_requires_exact_remaining_hit_area() {
    let region = LogicalRect::new(0.0, 0.0, 100.0, 100.0).expect("region is valid");
    let lower = SceneLayerKey::new(1);

    assert!(region_has_authoritative_area(region, lower, &[]));
    assert!(!region_has_authoritative_area(
        region,
        lower,
        &[occlusion(1, region, 2)],
    ));

    let tiled = [
        occlusion(
            2,
            LogicalRect::new(0.0, 0.0, 50.0, 100.0).expect("left tile is valid"),
            2,
        ),
        occlusion(
            3,
            LogicalRect::new(50.0, 0.0, 50.0, 100.0).expect("right tile is valid"),
            3,
        ),
    ];
    assert!(!region_has_authoritative_area(region, lower, &tiled));

    let gap = [
        occlusion(
            4,
            LogicalRect::new(0.0, 0.0, 49.0, 100.0).expect("left cover is valid"),
            2,
        ),
        occlusion(
            5,
            LogicalRect::new(51.0, 0.0, 49.0, 100.0).expect("right cover is valid"),
            3,
        ),
    ];
    assert!(region_has_authoritative_area(region, lower, &gap));
    assert!(region_has_authoritative_area(
        region,
        SceneLayerKey::new(3),
        &tiled,
    ));
}

fn background(surface: SurfaceId) -> DropTargetRecord {
    background_in(surface, bounds())
}

fn background_in(surface: SurfaceId, surface_bounds: LogicalRect) -> DropTargetRecord {
    DropTargetRecord::surface_background(
        SurfaceBackground::new(surface),
        HitRegion::new(surface_bounds),
        DropVisual::new(surface_bounds),
    )
}

fn manifest(
    surfaces: impl IntoIterator<Item = (SurfaceId, u64)>,
    manifest_revision: u64,
) -> SceneRequirementManifest {
    let authority = EngineAuthorityDomainId::new_for_test(91);
    let epoch = WorkspaceEpoch::new(2);
    let config = PresentationConfigRevision::new(3);
    let policy = PolicyRevision::new(4);
    let surfaces = surfaces
        .into_iter()
        .map(|(surface, revision)| {
            let ticket = SurfaceMeasurementTicket::new(
                authority,
                epoch,
                config,
                policy,
                SurfaceRequirementRevision::new(revision),
                surface,
            );
            let requirements = SurfaceRequirements::new(
                ticket,
                BTreeSet::new(),
                BTreeSet::new(),
                BTreeSet::new(),
                BTreeMap::new(),
            )
            .expect("empty rootless requirements are coherent");
            (surface, requirements)
        })
        .collect::<BTreeMap<_, _>>();
    SceneRequirementDraft::new_for_test(
        authority,
        WorkspaceVersion::new(epoch, WorkspaceRevision::new(5)),
        config,
        policy,
        RequirementRevision::new(manifest_revision),
        surfaces,
    )
    .and_then(|draft| draft.finalize(PopupPlaneRequirement::default()))
    .expect("test manifest is coherent")
}

fn measured_candidate(ticket: SurfaceMeasurementTicket) -> PresentationPlan {
    PresentationPlan::from_measurements(ticket, PopupPlaneRequirement::default(), bounds(), None)
}

fn headless_capture() -> SurfaceCoordinateCapture {
    SurfaceCoordinateCapture::Headless {
        authority_generation: CoordinateGeneration::new(7),
    }
}

fn install_ready(
    scenes: &mut SurfaceSceneSet,
    ticket: SurfaceMeasurementTicket,
    capture: SurfaceCoordinateCapture,
    serial: u64,
) -> (SurfaceSceneStamp, SurfacePresentationOutputTicket) {
    scenes
        .install_ready(
            measured_candidate(ticket),
            capture,
            ticket.authority_domain(),
            PresentationOutputSerial::new_for_test(serial),
        )
        .expect("candidate installs")
}

fn acknowledge_ready(
    scenes: &mut SurfaceSceneSet,
    output: SurfacePresentationOutputTicket,
    _serial: u64,
) {
    assert!(
        scenes
            .accept_observed_authority(PresentedSurfaceAuthority::mint_observed_for_test(
                output,
                CoordinateGeneration::new(7),
            ))
            .is_ok()
    );
}

fn validate(
    workspace: &Workspace,
    plan: PresentationPlan,
) -> Result<PresentationPlan, SceneBuildError> {
    let policy = DockPolicySnapshot::default();
    PresentationPlanValidator::new(workspace, &policy)
        .expect("workspace index is valid")
        .validate_and_canonicalize(plan)
}

#[test]
fn manifest_surface_validator_rejects_a_stale_workspace_revision() {
    let surface = SurfaceId::new(501);
    let workspace = rootless_workspace(surface);
    let manifest = manifest([(surface, 1)], 1);
    let expected = WorkspaceVersion::new(WorkspaceEpoch::new(2), WorkspaceRevision::new(6));
    let policy = DockPolicySnapshot::default();

    let result = PresentationPlanValidator::for_manifest_surface(
        &workspace, expected, &manifest, &policy, surface,
    );
    assert!(matches!(
        result,
        Err(SceneBuildError::WorkspaceIndexVersionMismatch {
            expected: rejected_expected,
            actual,
        }) if rejected_expected == expected && actual == manifest.workspace()
    ));
}

#[test]
fn manifest_surface_validator_rejects_a_plan_for_another_surface() {
    let surface = SurfaceId::new(511);
    let other_surface = SurfaceId::new(512);
    let mut builder = Workspace::builder();
    for (root, item, owner) in [
        (RootId::new(513), ItemId::new(515), surface),
        (RootId::new(514), ItemId::new(516), other_surface),
    ] {
        let tabs = builder.insert_node(Node::tabs([item]));
        builder.set_root(root, RootRecord::new(tabs).with_central(tabs));
        builder.set_surface(owner, SurfacePresentation::with_main(root));
    }
    let workspace = builder.build().expect("workspace is valid");
    let manifest = manifest([(surface, 1), (other_surface, 2)], 1);
    let policy = DockPolicySnapshot::default();
    let validator = PresentationPlanValidator::for_manifest_surface(
        &workspace,
        manifest.workspace(),
        &manifest,
        &policy,
        surface,
    )
    .expect("surface-bound validator is current");

    assert_eq!(
        validator.validate_and_canonicalize(PresentationPlan::new(other_surface, bounds())),
        Err(SceneBuildError::PresentationSurfaceMismatch {
            expected: surface,
            actual: other_surface,
        })
    );
}

fn publish_rootless_occlusions(plan: &mut PresentationPlan) {
    for (floating, offset, layer) in [
        (FloatingPresentationId::new(21), 10.0, 2),
        (FloatingPresentationId::new(22), 30.0, 3),
    ] {
        plan.push_drop_occlusion(DropOcclusionRecord::new(
            floating,
            HitRegion::new(
                LogicalRect::new(offset, offset, 200.0, 160.0)
                    .expect("contained rectangle is valid"),
            ),
            SceneLayerKey::new(layer),
        ));
    }
}

fn contained_minimum(floating: FloatingPresentationId) -> ContainedMinimumMeasurement {
    ContainedMinimumMeasurement::new(
        floating,
        LogicalSize::new(120.0, 90.0).expect("contained minimum is valid"),
    )
}

fn publish_rootless_minimums(plan: &mut PresentationPlan) {
    for floating in [
        FloatingPresentationId::new(22),
        FloatingPresentationId::new(21),
    ] {
        plan.push_contained_minimum(contained_minimum(floating));
    }
}

fn publish_rootless_facts(plan: &mut PresentationPlan) {
    publish_rootless_occlusions(plan);
    publish_rootless_minimums(plan);
}

fn rootless_workspace(surface: SurfaceId) -> Workspace {
    let mut builder = Workspace::builder();
    for (root, floating, item, offset) in [
        (RootId::new(11), FloatingPresentationId::new(21), 31, 10.0),
        (RootId::new(12), FloatingPresentationId::new(22), 32, 30.0),
    ] {
        let node = builder.insert_node(Node::tabs([ItemId::new(item)]));
        builder.set_root(root, RootRecord::new(node));
        builder.set_contained_floating(
            floating,
            ContainedFloating::new(
                root,
                LogicalRect::new(offset, offset, 200.0, 160.0)
                    .expect("contained rectangle is valid"),
            ),
        );
    }
    builder.set_surface(surface, SurfacePresentation::rootless());
    builder
        .attach_contained(surface, FloatingPresentationId::new(21))
        .expect("rootless surface exists");
    builder
        .attach_contained(surface, FloatingPresentationId::new(22))
        .expect("rootless surface exists");
    builder.build().expect("rootless workspace is valid")
}

fn rooted_workspace(surface: SurfaceId, root: RootId, item: ItemId) -> (Workspace, NodeId) {
    let mut builder = Workspace::builder();
    let tabs = builder.insert_node(Node::tabs([item]));
    builder.set_root(root, RootRecord::new(tabs));
    builder.set_surface(surface, SurfacePresentation::with_main(root));
    (builder.build().expect("rooted workspace is valid"), tabs)
}

fn center_guide(
    workspace: &Workspace,
    surface: SurfaceId,
    root: RootId,
    tabs: NodeId,
    layer: SceneLayerKey,
) -> DropGuideClusterRecord {
    let hit = LogicalRect::new(340.0, 250.0, 120.0, 100.0).expect("guide hit is valid");
    let draw = LogicalRect::new(370.0, 280.0, 60.0, 40.0).expect("guide draw is valid");
    let target = DropTargetRecord::new(
        DropTargetId::Center {
            surface,
            root,
            tabs,
        },
        DockTarget::Center(
            workspace
                .capture_tab_target(root, tabs)
                .expect("tabs target is current"),
        ),
        DropTargetAvailability::Available,
        HitRegion::new(hit),
        layer,
        DropVisual::new(bounds()),
    );
    DropGuideClusterRecord::inner_center(
        surface,
        root,
        tabs,
        HitRegion::new(bounds()),
        layer,
        DropGuideTargetRecord::new(target, draw),
    )
}

#[test]
fn surface_scene_set_roster_is_exact_and_stably_ordered() {
    let first = SurfaceId::new(1);
    let second = SurfaceId::new(2);
    let third = SurfaceId::new(3);
    let mut scenes =
        SurfaceSceneSet::new(&manifest([(second, 1), (first, 1)], 1)).expect("valid roster");
    let second_stamp = scenes.surface(second).expect("second exists").stamp();

    assert_eq!(
        scenes
            .surfaces()
            .map(|(surface, _)| *surface)
            .collect::<Vec<_>>(),
        [first, second]
    );
    assert!(scenes.surfaces().all(|(_, scene)| matches!(
        scene,
        SurfaceScene::Bootstrap(BootstrapSurfaceScene {
            reason: BootstrapSurfaceSceneReason::AwaitingContribution,
            ..
        })
    )));

    scenes
        .reconcile_manifest(&manifest([(third, 1), (second, 1)], 2))
        .expect("replacement roster reconciles");

    assert!(scenes.surface(first).is_none());
    assert_eq!(
        scenes.surface(second).expect("second remains").stamp(),
        second_stamp
    );
    assert!(matches!(
        scenes.surface(third),
        Some(SurfaceScene::Bootstrap(BootstrapSurfaceScene {
            reason: BootstrapSurfaceSceneReason::AwaitingContribution,
            ..
        }))
    ));
    assert_eq!(
        scenes
            .surfaces()
            .map(|(surface, _)| *surface)
            .collect::<Vec<_>>(),
        [second, third]
    );
}

#[test]
fn inactive_requirement_change_preserves_sibling_interaction_authority() {
    let changed = SurfaceId::new(10);
    let retained = SurfaceId::new(11);
    let initial = manifest([(changed, 1), (retained, 1)], 1);
    let mut scenes = SurfaceSceneSet::new(&initial).expect("scene set initializes");
    let mut painted = BTreeMap::new();

    for surface in [changed, retained] {
        let ticket = initial.surface(surface).expect("surface exists").ticket();
        let (stamp, output) = install_ready(&mut scenes, ticket, headless_capture(), surface.get());
        acknowledge_ready(&mut scenes, output, 1);
        painted.insert(surface, stamp);
    }

    scenes
        .reconcile_manifest(&manifest([(changed, 2), (retained, 1)], 2))
        .expect("requirements reconcile");

    let changed_scene = scenes.surface(changed).expect("changed surface remains");
    let SurfaceScene::Stale(changed_scene) = changed_scene else {
        panic!("changed surface must retain a paint-only fallback");
    };
    assert_eq!(
        changed_scene.reason(),
        StaleSurfaceSceneReason::RequirementsChanged
    );
    assert_eq!(changed_scene.paint_fallback().stamp(), painted[&changed]);
    assert!(scenes.ready_surface(changed).is_none());
    assert!(scenes.ready_surface(retained).is_some());
    assert_eq!(
        scenes
            .surface(retained)
            .and_then(SurfaceScene::paint_projection)
            .map(SurfacePaintProjection::plan_stamp),
        Some(painted[&retained])
    );
}

#[test]
fn removed_surface_revision_tombstone_prevents_aba() {
    let surface = SurfaceId::new(20);
    let mut scenes =
        SurfaceSceneSet::new(&manifest([(surface, 1)], 1)).expect("scene set initializes");
    let original = scenes.surface(surface).expect("surface exists").stamp();

    scenes
        .reconcile_manifest(&manifest(std::iter::empty(), 2))
        .expect("surface removal reconciles");
    assert!(scenes.surface(surface).is_none());

    scenes
        .reconcile_manifest(&manifest([(surface, 1)], 3))
        .expect("surface re-addition reconciles");
    let readded = scenes
        .surface(surface)
        .expect("surface is re-added")
        .stamp();
    assert!(readded.revision() > original.revision());
    assert_ne!(readded, original);
}

#[test]
fn installed_projection_binds_exact_ticket_and_coordinate_capture() {
    let surface = SurfaceId::new(30);
    let manifest = manifest([(surface, 8)], 1);
    let ticket = manifest.surface(surface).expect("surface exists").ticket();
    let capture = headless_capture();
    let mut scenes = SurfaceSceneSet::new(&manifest).expect("scene set initializes");

    let (stamp, output) = install_ready(&mut scenes, ticket, capture, 1);
    let ready = scenes
        .surface(surface)
        .and_then(SurfaceScene::ready)
        .expect("candidate is ready for painting");

    assert_eq!(stamp.requirement(), ticket);
    assert_eq!(stamp.surface(), surface);
    assert_eq!(ready.stamp(), stamp);
    assert_eq!(ready.output_ticket(), output);
    assert_eq!(ready.coordinate_capture(), capture);
    assert_eq!(ready.plan().measurement_ticket(), Some(ticket));
    assert!(scenes.ready_surface(surface).is_none());
}

#[test]
fn ready_surface_bounds_must_have_positive_area() {
    let zero = LogicalRect::new(0.0, 0.0, 0.0, 600.0).expect("zero bounds are representable");
    let rooted_surface = SurfaceId::new(40);
    let rooted_root = RootId::new(41);
    let (rooted, _) = rooted_workspace(rooted_surface, rooted_root, ItemId::new(42));
    assert_eq!(
        validate(&rooted, PresentationPlan::new(rooted_surface, zero)),
        Err(SceneBuildError::EmptyReadySurfaceBounds {
            surface: rooted_surface,
        })
    );

    let rootless_surface = SurfaceId::new(43);
    let rootless = rootless_workspace(rootless_surface);
    assert_eq!(
        validate(&rootless, PresentationPlan::new(rootless_surface, zero)),
        Err(SceneBuildError::EmptyReadySurfaceBounds {
            surface: rootless_surface,
        })
    );
}

#[test]
fn rootless_surface_requires_one_core_layered_background() {
    let surface = SurfaceId::new(50);
    let workspace = rootless_workspace(surface);
    assert_eq!(
        validate(&workspace, PresentationPlan::new(surface, bounds())),
        Err(SceneBuildError::MissingSurfaceBackground { surface })
    );

    let mut plan = PresentationPlan::new(surface, bounds());
    plan.set_surface_background(background(surface))
        .expect("first background is accepted");
    publish_rootless_facts(&mut plan);
    assert_eq!(
        plan.set_surface_background(background(surface)),
        Err(SceneBuildError::DuplicateSurfaceBackground { surface })
    );
    let plan = validate(&workspace, plan).expect("complete rootless plan validates");

    let background = plan
        .surface_background()
        .expect("background remains authoritative");
    assert_eq!(background.layer(), SceneLayerKey::new(1));
    assert_eq!(background.availability(), DropTargetAvailability::Available);
    assert_eq!(
        plan.contained_minimums()
            .iter()
            .map(|measurement| measurement.floating())
            .collect::<Vec<_>>(),
        [
            FloatingPresentationId::new(21),
            FloatingPresentationId::new(22),
        ]
    );
}

#[test]
fn rooted_surface_rejects_background_authority() {
    let surface = SurfaceId::new(60);
    let root = RootId::new(61);
    let (workspace, _) = rooted_workspace(surface, root, ItemId::new(62));
    let mut plan = PresentationPlan::new(surface, bounds());
    plan.set_surface_background(background(surface))
        .expect("record shape is valid before validation");

    assert!(matches!(
        validate(&workspace, plan),
        Err(SceneBuildError::UnexpectedSurfaceBackground {
            surface: rejected,
            ..
        }) if rejected == surface
    ));
}

#[test]
fn guide_cluster_layer_is_derived_from_root_presentation() {
    let surface = SurfaceId::new(70);
    let root = RootId::new(71);
    let (workspace, tabs) = rooted_workspace(surface, root, ItemId::new(72));

    let mut valid = PresentationPlan::new(surface, bounds());
    valid.push_drop_guide_cluster(center_guide(
        &workspace,
        surface,
        root,
        tabs,
        SceneLayerKey::new(1),
    ));
    let valid = validate(&workspace, valid).expect("main-layer guide validates");
    assert_eq!(valid.drop_guide_clusters().len(), 1);

    let mut invalid = PresentationPlan::new(surface, bounds());
    invalid.push_drop_guide_cluster(center_guide(
        &workspace,
        surface,
        root,
        tabs,
        SceneLayerKey::new(2),
    ));
    assert!(matches!(
        validate(&workspace, invalid),
        Err(SceneBuildError::DropGuideClusterLayerMismatch {
            surface: rejected,
            expected,
            actual,
            ..
        }) if rejected == surface
            && expected == SceneLayerKey::new(1)
            && actual == SceneLayerKey::new(2)
    ));
}

#[test]
fn contained_occlusion_layer_is_derived_from_roster_order() {
    let surface = SurfaceId::new(80);
    let workspace = rootless_workspace(surface);
    let mut plan = PresentationPlan::new(surface, bounds());
    plan.set_surface_background(background(surface))
        .expect("background is valid");
    plan.push_drop_occlusion(DropOcclusionRecord::new(
        FloatingPresentationId::new(21),
        HitRegion::new(
            LogicalRect::new(10.0, 10.0, 200.0, 160.0).expect("contained rectangle is valid"),
        ),
        SceneLayerKey::new(3),
    ));
    plan.push_drop_occlusion(DropOcclusionRecord::new(
        FloatingPresentationId::new(22),
        HitRegion::new(
            LogicalRect::new(30.0, 30.0, 200.0, 160.0).expect("contained rectangle is valid"),
        ),
        SceneLayerKey::new(3),
    ));
    publish_rootless_minimums(&mut plan);

    assert_eq!(
        validate(&workspace, plan),
        Err(SceneBuildError::DropOcclusionLayerMismatch {
            surface,
            floating: FloatingPresentationId::new(21),
            expected: SceneLayerKey::new(2),
            actual: SceneLayerKey::new(3),
        })
    );
}

#[test]
fn ready_surface_requires_exact_contained_occlusion_coverage() {
    let surface = SurfaceId::new(90);
    let workspace = rootless_workspace(surface);
    let mut plan = PresentationPlan::new(surface, bounds());
    plan.set_surface_background(background(surface))
        .expect("background is valid");
    plan.push_drop_occlusion(DropOcclusionRecord::new(
        FloatingPresentationId::new(21),
        HitRegion::new(
            LogicalRect::new(10.0, 10.0, 200.0, 160.0).expect("contained rectangle is valid"),
        ),
        SceneLayerKey::new(2),
    ));
    publish_rootless_minimums(&mut plan);

    assert_eq!(
        validate(&workspace, plan),
        Err(SceneBuildError::MissingDropOcclusion {
            surface,
            floating: FloatingPresentationId::new(22),
        })
    );
}

#[test]
fn ready_surface_rejects_partial_contained_occlusion_geometry() {
    let surface = SurfaceId::new(100);
    let workspace = rootless_workspace(surface);
    let mut plan = PresentationPlan::new(surface, bounds());
    plan.set_surface_background(background(surface))
        .expect("background is valid");
    plan.push_drop_occlusion(DropOcclusionRecord::new(
        FloatingPresentationId::new(21),
        HitRegion::new(LogicalRect::new(10.0, 10.0, 1.0, 1.0).expect("partial rectangle is valid")),
        SceneLayerKey::new(2),
    ));
    plan.push_drop_occlusion(DropOcclusionRecord::new(
        FloatingPresentationId::new(22),
        HitRegion::new(
            LogicalRect::new(30.0, 30.0, 200.0, 160.0).expect("contained rectangle is valid"),
        ),
        SceneLayerKey::new(3),
    ));
    publish_rootless_minimums(&mut plan);

    assert!(matches!(
        validate(&workspace, plan),
        Err(SceneBuildError::DropOcclusionGeometryMismatch {
            surface: rejected,
            floating,
            ..
        }) if rejected == surface && floating == FloatingPresentationId::new(21)
    ));
}

#[test]
fn ready_surface_preserves_outside_occlusion_geometry() {
    let surface = SurfaceId::new(110);
    let workspace = rootless_workspace(surface);
    let expected = vec![
        LogicalRect::new(10.0, 10.0, 200.0, 160.0).expect("contained rectangle is valid"),
        LogicalRect::new(30.0, 30.0, 200.0, 160.0).expect("contained rectangle is valid"),
    ];

    for surface_bounds in [
        LogicalRect::new(0.0, 0.0, 100.0, 100.0).expect("partial bounds are valid"),
        LogicalRect::new(400.0, 400.0, 100.0, 100.0).expect("disjoint bounds are valid"),
    ] {
        let mut plan = PresentationPlan::new(surface, surface_bounds);
        plan.set_surface_background(background_in(surface, surface_bounds))
            .expect("background is valid");
        publish_rootless_facts(&mut plan);
        let plan = validate(&workspace, plan).expect("durable occlusions remain unclipped");
        assert_eq!(
            plan.drop_occlusions()
                .iter()
                .map(|occlusion| occlusion.region().rect())
                .collect::<Vec<_>>(),
            expected
        );
    }
}

#[test]
fn ready_surface_rejects_zero_area_occlusion() {
    let surface = SurfaceId::new(120);
    let root = RootId::new(121);
    let floating = FloatingPresentationId::new(122);
    let durable = LogicalRect::new(10.0, 10.0, 200.0, 160.0).expect("durable rectangle is valid");
    let zero = LogicalRect::new(10.0, 10.0, 0.0, 160.0).expect("zero rectangle is representable");
    let mut builder = Workspace::builder();
    let tabs = builder.insert_node(Node::tabs([ItemId::new(123)]));
    builder.set_root(root, RootRecord::new(tabs));
    builder.set_contained_floating(floating, ContainedFloating::new(root, durable));
    builder.set_surface(surface, SurfacePresentation::rootless());
    builder
        .attach_contained(surface, floating)
        .expect("rootless surface exists");
    let workspace = builder.build().expect("workspace is valid");
    let mut plan = PresentationPlan::new(surface, bounds());
    plan.set_surface_background(background(surface))
        .expect("background is valid");
    plan.push_drop_occlusion(DropOcclusionRecord::new(
        floating,
        HitRegion::new(zero),
        SceneLayerKey::new(2),
    ));
    plan.push_contained_minimum(contained_minimum(floating));

    assert_eq!(
        validate(&workspace, plan),
        Err(SceneBuildError::EmptyDropOcclusionRegion { surface, floating })
    );
}

#[test]
fn ready_surface_requires_exact_contained_minimum_coverage() {
    let surface = SurfaceId::new(130);
    let workspace = rootless_workspace(surface);
    let mut plan = PresentationPlan::new(surface, bounds());
    plan.set_surface_background(background(surface))
        .expect("background is valid");
    publish_rootless_occlusions(&mut plan);
    plan.push_contained_minimum(contained_minimum(FloatingPresentationId::new(21)));

    assert_eq!(
        validate(&workspace, plan),
        Err(SceneBuildError::MissingContainedMinimumMeasurement {
            surface,
            floating: FloatingPresentationId::new(22),
        })
    );
}

#[test]
fn duplicate_contained_minimum_has_stable_identity_winner() {
    let surface = SurfaceId::new(140);
    let lower = FloatingPresentationId::new(21);
    let higher = FloatingPresentationId::new(22);

    for order in [
        [higher, higher, lower, lower],
        [lower, lower, higher, higher],
    ] {
        let mut plan = PresentationPlan::new(surface, bounds());
        for floating in order {
            plan.push_contained_minimum(contained_minimum(floating));
        }
        assert_eq!(
            validate(&Workspace::default(), plan),
            Err(SceneBuildError::DuplicateContainedMinimumMeasurement { floating: lower })
        );
    }
}

#[test]
fn unexpected_contained_minimum_has_stable_identity_winner() {
    let surface = SurfaceId::new(150);
    let workspace = rootless_workspace(surface);
    let lower = FloatingPresentationId::new(151);
    let higher = FloatingPresentationId::new(152);

    for order in [[higher, lower], [lower, higher]] {
        let mut plan = PresentationPlan::new(surface, bounds());
        plan.set_surface_background(background(surface))
            .expect("background is valid");
        publish_rootless_facts(&mut plan);
        for floating in order {
            plan.push_contained_minimum(contained_minimum(floating));
        }
        assert_eq!(
            validate(&workspace, plan),
            Err(SceneBuildError::UnexpectedContainedMinimumMeasurement {
                surface,
                floating: lower,
            })
        );
    }
}

#[test]
fn ready_surface_rejects_minimum_owned_by_another_surface() {
    let surface = SurfaceId::new(160);
    let other_surface = SurfaceId::new(161);
    let own_root = RootId::new(162);
    let other_root = RootId::new(163);
    let own_floating = FloatingPresentationId::new(164);
    let other_floating = FloatingPresentationId::new(165);
    let own_rect = LogicalRect::new(10.0, 10.0, 200.0, 160.0).expect("own rectangle is valid");
    let other_rect = LogicalRect::new(30.0, 30.0, 200.0, 160.0).expect("other rectangle is valid");
    let mut builder = Workspace::builder();
    let own_tabs = builder.insert_node(Node::tabs([ItemId::new(166)]));
    let other_tabs = builder.insert_node(Node::tabs([ItemId::new(167)]));
    builder.set_root(own_root, RootRecord::new(own_tabs));
    builder.set_root(other_root, RootRecord::new(other_tabs));
    builder.set_contained_floating(own_floating, ContainedFloating::new(own_root, own_rect));
    builder.set_contained_floating(
        other_floating,
        ContainedFloating::new(other_root, other_rect),
    );
    builder.set_surface(surface, SurfacePresentation::rootless());
    builder.set_surface(other_surface, SurfacePresentation::rootless());
    builder
        .attach_contained(surface, own_floating)
        .expect("source surface exists");
    builder
        .attach_contained(other_surface, other_floating)
        .expect("other surface exists");
    let workspace = builder.build().expect("workspace is valid");

    let mut plan = PresentationPlan::new(surface, bounds());
    plan.set_surface_background(background(surface))
        .expect("background is valid");
    plan.push_drop_occlusion(DropOcclusionRecord::new(
        own_floating,
        HitRegion::new(own_rect),
        SceneLayerKey::new(2),
    ));
    plan.push_contained_minimum(contained_minimum(own_floating));
    plan.push_contained_minimum(contained_minimum(other_floating));

    assert_eq!(
        validate(&workspace, plan),
        Err(SceneBuildError::UnexpectedContainedMinimumMeasurement {
            surface,
            floating: other_floating,
        })
    );
}

#[test]
fn zero_contained_minimum_is_preserved_and_canonicalized() {
    let surface = SurfaceId::new(170);
    let workspace = rootless_workspace(surface);
    let zero = LogicalSize::new(0.0, 0.0).expect("zero minimum is representable");
    let mut plan = PresentationPlan::new(surface, bounds());
    plan.set_surface_background(background(surface))
        .expect("background is valid");
    publish_rootless_occlusions(&mut plan);
    plan.push_contained_minimum(contained_minimum(FloatingPresentationId::new(22)));
    plan.push_contained_minimum(ContainedMinimumMeasurement::new(
        FloatingPresentationId::new(21),
        zero,
    ));

    let plan = validate(&workspace, plan).expect("zero minimum is valid");
    assert_eq!(
        plan.contained_minimums(),
        [
            ContainedMinimumMeasurement::new(FloatingPresentationId::new(21), zero),
            contained_minimum(FloatingPresentationId::new(22)),
        ]
    );
}
