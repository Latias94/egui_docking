use std::sync::Arc;

use super::*;
use crate::engine::{DockEngine, PreparedSurfacePaintCandidate};
use crate::geometry::{LogicalRect, LogicalSize};
use crate::graph::command::{CommandOutcome, DockFraction, Edge};
use crate::graph::{
    Axis, ContainedFloating, Node, RootRecord, SurfacePresentation, WorkspaceBuilder,
};
use crate::ids::{ItemId, NodeId, WorkspaceEpoch};
use crate::interaction::targeting::guide::{
    DropGuideEdgeSet, DropGuideScope, DropGuideSlot, DropGuideTargetRecord,
};
use crate::interaction::targeting::targets::{
    DropOcclusionRecord, DropTargetAvailability, DropTargetUnavailable, SurfaceBackground,
};
use crate::platform::viewport::CoordinateGeneration;
use crate::policy::{DockPolicy, DockPresentationMode, PolicyRevision};
use crate::presentation::manifest::{
    Measurement, SurfaceMeasurementTicket, SurfaceMeasurements, TabIntrinsic, TabStripMetrics,
};
use crate::presentation::observation::{PresentationOutputSerial, PresentedSurfaceAuthority};
use crate::presentation::scene::{
    ContainedMinimumMeasurement, PresentationPlanValidator, SurfaceCoordinateCapture,
};

const SOURCE_SURFACE: SurfaceId = SurfaceId::new(1);
const COMPLETE_SURFACE: SurfaceId = SurfaceId::new(2);
const TARGET_SURFACE: SurfaceId = SurfaceId::new(3);
const CROSS_TARGET_SURFACE: SurfaceId = SurfaceId::new(4);
const SOURCE_ROOT: RootId = RootId::new(11);
const COMPLETE_ROOT: RootId = RootId::new(12);
const CONTAINED_ROOT: RootId = RootId::new(13);
const OFFERED_ROOT: RootId = RootId::new(14);
const CROSS_CONTAINED_ROOT: RootId = RootId::new(15);
const CONTAINED: FloatingPresentationId = FloatingPresentationId::new(21);
const CROSS_CONTAINED: FloatingPresentationId = FloatingPresentationId::new(22);
const PROMOTION_BACK_ROOT: RootId = RootId::new(31);
const PROMOTION_ROOT: RootId = RootId::new(32);
const PROMOTION_FRONT_ROOT: RootId = RootId::new(33);
const PROMOTION_BACK: FloatingPresentationId = FloatingPresentationId::new(41);
const PROMOTION_FLOATING: FloatingPresentationId = FloatingPresentationId::new(42);
const PROMOTION_FRONT: FloatingPresentationId = FloatingPresentationId::new(43);

struct Fixture {
    workspace: Workspace,
    source_tabs: NodeId,
    complete_tabs: NodeId,
    contained_tabs: NodeId,
}

fn rect() -> LogicalRect {
    LogicalRect::new(0.0, 0.0, 800.0, 600.0).expect("test rectangle must be valid")
}

fn contained_rect() -> LogicalRect {
    LogicalRect::new(500.0, 350.0, 200.0, 150.0).expect("contained rectangle must be valid")
}

fn cross_contained_rect() -> LogicalRect {
    LogicalRect::new(600.0, 50.0, 140.0, 120.0).expect("cross contained rect must be valid")
}

fn contained_minimum(floating: FloatingPresentationId) -> ContainedMinimumMeasurement {
    ContainedMinimumMeasurement::new(
        floating,
        LogicalSize::new(120.0, 90.0).expect("contained minimum must be valid"),
    )
}

fn fixture() -> Fixture {
    let mut builder = Workspace::builder();
    let source_tabs = builder.insert_node(Node::tabs([ItemId::new(1), ItemId::new(2)]));
    let source_peer = builder.insert_node(Node::tabs([ItemId::new(5)]));
    let source_root = builder.insert_node(
        Node::equal_split(Axis::Horizontal, [source_tabs, source_peer])
            .expect("source split must be valid"),
    );
    let complete_tabs = builder.insert_node(Node::tabs([ItemId::new(3)]));
    let contained_tabs = builder.insert_node(Node::tabs([ItemId::new(4)]));
    let cross_contained_tabs = builder.insert_node(Node::tabs([ItemId::new(6)]));
    builder.set_root(SOURCE_ROOT, RootRecord::new(source_root));
    builder.set_root(COMPLETE_ROOT, RootRecord::new(complete_tabs));
    builder.set_root(CONTAINED_ROOT, RootRecord::new(contained_tabs));
    builder.set_root(CROSS_CONTAINED_ROOT, RootRecord::new(cross_contained_tabs));
    builder.set_surface(SOURCE_SURFACE, SurfacePresentation::with_main(SOURCE_ROOT));
    builder.set_surface(
        COMPLETE_SURFACE,
        SurfacePresentation::with_main(COMPLETE_ROOT),
    );
    builder.set_surface(TARGET_SURFACE, SurfacePresentation::rootless());
    builder.set_surface(CROSS_TARGET_SURFACE, SurfacePresentation::rootless());
    builder.set_contained_floating(
        CONTAINED,
        ContainedFloating::new(CONTAINED_ROOT, contained_rect()),
    );
    builder.set_contained_floating(
        CROSS_CONTAINED,
        ContainedFloating::new(CROSS_CONTAINED_ROOT, cross_contained_rect()),
    );
    builder
        .attach_contained(TARGET_SURFACE, CONTAINED)
        .expect("target surface exists");
    builder
        .attach_contained(CROSS_TARGET_SURFACE, CROSS_CONTAINED)
        .expect("cross target surface exists");
    Fixture {
        workspace: builder.build().expect("fixture workspace must be valid"),
        source_tabs,
        complete_tabs,
        contained_tabs,
    }
}

fn session() -> DragSessionId {
    DragSessionId::new(
        WorkspaceEpoch::new(1),
        crate::interaction::DragGeneration::new(1),
    )
}

fn bind_measurement_ticket(
    plan: &PresentationPlan,
    ticket: SurfaceMeasurementTicket,
) -> PresentationPlan {
    plan.rebind_measurement_ticket_for_test(ticket)
}

fn fixture_measurements(
    engine: &DockEngine,
    surface: SurfaceId,
    bounds: LogicalRect,
) -> SurfaceMeasurements {
    let requirements = engine
        .presentation_requirements()
        .surface(surface)
        .expect("fixture surface requirements must exist");
    let mut measurements = SurfaceMeasurements::new(requirements.ticket());
    measurements
        .set_bounds(requirements.bounds(), Measurement::Measured(bounds))
        .expect("fixture surface bounds answer must be unique");
    if let Some(key) = requirements.popup_plane_bounds() {
        measurements
            .set_popup_plane_bounds(key, Measurement::Measured(bounds))
            .expect("fixture popup-plane bounds answer must be unique");
    }
    let minimum = LogicalSize::new(0.0, 0.0).expect("fixture minimum must be valid");
    for key in requirements.pane_minimums() {
        measurements
            .insert_pane_minimum(key, Measurement::Measured(minimum))
            .expect("fixture pane minimum answer must be unique");
    }
    for key in requirements.tab_intrinsics() {
        measurements
            .insert_tab_intrinsic(
                key,
                Measurement::Measured(
                    TabIntrinsic::new(56.0).expect("fixture tab intrinsic must be valid"),
                ),
            )
            .expect("fixture tab intrinsic answer must be unique");
    }
    for key in requirements.tab_strips() {
        measurements
            .insert_tab_strip(
                key,
                Measurement::Measured(
                    TabStripMetrics::new(0.0, 0.0)
                        .expect("fixture tab strip metrics must be valid"),
                ),
            )
            .expect("fixture tab strip answer must be unique");
    }
    measurements
}

fn compile_fixture_surface_plan(
    engine: &DockEngine,
    surface: SurfaceId,
    bounds: LogicalRect,
) -> PresentationPlan {
    let token = engine
        .begin_surface_contribution(surface)
        .expect("fixture surface must be in the presentation roster");
    let contribution = engine
        .prepare_surface_contribution(token, fixture_measurements(engine, surface, bounds))
        .expect("fixture measurements must compile through the production path");
    match contribution.paint_candidate() {
        PreparedSurfacePaintCandidate::Ready(candidate) => candidate.plan().clone(),
        PreparedSurfacePaintCandidate::Retained
        | PreparedSurfacePaintCandidate::Unavailable { .. } => {
            panic!("fixture measurements must produce a ready paint candidate")
        }
    }
}

fn compile_fixture_surface_plan_without_guides(
    engine: &DockEngine,
    surface: SurfaceId,
    bounds: LogicalRect,
) -> PresentationPlan {
    let plan = crate::presentation::compiler::compile_surface_measurements_without_guides(
        engine.workspace(),
        engine.version(),
        engine.policy_snapshot(),
        engine.presentation_config(),
        engine.presentation_requirements(),
        &fixture_measurements(engine, surface, bounds),
        &crate::presentation::tab_strip::TabStripStateStore::default(),
        &[],
        None,
    )
    .expect("guide-disabled fixture measurements must compile");
    PresentationPlanValidator::new(engine.workspace(), engine.policy_snapshot())
        .expect("guide-disabled fixture validator must initialize")
        .validate_and_canonicalize(plan)
        .expect("guide-disabled fixture plan must validate")
}

fn ready_background(extra: Option<DropTargetRecord>) -> PresentationPlan {
    ready_background_in(rect(), extra)
}

fn ready_background_in(
    surface_bounds: LogicalRect,
    extra: Option<DropTargetRecord>,
) -> PresentationPlan {
    let mut ready = PresentationPlan::new(TARGET_SURFACE, surface_bounds);
    ready
        .set_surface_background(DropTargetRecord::surface_background(
            SurfaceBackground::new(TARGET_SURFACE),
            HitRegion::new(surface_bounds),
            DropVisual::new(surface_bounds),
        ))
        .expect("background record is valid");
    ready.push_drop_occlusion(DropOcclusionRecord::new(
        CONTAINED,
        HitRegion::new(contained_rect()),
        SceneLayerKey::new(2),
    ));
    ready.push_contained_minimum(contained_minimum(CONTAINED));
    if let Some(extra) = extra {
        ready.push_drop_target(extra.into());
    }
    ready
}

fn cross_surface_background(extra: Option<DropTargetRecord>) -> PresentationPlan {
    let mut ready = PresentationPlan::new(CROSS_TARGET_SURFACE, rect());
    ready
        .set_surface_background(DropTargetRecord::surface_background(
            SurfaceBackground::new(CROSS_TARGET_SURFACE),
            HitRegion::new(rect()),
            DropVisual::new(rect()),
        ))
        .expect("background record is valid");
    ready.push_drop_occlusion(DropOcclusionRecord::new(
        CROSS_CONTAINED,
        HitRegion::new(cross_contained_rect()),
        SceneLayerKey::new(2),
    ));
    ready.push_contained_minimum(contained_minimum(CROSS_CONTAINED));
    if let Some(extra) = extra {
        ready.push_drop_target(extra.into());
    }
    ready
}

fn seal(fixture: &Fixture, ready: PresentationPlan) -> SurfaceSceneSet {
    seal_workspace(&fixture.workspace, ready)
}

fn seal_workspace(workspace: &Workspace, ready: PresentationPlan) -> SurfaceSceneSet {
    let surface = ready.surface();
    let policy = DockPolicySnapshot::default();
    let mut ready = ready;
    if ready.surface_program().blocks().is_empty() {
        ready
            .finish_surface_program()
            .expect("synthetic fixture program should finish exactly once");
    }
    let ready = PresentationPlanValidator::new(workspace, &policy)
        .expect("fixture validator must initialize")
        .validate_and_canonicalize(ready)
        .expect("scene must validate");
    let engine =
        DockEngine::new(workspace.clone(), policy.to_policy()).expect("fixture engine is valid");
    let mut scenes = engine.scene().clone();
    let roster: Vec<_> = engine
        .presentation_requirements()
        .surfaces()
        .map(|(surface, requirements)| (surface, requirements.ticket()))
        .collect();
    let mut outputs = Vec::with_capacity(roster.len());
    for (index, (rostered_surface, ticket)) in roster.iter().copied().enumerate() {
        let plan = if rostered_surface == surface {
            bind_measurement_ticket(&ready, ticket)
        } else {
            compile_fixture_surface_plan(&engine, rostered_surface, ready.bounds())
        };
        let serial = u64::try_from(index + 1).expect("fixture presentation serial must fit");
        let (_stamp, output_ticket) = scenes
            .install_ready(
                plan,
                SurfaceCoordinateCapture::Headless {
                    authority_generation: CoordinateGeneration::default(),
                },
                ticket.authority_domain(),
                PresentationOutputSerial::new_for_test(serial),
            )
            .expect("validated candidate must install");
        outputs.push((output_ticket, serial));
    }
    for (output_ticket, serial) in outputs {
        let authority = PresentedSurfaceAuthority::mint_observed_for_test_with_emission(
            output_ticket,
            CoordinateGeneration::default(),
            serial,
        );
        assert!(scenes.accept_observed_authority(authority).is_ok());
    }
    scenes
}

fn partial_payload(fixture: &Fixture) -> MovePayload {
    MovePayload::Item(
        fixture
            .workspace
            .capture_item_source(SOURCE_ROOT, fixture.source_tabs, ItemId::new(1))
            .expect("partial source must be current"),
    )
}

fn guide_target_record(
    targets: &mut Vec<Arc<DropTargetRecord>>,
    location: DropTargetLocation,
) -> DropGuideTargetRecord {
    let target = Arc::new(DropTargetRecord::new(
        location,
        DropTargetAvailability::Available,
        HitRegion::new(rect()),
        SceneLayerKey::new(2),
        DropVisual::new(rect()),
    ));
    targets.push(Arc::clone(&target));
    DropGuideTargetRecord::new(target, rect())
}

fn complete_guide_clusters(
    fixture: &Fixture,
) -> (Vec<Arc<DropTargetRecord>>, [DropGuideClusterRecord; 2]) {
    let fraction = DockFraction::new(0.35).expect("guide fraction must be valid");
    let mut targets = Vec::new();
    let center = guide_target_record(
        &mut targets,
        DropTargetLocation::Center {
            surface: TARGET_SURFACE,
            root: CONTAINED_ROOT,
            tabs: fixture.contained_tabs,
        },
    );
    let inner_edges = {
        let mut inner_edge = |edge| {
            guide_target_record(
                &mut targets,
                DropTargetLocation::InnerEdge {
                    surface: TARGET_SURFACE,
                    root: CONTAINED_ROOT,
                    node: fixture.contained_tabs,
                    edge,
                    fraction,
                },
            )
        };
        DropGuideEdgeSet::new(
            inner_edge(Edge::Left),
            inner_edge(Edge::Right),
            inner_edge(Edge::Top),
            inner_edge(Edge::Bottom),
        )
    };
    let outer_edges = {
        let mut outer_edge = |edge| {
            guide_target_record(
                &mut targets,
                DropTargetLocation::OuterEdge {
                    surface: TARGET_SURFACE,
                    root: CONTAINED_ROOT,
                    node: fixture.contained_tabs,
                    edge,
                    fraction,
                },
            )
        };
        DropGuideEdgeSet::new(
            outer_edge(Edge::Left),
            outer_edge(Edge::Right),
            outer_edge(Edge::Top),
            outer_edge(Edge::Bottom),
        )
    };
    let inner = DropGuideClusterRecord::inner(
        TARGET_SURFACE,
        CONTAINED_ROOT,
        fixture.contained_tabs,
        HitRegion::new(rect()),
        SceneLayerKey::new(2),
        center,
        inner_edges,
    );
    let outer = DropGuideClusterRecord::outer(
        TARGET_SURFACE,
        CONTAINED_ROOT,
        HitRegion::new(rect()),
        SceneLayerKey::new(2),
        outer_edges,
    );
    (targets, [outer, inner])
}

fn install_complete_guide_targets(
    fixture: &Fixture,
    mut ready: PresentationPlan,
) -> (PresentationPlan, [DropGuideClusterRecord; 2]) {
    let (targets, clusters) = complete_guide_clusters(fixture);
    for target in targets {
        ready.push_drop_target(target);
    }
    (ready, clusters)
}

#[test]
fn complete_guide_affordance_preflights_only_the_unique_winner() {
    let fixture = fixture();
    let (ready, clusters) = install_complete_guide_targets(&fixture, ready_background(None));
    let scene = seal(&fixture, ready);
    let stamp = scene
        .ready_surface(TARGET_SURFACE)
        .expect("target scene must be ready")
        .stamp();
    let payload = partial_payload(&fixture);
    let policy = DockPolicySnapshot::default();
    let workspace_version = WorkspaceVersion::initial();
    let workspace_index = WorkspaceIndex::build(&fixture.workspace, workspace_version)
        .expect("fixture index must build");
    let assessment = DropEligibilityContext::new(
        &fixture.workspace,
        workspace_version,
        &workspace_index,
        &policy,
        scene
            .ready_surface(TARGET_SURFACE)
            .expect("target scene must be ready")
            .plan(),
        None,
        &payload,
        None,
    );

    reset_resolution_work();
    let (_, exact) = build_affordance(
        stamp,
        TARGET_SURFACE,
        LogicalPoint::new(10.0, 10.0).expect("point must be finite"),
        clusters.iter().collect(),
        &assessment,
    )
    .expect("guide resolution cannot expose an invariant");

    assert!(matches!(
        exact,
        Some(ExactGuidePreparation::Eligible { .. })
    ));
    let work = resolution_work();
    assert_eq!(work.drop_targets_assessed, 9);
    assert_eq!(work.geometric_winners, 1);
    assert_eq!(work.transaction_prepares, 1);
    assert_eq!(work.transaction_commands, 1);
    assert_eq!(work.workspace_deep_clones.transaction_candidates.calls, 1);
    // The winning command uses actual root issuance and builds no value proof.
    assert_eq!(work.root_fingerprint_builds, 0);
    assert_eq!(work.root_fingerprint_node_visits, 0);
}

#[test]
fn rejected_geometric_winner_never_falls_through_or_preflights() {
    let fixture = fixture();
    let (ready, clusters) = install_complete_guide_targets(&fixture, ready_background(None));
    let scene = seal(&fixture, ready);
    let stamp = scene
        .ready_surface(TARGET_SURFACE)
        .expect("target scene must be ready")
        .stamp();
    let payload = partial_payload(&fixture);
    let mut policy = DockPolicy::default();
    policy.set_allow_edge_split(false);
    let policy = policy.snapshot(PolicyRevision::default());
    let workspace_version = WorkspaceVersion::initial();
    let workspace_index = WorkspaceIndex::build(&fixture.workspace, workspace_version)
        .expect("fixture index must build");
    let assessment = DropEligibilityContext::new(
        &fixture.workspace,
        workspace_version,
        &workspace_index,
        &policy,
        scene
            .ready_surface(TARGET_SURFACE)
            .expect("target scene must be ready")
            .plan(),
        None,
        &payload,
        None,
    );

    reset_resolution_work();
    let (_, exact) = build_affordance(
        stamp,
        TARGET_SURFACE,
        LogicalPoint::new(10.0, 10.0).expect("point must be finite"),
        clusters.iter().collect(),
        &assessment,
    )
    .expect("guide resolution cannot expose an invariant");

    assert!(matches!(
        exact,
        Some(ExactGuidePreparation::Rejected {
            target: DropTargetId::OuterEdge {
                edge: Edge::Left,
                ..
            },
            reason: DropRejectionReason::Policy(PolicyRejection::EdgeSplitDisabled),
            ..
        })
    ));
    let work = resolution_work();
    assert_eq!(work.drop_targets_assessed, 9);
    assert_eq!(work.geometric_winners, 1);
    assert_eq!(work.transaction_prepares, 0);
    assert_eq!(work.transaction_commands, 0);
    assert_eq!(work.workspace_deep_clones.transaction_candidates.calls, 0);
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WorkloadDistribution {
    Balanced,
    HotRootSkew,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ResolutionWorkloadSpec {
    panes: usize,
    surfaces: usize,
    distribution: WorkloadDistribution,
}

struct ResolutionWorkload {
    workspace: Workspace,
    workspace_version: WorkspaceVersion,
    workspace_index: WorkspaceIndex,
    scene: SurfaceSceneSet,
    payload: MovePayload,
    offer: SurfaceBackgroundRootOffer,
    expected_clone_volume: structural_work::WorkspaceCloneVolume,
    source_root_nodes: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ResolutionWorkBaseline {
    spec: ResolutionWorkloadSpec,
    root_fingerprint_builds: usize,
    root_fingerprint_node_visits: usize,
    source_root_nodes: usize,
    transaction_clone_nodes: usize,
}

fn pane_distribution(spec: ResolutionWorkloadSpec) -> Vec<usize> {
    assert!(spec.surfaces > 0);
    assert!(spec.panes >= spec.surfaces * 2);
    match spec.distribution {
        WorkloadDistribution::Balanced => {
            let base = spec.panes / spec.surfaces;
            let remainder = spec.panes % spec.surfaces;
            (0..spec.surfaces)
                .map(|index| base + usize::from(index < remainder))
                .collect()
        }
        WorkloadDistribution::HotRootSkew => {
            let mut counts = vec![2; spec.surfaces];
            counts[0] += spec.panes - spec.surfaces * 2;
            counts
        }
    }
}

fn insert_balanced_root(
    builder: &mut WorkspaceBuilder,
    pane_count: usize,
    next_item: &mut u64,
) -> (NodeId, NodeId, ItemId) {
    let mut level = Vec::with_capacity(pane_count);
    let mut source = None;
    for _ in 0..pane_count {
        let item = ItemId::new(*next_item);
        *next_item += 1;
        let tabs = builder.insert_node(Node::tabs([item]));
        source.get_or_insert((tabs, item));
        level.push(tabs);
    }

    let mut depth = 0;
    while level.len() > 1 {
        let mut parents = Vec::with_capacity(level.len().div_ceil(2));
        for pair in level.chunks(2) {
            if let [left, right] = pair {
                let axis = if depth % 2 == 0 {
                    Axis::Horizontal
                } else {
                    Axis::Vertical
                };
                parents.push(
                    builder.insert_node(
                        Node::equal_split(axis, [*left, *right])
                            .expect("balanced workload split must be valid"),
                    ),
                );
            } else {
                parents.push(pair[0]);
            }
        }
        level = parents;
        depth += 1;
    }

    let (source_tabs, source_item) = source.expect("workload root must contain a pane");
    (level[0], source_tabs, source_item)
}

fn build_resolution_workload(spec: ResolutionWorkloadSpec) -> ResolutionWorkload {
    let pane_counts = pane_distribution(spec);
    let mut builder = Workspace::builder();
    let target_surface = TARGET_SURFACE;
    builder.set_surface(target_surface, SurfacePresentation::rootless());

    let mut next_item = 1_u64;
    let mut source = None;
    for (index, pane_count) in pane_counts.iter().copied().enumerate() {
        let index = u64::try_from(index).expect("workload index must fit u64");
        let root = RootId::new(100 + index);
        let (root_node, source_tabs, source_item) =
            insert_balanced_root(&mut builder, pane_count, &mut next_item);
        builder.set_root(root, RootRecord::new(root_node));
        if index == 0 {
            let floating = FloatingPresentationId::new(100);
            builder
                .set_contained_floating(floating, ContainedFloating::new(root, contained_rect()));
            builder
                .attach_contained(target_surface, floating)
                .expect("target surface must exist");
            source = Some((root, source_tabs, source_item));
        } else {
            builder.set_surface(
                SurfaceId::new(100 + index),
                SurfacePresentation::with_main(root),
            );
        }
    }
    let workspace = builder.build().expect("scale workload must be valid");
    let (source_root, source_tabs, source_item) =
        source.expect("scale workload must contain a source root");
    let payload = MovePayload::Item(
        workspace
            .capture_item_source(source_root, source_tabs, source_item)
            .expect("scale workload source must be current"),
    );
    let source_root_nodes = match &payload {
        MovePayload::Item(source) => workspace
            .root_fingerprint(source.root())
            .unwrap()
            .0
            .nodes
            .len(),
        MovePayload::Tabs(_) | MovePayload::Subtree(_) => {
            unreachable!("the scale workload always moves one item")
        }
    };
    let expected_clone_volume = structural_work::WorkspaceCloneVolume::capture(&workspace);

    let mut ready = PresentationPlan::new(target_surface, rect());
    ready
        .set_surface_background(DropTargetRecord::surface_background(
            SurfaceBackground::new(target_surface),
            HitRegion::new(rect()),
            DropVisual::new(rect()),
        ))
        .expect("scale workload background must be unique");
    ready.push_drop_occlusion(DropOcclusionRecord::new(
        FloatingPresentationId::new(100),
        HitRegion::new(contained_rect()),
        SceneLayerKey::new(2),
    ));
    ready.push_contained_minimum(contained_minimum(FloatingPresentationId::new(100)));
    let scene = seal_workspace(&workspace, ready);
    let workspace_version = WorkspaceVersion::initial();
    let workspace_index = WorkspaceIndex::build(&workspace, workspace_version)
        .expect("scale workload index must build");

    ResolutionWorkload {
        workspace,
        workspace_version,
        workspace_index,
        scene,
        payload,
        offer: SurfaceBackgroundRootOffer::new(RootId::new(1_000_000)),
        expected_clone_volume,
        source_root_nodes,
    }
}

fn run_resolution_workload(spec: ResolutionWorkloadSpec) -> ResolutionWorkBaseline {
    let workload = build_resolution_workload(spec);
    reset_resolution_work();

    let ready = workload
        .scene
        .ready_surface(TARGET_SURFACE)
        .expect("scale workload target must be presented");
    let query = resolve_presented_drop(
        ready.stamp(),
        ready.plan(),
        None,
        &workload.workspace,
        workload.workspace_version,
        &workload.workspace_index,
        &DockPolicySnapshot::default(),
        session(),
        workload.payload,
        Some(workload.offer),
        LogicalPoint::new(10.0, 10.0).expect("workload point must be finite"),
    )
    .expect("scale workload must not expose an invariant");
    let DropResolution::Resolved(resolved) = query.resolution() else {
        panic!("scale workload must resolve its background target");
    };
    assert!(matches!(
        resolved.command(),
        WorkspaceCommand::InstallMainRoot { .. }
    ));

    let work = resolution_work();
    assert_eq!(work.drop_targets_assessed, 1);
    assert_eq!(work.geometric_winners, 1);
    assert_eq!(work.transaction_prepares, 1);
    assert_eq!(work.transaction_commands, 1);
    assert_eq!(work.workspace_deep_clones.transaction_candidates.calls, 1);
    let clone_volume = work.workspace_deep_clones.transaction_candidates.volume;
    assert_eq!(clone_volume, workload.expected_clone_volume);
    assert_eq!(clone_volume.tab_items, spec.panes);
    assert_eq!(clone_volume.tab_mru_items, spec.panes);
    assert_eq!(clone_volume.roots, spec.surfaces);
    assert_eq!(clone_volume.surfaces, spec.surfaces);
    assert_eq!(clone_volume.contained_records, 1);
    assert_eq!(clone_volume.contained_roster_entries, 1);
    // Closed command writers and canonical rewrite effects issue authority
    // directly. The actual prepared geometry remains independently owned.
    assert_eq!(work.root_fingerprint_builds, 0);
    assert_eq!(work.root_fingerprint_node_visits, 0);
    assert_eq!(
        resolved.mutation.candidate.nodes().count(),
        clone_volume.nodes - 1
    );

    ResolutionWorkBaseline {
        spec,
        root_fingerprint_builds: work.root_fingerprint_builds,
        root_fingerprint_node_visits: work.root_fingerprint_node_visits,
        source_root_nodes: workload.source_root_nodes,
        transaction_clone_nodes: work
            .workspace_deep_clones
            .transaction_candidates
            .volume
            .nodes,
    }
}

#[test]
fn structural_drop_work_has_deterministic_scale_baselines() {
    let specs = [
        ResolutionWorkloadSpec {
            panes: 16,
            surfaces: 1,
            distribution: WorkloadDistribution::Balanced,
        },
        ResolutionWorkloadSpec {
            panes: 128,
            surfaces: 8,
            distribution: WorkloadDistribution::Balanced,
        },
        ResolutionWorkloadSpec {
            panes: 128,
            surfaces: 8,
            distribution: WorkloadDistribution::HotRootSkew,
        },
        ResolutionWorkloadSpec {
            panes: 1_024,
            surfaces: 32,
            distribution: WorkloadDistribution::Balanced,
        },
        ResolutionWorkloadSpec {
            panes: 1_024,
            surfaces: 32,
            distribution: WorkloadDistribution::HotRootSkew,
        },
    ];
    let baselines = specs.map(run_resolution_workload);

    assert_eq!(
        baselines,
        [
            ResolutionWorkBaseline {
                spec: specs[0],
                root_fingerprint_builds: 0,
                root_fingerprint_node_visits: 0,
                source_root_nodes: 31,
                transaction_clone_nodes: 31,
            },
            ResolutionWorkBaseline {
                spec: specs[1],
                root_fingerprint_builds: 0,
                root_fingerprint_node_visits: 0,
                source_root_nodes: 31,
                transaction_clone_nodes: 248,
            },
            ResolutionWorkBaseline {
                spec: specs[2],
                root_fingerprint_builds: 0,
                root_fingerprint_node_visits: 0,
                source_root_nodes: 226,
                transaction_clone_nodes: 247,
            },
            ResolutionWorkBaseline {
                spec: specs[3],
                root_fingerprint_builds: 0,
                root_fingerprint_node_visits: 0,
                source_root_nodes: 63,
                transaction_clone_nodes: 2_016,
            },
            ResolutionWorkBaseline {
                spec: specs[4],
                root_fingerprint_builds: 0,
                root_fingerprint_node_visits: 0,
                source_root_nodes: 1_922,
                transaction_clone_nodes: 2_015,
            },
        ]
    );
    assert!(baselines[2].source_root_nodes > baselines[1].source_root_nodes);
    assert!(baselines[4].source_root_nodes > baselines[3].source_root_nodes);
    assert!(baselines[3].transaction_clone_nodes > baselines[1].transaction_clone_nodes);
}

fn complete_payloads(fixture: &Fixture) -> Vec<MovePayload> {
    let node_source = || {
        fixture
            .workspace
            .capture_node_source(COMPLETE_ROOT, fixture.complete_tabs)
            .expect("complete source must be current")
    };
    vec![
        MovePayload::Item(
            fixture
                .workspace
                .capture_item_source(COMPLETE_ROOT, fixture.complete_tabs, ItemId::new(3))
                .expect("complete item source must be current"),
        ),
        MovePayload::Tabs(node_source()),
        MovePayload::Subtree(node_source()),
    ]
}

fn contained_complete_payloads(fixture: &Fixture) -> Vec<MovePayload> {
    let node_source = || {
        fixture
            .workspace
            .capture_node_source(CONTAINED_ROOT, fixture.contained_tabs)
            .expect("complete contained source must be current")
    };
    vec![
        MovePayload::Item(
            fixture
                .workspace
                .capture_item_source(CONTAINED_ROOT, fixture.contained_tabs, ItemId::new(4))
                .expect("complete contained item source must be current"),
        ),
        MovePayload::Tabs(node_source()),
        MovePayload::Subtree(node_source()),
    ]
}

fn partial_payloads(fixture: &Fixture) -> Vec<MovePayload> {
    let node_source = || {
        fixture
            .workspace
            .capture_node_source(SOURCE_ROOT, fixture.source_tabs)
            .expect("partial node source must be current")
    };
    vec![
        partial_payload(fixture),
        MovePayload::Tabs(node_source()),
        MovePayload::Subtree(node_source()),
    ]
}

fn resolve(
    fixture: &Fixture,
    scene: &SurfaceSceneSet,
    payload: MovePayload,
    offer: Option<SurfaceBackgroundRootOffer>,
) -> DropResolution {
    resolve_unguided_drop(
        scene,
        &fixture.workspace,
        &DockPolicySnapshot::default(),
        session(),
        payload,
        offer,
        TARGET_SURFACE,
        LogicalPoint::new(10.0, 10.0).expect("point must be finite"),
    )
    .expect("resolution cannot expose an invariant")
}

fn assert_same_surface_promotion(
    workspace: &Workspace,
    scene: &SurfaceSceneSet,
    payload: MovePayload,
    offer: Option<SurfaceBackgroundRootOffer>,
) {
    let resolution = resolve_unguided_drop(
        scene,
        workspace,
        &DockPolicySnapshot::default(),
        session(),
        payload,
        offer,
        TARGET_SURFACE,
        LogicalPoint::new(10.0, 10.0).expect("point must be finite"),
    )
    .expect("promotion resolution cannot expose an invariant");
    let DropResolution::Resolved(resolved) = resolution else {
        panic!("same-surface complete root must resolve as a promotion");
    };
    assert_eq!(
        resolved.target_id(),
        DropTargetId::SurfaceBackground {
            surface: TARGET_SURFACE,
        }
    );
    assert!(matches!(
        resolved.command(),
        WorkspaceCommand::PromoteContained {
            source,
            surface: TARGET_SURFACE,
            floating: PROMOTION_FLOATING,
        } if source.root() == PROMOTION_ROOT
    ));

    let mut candidate = workspace.clone();
    let report = WorkspaceTransaction::from_commands([resolved.command().clone()])
        .apply(&mut candidate, &DockPolicySnapshot::default())
        .expect("resolved promotion must commit");
    assert_eq!(
        report.outcomes(),
        &[CommandOutcome::ContainedPromoted {
            surface: TARGET_SURFACE,
            root: PROMOTION_ROOT,
            floating: PROMOTION_FLOATING,
        }]
    );
    assert_eq!(
        candidate.surface(TARGET_SURFACE),
        Some(&SurfacePresentation {
            main_root: Some(PROMOTION_ROOT),
            contained: vec![PROMOTION_BACK, PROMOTION_FRONT],
        })
    );
    assert!(candidate.contained_floating(PROMOTION_FLOATING).is_none());
    assert!(candidate.contained_floating(PROMOTION_BACK).is_some());
    assert!(candidate.contained_floating(PROMOTION_FRONT).is_some());
    assert!(candidate.root(PROMOTION_ROOT).is_some());
}

#[test]
fn complete_root_background_delivery_preserves_root_identity() {
    let fixture = fixture();
    let scene = seal(&fixture, ready_background(None));
    for payload in complete_payloads(&fixture) {
        let DropResolution::Resolved(resolved) = resolve(&fixture, &scene, payload, None) else {
            panic!("every complete-root payload must resolve without an offer");
        };
        assert!(matches!(
            resolved.command(),
            WorkspaceCommand::RehomeRoot {
                target: RootPresentationTarget::Main {
                    surface: TARGET_SURFACE
                },
                ..
            }
        ));
        let mut candidate = fixture.workspace.clone();
        WorkspaceTransaction::from_commands([resolved.command().clone()])
            .apply(&mut candidate, &DockPolicySnapshot::default())
            .expect("resolved command must commit");
        assert_eq!(
            candidate
                .surface(TARGET_SURFACE)
                .expect("target remains")
                .main_root,
            Some(COMPLETE_ROOT)
        );
    }
}

#[test]
fn background_preflight_preserves_typed_policy_rejection() {
    let fixture = fixture();
    let scene = seal(&fixture, ready_background(None));
    let mut policy = DockPolicy::default();
    policy.set_allow_tiled_presentation(false);
    let policy = policy.snapshot(PolicyRevision::default());
    let resolution = resolve_unguided_drop(
        &scene,
        &fixture.workspace,
        &policy,
        session(),
        partial_payload(&fixture),
        Some(SurfaceBackgroundRootOffer::new(OFFERED_ROOT)),
        TARGET_SURFACE,
        LogicalPoint::new(10.0, 10.0).expect("point must be finite"),
    )
    .expect("a policy rejection is a normal resolution outcome");

    let DropResolution::Rejected(rejected) = resolution else {
        panic!("disabled tiled presentation must reject the background winner");
    };
    assert!(
        matches!(
            rejected.candidates(),
            [candidate]
            if matches!(
                candidate.reason(),
                DropRejectionReason::Policy(PolicyRejection::PresentationModeDisabled {
                    mode: DockPresentationMode::Tiled,
                })
            )
        ),
        "unexpected background rejection: {:?}",
        rejected.candidates()
    );
}

#[test]
fn same_surface_contained_complete_root_promotes_named_floating_atomically() {
    let back_rect = LogicalRect::new(300.0, 100.0, 120.0, 100.0).expect("valid back rect");
    let promoted_rect = LogicalRect::new(0.0, 0.0, 120.0, 100.0).expect("valid promoted rect");
    let front_rect = LogicalRect::new(500.0, 300.0, 120.0, 100.0).expect("valid front rect");
    let mut builder = Workspace::builder();
    let back_tabs = builder.insert_node(Node::tabs([ItemId::new(31)]));
    let promoted_tabs = builder.insert_node(Node::tabs([ItemId::new(32)]));
    let front_tabs = builder.insert_node(Node::tabs([ItemId::new(33)]));
    builder.set_root(PROMOTION_BACK_ROOT, RootRecord::new(back_tabs));
    builder.set_root(PROMOTION_ROOT, RootRecord::new(promoted_tabs));
    builder.set_root(PROMOTION_FRONT_ROOT, RootRecord::new(front_tabs));
    builder.set_surface(TARGET_SURFACE, SurfacePresentation::rootless());
    for (floating, root, contained_rect) in [
        (PROMOTION_BACK, PROMOTION_BACK_ROOT, back_rect),
        (PROMOTION_FLOATING, PROMOTION_ROOT, promoted_rect),
        (PROMOTION_FRONT, PROMOTION_FRONT_ROOT, front_rect),
    ] {
        builder.set_contained_floating(floating, ContainedFloating::new(root, contained_rect));
        builder
            .attach_contained(TARGET_SURFACE, floating)
            .expect("target surface exists");
    }
    let workspace = builder.build().expect("workspace must be valid");

    let mut ready = PresentationPlan::new(TARGET_SURFACE, rect());
    ready
        .set_surface_background(DropTargetRecord::surface_background(
            SurfaceBackground::new(TARGET_SURFACE),
            HitRegion::new(rect()),
            DropVisual::new(rect()),
        ))
        .expect("background record is valid");
    for (floating, contained_rect, layer) in [
        (PROMOTION_BACK, back_rect, 2),
        (PROMOTION_FLOATING, promoted_rect, 3),
        (PROMOTION_FRONT, front_rect, 4),
    ] {
        ready.push_drop_occlusion(DropOcclusionRecord::new(
            floating,
            HitRegion::new(contained_rect),
            SceneLayerKey::new(layer),
        ));
        ready.push_contained_minimum(contained_minimum(floating));
    }
    let scene = seal_workspace(&workspace, ready);
    let node_source = || {
        workspace
            .capture_node_source(PROMOTION_ROOT, promoted_tabs)
            .expect("complete contained source must be current")
    };
    let payloads = [
        MovePayload::Item(
            workspace
                .capture_item_source(PROMOTION_ROOT, promoted_tabs, ItemId::new(32))
                .expect("complete item source must be current"),
        ),
        MovePayload::Tabs(node_source()),
        MovePayload::Subtree(node_source()),
    ];
    let irrelevant_offers = [
        None,
        Some(SurfaceBackgroundRootOffer::new(PROMOTION_BACK_ROOT)),
        Some(SurfaceBackgroundRootOffer::new(PROMOTION_ROOT)),
    ];

    for payload in payloads {
        for offer in irrelevant_offers {
            assert_same_surface_promotion(&workspace, &scene, payload.clone(), offer);
        }
    }
}

#[test]
fn cross_surface_contained_item_tab_and_title_payloads_rehome_to_unique_background() {
    let fixture = fixture();
    let scene = seal_workspace(&fixture.workspace, cross_surface_background(None));

    for payload in contained_complete_payloads(&fixture) {
        let query = resolve_drop(
            &scene,
            &fixture.workspace,
            &DockPolicySnapshot::default(),
            session(),
            payload,
            None,
            CROSS_TARGET_SURFACE,
            LogicalPoint::new(10.0, 10.0).expect("point must be finite"),
        )
        .expect("cross-surface resolution cannot expose an invariant");
        assert!(query.affordance().is_none());
        let DropResolution::Resolved(resolved) = query.resolution() else {
            panic!("every complete contained payload must resolve as a rehome");
        };
        assert_eq!(
            resolved.target_id(),
            DropTargetId::SurfaceBackground {
                surface: CROSS_TARGET_SURFACE,
            }
        );
        assert!(matches!(
            resolved.command(),
            WorkspaceCommand::RehomeRoot {
                source,
                target: RootPresentationTarget::Main {
                    surface: CROSS_TARGET_SURFACE,
                },
            } if source.root() == CONTAINED_ROOT
        ));

        let mut candidate = fixture.workspace.clone();
        WorkspaceTransaction::from_commands([resolved.command().clone()])
            .apply(&mut candidate, &DockPolicySnapshot::default())
            .expect("resolved cross-surface rehome must commit");
        assert_eq!(
            candidate
                .surface(CROSS_TARGET_SURFACE)
                .expect("cross target remains")
                .main_root,
            Some(CONTAINED_ROOT)
        );
        assert!(candidate.contained_floating(CONTAINED).is_none());
        assert_eq!(
            candidate.presentation_for_root(CONTAINED_ROOT),
            Some(RootPresentationOwner::Main {
                surface: CROSS_TARGET_SURFACE,
            })
        );
        assert_eq!(
            candidate
                .surface(CROSS_TARGET_SURFACE)
                .expect("cross target remains")
                .contained,
            vec![CROSS_CONTAINED]
        );
    }
}

#[test]
fn cross_surface_contained_background_rejection_never_falls_through() {
    let fixture = fixture();
    let engine = DockEngine::new(fixture.workspace.clone(), DockPolicy::default())
        .expect("cross-surface fixture engine must be valid");
    let mut plan = compile_fixture_surface_plan(&engine, CROSS_TARGET_SURFACE, rect());
    let guide = plan
        .drop_guide_clusters()
        .iter()
        .find(|cluster| {
            cluster.id().root == CROSS_CONTAINED_ROOT && cluster.id().scope == DropGuideScope::Outer
        })
        .and_then(|cluster| cluster.target(DropGuideSlot::Edge(Edge::Bottom)))
        .expect("production compilation must publish the visible contained outer guide");
    let target_id = guide.id();
    let guide_draw = guide.draw();
    let target = plan
        .drop_target(target_id)
        .expect("production compilation must publish the contained outer target");
    let location = target.location();
    let target_region = target.region();
    let target_layer = target.layer();
    let target_visual = target.visual();
    let replacement = Arc::new(DropTargetRecord::new(
        location,
        DropTargetAvailability::Unavailable(DropTargetUnavailable::AdapterUnavailable),
        target_region,
        target_layer,
        target_visual,
    ));
    assert!(plan.replace_drop_target_for_test(replacement));
    let scene = seal_workspace(&fixture.workspace, plan);
    let point = LogicalPoint::new(
        guide_draw.x() + guide_draw.width() * 0.5,
        guide_draw.y() + guide_draw.height() * 0.5,
    )
    .expect("visible guide midpoint must be finite");

    for payload in contained_complete_payloads(&fixture) {
        let query = resolve_drop(
            &scene,
            &fixture.workspace,
            &DockPolicySnapshot::default(),
            session(),
            payload,
            None,
            CROSS_TARGET_SURFACE,
            point,
        )
        .expect("expected target rejection cannot expose an invariant");
        assert!(
            matches!(
                query.resolution(),
                DropResolution::Rejected(rejected)
                    if rejected.candidates().len() == 1
                        && rejected.candidates()[0].target_id()
                            == target_id
                        && rejected.candidates()[0].reason()
                            == &DropRejectionReason::SceneUnavailable(
                                DropTargetUnavailable::AdapterUnavailable,
                            )
            ),
            "unexpected visible-target resolution: {:#?}",
            query.resolution()
        );
    }
}

#[test]
fn partial_background_delivery_requires_and_consumes_the_frozen_offer() {
    let fixture = fixture();
    let scene = seal(&fixture, ready_background(None));
    for payload in partial_payloads(&fixture) {
        let missing = resolve(&fixture, &scene, payload.clone(), None);
        assert!(matches!(
            missing,
            DropResolution::Rejected(rejected)
                if rejected.candidates().len() == 1
                    && rejected.candidates()[0].reason()
                        == &DropRejectionReason::SurfaceBackgroundRootOfferMissing
        ));

        let offered = resolve(
            &fixture,
            &scene,
            payload,
            Some(SurfaceBackgroundRootOffer::new(OFFERED_ROOT)),
        );
        let DropResolution::Resolved(resolved) = offered else {
            panic!("fresh offer must resolve every partial payload class");
        };
        assert!(matches!(
            resolved.command(),
            WorkspaceCommand::InstallMainRoot {
                surface: TARGET_SURFACE,
                root: OFFERED_ROOT,
                ..
            }
        ));
    }
}

#[test]
fn colliding_offer_rejects_atomically_without_fallthrough() {
    let fixture = fixture();
    let before = fixture.workspace.clone();
    let scene = seal(&fixture, ready_background(None));
    let resolution = resolve(
        &fixture,
        &scene,
        partial_payload(&fixture),
        Some(SurfaceBackgroundRootOffer::new(CONTAINED_ROOT)),
    );
    assert!(matches!(
        resolution,
        DropResolution::Rejected(rejected)
            if rejected.candidates().len() == 1
                && matches!(
                    rejected.candidates()[0].reason(),
                    DropRejectionReason::Prevalidation(TransactionError::Command { .. })
                )
    ));
    assert_eq!(fixture.workspace, before);
}

#[test]
fn rejected_front_target_never_falls_through_to_background() {
    let fixture = fixture();
    let target = DropTargetRecord::new(
        DropTargetLocation::Center {
            surface: TARGET_SURFACE,
            root: CONTAINED_ROOT,
            tabs: fixture.contained_tabs,
        },
        DropTargetAvailability::Unavailable(DropTargetUnavailable::AdapterUnavailable),
        HitRegion::new(rect()),
        SceneLayerKey::new(2),
        DropVisual::new(rect()),
    );
    let scene = seal(&fixture, ready_background(Some(target)));
    let resolution = resolve(
        &fixture,
        &scene,
        partial_payload(&fixture),
        Some(SurfaceBackgroundRootOffer::new(OFFERED_ROOT)),
    );
    assert!(matches!(
        resolution,
        DropResolution::Rejected(rejected)
            if rejected.candidates().len() == 1
                && rejected.candidates()[0].target_id().kind() == DropTargetKind::Center
    ));
}

#[test]
fn contained_occlusion_prevents_background_pass_through() {
    let fixture = fixture();
    let scene = seal(&fixture, ready_background(None));
    let resolution = resolve_unguided_drop(
        &scene,
        &fixture.workspace,
        &DockPolicySnapshot::default(),
        session(),
        partial_payload(&fixture),
        Some(SurfaceBackgroundRootOffer::new(OFFERED_ROOT)),
        TARGET_SURFACE,
        LogicalPoint::new(550.0, 400.0).expect("point must be finite"),
    )
    .expect("resolution cannot expose an invariant");

    assert!(matches!(resolution, DropResolution::KnownNone(_)));
}

#[test]
fn unclipped_occlusion_hits_only_within_ready_surface_intersection() {
    let fixture = fixture();
    let surface_bounds =
        LogicalRect::new(0.0, 0.0, 600.0, 450.0).expect("surface bounds are valid");
    let outside_target = DropTargetRecord::new(
        DropTargetLocation::Center {
            surface: TARGET_SURFACE,
            root: CONTAINED_ROOT,
            tabs: fixture.contained_tabs,
        },
        DropTargetAvailability::Available,
        HitRegion::new(
            LogicalRect::new(600.0, 350.0, 100.0, 100.0).expect("outside hit region is valid"),
        ),
        SceneLayerKey::new(2),
        DropVisual::new(
            LogicalRect::new(500.0, 350.0, 100.0, 100.0).expect("inside preview is valid"),
        ),
    );
    let scene = seal(
        &fixture,
        ready_background_in(surface_bounds, Some(outside_target)),
    );
    let offer = Some(SurfaceBackgroundRootOffer::new(OFFERED_ROOT));
    let intersection = LogicalPoint::new(550.0, 400.0).expect("point is finite");
    let clear_inside = LogicalPoint::new(100.0, 100.0).expect("point is finite");
    let raw_only = LogicalPoint::new(650.0, 400.0).expect("point is finite");

    let blocked = resolve_unguided_drop(
        &scene,
        &fixture.workspace,
        &DockPolicySnapshot::default(),
        session(),
        partial_payload(&fixture),
        offer,
        TARGET_SURFACE,
        intersection,
    )
    .expect("intersection resolution cannot expose an invariant");
    assert!(matches!(blocked, DropResolution::KnownNone(_)));

    let clear = resolve_unguided_drop(
        &scene,
        &fixture.workspace,
        &DockPolicySnapshot::default(),
        session(),
        partial_payload(&fixture),
        offer,
        TARGET_SURFACE,
        clear_inside,
    )
    .expect("clear resolution cannot expose an invariant");
    assert!(matches!(clear, DropResolution::Resolved(_)));

    let outside = resolve_unguided_drop(
        &scene,
        &fixture.workspace,
        &DockPolicySnapshot::default(),
        session(),
        partial_payload(&fixture),
        offer,
        TARGET_SURFACE,
        raw_only,
    )
    .expect("outside resolution cannot expose an invariant");
    assert!(matches!(outside, DropResolution::KnownNone(_)));

    let queried = resolve_drop(
        &scene,
        &fixture.workspace,
        &DockPolicySnapshot::default(),
        session(),
        partial_payload(&fixture),
        offer,
        TARGET_SURFACE,
        raw_only,
    )
    .expect("outside query cannot expose an invariant");
    assert!(matches!(queried.resolution(), DropResolution::KnownNone(_)));
    assert!(queried.affordance().is_none());
}

#[test]
fn complete_main_source_with_contained_siblings_still_suppresses_only_its_root() {
    let mut builder = Workspace::builder();
    let main_tabs = builder.insert_node(Node::tabs([ItemId::new(71)]));
    let sibling_tabs = builder.insert_node(Node::tabs([ItemId::new(72)]));
    let main_root = RootId::new(73);
    let sibling_root = RootId::new(74);
    let surface = SurfaceId::new(75);
    let sibling = FloatingPresentationId::new(76);
    builder.set_root(main_root, RootRecord::new(main_tabs));
    builder.set_root(sibling_root, RootRecord::new(sibling_tabs));
    builder.set_surface(surface, SurfacePresentation::with_main(main_root));
    builder.set_contained_floating(sibling, ContainedFloating::new(sibling_root, rect()));
    builder
        .attach_contained(surface, sibling)
        .expect("surface exists");
    let workspace = builder.build().expect("workspace must be valid");
    let payload = MovePayload::Tabs(
        workspace
            .capture_node_source(main_root, main_tabs)
            .expect("main source must be current"),
    );

    let workspace_version = WorkspaceVersion::initial();
    let workspace_index =
        WorkspaceIndex::build(&workspace, workspace_version).expect("suppression index must build");
    let source = workspace_index
        .capture_complete_root_source(&workspace, workspace_version, &payload)
        .expect("complete source analysis must succeed")
        .expect("complete root source must exist");
    let suppression = DropSourceSuppression::from_complete_root(&workspace, source.root())
        .expect("complete root suppresses");
    assert!(suppression.excludes_target(DropTargetId::Center {
        surface,
        root: main_root,
        tabs: main_tabs,
    }));
    assert!(!suppression.excludes_target(DropTargetId::Center {
        surface,
        root: sibling_root,
        tabs: sibling_tabs,
    }));
    assert!(!suppression.excludes_occlusion(sibling));
}

#[test]
fn disappearing_source_leaf_is_absent_from_guides_direct_and_magnetic_resolution() {
    let surface = SurfaceId::new(81);
    let root = RootId::new(82);
    let item = ItemId::new(83);
    let mut builder = Workspace::builder();
    let source_tabs = builder.insert_node(Node::tabs([item]));
    let remaining_tabs = builder.insert_node(Node::tabs([ItemId::new(84)]));
    let split = builder.insert_node(
        Node::equal_split(Axis::Horizontal, [source_tabs, remaining_tabs])
            .expect("source and remaining leaf form a valid split"),
    );
    builder.set_root(root, RootRecord::new(split));
    builder.set_surface(surface, SurfacePresentation::with_main(root));
    let workspace = builder.build().expect("self-docking workspace is valid");
    let engine = DockEngine::new(workspace.clone(), DockPolicy::default())
        .expect("self-docking engine is valid");
    let plan = compile_fixture_surface_plan(&engine, surface, rect());
    let source_guide = plan
        .drop_guide_clusters()
        .iter()
        .find(|cluster| cluster.id() == DropGuideClusterId::inner(surface, root, source_tabs))
        .expect("the unchanged durable source has an inner guide cluster");
    let points: Vec<_> = source_guide
        .targets()
        .map(|(_, target)| {
            let bounds = target.hit().rect();
            LogicalPoint::new(
                bounds.x() + bounds.width() * 0.5,
                bounds.y() + bounds.height() * 0.5,
            )
            .expect("source guide midpoint is finite")
        })
        .collect();
    let scene = seal_workspace(&workspace, plan);
    let source = workspace
        .capture_node_source(root, source_tabs)
        .expect("source leaf is current");
    for payload in [
        MovePayload::Item(
            workspace
                .capture_item_source(root, source_tabs, item)
                .expect("last source tab is current"),
        ),
        MovePayload::Tabs(source.clone()),
        MovePayload::Subtree(source),
    ] {
        for point in &points {
            let query = resolve_drop(
                &scene,
                &workspace,
                &DockPolicySnapshot::default(),
                session(),
                payload.clone(),
                None,
                surface,
                *point,
            )
            .expect("source exclusion leaves a valid drop query");
            assert!(
                matches!(query.resolution(), DropResolution::KnownNone(_)),
                "a removed source leaf cannot be an exact or magnetic destination: {payload:?}",
            );
            let clusters = query
                .affordance()
                .expect("root outer guides remain")
                .clusters();
            assert_eq!(clusters.len(), 1);
            assert_eq!(clusters[0].id(), DropGuideClusterId::outer(surface, root));
            assert_eq!(clusters[0].targets().len(), 4);
            let unguided = resolve_unguided_drop(
                &scene,
                &workspace,
                &DockPolicySnapshot::default(),
                session(),
                payload.clone(),
                None,
                surface,
                *point,
            )
            .expect("unguided source exclusion remains valid");
            assert!(matches!(unguided, DropResolution::KnownNone(_)));
        }
    }
}

#[test]
fn same_root_edge_preview_matches_the_post_detach_payload_layout() {
    let surface = SurfaceId::new(91);
    let root = RootId::new(92);
    let mut builder = Workspace::builder();
    let source_tabs = builder.insert_node(Node::tabs([ItemId::new(93)]));
    let target_tabs = builder.insert_node(Node::tabs([ItemId::new(94)]));
    let split = builder.insert_node(
        Node::split(Axis::Horizontal, [source_tabs, target_tabs], [0.25, 0.75])
            .expect("weighted source layout must be valid"),
    );
    builder.set_root(root, RootRecord::new(split));
    builder.set_surface(surface, SurfacePresentation::with_main(root));
    let workspace = builder.build().expect("preview workspace must be valid");
    let engine = DockEngine::new(workspace.clone(), DockPolicy::default())
        .expect("preview engine must be valid");
    let plan = compile_fixture_surface_plan(&engine, surface, rect());
    assert!(plan.layout_facts().is_some());
    let target_id = DropTargetId::InnerEdge {
        surface,
        root,
        node: target_tabs,
        edge: Edge::Left,
    };
    let target = plan
        .drop_guide_clusters()
        .iter()
        .flat_map(DropGuideClusterRecord::targets)
        .map(|(_, target)| target)
        .find(|target| target.id() == target_id)
        .expect("the target pane must expose a left inner guide");
    let hit = target.hit().rect();
    let point = LogicalPoint::new(hit.x() + hit.width() * 0.5, hit.y() + hit.height() * 0.5)
        .expect("guide midpoint must be finite");
    let source = MovePayload::Tabs(
        workspace
            .capture_node_source(root, source_tabs)
            .expect("source tabs must be current"),
    );
    let scene = seal_workspace(&workspace, plan);
    assert!(
        scene
            .ready_surface(surface)
            .expect("sealed target surface must remain ready")
            .plan()
            .layout_facts()
            .is_some()
    );
    let resolved = resolve_drop(
        &scene,
        &workspace,
        &DockPolicySnapshot::default(),
        session(),
        source,
        None,
        surface,
        point,
    )
    .expect("same-root edge resolution must remain valid");
    let DropResolution::Resolved(resolved) = resolved.resolution() else {
        panic!("the exact inner guide must resolve");
    };

    let mut committed = workspace.clone();
    WorkspaceTransaction::from_commands([resolved.command().clone()])
        .apply(&mut committed, &DockPolicySnapshot::default())
        .expect("the prevalidated drop command must commit");
    let committed_engine = DockEngine::new(committed, DockPolicy::default())
        .expect("the committed workspace must remain valid");
    let committed_plan = compile_fixture_surface_plan(&committed_engine, surface, rect());
    let payload_bounds = committed_plan
        .pane_records()
        .iter()
        .find(|pane| pane.id().tabs == source_tabs)
        .expect("the moved tabs node must remain the payload leaf")
        .bounds();

    assert_eq!(resolved.visual().rect(), payload_bounds);
}

#[test]
fn same_root_center_preview_matches_the_post_detach_content_layout() {
    let surface = SurfaceId::new(95);
    let root = RootId::new(96);
    let mut builder = Workspace::builder();
    let source_tabs = builder.insert_node(Node::tabs([ItemId::new(97)]));
    let target_tabs = builder.insert_node(Node::tabs([ItemId::new(98)]));
    let split = builder.insert_node(
        Node::split(Axis::Horizontal, [source_tabs, target_tabs], [0.25, 0.75])
            .expect("weighted center-preview layout must be valid"),
    );
    builder.set_root(root, RootRecord::new(split));
    builder.set_surface(surface, SurfacePresentation::with_main(root));
    let workspace = builder
        .build()
        .expect("center-preview workspace must be valid");
    let engine = DockEngine::new(workspace.clone(), DockPolicy::default())
        .expect("center-preview engine must be valid");
    let plan = compile_fixture_surface_plan(&engine, surface, rect());
    let target_id = DropTargetId::Center {
        surface,
        root,
        tabs: target_tabs,
    };
    let target = plan
        .drop_guide_clusters()
        .iter()
        .flat_map(DropGuideClusterRecord::targets)
        .map(|(_, target)| target)
        .find(|target| target.id() == target_id)
        .expect("the target pane must expose a center guide");
    let stale_visual = target.preview();
    let hit = target.hit().rect();
    let point = LogicalPoint::new(hit.x() + hit.width() * 0.5, hit.y() + hit.height() * 0.5)
        .expect("guide midpoint must be finite");
    let source = MovePayload::Tabs(
        workspace
            .capture_node_source(root, source_tabs)
            .expect("source tabs must be current"),
    );
    let scene = seal_workspace(&workspace, plan);
    let resolved = resolve_drop(
        &scene,
        &workspace,
        &DockPolicySnapshot::default(),
        session(),
        source,
        None,
        surface,
        point,
    )
    .expect("same-root center resolution must remain valid");
    let DropResolution::Resolved(resolved) = resolved.resolution() else {
        panic!("the exact center guide must resolve");
    };

    let mut committed = workspace.clone();
    WorkspaceTransaction::from_commands([resolved.command().clone()])
        .apply(&mut committed, &DockPolicySnapshot::default())
        .expect("the prevalidated center command must commit");
    let committed_engine = DockEngine::new(committed, DockPolicy::default())
        .expect("the committed center workspace must remain valid");
    let committed_plan = compile_fixture_surface_plan(&committed_engine, surface, rect());
    let content_bounds = committed_plan
        .pane_records()
        .iter()
        .find(|pane| pane.id().tabs == target_tabs)
        .expect("the target tabs must remain after center merge")
        .content_bounds();

    assert_ne!(stale_visual, content_bounds);
    assert_eq!(resolved.visual().rect(), content_bounds);
}

#[test]
fn non_guide_center_basin_resolves_center_target() {
    let surface = SurfaceId::new(99);
    let root = RootId::new(100);
    let mut builder = Workspace::builder();
    let source_tabs = builder.insert_node(Node::tabs([ItemId::new(101)]));
    let target_tabs = builder.insert_node(Node::tabs([ItemId::new(102)]));
    let split = builder.insert_node(
        Node::split(Axis::Horizontal, [source_tabs, target_tabs], [0.25, 0.75])
            .expect("weighted magnetic-basin layout must be valid"),
    );
    builder.set_root(root, RootRecord::new(split));
    builder.set_surface(surface, SurfacePresentation::with_main(root));
    let workspace = builder
        .build()
        .expect("magnetic-basin workspace must be valid");
    let engine = DockEngine::new(workspace.clone(), DockPolicy::default())
        .expect("magnetic-basin engine must be valid");
    let plan = compile_fixture_surface_plan(&engine, surface, rect());
    let target_id = DropTargetId::Center {
        surface,
        root,
        tabs: target_tabs,
    };
    let content = plan
        .pane_records()
        .iter()
        .find(|pane| pane.id().tabs == target_tabs)
        .expect("the target pane must be presented")
        .content_bounds();
    let point = LogicalPoint::new(
        content.x() + content.width() * 0.65,
        content.y() + content.height() * 0.65,
    )
    .expect("the magnetic-basin point must be finite");
    assert!(plan.drop_guide_clusters().iter().all(|cluster| {
        cluster
            .targets()
            .all(|(_, target)| !target.hit().contains(point))
    }));
    let source = MovePayload::Tabs(
        workspace
            .capture_node_source(root, source_tabs)
            .expect("source tabs must be current"),
    );
    let scene = seal_workspace(&workspace, plan);
    let resolved = resolve_drop(
        &scene,
        &workspace,
        &DockPolicySnapshot::default(),
        session(),
        source,
        None,
        surface,
        point,
    )
    .expect("magnetic-basin resolution must remain valid");
    let DropResolution::Resolved(resolved) = resolved.resolution() else {
        panic!(
            "the ordinary pane center basin must resolve a docking target, got {:?}",
            resolved.resolution()
        );
    };

    assert_eq!(resolved.target_id(), target_id);
}

#[test]
fn magnetic_center_resolution_does_not_require_guide_records() {
    let surface = SurfaceId::new(103);
    let root = RootId::new(104);
    let mut builder = Workspace::builder();
    let source_tabs = builder.insert_node(Node::tabs([ItemId::new(105)]));
    let target_tabs = builder.insert_node(Node::tabs([ItemId::new(106)]));
    let split = builder.insert_node(
        Node::split(Axis::Horizontal, [source_tabs, target_tabs], [0.25, 0.75])
            .expect("guide-independent magnetic layout must be valid"),
    );
    builder.set_root(root, RootRecord::new(split));
    builder.set_surface(surface, SurfacePresentation::with_main(root));
    let workspace = builder
        .build()
        .expect("guide-independent magnetic workspace must be valid");
    let engine = DockEngine::new(workspace.clone(), DockPolicy::default())
        .expect("guide-independent magnetic engine must be valid");
    let guided = compile_fixture_surface_plan(&engine, surface, rect());
    let edge_target = DropTargetId::InnerEdge {
        surface,
        root,
        node: target_tabs,
        edge: Edge::Left,
    };
    let former_guide_hit = guided
        .drop_guide_clusters()
        .iter()
        .flat_map(|cluster| cluster.targets())
        .find_map(|(_, target)| (target.id() == edge_target).then(|| target.hit().rect()))
        .expect("guided compilation must expose the target pane's left guide hit");
    let point = LogicalPoint::new(
        former_guide_hit.x() + former_guide_hit.width() * 0.5,
        former_guide_hit.y() + former_guide_hit.height() * 0.5,
    )
    .expect("former guide center must be finite");

    let plan = compile_fixture_surface_plan_without_guides(&engine, surface, rect());
    let content = plan
        .pane_records()
        .iter()
        .find(|pane| pane.id().tabs == target_tabs)
        .expect("the target pane must be presented")
        .content_bounds();
    assert!(plan.drop_guide_clusters().is_empty());
    assert!(
        crate::interaction::targeting::field::pane_center_basin(
            content,
            engine.presentation_config()
        )
        .expect("pane center basin must be valid")
        .contains(point),
        "the former edge-guide button must overlap the magnetic center basin for this proof"
    );

    let source = MovePayload::Tabs(
        workspace
            .capture_node_source(root, source_tabs)
            .expect("source tabs must be current"),
    );
    let scene = seal_workspace(&workspace, plan);
    let query = resolve_drop(
        &scene,
        &workspace,
        &DockPolicySnapshot::default(),
        session(),
        source,
        None,
        surface,
        point,
    )
    .expect("guide-independent magnetic resolution must remain valid");
    let DropResolution::Resolved(resolved) = query.resolution() else {
        panic!("the guide-independent center basin must resolve: {query:?}");
    };
    assert_eq!(
        resolved.target_id(),
        DropTargetId::Center {
            surface,
            root,
            tabs: target_tabs,
        }
    );
    assert!(query.affordance().is_none());
}

#[test]
fn same_root_tab_gap_preview_tracks_the_inserted_tab_in_the_future_strip() {
    let surface = SurfaceId::new(111);
    let root = RootId::new(112);
    let moved = ItemId::new(113);
    let mut builder = Workspace::builder();
    let source_tabs = builder.insert_node(Node::tabs([moved, ItemId::new(116)]));
    let target_tabs = builder.insert_node(Node::tabs_with_selection(
        [ItemId::new(114), ItemId::new(115)],
        Some(ItemId::new(115)),
    ));
    let split = builder.insert_node(
        Node::split(Axis::Horizontal, [source_tabs, target_tabs], [0.8, 0.2])
            .expect("weighted tab-gap layout must be valid"),
    );
    builder.set_root(root, RootRecord::new(split).with_central(target_tabs));
    builder.set_surface(surface, SurfacePresentation::with_main(root));
    let workspace = builder
        .build()
        .expect("tab-gap preview workspace must be valid");
    let engine = DockEngine::new(workspace.clone(), DockPolicy::default())
        .expect("tab-gap preview engine must be valid");
    let plan = compile_fixture_surface_plan(&engine, surface, rect());
    let target_id = DropTargetId::TabGap {
        surface,
        root,
        tabs: target_tabs,
        index: 2,
    };
    assert!(
        plan.presentation_menu_anchor_records()
            .iter()
            .any(|anchor| {
                anchor.host()
                    == crate::presentation::scene::PresentationMenuAnchorHost::TabBar(
                        crate::presentation::scene::TabBarSceneId {
                            root,
                            tabs: target_tabs,
                        },
                    )
            })
    );
    let target = plan
        .drop_targets()
        .iter()
        .find(|target| target.id() == target_id)
        .expect("the target strip must expose its middle tab gap");
    let stale_visual = target.visual().rect();
    let hit = target.region().rect();
    let point = LogicalPoint::new(hit.x() + hit.width() * 0.5, hit.y() + hit.height() * 0.5)
        .expect("tab-gap midpoint must be finite");
    let source = MovePayload::Item(
        workspace
            .capture_item_source(root, source_tabs, moved)
            .expect("source item must be current"),
    );
    let scene = seal_workspace(&workspace, plan);
    let resolved = resolve_drop(
        &scene,
        &workspace,
        &DockPolicySnapshot::default(),
        session(),
        source,
        None,
        surface,
        point,
    )
    .expect("same-root tab-gap resolution must remain valid");
    let DropResolution::Resolved(resolved) = resolved.resolution() else {
        panic!("the exact tab gap must resolve");
    };

    let mut committed = workspace.clone();
    WorkspaceTransaction::from_commands([resolved.command().clone()])
        .apply(&mut committed, &DockPolicySnapshot::default())
        .expect("the prevalidated tab-gap command must commit");
    let committed_engine = DockEngine::new(committed, DockPolicy::default())
        .expect("the committed tab-gap workspace must remain valid");
    let committed_plan = compile_fixture_surface_plan(&committed_engine, surface, rect());
    let bar = committed_plan
        .tab_bar_records()
        .iter()
        .find(|bar| bar.id().root == root && bar.id().tabs == target_tabs)
        .expect("the target tab bar must remain after insertion");
    let inserted = bar
        .members()
        .iter()
        .find(|member| member.tab().item == moved)
        .expect("the future strip must contain the moved item")
        .full_bounds();
    let width = stale_visual.width().min(bar.viewport().width());
    let marker_x = (inserted.x() - width * 0.5).clamp(
        bar.viewport().x(),
        (bar.viewport().max().x() - width).max(bar.viewport().x()),
    );
    let expected = LogicalRect::new(marker_x, bar.viewport().y(), width, bar.viewport().height())
        .expect("future tab-gap marker must be valid");

    assert_ne!(stale_visual, expected);
    assert_eq!(resolved.visual().rect(), expected);
}

#[test]
fn future_tab_gap_preview_reselects_the_menu_host_after_the_old_host_is_pruned() {
    let surface = SurfaceId::new(121);
    let root = RootId::new(122);
    let moved = ItemId::new(123);
    let mut builder = Workspace::builder();
    let old_host = builder.insert_node(Node::tabs([moved]));
    let target_tabs = builder.insert_node(Node::tabs_with_selection(
        [ItemId::new(124), ItemId::new(125)],
        Some(ItemId::new(125)),
    ));
    let trailing_tabs = builder.insert_node(Node::tabs([ItemId::new(126)]));
    let split = builder.insert_node(
        Node::split(
            Axis::Horizontal,
            [old_host, target_tabs, trailing_tabs],
            [0.2, 0.14, 0.66],
        )
        .expect("weighted menu-host layout must be valid"),
    );
    builder.set_root(root, RootRecord::new(split));
    builder.set_surface(surface, SurfacePresentation::with_main(root));
    let workspace = builder.build().expect("menu-host fixture must be valid");
    let engine = DockEngine::new(workspace.clone(), DockPolicy::default())
        .expect("menu-host preview engine must be valid");
    let plan = compile_fixture_surface_plan(&engine, surface, rect());
    assert!(
        plan.presentation_menu_anchor_records()
            .iter()
            .any(|anchor| {
                anchor.host()
                    == crate::presentation::scene::PresentationMenuAnchorHost::TabBar(
                        crate::presentation::scene::TabBarSceneId {
                            root,
                            tabs: old_host,
                        },
                    )
            })
    );

    let target_id = DropTargetId::TabGap {
        surface,
        root,
        tabs: target_tabs,
        index: 2,
    };
    let target = plan
        .drop_targets()
        .iter()
        .find(|target| target.id() == target_id)
        .expect("the narrow target strip must expose its trailing gap");
    let hit = target.region().rect();
    let point = LogicalPoint::new(hit.x() + hit.width() * 0.5, hit.y() + hit.height() * 0.5)
        .expect("tab-gap midpoint must be finite");
    let source = MovePayload::Item(
        workspace
            .capture_item_source(root, old_host, moved)
            .expect("source item must be current"),
    );
    let scene = seal_workspace(&workspace, plan);
    let resolved = resolve_drop(
        &scene,
        &workspace,
        &DockPolicySnapshot::default(),
        session(),
        source,
        None,
        surface,
        point,
    )
    .expect("candidate menu-host resolution must remain valid");
    let DropResolution::Resolved(resolved) = resolved.resolution() else {
        panic!("the exact trailing tab gap must resolve");
    };

    let mut committed = workspace.clone();
    WorkspaceTransaction::from_commands([resolved.command().clone()])
        .apply(&mut committed, &DockPolicySnapshot::default())
        .expect("the prevalidated menu-host command must commit");
    let committed_engine = DockEngine::new(committed, DockPolicy::default())
        .expect("the committed menu-host workspace must remain valid");
    let committed_plan = compile_fixture_surface_plan(&committed_engine, surface, rect());
    assert!(
        committed_plan
            .presentation_menu_anchor_records()
            .iter()
            .any(|anchor| {
                anchor.host()
                    == crate::presentation::scene::PresentationMenuAnchorHost::TabBar(
                        crate::presentation::scene::TabBarSceneId {
                            root,
                            tabs: target_tabs,
                        },
                    )
                    && anchor.is_ready()
            }),
        "candidate anchors: {:?}; bars: {:?}",
        committed_plan.presentation_menu_anchor_records(),
        committed_plan.tab_bar_records(),
    );
    let bar = committed_plan
        .tab_bar_records()
        .iter()
        .find(|bar| bar.id().root == root && bar.id().tabs == target_tabs)
        .expect("the target tab bar must survive menu-host reselection");
    let inserted = bar
        .members()
        .iter()
        .find(|member| member.tab().item == moved)
        .expect("the future strip must contain the moved item")
        .full_bounds();
    let marker_width = resolved.visual().rect().width().min(bar.viewport().width());
    let marker_x = (inserted.x() - marker_width * 0.5).clamp(
        bar.viewport().x(),
        (bar.viewport().max().x() - marker_width).max(bar.viewport().x()),
    );
    let expected = LogicalRect::new(
        marker_x,
        bar.viewport().y(),
        marker_width,
        bar.viewport().height(),
    )
    .expect("candidate-aware tab-gap marker must be valid");

    assert_eq!(resolved.visual().rect(), expected);
}

#[test]
fn cross_surface_unselected_item_preview_uses_retained_source_measurements() {
    let source_surface = SurfaceId::new(101);
    let target_surface = SurfaceId::new(102);
    let source_root = RootId::new(103);
    let target_root = RootId::new(104);
    let moved = ItemId::new(105);
    let mut builder = Workspace::builder();
    let source_tabs = builder.insert_node(Node::tabs_with_selection(
        [moved, ItemId::new(106)],
        Some(ItemId::new(106)),
    ));
    let target_tabs = builder.insert_node(Node::tabs([ItemId::new(107)]));
    builder.set_root(source_root, RootRecord::new(source_tabs));
    builder.set_root(target_root, RootRecord::new(target_tabs));
    builder.set_surface(source_surface, SurfacePresentation::with_main(source_root));
    builder.set_surface(target_surface, SurfacePresentation::with_main(target_root));
    let workspace = builder
        .build()
        .expect("cross-surface fixture must be valid");
    let engine = DockEngine::new(workspace.clone(), DockPolicy::default())
        .expect("cross-surface preview engine must be valid");
    let plan = compile_fixture_surface_plan(&engine, target_surface, rect());
    let target_id = DropTargetId::InnerEdge {
        surface: target_surface,
        root: target_root,
        node: target_tabs,
        edge: Edge::Right,
    };
    let target = plan
        .drop_guide_clusters()
        .iter()
        .flat_map(DropGuideClusterRecord::targets)
        .map(|(_, target)| target)
        .find(|target| target.id() == target_id)
        .expect("target pane must expose a right inner guide");
    let hit = target.hit().rect();
    let point = LogicalPoint::new(hit.x() + hit.width() * 0.5, hit.y() + hit.height() * 0.5)
        .expect("guide midpoint must be finite");
    let source = MovePayload::Item(
        workspace
            .capture_item_source(source_root, source_tabs, moved)
            .expect("unselected source item must be current"),
    );
    let scene = seal_workspace(&workspace, plan);
    let resolved = resolve_drop(
        &scene,
        &workspace,
        &DockPolicySnapshot::default(),
        session(),
        source,
        None,
        target_surface,
        point,
    )
    .expect("cross-surface edge resolution must use retained source measurements");
    let DropResolution::Resolved(resolved) = resolved.resolution() else {
        panic!("the exact cross-surface guide must resolve");
    };

    let mut committed = workspace.clone();
    WorkspaceTransaction::from_commands([resolved.command().clone()])
        .apply(&mut committed, &DockPolicySnapshot::default())
        .expect("the cross-surface drop command must commit");
    let committed_engine = DockEngine::new(committed, DockPolicy::default())
        .expect("the committed cross-surface workspace must remain valid");
    let committed_plan = compile_fixture_surface_plan(&committed_engine, target_surface, rect());
    let payload_bounds = committed_plan
        .pane_records()
        .iter()
        .find(|pane| committed_plan.pane_selected(pane) == Some(moved))
        .expect("the moved item must own its new target leaf")
        .bounds();

    assert_eq!(resolved.visual().rect(), payload_bounds);
}
