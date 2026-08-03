mod support;

use dockspace::PresentationCompilationError;
use dockspace::command::{DockTarget, WorkspaceCommand};
use dockspace::engine::{
    DockEngine, EngineInput, PreparedSurfaceContribution, SurfaceContributionPrepareError,
};
use dockspace::geometry::{LogicalRect, LogicalSize};
use dockspace::graph::{Node, RootRecord, SurfacePresentation, Workspace};
use dockspace::ids::{ItemId, RootId, StableInputSourceId, SurfaceId};
use dockspace::policy::DockPolicy;
use dockspace::presentation_config::DockPresentationConfig;
use dockspace::scene::SurfaceScene;
use dockspace::scene_manifest::{
    ManifestMeasurementError, Measurement, MeasurementValidationError, SurfaceMeasurements,
    TabIntrinsic, TabStripMetrics,
};
use dockspace::transition::{
    EngineTransition, InputOutcome, SurfaceContributionOutcome, SurfaceContributionRejection,
};
use support::{TestPresentationHost, publish_surface, submit_input};

const SURFACE: SurfaceId = SurfaceId::new(1);
const ROOT: RootId = RootId::new(2);
const MANIFEST_INPUT_SOURCE: StableInputSourceId = StableInputSourceId::new(0xA11F);

fn workspace(surface: SurfaceId, root: RootId, items: &[ItemId]) -> Workspace {
    let mut builder = Workspace::builder();
    let tabs = builder.insert_node(Node::tabs(items.iter().copied()));
    builder.set_root(root, RootRecord::new(tabs).with_central(tabs));
    builder.set_surface(surface, SurfacePresentation::with_main(root));
    builder.build().expect("manifest fixture should validate")
}

fn complete_measurements(engine: &DockEngine, surface: SurfaceId) -> SurfaceMeasurements {
    let requirements = engine
        .presentation_requirements()
        .surface(surface)
        .expect("surface requirements should exist");
    let mut measurements = SurfaceMeasurements::new(requirements.ticket());
    measurements
        .set_bounds(
            requirements.bounds(),
            Measurement::Measured(
                LogicalRect::new(0.0, 0.0, 640.0, 480.0).expect("bounds should be valid"),
            ),
        )
        .expect("bounds should be submitted once");
    if let Some(key) = requirements.popup_plane_bounds() {
        measurements
            .set_popup_plane_bounds(
                key,
                Measurement::Measured(
                    LogicalRect::new(0.0, 0.0, 640.0, 480.0)
                        .expect("popup-plane bounds should be valid"),
                ),
            )
            .expect("popup-plane bounds should be submitted once");
    }
    let minimum = LogicalSize::new(0.0, 0.0).expect("minimum should be valid");
    for key in requirements.pane_minimums() {
        measurements
            .insert_pane_minimum(key, Measurement::Measured(minimum))
            .expect("pane minimum should be submitted once");
    }
    for key in requirements.tab_intrinsics() {
        measurements
            .insert_tab_intrinsic(
                key,
                Measurement::Measured(
                    TabIntrinsic::new(56.0).expect("tab intrinsic should be valid"),
                ),
            )
            .expect("tab intrinsic should be submitted once");
    }
    for key in requirements.tab_strips() {
        measurements
            .insert_tab_strip(
                key,
                Measurement::Measured(
                    TabStripMetrics::new(0.0, 0.0).expect("tab strip should be valid"),
                ),
            )
            .expect("tab strip should be submitted once");
    }
    measurements
}

fn submit_prepared(
    engine: &mut DockEngine,
    host: &mut TestPresentationHost,
    contribution: PreparedSurfaceContribution,
) -> EngineTransition {
    let mut frame = host.begin(engine);
    frame
        .push_surface_contribution(contribution)
        .expect("one test tick carries one surface contribution");
    host.finish(frame, engine)
}

#[test]
fn engine_exposes_a_core_derived_exact_surface_manifest() {
    let engine = DockEngine::new(
        workspace(SURFACE, ROOT, &[ItemId::new(10), ItemId::new(11)]),
        DockPolicy::default(),
    )
    .expect("engine should construct");
    let manifest = engine.presentation_requirements();
    let requirements = manifest.surface(SURFACE).expect("surface should exist");

    assert_eq!(manifest.workspace(), engine.version());
    assert_eq!(manifest.surfaces().len(), 1);
    assert_eq!(requirements.ticket().surface(), SURFACE);
    assert_eq!(requirements.pane_minimums().len(), 2);
    assert_eq!(requirements.tab_intrinsics().len(), 2);
    assert_eq!(requirements.tab_strips().len(), 1);
}

#[test]
fn incomplete_exact_set_fails_during_prepare_without_entering_a_reducer_tick() {
    let engine = DockEngine::new(
        workspace(SURFACE, ROOT, &[ItemId::new(10)]),
        DockPolicy::default(),
    )
    .expect("engine should construct");
    let token = engine
        .begin_surface_contribution(SURFACE)
        .expect("surface contribution should begin");
    let incomplete = SurfaceMeasurements::new(token.ticket());
    let before = engine.scene().clone();

    let error = engine
        .prepare_surface_contribution(token, incomplete)
        .expect_err("missing exact-set answers must fail during preparation");

    assert!(matches!(
        error,
        SurfaceContributionPrepareError::Compilation(PresentationCompilationError::Manifest(_))
    ));
    assert_eq!(engine.scene(), &before);
}

#[test]
fn committed_topology_change_atomically_replaces_requirement_tickets() {
    let mut engine = DockEngine::new(
        workspace(SURFACE, ROOT, &[ItemId::new(10)]),
        DockPolicy::default(),
    )
    .expect("engine should construct");
    let mut host = TestPresentationHost::new(&mut engine);
    let before_manifest = engine.presentation_requirements().revision();
    let before_surface = engine
        .presentation_requirements()
        .surface(SURFACE)
        .expect("surface should exist")
        .ticket()
        .surface_requirement();
    let tabs = engine
        .workspace()
        .root(ROOT)
        .expect("root should exist")
        .node;
    let target = engine
        .workspace()
        .capture_tab_target(ROOT, tabs)
        .expect("target should be current");

    let expected = engine.version();
    submit_input(
        &mut engine,
        &mut host,
        MANIFEST_INPUT_SOURCE,
        EngineInput::WorkspaceCommand {
            expected,
            command: WorkspaceCommand::Open {
                item: ItemId::new(11),
                target: DockTarget::Center(target),
            },
        },
    )
    .expect("command and manifest replacement should commit atomically");

    let manifest = engine.presentation_requirements();
    let requirements = manifest.surface(SURFACE).expect("surface should exist");
    assert_eq!(manifest.workspace(), engine.version());
    assert!(manifest.revision() > before_manifest);
    assert!(requirements.ticket().surface_requirement() > before_surface);
    assert!(
        requirements
            .tab_intrinsics()
            .any(|key| key.tab().item == ItemId::new(11))
    );
}

#[test]
fn workspace_replacement_replaces_the_manifest_surface_roster_exactly() {
    let mut engine = DockEngine::new(
        workspace(SURFACE, ROOT, &[ItemId::new(10)]),
        DockPolicy::default(),
    )
    .expect("engine should construct");
    let mut host = TestPresentationHost::new(&mut engine);
    let replacement_surface = SurfaceId::new(20);
    let replacement_root = RootId::new(21);

    submit_input(
        &mut engine,
        &mut host,
        MANIFEST_INPUT_SOURCE,
        EngineInput::ReplaceWorkspace(workspace(
            replacement_surface,
            replacement_root,
            &[ItemId::new(30)],
        )),
    )
    .expect("replacement and manifest should commit atomically");

    let surfaces = engine
        .presentation_requirements()
        .surfaces()
        .map(|(surface, _)| surface)
        .collect::<Vec<_>>();
    assert_eq!(surfaces, [replacement_surface]);
    assert_eq!(
        engine.presentation_requirements().workspace(),
        engine.version()
    );
}

#[test]
fn presentation_config_change_invalidates_scene_and_every_old_measurement_ticket() {
    let mut engine = DockEngine::new(
        workspace(SURFACE, ROOT, &[ItemId::new(10), ItemId::new(11)]),
        DockPolicy::default(),
    )
    .expect("engine should construct");
    let mut host = TestPresentationHost::new(&mut engine);
    let old_measurements = complete_measurements(&engine, SURFACE);
    let old_ticket = old_measurements.ticket();
    publish_surface(
        &mut engine,
        &mut host,
        SURFACE,
        LogicalRect::new(0.0, 0.0, 640.0, 480.0).expect("bounds should be valid"),
    );
    assert!(engine.scene().ready_surface(SURFACE).is_some());
    let old_token = engine
        .begin_surface_contribution(SURFACE)
        .expect("surface contribution should begin");
    let prepared = engine
        .prepare_surface_contribution(old_token, old_measurements.clone())
        .expect("old measurements are authoritative before config replacement");

    let workspace_version = engine.version();
    let old_manifest_revision = engine.presentation_requirements().revision();
    let config = DockPresentationConfig::builder()
        .tab_bar_height(32.0)
        .build()
        .expect("replacement config should be valid");
    let transition = submit_input(
        &mut engine,
        &mut host,
        MANIFEST_INPUT_SOURCE,
        EngineInput::ReplacePresentationConfig {
            expected: workspace_version,
            config,
        },
    )
    .expect("config should reduce");

    assert!(matches!(
        transition.reduced_inputs(),
        [input]
            if matches!(
                input.outcome(),
                InputOutcome::PresentationConfigReplaced { changed: true, revision }
                    if *revision > old_ticket.config()
            )
    ));
    assert_eq!(engine.version(), workspace_version);
    assert!(matches!(
        engine.scene().surface(SURFACE),
        Some(SurfaceScene::Stale(_))
    ));
    assert!(engine.scene().ready_surface(SURFACE).is_none());
    let current = engine.presentation_requirements();
    let current_ticket = current
        .surface(SURFACE)
        .expect("surface should remain present")
        .ticket();
    assert!(current.revision() > old_manifest_revision);
    assert_eq!(
        current_ticket.surface_requirement(),
        old_ticket.surface_requirement(),
        "configuration has its own ticket revision and does not falsify surface semantics"
    );
    assert!(current_ticket.config() > old_ticket.config());

    let error = current
        .validate_surface(&old_measurements)
        .expect_err("old measurements must not validate under new geometry");
    assert!(matches!(
        error,
        ManifestMeasurementError::InvalidContribution {
            source: MeasurementValidationError::ConfigMismatch {
                expected,
                actual,
            },
            ..
        } if expected == current_ticket.config() && actual == old_ticket.config()
    ));

    let transition = submit_prepared(&mut engine, &mut host, prepared);
    assert!(matches!(
        transition.surface_contributions(),
        [SurfaceContributionOutcome::Rejected {
            surface: SURFACE,
            reason: SurfaceContributionRejection::StaleBase { submitted, current },
        }] if *submitted == old_token.base() && current.is_some_and(|stamp| stamp != old_token.base())
    ));
}

#[test]
fn policy_change_invalidates_the_manifest_and_a_pending_surface_contribution() {
    let mut engine = DockEngine::new(
        workspace(SURFACE, ROOT, &[ItemId::new(10)]),
        DockPolicy::default(),
    )
    .expect("engine should construct");
    let mut host = TestPresentationHost::new(&mut engine);
    let measurements = complete_measurements(&engine, SURFACE);
    let old_ticket = measurements.ticket();
    let old_manifest_revision = engine.presentation_requirements().revision();
    let token = engine
        .begin_surface_contribution(SURFACE)
        .expect("surface contribution should begin");
    let prepared = engine
        .prepare_surface_contribution(token, measurements.clone())
        .expect("measurements are authoritative before policy replacement");

    let expected = engine.version();
    let mut policy = engine.policy().clone();
    policy.set_allow_native_surfaces(true);
    submit_input(
        &mut engine,
        &mut host,
        MANIFEST_INPUT_SOURCE,
        EngineInput::ReplacePolicy { expected, policy },
    )
    .expect("policy replacement should advance workspace authority");
    let current = engine.presentation_requirements();
    let current_ticket = current
        .surface(SURFACE)
        .expect("surface should remain present")
        .ticket();
    assert!(current_ticket.policy() > old_ticket.policy());
    assert!(current.revision() > old_manifest_revision);
    assert_eq!(
        current_ticket.surface_requirement(),
        old_ticket.surface_requirement(),
        "policy has its own ticket revision and does not falsify surface semantics"
    );

    let error = current
        .validate_surface(&measurements)
        .expect_err("old measurements must not validate under new policy");
    assert!(matches!(
        error,
        ManifestMeasurementError::InvalidContribution {
            source: MeasurementValidationError::PolicyMismatch { expected, actual },
            ..
        } if expected == current_ticket.policy() && actual == old_ticket.policy()
    ));

    let transition = submit_prepared(&mut engine, &mut host, prepared);
    assert!(matches!(
        transition.surface_contributions(),
        [SurfaceContributionOutcome::Rejected {
            surface: SURFACE,
            reason: SurfaceContributionRejection::StaleBase { submitted, current },
        }] if *submitted == token.base() && current.is_some_and(|stamp| stamp != token.base())
    ));
}

#[test]
fn prepare_rejects_measurements_from_an_older_manifest_even_with_a_current_token() {
    let mut engine = DockEngine::new(
        workspace(SURFACE, ROOT, &[ItemId::new(10), ItemId::new(11)]),
        DockPolicy::default(),
    )
    .expect("engine should construct");
    let mut host = TestPresentationHost::new(&mut engine);
    let measurements = complete_measurements(&engine, SURFACE);
    let actual = measurements.ticket();
    let workspace_version = engine.version();

    submit_input(
        &mut engine,
        &mut host,
        MANIFEST_INPUT_SOURCE,
        EngineInput::ReplacePresentationConfig {
            expected: workspace_version,
            config: DockPresentationConfig::builder()
                .tab_bar_height(32.0)
                .build()
                .expect("replacement config should be valid"),
        },
    )
    .expect("config should replace the manifest");
    assert_eq!(engine.version(), workspace_version);
    let expected = engine
        .presentation_requirements()
        .surface(SURFACE)
        .expect("surface should remain present")
        .ticket();
    assert_ne!(expected, actual);

    let token = engine
        .begin_surface_contribution(SURFACE)
        .expect("current contribution should begin");
    let before = engine.scene().clone();
    let error = engine
        .prepare_surface_contribution(token, measurements)
        .expect_err("a foreign ticket cannot become a prepared contribution");

    assert!(matches!(
        error,
        SurfaceContributionPrepareError::TicketMismatch {
            surface: SURFACE,
            expected: current,
            actual: rejected,
        } if current == expected && rejected == actual
    ));
    assert_eq!(engine.scene(), &before);
    assert!(matches!(
        engine.scene().surface(SURFACE),
        Some(SurfaceScene::Bootstrap(_))
    ));
}
