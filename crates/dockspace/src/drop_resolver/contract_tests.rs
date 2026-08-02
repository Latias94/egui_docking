use crate as dockspace;

use super::resolve_unguided_drop;
use dockspace::command::{DockFraction, DockTarget, Edge, MovePayload, WorkspaceCommand};
use dockspace::drop_resolver::{DropRejectionReason, DropResolution, DropSurfaceUnavailable};
use dockspace::drop_target::{
    DropDestination, DropOcclusionRecord, DropTargetAvailability, DropTargetId, DropTargetRecord,
    DropTargetUnavailable, DropVisual, SceneLayerKey, SurfaceBackground,
};
use dockspace::engine::{DockEngine, PreparedSurfacePaintCandidate};
use dockspace::geometry::{LogicalPoint, LogicalRect, LogicalSize};
use dockspace::graph::{
    Axis, ContainedFloating, Node, RootRecord, SurfacePresentation, Workspace, WorkspaceBuilder,
};
use dockspace::hit_region::HitRegion;
use dockspace::ids::{FloatingPresentationId, ItemId, NodeId, RootId, SurfaceId, WorkspaceEpoch};
use dockspace::intent::SurfaceBackgroundRootOffer;
use dockspace::interaction::{DragGeneration, DragSessionId};
use dockspace::policy::DockPolicySnapshot;
use dockspace::presentation_observation::{PresentationOutputSerial, PresentedSurfaceAuthority};
use dockspace::scene::{
    ContainedMinimumMeasurement, PaneRecord, PaneSceneId, PresentationPlan,
    PresentationPlanValidator, SceneBuildError, SurfaceCoordinateCapture, SurfaceScene,
    SurfaceSceneSet,
};
use dockspace::scene_manifest::{
    Measurement, SurfaceMeasurementTicket, SurfaceMeasurements, TabIntrinsic, TabStripMetrics,
};
use dockspace::transaction::WorkspaceTransaction;
use dockspace::viewport::CoordinateGeneration;

const SOURCE_ROOT: RootId = RootId::new(1);
const TARGET_ROOT: RootId = RootId::new(2);
const FLOATING_ROOT: RootId = RootId::new(3);
const SOURCE_SURFACE: SurfaceId = SurfaceId::new(1);
const TARGET_SURFACE: SurfaceId = SurfaceId::new(2);
const MISSING_SURFACE: SurfaceId = SurfaceId::new(3);
const FLOATING: FloatingPresentationId = FloatingPresentationId::new(1);

#[derive(Debug)]
struct Fixture {
    workspace: Workspace,
    source_tabs: NodeId,
    source_subtree: NodeId,
    target_root_node: NodeId,
    target_tabs_a: NodeId,
    target_tabs_b: NodeId,
    floating_tabs: NodeId,
}

#[derive(Debug)]
struct RootlessFixture {
    workspace: Workspace,
    source_tabs: NodeId,
    contained_tabs: NodeId,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PayloadSpec {
    Item,
    Tabs,
    Subtree,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TargetSpec {
    TabGap,
    Center,
    InnerEdge,
    OuterEdge,
}

fn fixture() -> Fixture {
    let mut builder = WorkspaceBuilder::new();

    let source_tabs_a = builder.insert_node(Node::tabs([ItemId::new(1), ItemId::new(2)]));
    let source_tabs_b = builder.insert_node(Node::tabs([ItemId::new(3)]));
    let source_subtree = builder.insert_node(
        Node::equal_split(Axis::Vertical, [source_tabs_a, source_tabs_b])
            .expect("valid source subtree"),
    );
    let source_tabs = builder.insert_node(Node::tabs([ItemId::new(4)]));
    let source_root_node = builder.insert_node(
        Node::equal_split(Axis::Horizontal, [source_subtree, source_tabs])
            .expect("valid source root"),
    );

    let target_tabs_a = builder.insert_node(Node::tabs([ItemId::new(10), ItemId::new(11)]));
    let target_tabs_b = builder.insert_node(Node::tabs([ItemId::new(12)]));
    let target_root_node = builder.insert_node(
        Node::equal_split(Axis::Horizontal, [target_tabs_a, target_tabs_b])
            .expect("valid target root"),
    );
    let floating_tabs = builder.insert_node(Node::tabs([ItemId::new(20)]));

    builder.set_root(SOURCE_ROOT, RootRecord::new(source_root_node));
    builder.set_root(TARGET_ROOT, RootRecord::new(target_root_node));
    builder.set_root(FLOATING_ROOT, RootRecord::new(floating_tabs));
    builder.set_surface(SOURCE_SURFACE, SurfacePresentation::with_main(SOURCE_ROOT));
    builder.set_surface(TARGET_SURFACE, SurfacePresentation::with_main(TARGET_ROOT));
    builder.set_contained_floating(
        FLOATING,
        ContainedFloating::new(FLOATING_ROOT, rect(20.0, 20.0, 60.0, 60.0)),
    );
    builder
        .attach_contained(TARGET_SURFACE, FLOATING)
        .expect("target surface exists");

    Fixture {
        workspace: builder.build().expect("fixture workspace is valid"),
        source_tabs,
        source_subtree,
        target_root_node,
        target_tabs_a,
        target_tabs_b,
        floating_tabs,
    }
}

fn rootless_fixture() -> RootlessFixture {
    let mut builder = WorkspaceBuilder::new();
    let source_tabs = builder.insert_node(Node::tabs([ItemId::new(1), ItemId::new(2)]));
    let contained_tabs = builder.insert_node(Node::tabs([ItemId::new(20)]));

    builder.set_root(SOURCE_ROOT, RootRecord::new(source_tabs));
    builder.set_root(FLOATING_ROOT, RootRecord::new(contained_tabs));
    builder.set_surface(SOURCE_SURFACE, SurfacePresentation::with_main(SOURCE_ROOT));
    builder.set_surface(TARGET_SURFACE, SurfacePresentation::rootless());
    builder.set_contained_floating(
        FLOATING,
        ContainedFloating::new(FLOATING_ROOT, rect(20.0, 20.0, 60.0, 60.0)),
    );
    builder
        .attach_contained(TARGET_SURFACE, FLOATING)
        .expect("target surface exists");

    RootlessFixture {
        workspace: builder.build().expect("rootless fixture is valid"),
        source_tabs,
        contained_tabs,
    }
}

fn session() -> DragSessionId {
    DragSessionId::new(WorkspaceEpoch::default(), DragGeneration::new(1))
}

fn rect(x: f64, y: f64, width: f64, height: f64) -> LogicalRect {
    LogicalRect::new(x, y, width, height).expect("test rectangle is valid")
}

fn point(x: f64, y: f64) -> LogicalPoint {
    LogicalPoint::new(x, y).expect("test point is finite")
}

fn payload(fixture: &Fixture, spec: PayloadSpec) -> MovePayload {
    match spec {
        PayloadSpec::Item => MovePayload::Item(
            fixture
                .workspace
                .capture_item_source(SOURCE_ROOT, fixture.source_tabs, ItemId::new(4))
                .expect("item source is valid"),
        ),
        PayloadSpec::Tabs => MovePayload::Tabs(
            fixture
                .workspace
                .capture_node_source(SOURCE_ROOT, fixture.source_tabs)
                .expect("tabs source is valid"),
        ),
        PayloadSpec::Subtree => MovePayload::Subtree(
            fixture
                .workspace
                .capture_node_source(SOURCE_ROOT, fixture.source_subtree)
                .expect("subtree source is valid"),
        ),
    }
}

fn target_record(
    fixture: &Fixture,
    spec: TargetSpec,
    availability: DropTargetAvailability,
    region: LogicalRect,
    layer: u64,
) -> DropTargetRecord {
    let fraction = DockFraction::new(0.35).expect("valid fraction");
    let (id, target) = match spec {
        TargetSpec::TabGap => (
            DropTargetId::TabGap {
                surface: TARGET_SURFACE,
                root: TARGET_ROOT,
                tabs: fixture.target_tabs_a,
                index: 1,
            },
            DockTarget::TabGap {
                target: fixture
                    .workspace
                    .capture_tab_target(TARGET_ROOT, fixture.target_tabs_a)
                    .expect("tab target is valid"),
                index: 1,
            },
        ),
        TargetSpec::Center => (
            DropTargetId::Center {
                surface: TARGET_SURFACE,
                root: TARGET_ROOT,
                tabs: fixture.target_tabs_a,
            },
            DockTarget::Center(
                fixture
                    .workspace
                    .capture_tab_target(TARGET_ROOT, fixture.target_tabs_a)
                    .expect("center target is valid"),
            ),
        ),
        TargetSpec::InnerEdge => (
            DropTargetId::InnerEdge {
                surface: TARGET_SURFACE,
                root: TARGET_ROOT,
                node: fixture.target_tabs_a,
                edge: Edge::Left,
            },
            DockTarget::InnerEdge(
                fixture
                    .workspace
                    .capture_inner_edge_target(
                        TARGET_ROOT,
                        fixture.target_tabs_a,
                        Edge::Left,
                        fraction,
                    )
                    .expect("inner edge target is valid"),
            ),
        ),
        TargetSpec::OuterEdge => (
            DropTargetId::OuterEdge {
                surface: TARGET_SURFACE,
                root: TARGET_ROOT,
                node: fixture.target_root_node,
                edge: Edge::Right,
            },
            DockTarget::OuterEdge(
                fixture
                    .workspace
                    .capture_outer_edge_target(TARGET_ROOT, Edge::Right, fraction)
                    .expect("outer edge target is valid"),
            ),
        ),
    };
    DropTargetRecord::new(
        id,
        target,
        availability,
        HitRegion::new(region),
        SceneLayerKey::new(layer),
        DropVisual::new(region),
    )
}

fn center_record(
    fixture: &Fixture,
    tabs: NodeId,
    region: LogicalRect,
    layer: u64,
) -> DropTargetRecord {
    DropTargetRecord::new(
        DropTargetId::Center {
            surface: TARGET_SURFACE,
            root: TARGET_ROOT,
            tabs,
        },
        DockTarget::Center(
            fixture
                .workspace
                .capture_tab_target(TARGET_ROOT, tabs)
                .expect("center target is valid"),
        ),
        DropTargetAvailability::Available,
        HitRegion::new(region),
        SceneLayerKey::new(layer),
        DropVisual::new(region),
    )
}

fn floating_outer_edge_record(
    fixture: &Fixture,
    region: LogicalRect,
    layer: u64,
) -> DropTargetRecord {
    let fraction = DockFraction::new(0.35).expect("valid fraction");
    DropTargetRecord::new(
        DropTargetId::OuterEdge {
            surface: TARGET_SURFACE,
            root: FLOATING_ROOT,
            node: fixture.floating_tabs,
            edge: Edge::Right,
        },
        DockTarget::OuterEdge(
            fixture
                .workspace
                .capture_outer_edge_target(FLOATING_ROOT, Edge::Right, fraction)
                .expect("floating edge target is valid"),
        ),
        DropTargetAvailability::Available,
        HitRegion::new(region),
        SceneLayerKey::new(layer),
        DropVisual::new(region),
    )
}

fn floating_center_record(fixture: &Fixture, region: LogicalRect, layer: u64) -> DropTargetRecord {
    DropTargetRecord::new(
        DropTargetId::Center {
            surface: TARGET_SURFACE,
            root: FLOATING_ROOT,
            tabs: fixture.floating_tabs,
        },
        DockTarget::Center(
            fixture
                .workspace
                .capture_tab_target(FLOATING_ROOT, fixture.floating_tabs)
                .expect("floating center target is valid"),
        ),
        DropTargetAvailability::Available,
        HitRegion::new(region),
        SceneLayerKey::new(layer),
        DropVisual::new(region),
    )
}

fn floating_occlusion(region: LogicalRect, layer: u64) -> DropOcclusionRecord {
    DropOcclusionRecord::new(FLOATING, HitRegion::new(region), SceneLayerKey::new(layer))
}

fn floating_minimum() -> ContainedMinimumMeasurement {
    ContainedMinimumMeasurement::new(
        FLOATING,
        LogicalSize::new(0.0, 0.0).expect("contained minimum must be valid"),
    )
}

fn topology_destination(record: &DropTargetRecord) -> DockTarget {
    match record.destination() {
        DropDestination::Topology(target) => target.clone(),
        DropDestination::SurfaceBackground(_) => {
            panic!("test expected a topology destination")
        }
    }
}

fn bind_measurement_ticket(
    plan: &PresentationPlan,
    ticket: SurfaceMeasurementTicket,
) -> PresentationPlan {
    let mut measured = PresentationPlan::from_measurements(
        ticket,
        crate::tab_strip::PopupPlaneRequirement::default(),
        plan.bounds(),
        None,
    );
    for record in plan.pane_records().iter().cloned() {
        measured.push_pane_record(record);
    }
    for record in plan.tab_records().iter().cloned() {
        measured.push_tab_record(record);
    }
    for record in plan.tab_bar_records().iter().cloned() {
        measured.push_tab_bar_record(record);
    }
    for record in plan.splitter_gap_records().iter().copied() {
        measured.push_splitter_gap_record(record);
    }
    for record in plan.splitter_records().iter().cloned() {
        measured.push_splitter_record(record);
    }
    for record in plan.splitter_junction_records().iter().cloned() {
        measured.push_splitter_junction_record(record);
    }
    for record in plan.contained_records().iter().cloned() {
        measured.push_contained_record(record);
    }
    for minimum in plan.contained_minimums().iter().copied() {
        measured.push_contained_minimum(minimum);
    }
    if let Some(background) = plan.surface_background().cloned() {
        measured
            .set_surface_background(background)
            .expect("validated background remains unique");
    }
    for occlusion in plan.drop_occlusions().iter().copied() {
        measured.push_drop_occlusion(occlusion);
    }
    for cluster in plan.drop_guide_clusters().iter().cloned() {
        measured.push_drop_guide_cluster(cluster);
    }
    for target in plan.drop_targets().iter().cloned() {
        measured.push_drop_target(target);
    }
    measured
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
        PreparedSurfacePaintCandidate::Retained { .. }
        | PreparedSurfacePaintCandidate::Unavailable { .. } => {
            panic!("fixture measurements must produce a ready paint candidate")
        }
    }
}

fn publish_ready(
    workspace: &Workspace,
    policy: &DockPolicySnapshot,
    ready: PresentationPlan,
    painted: bool,
) -> Result<SurfaceSceneSet, SceneBuildError> {
    let surface = ready.surface();
    let ready =
        PresentationPlanValidator::new(workspace, policy)?.validate_and_canonicalize(ready)?;
    let engine = DockEngine::new(workspace.clone(), policy.to_policy())
        .expect("fixture engine must be valid");
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
        let (_stamp, output_ticket) = scenes.install_ready(
            plan,
            SurfaceCoordinateCapture::Headless {
                authority_generation: CoordinateGeneration::default(),
            },
            ticket.authority_domain(),
            PresentationOutputSerial::new_for_test(serial),
        )?;
        outputs.push((rostered_surface, output_ticket, serial));
    }
    for (rostered_surface, output_ticket, serial) in outputs {
        if painted || rostered_surface != surface {
            let authority = PresentedSurfaceAuthority::mint_observed_for_test_with_emission(
                output_ticket,
                CoordinateGeneration::default(),
                serial,
            );
            assert!(scenes.accept_observed_authority(authority).is_ok());
        }
    }
    Ok(scenes)
}

fn bootstrap_scene(workspace: &Workspace, policy: &DockPolicySnapshot) -> SurfaceSceneSet {
    DockEngine::new(workspace.clone(), policy.to_policy())
        .expect("fixture engine must be valid")
        .scene()
        .clone()
}

fn seal_scene(
    fixture: &Fixture,
    policy: &DockPolicySnapshot,
    records: impl IntoIterator<Item = DropTargetRecord>,
) -> SurfaceSceneSet {
    seal_scene_with_occlusions(fixture, policy, records, [])
}

fn seal_scene_with_occlusions(
    fixture: &Fixture,
    policy: &DockPolicySnapshot,
    records: impl IntoIterator<Item = DropTargetRecord>,
    occlusions: impl IntoIterator<Item = DropOcclusionRecord>,
) -> SurfaceSceneSet {
    let mut ready = PresentationPlan::new(TARGET_SURFACE, rect(0.0, 0.0, 100.0, 100.0));
    for record in records {
        ready.push_drop_target(record);
    }
    for occlusion in occlusions {
        ready.push_drop_occlusion(occlusion);
    }
    if !ready
        .drop_occlusions()
        .iter()
        .any(|occlusion| occlusion.floating() == FLOATING)
    {
        ready.push_drop_occlusion(floating_occlusion(rect(20.0, 20.0, 60.0, 60.0), 2));
    }
    ready.push_contained_minimum(floating_minimum());
    publish_ready(&fixture.workspace, policy, ready, true).expect("scene facts are valid")
}

fn rootless_background_ready() -> PresentationPlan {
    let bounds = rect(0.0, 0.0, 100.0, 100.0);
    let mut ready = PresentationPlan::new(TARGET_SURFACE, bounds);
    ready
        .set_surface_background(DropTargetRecord::surface_background(
            SurfaceBackground::new(TARGET_SURFACE),
            HitRegion::new(bounds),
            DropVisual::new(bounds),
        ))
        .expect("rootless background is unique");
    ready
}

fn rootless_ready() -> PresentationPlan {
    let mut ready = rootless_background_ready();
    ready.push_drop_occlusion(floating_occlusion(rect(20.0, 20.0, 60.0, 60.0), 2));
    ready
}

fn publish_rootless_ready(
    fixture: &RootlessFixture,
    mut ready: PresentationPlan,
) -> Result<SurfaceSceneSet, SceneBuildError> {
    ready.push_contained_minimum(floating_minimum());
    publish_ready(
        &fixture.workspace,
        &DockPolicySnapshot::default(),
        ready,
        true,
    )
}

fn assert_scene_error(
    fixture: &Fixture,
    policy: &DockPolicySnapshot,
    mut ready: PresentationPlan,
    predicate: impl Fn(&SceneBuildError) -> bool,
) {
    if !ready
        .drop_occlusions()
        .iter()
        .any(|occlusion| occlusion.floating() == FLOATING)
    {
        ready.push_drop_occlusion(floating_occlusion(rect(20.0, 20.0, 60.0, 60.0), 2));
    }
    ready.push_contained_minimum(floating_minimum());
    let error = PresentationPlanValidator::new(&fixture.workspace, policy)
        .expect("fixture validator must initialize")
        .validate_and_canonicalize(ready)
        .expect_err("malformed fact must fail validation");
    assert!(predicate(&error), "unexpected error: {error:?}");
}

fn resolve(
    scene: &SurfaceSceneSet,
    fixture: &Fixture,
    policy: &DockPolicySnapshot,
    source: MovePayload,
    at: LogicalPoint,
) -> DropResolution {
    resolve_on_surface(scene, fixture, policy, source, TARGET_SURFACE, at)
}

fn resolve_on_surface(
    scene: &SurfaceSceneSet,
    fixture: &Fixture,
    policy: &DockPolicySnapshot,
    source: MovePayload,
    surface: SurfaceId,
    at: LogicalPoint,
) -> DropResolution {
    resolve_unguided_drop(
        scene,
        &fixture.workspace,
        policy,
        session(),
        source,
        None,
        surface,
        at,
    )
    .expect("fixture resolution cannot expose an invariant failure")
}

#[test]
fn all_sixteen_target_eligibility_masks_validate_only_the_unique_winner() {
    let fixture = fixture();
    let policy = DockPolicySnapshot::default();
    let specs = [
        TargetSpec::TabGap,
        TargetSpec::Center,
        TargetSpec::InnerEdge,
        TargetSpec::OuterEdge,
    ];

    for mask in 0_u8..16 {
        let records: Vec<_> = specs
            .iter()
            .copied()
            .enumerate()
            .map(|(index, spec)| {
                let availability = if mask & (1 << index) != 0 {
                    DropTargetAvailability::Available
                } else {
                    DropTargetAvailability::Unavailable(DropTargetUnavailable::AdapterUnavailable)
                };
                target_record(
                    &fixture,
                    spec,
                    availability,
                    rect(10.0, 10.0, 80.0, 80.0),
                    1,
                )
            })
            .collect();
        let expected = target_record(
            &fixture,
            TargetSpec::TabGap,
            DropTargetAvailability::Available,
            rect(10.0, 10.0, 80.0, 80.0),
            1,
        )
        .id();
        let scene = seal_scene(&fixture, &policy, records);
        let resolution = resolve(
            &scene,
            &fixture,
            &policy,
            payload(&fixture, PayloadSpec::Item),
            point(15.0, 15.0),
        );

        match (mask & 1 != 0, resolution) {
            (true, DropResolution::Resolved(resolved)) => {
                assert_eq!(resolved.target_id(), expected, "mask {mask:04b}");
            }
            (false, DropResolution::Rejected(rejected)) => {
                assert_eq!(rejected.candidates().len(), 1, "mask {mask:04b}");
                assert_eq!(rejected.candidates()[0].target_id(), expected);
            }
            pair => panic!("unexpected mask result {mask:04b}: {pair:?}"),
        }
    }
}

#[test]
fn payload_target_and_policy_matrix_is_prevalidated_by_u3() {
    let fixture = fixture();
    let payloads = [PayloadSpec::Item, PayloadSpec::Tabs, PayloadSpec::Subtree];
    let targets = [
        TargetSpec::TabGap,
        TargetSpec::Center,
        TargetSpec::InnerEdge,
        TargetSpec::OuterEdge,
    ];

    for payload_spec in payloads {
        for target_spec in targets {
            for enabled in [false, true] {
                let mut policy = DockPolicySnapshot::default().to_policy();
                match target_spec {
                    TargetSpec::TabGap | TargetSpec::Center => {
                        policy.set_allow_tab_merge(enabled);
                    }
                    TargetSpec::InnerEdge | TargetSpec::OuterEdge => {
                        policy.set_allow_edge_split(enabled);
                    }
                }
                let policy = policy.snapshot(dockspace::policy::PolicyRevision::default());
                let scene = seal_scene(
                    &fixture,
                    &policy,
                    [target_record(
                        &fixture,
                        target_spec,
                        DropTargetAvailability::Available,
                        rect(0.0, 0.0, 100.0, 100.0),
                        1,
                    )],
                );
                let result = resolve(
                    &scene,
                    &fixture,
                    &policy,
                    payload(&fixture, payload_spec),
                    point(10.0, 10.0),
                );
                let split_into_tabs = payload_spec == PayloadSpec::Subtree
                    && matches!(target_spec, TargetSpec::TabGap | TargetSpec::Center);

                if enabled && !split_into_tabs {
                    assert!(
                        matches!(result, DropResolution::Resolved(_)),
                        "{payload_spec:?} x {target_spec:?} x enabled"
                    );
                } else {
                    assert!(
                        matches!(result, DropResolution::Rejected(_)),
                        "{payload_spec:?} x {target_spec:?} x enabled={enabled}"
                    );
                }
            }
        }
    }
}

#[test]
fn same_structural_layer_uses_lowest_structural_id() {
    let fixture = fixture();
    let policy = DockPolicySnapshot::default();
    let overlap = rect(0.0, 0.0, 100.0, 100.0);
    let first = center_record(&fixture, fixture.target_tabs_a, overlap, 1);
    let second = center_record(&fixture, fixture.target_tabs_b, overlap, 1);
    let expected = first.id().min(second.id());
    let scene = seal_scene(&fixture, &policy, [second, first]);
    let resolved = resolve(
        &scene,
        &fixture,
        &policy,
        payload(&fixture, PayloadSpec::Item),
        point(10.0, 10.0),
    );
    assert!(matches!(
        resolved,
        DropResolution::Resolved(resolved) if resolved.target_id() == expected
    ));
}

#[test]
fn contained_chrome_occludes_lower_layer_drop_targets() {
    let fixture = fixture();
    let policy = DockPolicySnapshot::default();
    let background = target_record(
        &fixture,
        TargetSpec::TabGap,
        DropTargetAvailability::Available,
        rect(0.0, 0.0, 100.0, 100.0),
        1,
    );
    let scene = seal_scene_with_occlusions(
        &fixture,
        &policy,
        [background],
        [floating_occlusion(rect(20.0, 20.0, 60.0, 60.0), 2)],
    );

    assert!(matches!(
        resolve(
            &scene,
            &fixture,
            &policy,
            payload(&fixture, PayloadSpec::Item),
            point(25.0, 25.0),
        ),
        DropResolution::KnownNone(_)
    ));
}

#[test]
fn front_contained_layer_wins_before_back_target_class_priority() {
    let fixture = fixture();
    let policy = DockPolicySnapshot::default();
    let overlap = rect(30.0, 40.0, 40.0, 30.0);
    let background = target_record(
        &fixture,
        TargetSpec::TabGap,
        DropTargetAvailability::Available,
        overlap,
        1,
    );
    let foreground = floating_outer_edge_record(&fixture, overlap, 2);
    let foreground_id = foreground.id();
    let scene = seal_scene_with_occlusions(
        &fixture,
        &policy,
        [background, foreground],
        [floating_occlusion(rect(20.0, 20.0, 60.0, 60.0), 2)],
    );

    assert!(matches!(
        resolve(
            &scene,
            &fixture,
            &policy,
            payload(&fixture, PayloadSpec::Item),
            point(50.0, 50.0),
        ),
        DropResolution::Resolved(resolved) if resolved.target_id() == foreground_id
    ));
}

#[test]
fn rejected_front_candidate_never_falls_through_its_occlusion() {
    let fixture = fixture();
    let policy = DockPolicySnapshot::default();
    let overlap = rect(30.0, 40.0, 40.0, 30.0);
    let background = target_record(
        &fixture,
        TargetSpec::OuterEdge,
        DropTargetAvailability::Available,
        overlap,
        1,
    );
    let foreground = floating_center_record(&fixture, overlap, 2);
    let foreground_id = foreground.id();
    let scene = seal_scene_with_occlusions(
        &fixture,
        &policy,
        [background, foreground],
        [floating_occlusion(rect(20.0, 20.0, 60.0, 60.0), 2)],
    );

    let DropResolution::Rejected(rejected) = resolve(
        &scene,
        &fixture,
        &policy,
        payload(&fixture, PayloadSpec::Subtree),
        point(50.0, 50.0),
    ) else {
        panic!("the visible center target must reject a subtree without exposing the back layer");
    };
    assert_eq!(rejected.candidates().len(), 1);
    assert_eq!(rejected.candidates()[0].target_id(), foreground_id);
}

#[test]
fn targets_remain_resolvable_outside_contained_occlusion() {
    let fixture = fixture();
    let policy = DockPolicySnapshot::default();
    let background = target_record(
        &fixture,
        TargetSpec::Center,
        DropTargetAvailability::Available,
        rect(0.0, 0.0, 100.0, 100.0),
        1,
    );
    let background_id = background.id();
    let scene = seal_scene_with_occlusions(
        &fixture,
        &policy,
        [background],
        [floating_occlusion(rect(20.0, 20.0, 60.0, 60.0), 2)],
    );

    assert!(matches!(
        resolve(
            &scene,
            &fixture,
            &policy,
            payload(&fixture, PayloadSpec::Item),
            point(10.0, 10.0),
        ),
        DropResolution::Resolved(resolved) if resolved.target_id() == background_id
    ));
}

#[test]
fn half_open_regions_assign_shared_edges_once_and_exclude_all_max_edges() {
    let fixture = fixture();
    let policy = DockPolicySnapshot::default();
    let left = center_record(
        &fixture,
        fixture.target_tabs_a,
        rect(0.0, 0.0, 50.0, 100.0),
        1,
    );
    let right = center_record(
        &fixture,
        fixture.target_tabs_b,
        rect(50.0, 0.0, 50.0, 100.0),
        1,
    );
    let left_id = left.id();
    let right_id = right.id();
    let scene = seal_scene(&fixture, &policy, [right, left]);

    for (at, expected) in [
        (point(0.0, 0.0), Some(left_id)),
        (point(49.999_999, 99.999_999), Some(left_id)),
        (point(50.0, 10.0), Some(right_id)),
        (point(99.999_999, 99.999_999), Some(right_id)),
        (point(100.0, 20.0), None),
        (point(20.0, 100.0), None),
    ] {
        let result = resolve(
            &scene,
            &fixture,
            &policy,
            payload(&fixture, PayloadSpec::Item),
            at,
        );
        match expected {
            Some(expected) => assert!(matches!(
                result,
                DropResolution::Resolved(resolved) if resolved.target_id() == expected
            )),
            None => assert!(matches!(result, DropResolution::KnownNone(_))),
        }
    }
}

#[test]
fn zero_area_target_has_no_geometric_hit() {
    let fixture = fixture();
    let policy = DockPolicySnapshot::default();
    let target = target_record(
        &fixture,
        TargetSpec::Center,
        DropTargetAvailability::Available,
        rect(0.0, 0.0, 100.0, 100.0),
        1,
    );
    let zero_hit = DropTargetRecord::new(
        target.id(),
        topology_destination(&target),
        target.availability(),
        HitRegion::new(rect(40.0, 10.0, 0.0, 80.0)),
        target.layer(),
        DropVisual::new(rect(40.0, 10.0, 10.0, 80.0)),
    );
    let scene = seal_scene(&fixture, &policy, [zero_hit]);

    assert!(matches!(
        resolve(
            &scene,
            &fixture,
            &policy,
            payload(&fixture, PayloadSpec::Item),
            point(40.0, 10.0),
        ),
        DropResolution::KnownNone(_)
    ));
}

#[test]
fn missing_bootstrap_pending_paint_and_ready_surfaces_are_distinct() {
    let fixture = fixture();
    let policy = DockPolicySnapshot::default();
    let bootstrap = bootstrap_scene(&fixture.workspace, &policy);
    let mut pending_plan = PresentationPlan::new(TARGET_SURFACE, rect(0.0, 0.0, 100.0, 100.0));
    pending_plan.push_drop_occlusion(floating_occlusion(rect(20.0, 20.0, 60.0, 60.0), 2));
    pending_plan.push_contained_minimum(floating_minimum());
    let pending = publish_ready(&fixture.workspace, &policy, pending_plan, false)
        .expect("pending projection is valid");
    let ready = seal_scene(&fixture, &policy, []);
    let source = payload(&fixture, PayloadSpec::Item);

    let bootstrap_result = resolve(
        &bootstrap,
        &fixture,
        &policy,
        source.clone(),
        point(10.0, 10.0),
    );
    assert!(matches!(
        bootstrap_result,
        DropResolution::Unavailable(unavailable)
            if unavailable.reason() == DropSurfaceUnavailable::Bootstrap
    ));
    let missing_result = resolve_on_surface(
        &bootstrap,
        &fixture,
        &policy,
        source.clone(),
        MISSING_SURFACE,
        point(10.0, 10.0),
    );
    assert!(matches!(
        missing_result,
        DropResolution::Unavailable(unavailable)
            if unavailable.reason() == DropSurfaceUnavailable::MissingSurface
    ));
    assert!(matches!(
        resolve(
            &pending,
            &fixture,
            &policy,
            source.clone(),
            point(10.0, 10.0),
        ),
        DropResolution::Unavailable(unavailable)
            if unavailable.reason() == DropSurfaceUnavailable::PendingPaint
    ));
    assert!(matches!(
        resolve(&ready, &fixture, &policy, source, point(10.0, 10.0),),
        DropResolution::KnownNone(_)
    ));
}

#[test]
fn unpainted_next_candidate_does_not_replace_the_painted_hit_authority() {
    let fixture = fixture();
    let policy = DockPolicySnapshot::default();
    let bounds = rect(0.0, 0.0, 100.0, 100.0);
    let painted_target = target_record(
        &fixture,
        TargetSpec::Center,
        DropTargetAvailability::Available,
        bounds,
        1,
    );
    let painted_target_id = painted_target.id();
    let mut scenes = seal_scene(&fixture, &policy, [painted_target]);

    let mut next = PresentationPlan::new(TARGET_SURFACE, bounds);
    next.push_drop_target(target_record(
        &fixture,
        TargetSpec::OuterEdge,
        DropTargetAvailability::Available,
        bounds,
        1,
    ));
    next.push_drop_occlusion(floating_occlusion(rect(20.0, 20.0, 60.0, 60.0), 2));
    next.push_contained_minimum(floating_minimum());
    let next = PresentationPlanValidator::new(&fixture.workspace, &policy)
        .expect("fixture validator must initialize")
        .validate_and_canonicalize(next)
        .expect("next projection must validate");
    let ticket = scenes
        .surface(TARGET_SURFACE)
        .expect("target surface must remain rostered")
        .stamp()
        .requirement();
    scenes
        .install_ready(
            bind_measurement_ticket(&next, ticket),
            SurfaceCoordinateCapture::Headless {
                authority_generation: CoordinateGeneration::default(),
            },
            ticket.authority_domain(),
            PresentationOutputSerial::new_for_test(2),
        )
        .expect("next projection must install without becoming painted authority");

    let projected_target = scenes
        .surface(TARGET_SURFACE)
        .and_then(SurfaceScene::paint_projection)
        .expect("next projection remains paintable")
        .plan()
        .drop_targets()[0]
        .id();
    assert_ne!(projected_target, painted_target_id);

    let DropResolution::Resolved(resolved) = resolve(
        &scenes,
        &fixture,
        &policy,
        payload(&fixture, PayloadSpec::Item),
        point(10.0, 10.0),
    ) else {
        panic!("the last painted target must remain the sole hit authority");
    };
    assert_eq!(resolved.target_id(), painted_target_id);
}

#[test]
fn rootless_ready_requires_one_background_and_exact_structural_occlusion() {
    let fixture = rootless_fixture();
    let bounds = rect(0.0, 0.0, 100.0, 100.0);

    let missing_background = {
        let mut ready = PresentationPlan::new(TARGET_SURFACE, bounds);
        ready.push_drop_occlusion(floating_occlusion(rect(20.0, 20.0, 60.0, 60.0), 2));
        ready
    };
    assert!(matches!(
        publish_rootless_ready(&fixture, missing_background),
        Err(SceneBuildError::MissingSurfaceBackground {
            surface: TARGET_SURFACE,
        })
    ));

    let mut duplicate_background = rootless_ready();
    assert!(matches!(
        duplicate_background.set_surface_background(DropTargetRecord::surface_background(
            SurfaceBackground::new(TARGET_SURFACE),
            HitRegion::new(bounds),
            DropVisual::new(bounds),
        )),
        Err(SceneBuildError::DuplicateSurfaceBackground {
            surface: TARGET_SURFACE,
        })
    ));

    let missing_occlusion = rootless_background_ready();
    assert!(matches!(
        publish_rootless_ready(&fixture, missing_occlusion),
        Err(SceneBuildError::MissingDropOcclusion {
            surface: TARGET_SURFACE,
            floating: FLOATING,
        })
    ));

    let wrong_layer = {
        let mut ready = rootless_background_ready();
        ready.push_drop_occlusion(floating_occlusion(rect(20.0, 20.0, 60.0, 60.0), 3));
        ready
    };
    assert!(matches!(
        publish_rootless_ready(&fixture, wrong_layer),
        Err(SceneBuildError::DropOcclusionLayerMismatch {
            surface: TARGET_SURFACE,
            floating: FLOATING,
            ..
        })
    ));

    let wrong_geometry = {
        let mut ready = rootless_background_ready();
        ready.push_drop_occlusion(floating_occlusion(rect(21.0, 20.0, 59.0, 60.0), 2));
        ready
    };
    assert!(matches!(
        publish_rootless_ready(&fixture, wrong_geometry),
        Err(SceneBuildError::DropOcclusionGeometryMismatch {
            surface: TARGET_SURFACE,
            floating: FLOATING,
            ..
        })
    ));

    publish_rootless_ready(&fixture, rootless_ready())
        .expect("one background plus the exact roster occlusion is valid");
}

#[test]
fn rootless_background_requires_partial_offer_and_promotes_same_surface_contained_root() {
    let fixture = rootless_fixture();
    let scene =
        publish_rootless_ready(&fixture, rootless_ready()).expect("rootless ready scene is valid");
    let partial = MovePayload::Item(
        fixture
            .workspace
            .capture_item_source(SOURCE_ROOT, fixture.source_tabs, ItemId::new(1))
            .expect("partial source is current"),
    );

    let missing_offer = resolve_unguided_drop(
        &scene,
        &fixture.workspace,
        &DockPolicySnapshot::default(),
        session(),
        partial.clone(),
        None,
        TARGET_SURFACE,
        point(10.0, 10.0),
    )
    .expect("missing offer is an expected rejection");
    assert!(matches!(
        missing_offer,
        DropResolution::Rejected(rejected)
            if rejected.candidates().len() == 1
                && rejected.candidates()[0].reason()
                    == &DropRejectionReason::SurfaceBackgroundRootOfferMissing
    ));

    let offered_root = RootId::new(99);
    let DropResolution::Resolved(partial_drop) = resolve_unguided_drop(
        &scene,
        &fixture.workspace,
        &DockPolicySnapshot::default(),
        session(),
        partial,
        Some(SurfaceBackgroundRootOffer::new(offered_root)),
        TARGET_SURFACE,
        point(10.0, 10.0),
    )
    .expect("fresh offer resolution cannot expose an invariant") else {
        panic!("fresh partial offer must resolve");
    };
    assert!(matches!(
        partial_drop.command(),
        WorkspaceCommand::InstallMainRoot {
            surface: TARGET_SURFACE,
            root,
            ..
        } if *root == offered_root
    ));
    let mut partial_candidate = fixture.workspace.clone();
    WorkspaceTransaction::from_commands([partial_drop.command().clone()])
        .apply(&mut partial_candidate, &DockPolicySnapshot::default())
        .expect("resolved partial background drop commits");
    assert_eq!(
        partial_candidate.surface(TARGET_SURFACE),
        Some(&SurfacePresentation {
            main_root: Some(offered_root),
            contained: vec![FLOATING],
        })
    );

    let complete_contained = MovePayload::Item(
        fixture
            .workspace
            .capture_item_source(FLOATING_ROOT, fixture.contained_tabs, ItemId::new(20))
            .expect("complete contained source is current"),
    );
    let DropResolution::Resolved(promotion) = resolve_unguided_drop(
        &scene,
        &fixture.workspace,
        &DockPolicySnapshot::default(),
        session(),
        complete_contained,
        None,
        TARGET_SURFACE,
        point(10.0, 10.0),
    )
    .expect("same-surface promotion cannot expose an invariant") else {
        panic!("complete contained root must resolve as a promotion");
    };
    assert!(matches!(
        promotion.command(),
        WorkspaceCommand::PromoteContained {
            source,
            surface: TARGET_SURFACE,
            floating: FLOATING,
        } if source.root() == FLOATING_ROOT
    ));
    let mut promotion_candidate = fixture.workspace.clone();
    WorkspaceTransaction::from_commands([promotion.command().clone()])
        .apply(&mut promotion_candidate, &DockPolicySnapshot::default())
        .expect("resolved contained promotion commits");
    assert_eq!(
        promotion_candidate.surface(TARGET_SURFACE),
        Some(&SurfacePresentation::with_main(FLOATING_ROOT))
    );
    assert!(promotion_candidate.contained_floating(FLOATING).is_none());
}

#[test]
fn split_subtree_into_tabs_is_an_expected_prevalidation_rejection() {
    let fixture = fixture();
    let policy = DockPolicySnapshot::default();
    let scene = seal_scene(
        &fixture,
        &policy,
        [target_record(
            &fixture,
            TargetSpec::Center,
            DropTargetAvailability::Available,
            rect(0.0, 0.0, 100.0, 100.0),
            1,
        )],
    );

    let DropResolution::Rejected(rejected) = resolve(
        &scene,
        &fixture,
        &policy,
        payload(&fixture, PayloadSpec::Subtree),
        point(10.0, 10.0),
    ) else {
        panic!("split subtree must be rejected by exact command prevalidation");
    };
    assert!(matches!(
        rejected.candidates()[0].reason(),
        DropRejectionReason::Prevalidation(dockspace::error::TransactionError::Command {
            source: dockspace::error::CommandError::SplitPayloadIntoTabs { .. },
            ..
        })
    ));
}

#[test]
fn stale_target_is_retained_as_explicitly_unavailable() {
    let mut fixture = fixture();
    let policy = DockPolicySnapshot::default();
    let old_target = target_record(
        &fixture,
        TargetSpec::Center,
        DropTargetAvailability::Available,
        rect(0.0, 0.0, 100.0, 100.0),
        1,
    );
    let select = fixture
        .workspace
        .capture_item_source(TARGET_ROOT, fixture.target_tabs_a, ItemId::new(11))
        .expect("selection source exists");
    WorkspaceTransaction::from_commands([dockspace::command::WorkspaceCommand::Select {
        source: select,
    }])
    .apply(&mut fixture.workspace, &policy)
    .expect("selection changes target fingerprint");

    let scene = seal_scene(&fixture, &policy, [old_target]);
    let ready = scene
        .ready_surface(TARGET_SURFACE)
        .expect("painted target surface must be interaction-ready");
    assert_eq!(
        ready.plan().drop_targets()[0].availability(),
        DropTargetAvailability::Unavailable(DropTargetUnavailable::Stale)
    );
}

fn permutations(values: &mut [usize], start: usize, output: &mut Vec<Vec<usize>>) {
    if start == values.len() {
        output.push(values.to_vec());
        return;
    }
    for index in start..values.len() {
        values.swap(start, index);
        permutations(values, start + 1, output);
        values.swap(start, index);
    }
}

#[test]
fn every_target_insertion_permutation_seals_and_resolves_identically() {
    let fixture = fixture();
    let policy = DockPolicySnapshot::default();
    let records = [
        target_record(
            &fixture,
            TargetSpec::TabGap,
            DropTargetAvailability::Available,
            rect(0.0, 0.0, 100.0, 100.0),
            1,
        ),
        target_record(
            &fixture,
            TargetSpec::Center,
            DropTargetAvailability::Available,
            rect(0.0, 0.0, 100.0, 100.0),
            1,
        ),
        target_record(
            &fixture,
            TargetSpec::InnerEdge,
            DropTargetAvailability::Available,
            rect(0.0, 0.0, 100.0, 100.0),
            1,
        ),
        target_record(
            &fixture,
            TargetSpec::OuterEdge,
            DropTargetAvailability::Available,
            rect(0.0, 0.0, 100.0, 100.0),
            1,
        ),
    ];
    let mut orders = Vec::new();
    permutations(&mut [0, 1, 2, 3], 0, &mut orders);
    let mut reference_targets = None;

    for order in orders {
        let scene = seal_scene(
            &fixture,
            &policy,
            order.into_iter().map(|index| records[index].clone()),
        );
        let canonical_targets = scene
            .ready_surface(TARGET_SURFACE)
            .expect("painted target surface must be interaction-ready")
            .plan()
            .drop_targets()
            .to_vec();
        if let Some(reference) = &reference_targets {
            assert_eq!(&canonical_targets, reference);
        } else {
            reference_targets = Some(canonical_targets);
        }
        let result = resolve(
            &scene,
            &fixture,
            &policy,
            payload(&fixture, PayloadSpec::Item),
            point(10.0, 10.0),
        );
        assert!(matches!(
            result,
            DropResolution::Resolved(resolved)
                if resolved.target_id().kind() == dockspace::drop_target::DropTargetKind::TabGap
        ));
    }
}

#[test]
fn malformed_semantic_facts_and_target_id_mismatches_fail_seal() {
    let fixture = fixture();
    let policy = DockPolicySnapshot::default();
    let bounds = rect(0.0, 0.0, 100.0, 100.0);

    let mut invalid_occlusion = PresentationPlan::new(TARGET_SURFACE, bounds);
    invalid_occlusion.push_drop_occlusion(DropOcclusionRecord::new(
        FloatingPresentationId::new(999),
        HitRegion::new(bounds),
        SceneLayerKey::new(1),
    ));
    assert_scene_error(&fixture, &policy, invalid_occlusion, |error| {
        matches!(error, SceneBuildError::InvalidDropOcclusion { .. })
    });

    let edge = target_record(
        &fixture,
        TargetSpec::InnerEdge,
        DropTargetAvailability::Available,
        bounds,
        0,
    );
    let mismatched = DropTargetRecord::new(
        DropTargetId::Center {
            surface: TARGET_SURFACE,
            root: TARGET_ROOT,
            tabs: fixture.target_tabs_a,
        },
        topology_destination(&edge),
        DropTargetAvailability::Available,
        HitRegion::new(bounds),
        SceneLayerKey::new(1),
        DropVisual::new(bounds),
    );
    let mut invalid_target = PresentationPlan::new(TARGET_SURFACE, bounds);
    invalid_target.push_drop_target(mismatched);
    assert_scene_error(&fixture, &policy, invalid_target, |error| {
        matches!(error, SceneBuildError::TargetSemanticMismatch { .. })
    });
}

#[test]
fn invalid_inner_edges_and_unpaintable_visuals_fail_seal() {
    let fixture = fixture();
    let policy = DockPolicySnapshot::default();
    let bounds = rect(0.0, 0.0, 100.0, 100.0);
    let split_edge = fixture
        .workspace
        .capture_outer_edge_target(
            TARGET_ROOT,
            Edge::Left,
            DockFraction::new(0.35).expect("valid fraction"),
        )
        .expect("split edge reference is current");
    let mut invalid_inner_edge = PresentationPlan::new(TARGET_SURFACE, bounds);
    invalid_inner_edge.push_drop_target(DropTargetRecord::new(
        DropTargetId::InnerEdge {
            surface: TARGET_SURFACE,
            root: TARGET_ROOT,
            node: fixture.target_root_node,
            edge: Edge::Left,
        },
        DockTarget::InnerEdge(split_edge),
        DropTargetAvailability::Available,
        HitRegion::new(bounds),
        SceneLayerKey::new(1),
        DropVisual::new(bounds),
    ));
    assert_scene_error(&fixture, &policy, invalid_inner_edge, |error| {
        matches!(error, SceneBuildError::TargetSemanticMismatch { .. })
    });

    let center = center_record(&fixture, fixture.target_tabs_a, bounds, 1);
    let mut empty_visual = PresentationPlan::new(TARGET_SURFACE, bounds);
    empty_visual.push_drop_target(DropTargetRecord::new(
        center.id(),
        topology_destination(&center),
        center.availability(),
        center.region(),
        center.layer(),
        DropVisual::new(rect(10.0, 10.0, 0.0, 30.0)),
    ));
    assert_scene_error(&fixture, &policy, empty_visual, |error| {
        matches!(error, SceneBuildError::EmptyDropVisual { .. })
    });

    let mut outside_visual = PresentationPlan::new(TARGET_SURFACE, bounds);
    outside_visual.push_drop_target(DropTargetRecord::new(
        center.id(),
        topology_destination(&center),
        center.availability(),
        center.region(),
        center.layer(),
        DropVisual::new(rect(90.0, 10.0, 20.0, 30.0)),
    ));
    assert_scene_error(&fixture, &policy, outside_visual, |error| {
        matches!(error, SceneBuildError::DropVisualOutsideSurface { .. })
    });
}

#[test]
fn duplicate_semantic_ids_are_rejected_before_install() {
    let fixture = fixture();
    let policy = DockPolicySnapshot::default();
    let bounds = rect(0.0, 0.0, 100.0, 100.0);
    let duplicate_id = PaneSceneId {
        root: TARGET_ROOT,
        tabs: fixture.target_tabs_a,
    };
    let mut ready = PresentationPlan::new(TARGET_SURFACE, bounds);
    ready.push_pane_record(PaneRecord::new(
        duplicate_id,
        bounds,
        bounds,
        Some(ItemId::new(10)),
        SceneLayerKey::new(1),
    ));
    ready.push_pane_record(PaneRecord::new(
        duplicate_id,
        bounds,
        bounds,
        Some(ItemId::new(10)),
        SceneLayerKey::new(1),
    ));
    assert_scene_error(
        &fixture,
        &policy,
        ready,
        |error| matches!(error, SceneBuildError::DuplicatePane { id } if *id == duplicate_id),
    );

    let mut duplicate_occlusion = PresentationPlan::new(TARGET_SURFACE, bounds);
    let occlusion = floating_occlusion(bounds, 1);
    duplicate_occlusion.push_drop_occlusion(occlusion);
    duplicate_occlusion.push_drop_occlusion(occlusion);
    assert_scene_error(&fixture, &policy, duplicate_occlusion, |error| {
        matches!(
            error,
            SceneBuildError::DuplicateDropOcclusion { floating } if *floating == FLOATING
        )
    });
}
